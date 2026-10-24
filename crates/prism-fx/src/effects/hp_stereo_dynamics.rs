//! Hyperprism Stereo Dynamics: Hyperprism-PPC 1.2.1's process 20 (class
//! EDynamics, routine `Balance`), as its code does it.
//!
//! The window calls its axes "Right-Left Gain" and "Right+Left Gain", which
//! read as side and mid, but the code does no mid/side arithmetic at all: X is
//! the right channel's gain and Y the left channel's, linear, 0–100 % of
//! unity. It is the Pan's routine with the overall gain left at 1. Both start
//! at 50 %, so the effect starts 6 dB down. The labels here say what the
//! controls do rather than repeat those captions.
//!
//! The gains glide by Hyperprism's smoothing: every 45th sample, `a·target +
//! (1 − a)·value`, a from the Smoothing slider (50 % is a = 0.01, about
//! 101 ms). A mono input came out as the same two gains on the one channel,
//! which equal sides give here. The original would take typed gains up to
//! 32767 %; the range here is the window's.

use super::hp_stereo_shared::Stepper;
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const RIGHT: usize = 0;
const LEFT: usize = 1;
const SMOOTHING: usize = 2;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("rightGain", "Right gain", 0.0, 100.0, 50.0).unit("%").smooth(0.0),
    ParamSpec::new("leftGain", "Left gain", 0.0, 100.0, 50.0).unit("%").smooth(0.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 50.0).unit("%").kind(ParamKind::Int).smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-stereo-dynamics",
    code: 58,
    version: 1,
    label: "Hyperprism Stereo Dynamics",
    category: Category::Stereo,
    params: PARAMS,
    panel: Panel {
        // The original: the X/Y pad, right gain across and left gain up, and
        // the smoothing slider in its settings.
        groups: &[
            Group { label: "Gains", keys: &["rightGain", "leftGain"] },
            Group { label: "Settings", keys: &["smoothing"] },
        ],
        primary: &["rightGain", "leftGain"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "rightGain", x_range: (0.0, 100.0), y: "leftGain", y_range: (0.0, 100.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Stereo Dynamics",
    sound: None,
};

pub struct HpStereoDynamics {
    values: [f32; 3],
    /// Targets: left gain, right gain, and the overall gain, which this
    /// process leaves at 1 but still glides like the others.
    target: [f32; 3],
    smoothed: [f32; 3],
    step: Stepper,
}

impl HpStereoDynamics {
    fn targets(&mut self) {
        let gain = |percent: f32| (percent as f64 * 0.01f32 as f64) as f32;
        self.target = [gain(self.values[LEFT]), gain(self.values[RIGHT]), 1.0];
    }
}

impl Kernel for HpStereoDynamics {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 3];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let step = Stepper::new(sample_rate, values[SMOOTHING]);
        let mut k = HpStereoDynamics { values, target: [0.0; 3], smoothed: [0.0; 3], step };
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
