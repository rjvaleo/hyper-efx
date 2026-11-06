//! Hyperprism Compressor: Hyperprism-PPC 1.2.1's process 36 (class
//! ECompressor, routine `Compressor`), as its code does it.
//!
//! A look-ahead limiter built from two 1024-frame delay rings, A and B. Each
//! frame, the input times a sin² window is written to each ring — the two
//! windows half a block apart, so they sum to one — after the sample a block
//! ago is read back. The output is ½(A + B) × (gA + gB): the input 1024
//! frames late, times the mean of two block gains. That delay is reported as
//! latency.
//!
//! - **Peak per block.** Each ring keeps the largest windowed |sample| of
//!   its block, both channels together (fully linked). When its block ends —
//!   every 1024 frames, B 512 frames after A — the envelope moves once,
//!   env ← a·peak + (1 − a)·env, with the attack coefficient if the peak is
//!   over the envelope and the release one if not, and the block's gain is
//!   set. The gain then holds for the next block, which is the one whose peak
//!   it came from: a true look-ahead, stepping every 512 frames.
//! - **Curve.** A hard knee at infinite ratio: under the Compression level T
//!   the gain is Volume / T (makeup); over it, Volume / env, which holds the
//!   peak envelope at the Volume. Both levels are dBFS, a float of 1 being
//!   the original's 32768.
//! - **Times.** a = `CalcLowPassParameter(ms, ½, SR / 1024)`, at the block
//!   rate: the time constant grows with the square of the setting (200 ms →
//!   165 ms, 1000 ms → 4.4 s), and under about 46 ms the coefficient is
//!   1.1547, so the envelope overshoots the peak on its first block — the
//!   original's formula, kept.
//!
//! At another sample rate the block is 1024 × sr / 44100 frames (kept even),
//! so the look-ahead and the gain steps keep their length in milliseconds,
//! and the coefficients are taken at that block rate.
//!
//! The pad is the original's: Volume across, Compression up. The original's
//! Volume axis runs from 0 dB at the left to −96 dB at the right; a pad pair
//! here runs low to high, so the axis is −96 to 0, reversed from the
//! original.
//!
//! Added (not in the original): Look-ahead, 128–2048 frames at 44.1 kHz,
//! default 1024: the block, and with it the rings, the window, the reported
//! latency and the gain steps (half a block apart). It is scaled with the rate
//! and kept even as the original block is, and the attack and release keep
//! their times at the new block rate. The rings are made for the longest, so
//! a change allocates nothing; it starts the rings afresh, empty and B half a
//! block on, keeping the envelopes and gains, so the output drops out for one
//! new block. Default 1024 is the original exactly.
//!
//! Changed from the original: a mono input takes the stereo path (equal
//! sides give the same result). The original's mono path started ring B a
//! block out of step — its offset assumed stereo interleaving — which left
//! out half the window sum, and dipped the level, for its first 1536
//! samples.

use super::hp_stereo_shared::{low_pass_coefficient, scaled};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const VOLUME: usize = 0;
const COMPRESSION: usize = 1;
const ATTACK: usize = 2;
const RELEASE: usize = 3;
const LOOK_AHEAD: usize = 4;

