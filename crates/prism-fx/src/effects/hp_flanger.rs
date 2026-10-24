//! Hyperprism Flanger: Hyperprism-PPC 1.2.1's Flanger process (EModulation,
//! process 2), as its code does it.
//!
//! The Modulation routine: one delay line per side (32 768 frames at
//! 44.1 kHz), read `d = depth·0.5·(1 − cos)` frames back from a 1024-entry
//! raised-cosine table, linearly between the frames `int(d)` and `int(d) + 1`
//! back. The sweep runs from exactly 0 — the newest sample, the dry sound
//! itself — up to the depth, and both sides sweep in phase.
//!
//! - **Feedback.** The line takes `x + 0.25·w`, where `w` is the previous
//!   sample's wet sound, so the loop is one sample longer than the delay.
//! - **Mix.** `0.5·wet + 0.5·dry`.
//! - **Pad.** X is the frequency, 0–4 Hz (`v·1024/SR` table steps a sample);
//!   Y is the depth, 0–100 samples at 44.1 kHz (the same time at other
//!   rates). Both linear, from the window, starting at 2 Hz and 0.
//! - **Smoothing.** The additional setting, 0–100 %, default 50 % (the
//!   window's slider): `a = int(10^(0.04·(100 − s)))·0.0001`, and every 45th
//!   sample rate and depth step `v ← a·target + (1 − a)·v`.

use super::hp_mod_common::{increment, samples, Modulation, Stepper};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const RATE: usize = 0;
const DEPTH: usize = 1;
const SMOOTHING: usize = 2;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("rateHz", "Frequency", 0.0, 4.0, 2.0).unit("Hz").smooth(100.0),
    ParamSpec::new("depth", "Depth", 0.0, 100.0, 0.0).unit("samples").smooth(100.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 50.0).unit("%").kind(ParamKind::Int).smooth(0.0).glides("rateHz"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-flanger",
    code: 49,
    version: 1,
    label: "Hyperprism Flanger",
    category: Category::Modulation,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Pad", keys: &["rateHz", "depth"] },
            Group { label: "Additional settings", keys: &["smoothing"] },
        ],
        primary: &["rateHz", "depth"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "rateHz", x_range: (0.0, 4.0), y: "depth", y_range: (0.0, 100.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Flanger",
    sound: None,
};

pub struct HpFlanger {
    sample_rate: f32,
    values: [f32; 3],
    modulation: Modulation,
    steps: Stepper,
}

impl HpFlanger {
    fn targets(&self) -> (f32, f32) {
        let depth = samples(self.values[DEPTH] as f64, self.sample_rate) as f32;
        (increment(self.values[RATE], self.sample_rate), depth)
    }
}

impl Kernel for HpFlanger {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 3];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpFlanger {
            sample_rate,
            values,
            modulation: Modulation::new(sample_rate, 0.5, 0.5, 0.25),
            steps: Stepper::new(sample_rate, 44.0, 44.0, values[SMOOTHING]),
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
        if index == SMOOTHING {
            self.steps.set_percent(self.values[SMOOTHING]);
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.steps.reset();
        self.modulation.clear();
        (self.modulation.rate, self.modulation.depth) = self.targets();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.steps.due() {
            let (rate, depth) = self.targets();
            self.modulation.rate = self.steps.step(self.modulation.rate, rate);
            self.modulation.depth = self.steps.step(self.modulation.depth, depth);
        }
        self.modulation.tick(frame);
    }

    fn tail_seconds(&self) -> f32 {
        // Each pass loses three quarters; a few passes of at most 2.3 ms.
        0.05
    }
}
