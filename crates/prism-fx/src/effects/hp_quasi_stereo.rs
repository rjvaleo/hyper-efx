//! Hyperprism Quasi Stereo: Hyperprism-PPC 1.2.1's process 24 (class
//! EQuasiStereo, routine `QuasiStereo`), as its code does it.
//!
//! The mid, ½(L + R), goes through a fixed delay and a 12 dB/octave
//! Butterworth high-pass, and the result, times the depth, is added to the
//! left and taken from the right. Above the low cut that makes complementary
//! combs on the two sides; the sum L + R is the input untouched, and the bass
//! stays in the middle.
//!
//! - **Delay.** `int(0.001 × ms × SR)` samples (1102 at 25 ms), exact: the
//!   write runs that far ahead of the read and happens first, so 0 ms is no
//!   delay. A new time moves the two pointers back to the start of the ring
//!   without clearing it, as the original does, so the ring's old contents
//!   play once. The ring is 1920 samples at 44.1 kHz, scaled with the rate.
//! - **High-pass.** The β/γ/α form with Q = 1/√2: w = 2π·fc/SR, β = ½(1 −
//!   sin w/√2)/(1 + sin w/√2), γ = (½ + β)cos w, α = ¼(½ + β + γ), and
//!   y = 2(αx − 2αx₁ + αx₂ + γy₁ − βy₂). The corner is held between
//!   0.00022675·SR and 0.46031·SR (10 Hz to 20.3 kHz at 44.1 kHz).
//! - **Smoothing.** α, β, γ and the depth glide separately by Hyperprism's
//!   smoothing: every 45th sample, `a·target + (1 − a)·value`.
//!
//! The original's mono path injects with the opposite sign — `x − d·y` on the
//! left and `x + d·y` on the right — and turns mono into stereo; Mono input
//! takes the left input as that mono signal. The window's low cut runs 0 to
//! 6000 Hz; the original would take typed values past it, to the 20.3 kHz
//! clamp.
//!
//! Not reproduced: the original's depth slot starts at 100 (×100) until the
//! window sends its start value; here the depth is always the control's.

use super::hp_stereo_shared::{scaled, Stepper};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const DEPTH: usize = 0;
const LOW_CUT: usize = 1;
const DELAY: usize = 2;
const SMOOTHING: usize = 3;
const MONO: usize = 4;

