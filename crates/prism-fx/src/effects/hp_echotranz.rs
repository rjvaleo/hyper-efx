//! Hyperprism EchoTranz: Hyperprism-PPC 1.2.1's EchoTranz, as its code does
//! it (class EDelay, process 2, routine `Delay`: 1.0x's Echo under a new
//! name).
//!
//! One stereo line read at a gliding, fractional time. Every sample the time
//! takes the one-pole step `d ← a·t + b·d` twice, `b = 1 − a`, so it covers
//! `1 − b²` of the way each sample, and is held to at least one sample. The
//! read is linear between the two samples around `d`, so a moving time is
//! heard as tape-like transposition, by `1 − Δd` per sample: at speed 75,
//! halving a second's delay starts at about 1.44 × the pitch.
//!
//! "Param. Speed Ch." sets the pole: `a = 1e-8 × int(10^(4v/100))`, from
//! 1e-8 at 1 % to 9.12e-5 at 99 % (a 1134 s to 0.12 s time constant at
//! 44.1 kHz; 11.3 s at the default 50 %), and at 100 % the time jumps.
//!
//! `out = in·(1 − mix) + mix·y`, and the line takes `in + feedback·p`, where
//! `p` is the whole-sample tap below the read, not the interpolated `y`. Mix
//! and feedback are not smoothed. Both sides use the same time. The
//! arithmetic is the original's, fused multiply-adds included; at rates other
//! than 44.1 kHz the pole is scaled so each glide takes the same time.
//!
//! Defaults: Param. Speed Ch. 50 % and Mix 50 % are the window's slider
//! values, which the original reads as its defaults; the edit fields beside
//! them show 100 and 0.
//!
//! Changed from the original: feedback stops at 100 % (the original's field
//! went to 32 767 %; with the whole-sample loop, 100 % holds without running
//! away). Settings made before the first frame land at once, as the
//! original's initialisation copies the target time into the glide.

use super::hp_delay_law::{samples, speed, Line};
use super::hp_echo::{fraction, LONGEST_MS};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const DELAY: usize = 0;
const FEEDBACK: usize = 1;
const MIX: usize = 2;
const SPEED: usize = 3;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("delayMs", "Delay", 0.0, LONGEST_MS, 1000.0).unit("ms").smooth(0.0),
    ParamSpec::new("feedback", "Feedback", 0.0, 100.0, 0.0).unit("%").smooth(0.0),
    ParamSpec::new("mix", "Mix", 0.0, 100.0, 50.0).unit("%").smooth(0.0),
    ParamSpec::new("speed", "Param. Speed Ch.", 1.0, 100.0, 50.0)
        .unit("%")
        .kind(ParamKind::Int)
        .smooth(0.0)
        .glides("delayMs"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-echotranz",
    code: 42,
    version: 1,
    label: "Hyperprism EchoTranz",
    category: Category::Delay,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Pad", keys: &["delayMs", "feedback"] },
            Group { label: "Additional settings", keys: &["speed", "mix"] },
        ],
        primary: &["delayMs", "feedback", "mix"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "delayMs", x_range: (0.0, LONGEST_MS), y: "feedback", y_range: (0.0, 100.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: EchoTranz",
    sound: None,
};

pub struct HpEchoTranz {
    sample_rate: f32,
    values: [f32; 4],
    line: Line,
    /// The time the read is at, in samples (the original's double at +0x78).
    time: f64,
    /// The time it glides to (`p[0]`).
    target: f32,
    a: f64,
    b: f64,
    feedback: f32,
    mix: f32,
    dry: f32,
    started: bool,
}

impl HpEchoTranz {
    fn apply(&mut self) {
        self.target = samples(self.values[DELAY], self.sample_rate);
        (self.a, self.b) = speed(self.values[SPEED], self.sample_rate);
        self.feedback = fraction(self.values[FEEDBACK]);
        self.mix = fraction(self.values[MIX]);
        self.dry = (1.0 - self.mix as f64) as f32;
        if !self.started {
            self.time = self.target as f64;
        }
    }
}

impl Kernel for HpEchoTranz {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 4];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let longest = (samples(LONGEST_MS, sample_rate) as usize).max(1) + 1;
        let mut k = HpEchoTranz {
            sample_rate,
            values,
            line: Line::new(longest),
            time: 0.0,
            target: 0.0,
            a: 1.0,
            b: 0.0,
            feedback: 0.0,
            mix: 0.0,
            dry: 0.0,
            started: false,
        };
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        self.values[index] = spec.clamp(value);
        self.apply();
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.line.clear();
        self.started = false;
        self.apply();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        self.started = true;
        let (a, b, t) = (self.a, self.b, self.target as f64);
        let mut d = a.mul_add(t, b * a.mul_add(t, b * self.time));
        if d < 1.0 {
            d = 1.0;
        }
        self.time = d;
        let whole = d as i32;
        let f = (d - (whole as f32) as f64) as f32;
        let p = self.line.at(whole as usize);
        let older = self.line.at(whole as usize + 1);
        let mut written = [0.0f32; 2];
        for (ch, s) in frame.iter_mut().enumerate() {
            let x = *s;
            let y = f.mul_add(older[ch] - p[ch], p[ch]);
            *s = x.mul_add(self.dry, y * self.mix);
            written[ch] = self.feedback.mul_add(p[ch], x);
        }
        self.line.push(written);
    }

    /// Until the repeats are 60 dB down, or a minute where the loop holds.
    fn tail_seconds(&self) -> f32 {
        let longest = (self.time as f32).max(self.target) / self.sample_rate;
        let gain = self.feedback;
        if gain >= 1.0 {
            return 60.0;
        }
        if gain <= 0.0 {
            return longest;
        }
        (longest * (0.001f32.ln() / gain.ln()).ceil()).min(60.0)
    }
}