/// The longest look-ahead, in frames at 44.1 kHz.
const LONGEST: f32 = 2048.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("volumeDb", "Volume", -96.0, 0.0, 0.0).unit("dB").smooth(0.0),
    ParamSpec::new("compressionDb", "Compression", -96.0, 0.0, 0.0).unit("dB").smooth(0.0),
    ParamSpec::new("attackMs", "Attack", 0.0, 1_000.0, 100.0).unit("ms").kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("releaseMs", "Release", 0.0, 1_000.0, 1_000.0).unit("ms").kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("lookAhead", "Look-ahead", 128.0, LONGEST, 1_024.0)
        .unit("samples")
        .kind(ParamKind::Int)
        .smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-compressor",
    code: 63,
    version: 1,
    label: "Hyperprism Compressor",
    category: Category::Dynamics,
    params: PARAMS,
    panel: Panel {
        // The original: the X/Y pad, volume across and compression up, and
        // the attack and release sliders in its settings.
        groups: &[
            Group { label: "Levels", keys: &["volumeDb", "compressionDb"] },
            Group { label: "Settings", keys: &["attackMs", "releaseMs", "lookAhead"] },
        ],
        primary: &["volumeDb", "compressionDb", "attackMs", "releaseMs"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "volumeDb", x_range: (-96.0, 0.0), y: "compressionDb", y_range: (-96.0, 0.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Compressor",
    sound: None,
};

/// One of the two rings and its block.
struct Ring {
    frames: Box<[[f32; 2]]>,
    /// Where in the block, and so in the ring and the window.
    at: usize,
    peak: f32,
    env: f32,
    gain: f32,
}

impl Ring {
    fn new(length: usize) -> Self {
        Ring { frames: vec![[0.0; 2]; length].into_boxed_slice(), at: 0, peak: 0.0, env: 0.0, gain: 1.0 }
    }

    fn clear(&mut self, at: usize) {
        self.frames.fill([0.0; 2]);
        self.at = at;
        self.peak = 0.0;
        self.env = 0.0;
        self.gain = 1.0;
    }
}

/// The larger of the peak and |s|, as the routine compares them.
#[inline]
fn peak_of(peak: f32, s: f32) -> f32 {
    if s > peak {
        s
    } else if peak < -s {
        -s
    } else {
        peak
    }
}

pub struct HpCompressor {
    sample_rate: f32,
    values: [f32; 5],
    block: usize,
    window: Box<[f32]>,
    a: Ring,
    b: Ring,
    volume: f32,
    threshold: f32,
    /// Attack and release coefficients and their complements.
    attack: (f32, f32),
    release: (f32, f32),
}

/// A look-ahead in frames at 44.1 kHz as a block at this rate: scaled, kept even.
fn block_for(frames: f32, sample_rate: f32) -> usize {
    2 * scaled(0.5 * frames, sample_rate)
}

impl HpCompressor {
    /// The sin² window over the block, into the front of the longest one.
    fn shape_window(&mut self) {
        let step = core::f64::consts::PI / self.block as f64;
        for (i, w) in self.window[..self.block].iter_mut().enumerate() {
            let s = (i as f64 * step).sin();
            *w = (s * s) as f32;
        }
    }

    fn update(&mut self) {
        let level = |db: f32| 10f64.powf(0.05 * db as f64) as f32;
        self.volume = level(self.values[VOLUME]);
        self.threshold = level(self.values[COMPRESSION]);
        let fs = (self.sample_rate / self.block as f32) as f64;
        let coefficient = |ms: f32| {
            let a = low_pass_coefficient(ms as f64, 0.5, fs) as f32;
            (a, (1.0 - a as f64) as f32)
        };
        self.attack = coefficient(self.values[ATTACK]);
        self.release = coefficient(self.values[RELEASE]);
    }

    /// The end of a ring's block: the envelope moves and the gain is set.
    fn block_end(ring: &mut Ring, attack: (f32, f32), release: (f32, f32), makeup: f32, threshold: f32) {
        let (a, b) = if ring.peak <= ring.env { release } else { attack };
        ring.env = ring.peak * a + ring.env * b;
        ring.gain = if threshold < ring.env { (makeup * threshold) / ring.env } else { makeup };
        ring.peak = 0.0;
        ring.at = 0;
    }
}

impl Kernel for HpCompressor {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 5];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let block = block_for(values[LOOK_AHEAD], sample_rate);
        let longest = block_for(LONGEST, sample_rate);
        let mut k = HpCompressor {
            sample_rate,
            values,
            block,
            window: vec![0.0; longest].into_boxed_slice(),
            a: Ring::new(longest),
            b: Ring::new(longest),
            volume: 1.0,
            threshold: 1.0,
            attack: (1.0, 0.0),
            release: (1.0, 0.0),
        };
        k.shape_window();
        k.update();
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        self.values[index] = spec.clamp(value);
        if index == LOOK_AHEAD {
            let block = block_for(self.values[LOOK_AHEAD], self.sample_rate);
            if block != self.block {
                self.block = block;
                self.shape_window();
                for (ring, at) in [(&mut self.a, 0), (&mut self.b, block / 2)] {
                    ring.frames.fill([0.0; 2]);
                    ring.at = at;
                    ring.peak = 0.0;
                }
            }
        }
        self.update();
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    /// `InitializeCompressor`: rings empty, gains 1, ring B half a block on.
    fn reset(&mut self) {
        self.a.clear(0);
        self.b.clear(self.block / 2);
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let (ia, ib) = (self.a.at, self.b.at);
        let (wa, wb) = (self.window[ia], self.window[ib]);
        let gains = self.a.gain + self.b.gain;
        for (c, x) in frame.iter_mut().enumerate() {
            let input = *x;
            *x = 0.5 * ((self.a.frames[ia][c] + self.b.frames[ib][c]) * gains);
            let sa = input * wa;
            self.a.peak = peak_of(self.a.peak, sa);
            self.a.frames[ia][c] = sa;
            let sb = input * wb;
            self.b.peak = peak_of(self.b.peak, sb);
            self.b.frames[ib][c] = sb;
        }
        self.a.at += 1;
        self.b.at += 1;
        let makeup = self.volume / self.threshold;
        if self.a.at >= self.block {
            Self::block_end(&mut self.a, self.attack, self.release, makeup, self.threshold);
        }
        if self.b.at >= self.block {
            Self::block_end(&mut self.b, self.attack, self.release, makeup, self.threshold);
        }
    }

    fn latency_frames(&self) -> u32 {
        self.block as u32
    }

    fn tail_seconds(&self) -> f32 {
        2.0 * self.block as f32 / self.sample_rate
    }
}

#[cfg(test)]
mod tests {
    use super::super::hp_delay_law::golden::hash_of;
    use super::*;

    fn hashes() -> [u64; 4] {
        let h = hash_of::<HpCompressor>;
        [
            h(44_100.0, 3.0, &[], &[]),
            h(44_100.0, 3.0, &[(COMPRESSION, -20.0), (VOLUME, -3.0)], &[(66_150, ATTACK, 10.0)]),
            h(44_100.0, 3.0, &[(COMPRESSION, -30.0), (ATTACK, 20.0), (RELEASE, 300.0)], &[(50_000, COMPRESSION, -10.0)]),
            h(48_000.0, 3.0, &[(COMPRESSION, -24.0)], &[(72_000, RELEASE, 100.0)]),
        ]
    }

    fn compressor(settings: &[(usize, f32)]) -> HpCompressor {
        let mut k = HpCompressor::new(44_100.0);
        for &(i, v) in settings {
            k.set_param(i, v);
        }
        k
    }

    /// The first output frame over `level`, for a step from silence to 0.5.
    fn arrival(k: &mut HpCompressor, level: f32) -> usize {
        (0..20_000)
            .position(|i| {
                let x = if i >= 1_000 { 0.5 } else { 0.0 };
                let mut f = [x, x];
                k.tick(&mut f);
                f[0].abs() > level
            })
            .unwrap()
    }

    #[test]
    fn look_ahead_sets_the_latency() {
        for (frames, latency) in [(128.0, 128), (512.0, 512), (1_024.0, 1_024), (2_048.0, 2_048), (301.0, 302)] {
            let mut k = compressor(&[(LOOK_AHEAD, frames)]);
            assert_eq!(Kernel::latency_frames(&k), latency);
            // The sound arrives that late (the window starts at zero, so it
            // is heard a few frames past the step).
            let at = arrival(&mut k, 1e-4) - 1_000;
            assert!(at >= latency as usize && at < latency as usize + latency as usize / 8, "{frames}: {at}");
        }
        let at_48 = HpCompressor::new(48_000.0);
        assert_eq!(Kernel::latency_frames(&compressor(&[])), 1_024);
        assert_eq!(Kernel::latency_frames(&at_48), 1_114);
    }

    #[test]
    fn look_ahead_sets_the_attack_steps() {
        // A steady level into compression with a 200 ms attack: the gain comes
        // down at the end of each ring's block, so it steps every half block.
        let attack = |frames: f32| {
            let mut k = compressor(&[(LOOK_AHEAD, frames), (COMPRESSION, -20.0), (VOLUME, -20.0), (ATTACK, 200.0)]);
            let out: Vec<f32> = (0..44_100)
                .map(|_| {
                    let mut f = [0.5, 0.5];
                    k.tick(&mut f);
                    f[0]
                })
                .collect();
            let latency = Kernel::latency_frames(&k) as usize;
            let moves: Vec<usize> = (latency + 1..out.len()).filter(|&i| out[i] != out[i - 1]).collect();
            let gaps: Vec<usize> = moves.windows(2).map(|w| w[1] - w[0]).collect();
            // Where the output first falls by half its way down (0.5 → 0.1).
            let half = out[latency..].iter().position(|v| *v < 0.3).unwrap();
            (gaps, half)
        };
        let (short, short_half) = attack(256.0);
        let (long, long_half) = attack(2_048.0);
        assert!(short.iter().take(50).all(|g| *g == 128), "{:?}", &short[..10]);
        assert!(long.iter().take(10).all(|g| *g == 1_024), "{:?}", &long[..5]);
        // The attack law is the original's, taken at the block rate, so a
        // long block's few big steps are down before the sound is heard; a
        // short one's many small ones take a quarter second.
        assert_eq!(long_half, 0);
        assert!((10_000..15_000).contains(&short_half), "{short_half}");
    }

    #[test]
    fn a_new_look_ahead_needs_no_allocation_and_keeps_going() {
        let mut k = compressor(&[]);
        let (a, w) = (k.a.frames.as_ptr(), k.window.as_ptr());
        for frames in [128.0, 2_048.0, 700.0, 1_024.0] {
            k.set_param(LOOK_AHEAD, frames);
            assert_eq!(k.a.frames.as_ptr(), a);
            assert_eq!(k.window.as_ptr(), w);
            assert!(k.window[k.block / 2] > 0.999 && k.window[0] == 0.0);
            assert_eq!(k.b.at, k.block / 2);
            // A steady sine comes back at its level once the rings refill.
            let mut peak = 0.0f32;
            for i in 0..3 * k.block {
                let x = 0.25 * (i as f32 * 0.05).sin();
                let mut f = [x, x];
                k.tick(&mut f);
                if i > 2 * k.block {
                    peak = peak.max(f[0].abs());
                }
            }
            assert!((peak - 0.25).abs() < 0.01, "{frames}: {peak}");
        }
    }

    #[test]
    fn defaults_render_as_the_original() {
        assert_eq!(hashes(), [17773462619412784512, 7585690021801195608, 1236947287853948128, 10550890209013170603]);
    }
}