/// The ring at 44.1 kHz.
const RING: f32 = 1_920.0;
const LONGEST_MS: f32 = 40.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("depth", "Depth", 0.0, 100.0, 50.0).unit("%").smooth(0.0),
    ParamSpec::new("lowCutHz", "Low Cut", 0.0, 6_000.0, 50.0).unit("Hz").smooth(0.0),
    ParamSpec::new("delayMs", "Delay Time", 0.0, LONGEST_MS, 25.0).unit("ms").kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 50.0).unit("%").kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("mono", "Mono input", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-quasi-stereo",
    code: 59,
    version: 1,
    label: "Hyperprism Quasi Stereo",
    category: Category::Stereo,
    params: PARAMS,
    panel: Panel {
        // The original: the X/Y pad, depth across and low cut up, and the
        // delay time and smoothing sliders in its settings.
        groups: &[
            Group { label: "Width", keys: &["depth", "lowCutHz"] },
            Group { label: "Settings", keys: &["delayMs", "smoothing", "mono"] },
        ],
        primary: &["depth", "lowCutHz"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "depth", x_range: (0.0, 100.0), y: "lowCutHz", y_range: (0.0, 6_000.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Quasi Stereo",
    sound: None,
};

/// `(p × q + t)` rounded once to single, as the routine's multiply-adds are.
#[inline]
fn madd(p: f32, q: f32, t: f32) -> f32 {
    (p as f64 * q as f64 + t as f64) as f32
}

pub struct HpQuasiStereo {
    sample_rate: f32,
    values: [f32; 5],
    ring: Box<[f32]>,
    write: usize,
    read: usize,
    /// Targets: α, β, γ, depth.
    target: [f32; 4],
    smoothed: [f32; 4],
    /// The filter as the routine holds it: b0, b1, b2, a1, a2.
    coef: [f32; 5],
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
    step: Stepper,
}

impl HpQuasiStereo {
    fn delay(&self) -> usize {
        (0.001 * (self.values[DELAY] * self.sample_rate) as f64) as usize
    }

    /// `SetHighpassParameters`.
    fn high_pass(&mut self) {
        let sr = self.sample_rate as f64;
        let lo = (0.000_226_75 * sr) as f32 as f64;
        let hi = (0.460_31 * sr) as f32 as f64;
        let fc = self.values[LOW_CUT] as f64;
        let f = if lo <= fc { if hi < fc { hi } else { fc } } else { lo };
        let w = ((std::f64::consts::TAU * f) / sr) as f32 as f64;
        let s = w.sin() as f32 as f64;
        let c = w.cos() as f32 as f64;
        let root2 = 2f64.sqrt();
        let beta = (0.5 * ((1.0 - s / root2) / (1.0 + s / root2))) as f32;
        let half_beta = 0.5 + beta as f64;
        let gamma = (half_beta * c) as f32;
        let alpha = (0.25 * (half_beta + gamma as f64)) as f32;
        self.target[0] = alpha;
        self.target[1] = beta;
        self.target[2] = gamma;
    }

    fn depth(&mut self) {
        self.target[3] = (0.01 * self.values[DEPTH] as f64) as f32;
    }

    fn coefficients(&mut self) {
        let [alpha, beta, gamma, _] = self.smoothed;
        self.coef = [alpha, (-2.0 * alpha as f64) as f32, alpha, gamma, -beta];
    }

    /// The pointers back to the start of the ring, the write ahead by the delay.
    fn taps(&mut self) {
        self.write = self.delay();
        self.read = 0;
    }
}

impl Kernel for HpQuasiStereo {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 5];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let longest = (0.001 * (LONGEST_MS * sample_rate) as f64) as usize;
        let length = scaled(RING, sample_rate).max(longest + 1);
        let mut k = HpQuasiStereo {
            sample_rate,
            values,
            ring: vec![0.0; length].into_boxed_slice(),
            write: 0,
            read: 0,
            target: [0.0; 4],
            smoothed: [0.0; 4],
            coef: [0.0; 5],
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            step: Stepper::new(sample_rate, values[SMOOTHING]),
        };
        k.high_pass();
        k.depth();
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        let v = spec.clamp(value);
        let changed = v != self.values[index];
        self.values[index] = v;
        match index {
            DEPTH => self.depth(),
            LOW_CUT => self.high_pass(),
            DELAY if changed => self.taps(),
            SMOOTHING => self.step.set_smoothing(v),
            _ => {}
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.ring.fill(0.0);
        self.taps();
        self.smoothed = self.target;
        self.coefficients();
        (self.x1, self.x2, self.y1, self.y2) = (0.0, 0.0, 0.0, 0.0);
        self.step.restart();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.step.fires() {
            for (s, t) in self.smoothed.iter_mut().zip(self.target) {
                *s = self.step.toward(*s, t);
            }
            self.coefficients();
        }
        let mono = self.values[MONO] > 0.5;
        let (l, r) = (frame[0], frame[1]);
        self.ring[self.write] = if mono { l } else { (0.5 * (l + r) as f64) as f32 };
        let x0 = self.ring[self.read];
        let [b0, b1, b2, a1, a2] = self.coef;
        let t = (b1 as f64 * self.x1 as f64) as f32;
        let t = madd(b0, x0, t);
        let t = madd(b2, self.x2, t);
        let t = madd(a1, self.y1, t);
        let t = madd(a2, self.y2, t);
        let y = 2.0 * t;
        let wet = y as f64 * self.smoothed[3] as f64;
        if mono {
            frame[0] = -((wet - l as f64) as f32);
            frame[1] = (wet + l as f64) as f32;
        } else {
            frame[0] = (wet + l as f64) as f32;
            frame[1] = -((wet - r as f64) as f32);
        }
        (self.x2, self.x1, self.y2, self.y1) = (self.x1, x0, self.y1, y);
        self.write += 1;
        if self.write >= self.ring.len() {
            self.write = 0;
        }
        self.read += 1;
        if self.read >= self.ring.len() {
            self.read = 0;
        }
    }

    fn tail_seconds(&self) -> f32 {
        self.delay() as f32 / self.sample_rate + 0.1
    }
}
