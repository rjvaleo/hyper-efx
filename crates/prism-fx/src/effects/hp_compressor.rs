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

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("volumeDb", "Volume", -96.0, 0.0, 0.0).unit("dB").smooth(0.0),
    ParamSpec::new("compressionDb", "Compression", -96.0, 0.0, 0.0).unit("dB").smooth(0.0),
    ParamSpec::new("attackMs", "Attack", 0.0, 1_000.0, 100.0).unit("ms").kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("releaseMs", "Release", 0.0, 1_000.0, 1_000.0).unit("ms").kind(ParamKind::Int).smooth(0.0),
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
            Group { label: "Settings", keys: &["attackMs", "releaseMs"] },
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
    values: [f32; 4],
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

impl HpCompressor {
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
        let mut values = [0.0; 4];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let block = 2 * scaled(512.0, sample_rate);
        let step = core::f64::consts::PI / block as f64;
        let window = (0..block)
            .map(|i| {
                let s = (i as f64 * step).sin();
                (s * s) as f32
            })
            .collect();
        let mut k = HpCompressor {
            sample_rate,
            values,
            block,
            window,
            a: Ring::new(block),
            b: Ring::new(block),
            volume: 1.0,
            threshold: 1.0,
            attack: (1.0, 0.0),
            release: (1.0, 0.0),
        };
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
