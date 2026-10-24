//! Hyperprism Pan: Hyperprism-PPC 1.2.1's process 21, "Balance" (class
//! EDynamics, routine `Balance`), as its code does it.
//!
//! A balance, not a pan: each input channel stays on its own side and is
//! scaled by a constant-power law, `gL = cos θ`, `gR = sin θ` with
//! θ = π/4 · (1 + balance / 100). The centre is therefore 0.707 on both
//! sides, 3 dB down. The gain then follows in whole decibels — the original
//! truncates the dial toward zero before 10^(dB/20), so −6.7 is −6.
//!
//! The three gains glide by Hyperprism's own smoothing: on every 45th sample
//! each moves `a·target + (1 − a)·value`, a = 0.0001 × int(10^(0.04 × (100 −
//! Smoothing))), and holds in between (Smoothing 50 % is a = 0.01, a time
//! constant of about 101 ms). A mono input in the original came out as the
//! same balance of the one channel, which is what a stereo input with equal
//! sides gives here.
//!
//! The pad is the original's X/Y window: balance across, −100 to 100 %, and
//! gain up, −96 to 0 dB. The original would also take typed balances past
//! ±100 %, which turn θ past 0…π/2 and invert a channel; the range here is
//! the window's.

use super::hp_stereo_shared::Stepper;
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const BALANCE: usize = 0;
const GAIN_DB: usize = 1;
const SMOOTHING: usize = 2;

/// The original's π/4: a double a little short of the true value.
const QUARTER_PI: f64 = 0.785_398_141_75;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("balance", "LR Balance", -100.0, 100.0, 0.0).unit("%").smooth(0.0),
    ParamSpec::new("gainDb", "Gain", -96.0, 0.0, 0.0).unit("dB").smooth(0.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 50.0).unit("%").kind(ParamKind::Int).smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-pan",
    code: 56,
    version: 1,
    label: "Hyperprism Pan",
    category: Category::Stereo,
    params: PARAMS,
    panel: Panel {
        // The original: the X/Y pad, balance across and gain up, and the
        // smoothing slider in its settings.
        groups: &[
            Group { label: "Pan", keys: &["balance", "gainDb"] },
            Group { label: "Settings", keys: &["smoothing"] },
        ],
        primary: &["balance", "gainDb"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "balance", x_range: (-100.0, 100.0), y: "gainDb", y_range: (-96.0, 0.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Pan",
    sound: None,
};

pub struct HpPan {
    values: [f32; 3],
    /// Targets: left gain, right gain, overall gain.
    target: [f32; 3],
    smoothed: [f32; 3],
    step: Stepper,
}

impl HpPan {
    fn targets(&mut self) {
        let theta = QUARTER_PI * (1.0 + (self.values[BALANCE] as f64 * 0.01f32 as f64) as f32 as f64);
        self.target[0] = theta.cos() as f32;
        self.target[1] = theta.sin() as f32;
        self.target[2] = 10f64.powf(self.values[GAIN_DB] as i32 as f64 / 20.0) as f32;
    }
}

impl Kernel for HpPan {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 3];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpPan { values, target: [0.0; 3], smoothed: [0.0; 3], step: Stepper::new(sample_rate, values[SMOOTHING]) };
        k.targets();
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        self.values[index] = spec.clamp(value);
        match index {
            SMOOTHING => self.step.set_smoothing(self.values[SMOOTHING]),
            _ => self.targets(),
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.smoothed = self.target;
        self.step.restart();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.step.fires() {
            for (s, t) in self.smoothed.iter_mut().zip(self.target) {
                *s = self.step.toward(*s, t);
            }
        }
        let [gl, gr, g] = self.smoothed;
        frame[0] = g * (frame[0] * gl);
        frame[1] = g * (frame[1] * gr);
    }
}
