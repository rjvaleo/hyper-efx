//! The pieces MultiGrainLive's Dattorro family shares, carried over from its
//! `fx/src/dattorro.rs` so the ports render what the originals render.
//!
//! The originals process interleaved blocks; everything here works a frame at
//! a time on two channels, with the same arithmetic in the same order. None of
//! the originals smooths a control: a value set between blocks applies from the
//! next frame on, and so it does here. Where an original worked a coefficient
//! out once per block from controls that cannot change inside a block, it is
//! worked out here when a control moves instead — the same numbers.
//!
//! MultiGrainLive keeps its rate as whole hertz, and several lengths come from
//! integer arithmetic on it, so the kernels keep it that way too.

use core::f32::consts::{PI, TAU};

/// The rate as MultiGrainLive holds it: whole hertz, at least one.
pub(crate) fn whole_rate(sample_rate: f32) -> u32 {
    if sample_rate.is_finite() {
        sample_rate.max(1.0).round() as u32
    } else {
        48_000
    }
}

/// The original's delay line: written at `w`, which also holds the oldest
/// sample, and read `delay` samples behind it with linear interpolation.
pub(crate) struct Delay {
    pub b: Vec<f32>,
    pub w: usize,
}

impl Delay {
    pub fn new(n: usize) -> Self {
        Delay { b: vec![0.0; n.max(2)], w: 0 }
    }

    #[inline]
    pub fn tap(&self, delay: f32) -> f32 {
        let n = self.b.len();
        let p = (self.w as f32 - delay).rem_euclid(n as f32);
        let i = p.floor() as usize;
        let f = p - i as f32;
        self.b[i % n] + (self.b[(i + 1) % n] - self.b[i % n]) * f
    }

    #[inline]
    pub fn push(&mut self, x: f32) {
        self.b[self.w] = x;
        self.w = (self.w + 1) % self.b.len();
    }

    #[inline]
    pub fn step(&mut self, x: f32) -> f32 {
        let y = self.b[self.w];
        self.push(x);
        y
    }

    pub fn clear(&mut self) {
        self.b.fill(0.0);
        self.w = 0;
    }
}

/// The lattice all-pass over a whole line.
#[inline]
pub(crate) fn ap(d: &mut Delay, x: f32, g: f32) -> f32 {
    let z = d.b[d.w];
    let y = z - g * x;
    d.push(x + g * y);
    y
}

/// The lattice all-pass read at a fractional delay.
#[inline]
pub(crate) fn ap_mod(d: &mut Delay, x: f32, g: f32, delay: f32) -> f32 {
    let z = d.tap(delay);
    let y = z - g * x;
    d.push(x + g * y);
    y
}

/// `TAU`, as the originals widen it for their f64 phase accumulators: the f32
/// constant cast up, not the f64 one.
pub(crate) const TAU_WIDE: f64 = TAU as f64;

/// One step of an LFO phase kept in f64, as the originals advance theirs.
#[inline]
pub(crate) fn advance(phase: f64, hz: f32, sr: u32) -> f64 {
    (phase + TAU_WIDE * hz as f64 / sr as f64).rem_euclid(TAU_WIDE)
}

// ---- the three musical filters (Part 1, sections 2-3) ---------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilterMode {
    Notch,
    Resonator,
    Regalia,
}

/// `MusicalFilter`: one biquad per side in transposed direct form II, with a
/// dry/wet blend after it.
pub(crate) struct Musical {
    pub mode: FilterMode,
    pub frequency: f32,
    pub bandwidth: f32,
    pub gain_db: f32,
    pub mix: f32,
    sr: u32,
    c: [f32; 5],
    z1: [f32; 2],
    z2: [f32; 2],
}

impl Musical {
    pub fn new(mode: FilterMode, sr: u32) -> Self {
        let mut m = Musical {
            mode,
            frequency: 1000.0,
            bandwidth: 200.0,
            gain_db: 6.0,
            mix: 1.0,
            sr,
            c: [0.0; 5],
            z1: [0.0; 2],
            z2: [0.0; 2],
        };
        m.update();
        m
    }

    /// Work the coefficients out again from the fields, as the original did at
    /// the top of every block.
    pub fn update(&mut self) {
        let fs = self.sr.max(1) as f32;
        let f = self.frequency.clamp(1.0, fs * 0.49);
        let q = (f / self.bandwidth.max(0.1)).max(0.025);
        let w = TAU * f / fs;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q);
        let (b0, b1, b2, a0, a1, a2) = match self.mode {
            FilterMode::Notch => (1.0, -2.0 * c, 1.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha),
            FilterMode::Resonator => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * c, 1.0 - alpha),
            FilterMode::Regalia => {
                let a = 10f32.powf(self.gain_db / 40.0);
                (1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a)
            }
        };
        self.c = [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0];
    }

    pub fn clear(&mut self) {
        self.z1 = [0.0; 2];
        self.z2 = [0.0; 2];
    }

    #[inline]
    pub fn run(&mut self, ch: usize, x: f32) -> f32 {
        let c = &self.c;
        let y = c[0] * x + self.z1[ch];
        self.z1[ch] = c[1] * x - c[3] * y + self.z2[ch];
        self.z2[ch] = c[2] * x - c[4] * y;
        let y = if y.is_finite() { y } else { 0.0 };
        x + (y - x) * self.mix
    }
}

/// The damping filter's coefficient: a one-pole low-pass at `cutoff`.
#[inline]
pub(crate) fn damping_coefficient(cutoff: f32, sr: u32) -> f32 {
    1.0 - (-TAU * cutoff / sr.max(1) as f32).exp()
}

// ---- moving delays (Part 2) ------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModMode {
    Chorus,
    Flanger,
    Vibrato,
}

/// `ModDelay`: the white chorus, flanger and vibrato. One 160 ms line per side
/// read by a sine LFO, the right side `stereo · π` ahead, with linear or
/// first-order all-pass interpolation and feedback into the line.
pub(crate) struct ModDelay {
    pub p: [f32; 8],
    buf: [Vec<f32>; 2],
    frames: usize,
    write: usize,
    phase: f64,
    ap_x: [f32; 2],
    ap_y: [f32; 2],
    sr: u32,
}

impl ModDelay {
    pub fn new(mode: ModMode, sr: u32) -> Self {
        let mut p = [14.0, 7.0, 0.55, 0.12, 0.8, 1.0, 0.5, 0.0];
        match mode {
            ModMode::Flanger => {
                p[0] = 1.5;
                p[1] = 1.2;
                p[3] = 0.65;
                p[4] = 1.0;
            }
            ModMode::Vibrato => {
                p[0] = 8.0;
                p[1] = 3.0;
                p[4] = 1.0;
                p[5] = 0.0;
            }
            ModMode::Chorus => {}
        }
        // The original's interleaved line, in frames.
        let frames = (sr as f32 * 0.16) as usize + 4;
        ModDelay {
            p,
            buf: [vec![0.0; frames], vec![0.0; frames]],
            frames,
            write: 0,
            phase: 0.0,
            ap_x: [0.0; 2],
            ap_y: [0.0; 2],
            sr,
        }
    }

    fn read(&mut self, ch: usize, delay: f32) -> f32 {
        let frames = self.frames;
        let pos = self.write as f32 - delay;
        let pos = pos.rem_euclid(frames as f32);
        let i = pos.floor() as usize;
        let frac = pos - i as f32;
        let x0 = self.buf[ch][i % frames];
        let x1 = self.buf[ch][(i + 1) % frames];
        if self.p[7] < 0.5 {
            x0 + (x1 - x0) * frac
        } else {
            let a = (1.0 - frac) / (1.0 + frac).max(1e-6);
            let y = a * (x0 - self.ap_y[ch]) + self.ap_x[ch];
            let y = if y.is_finite() { y.clamp(-8.0, 8.0) } else { 0.0 };
            self.ap_x[ch] = x1;
            self.ap_y[ch] = y;
            y
        }
    }

    pub fn tick(&mut self, frame: &mut [f32; 2]) {
        let sr = self.sr as f32;
        let ph = self.phase as f32;
        for (ch, out) in frame.iter_mut().enumerate() {
            let phase = ph + self.p[6] * ch as f32 * PI;
            let d = ((self.p[0] + self.p[1] * phase.sin()).max(0.05) * sr / 1000.0).min((self.frames - 2) as f32);
            let wet = self.read(ch, d);
            let x = *out;
            self.buf[ch][self.write] = x + wet * self.p[3];
            *out = x * self.p[5] + wet * self.p[4];
        }
        self.write = (self.write + 1) % self.frames;
        self.phase = advance(self.phase, self.p[2], self.sr);
    }

    pub fn clear(&mut self) {
        for b in &mut self.buf {
            b.fill(0.0);
        }
        self.write = 0;
        self.phase = 0.0;
        self.ap_x = [0.0; 2];
        self.ap_y = [0.0; 2];
    }
}

// ---- PN noise (Part 3, section 8) ------------------------------------------------

/// The 24-bit register's next state: taps 0, 1, 3 and 4, fed in at the top,
/// never left at zero.
#[inline]
pub(crate) fn shift(reg: u32) -> u32 {
    let bit = (reg ^ (reg >> 1) ^ (reg >> 3) ^ (reg >> 4)) & 1;
    let reg = (reg >> 1) | (bit << 23);
    if reg == 0 {
        1
    } else {
        reg
    }
}

/// The register a seed control starts.
#[inline]
pub(crate) fn seeded(seed: f32) -> u32 {
    (seed as u32).max(1) & 0xff_ffff
}

// ---- windowed pitch shifting (Part 2, 4.4 and 6) ---------------------------------

/// `PitchCore`: two taps half a window apart sweeping a half-second line per
/// side, crossfaded by a raised cosine.
pub(crate) struct PitchCore {
    d: [Delay; 2],
    phase: f64,
    sr: u32,
}

impl PitchCore {
    pub fn new(sr: u32) -> Self {
        let n = (sr as usize / 2).max(8);
        PitchCore { d: [Delay::new(n), Delay::new(n)], phase: 0.25, sr }
    }

    pub fn frame(&mut self, input: &[f32; 2], out: &mut [f32; 2], ratio: f32, window_ms: f32) {
        let win = (window_ms * self.sr as f32 / 1000.0).clamp(32.0, (self.d[0].b.len() - 2) as f32);
        for ch in 0..2 {
            self.d[ch].push(input[ch]);
            let p = self.phase as f32;
            let p2 = (p + 0.5).fract();
            let a = self.d[ch].tap(1.0 + p * win);
            let b = self.d[ch].tap(1.0 + p2 * win);
            let wa = 0.5 - 0.5 * (TAU * p).cos();
            let wb = 1.0 - wa;
            out[ch] = a * wa + b * wb;
        }
        self.phase = (self.phase + (1.0 - ratio) as f64 / win as f64).rem_euclid(1.0);
    }

    pub fn clear(&mut self) {
        for d in &mut self.d {
            d.clear();
        }
        self.phase = 0.25;
    }
}

/// Set `values[index]` from `value` clamped by its spec, and say whether the
/// index was one.
#[inline]
pub(crate) fn store(params: &[crate::ParamSpec], values: &mut [f32], index: usize, value: f32) -> bool {
    match params.get(index) {
        Some(spec) => {
            values[index] = spec.clamp(value);
            true
        }
        None => false,
    }
}

/// A control as the originals have it: no glide, because none of them glides.
pub(crate) const fn spec(key: &'static str, label: &'static str, min: f32, max: f32, default: f32) -> crate::ParamSpec {
    crate::ParamSpec::new(key, label, min, max, default).smooth(0.0)
}

/// How long a biquad with this bandwidth rings, to 60 dB down.
pub(crate) fn ring_seconds(bandwidth_hz: f32) -> f32 {
    (6.91 / (PI * bandwidth_hz.max(0.1))).min(30.0)
}
