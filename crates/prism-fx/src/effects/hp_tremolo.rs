//! Hyperprism Tremolo: Hyperprism-PPC 1.2.1's Tremolo process (EModulation,
//! process 5), as its code does it.
//!
//! The RingMod routine over a 1024-entry raised-cosine table (`0.5·(1 − cos)`,
//! read with linear interpolation): the gain is `1 − d + d·lfo`, from `1 − d`
//! up to 1, starting at the bottom of the cycle. One phase serves both sides,
//! so there is no auto-pan.
//!
//! - **Pad.** X is the frequency, 0–4 Hz (a table step of `v·1024/SR` a
//!   sample); Y is the depth, 0–100 % (`d = v·0.01`). Both linear, from the
//!   window's edit fields, starting at 2 Hz and 0 %.
//! - **Smoothing.** The additional setting, 0–100 %, default 50 % (the
//!   window's slider): `a = int(10^(0.04·(100 − s)))·0.0001`. Every 45th
//!   sample rate and depth each step `v ← a·target + (1 − a)·v`, and hold
//!   between steps. The count runs on the kernel's own samples.
//!
//! Changed from the original: the stereo loop's dry term was `(d − 1)·x`, so
//! stereo below full depth was inverted and quieter than mono; both sides now
//! take the mono law. The phase wrapped at 1023, not 1024, running 1024/1023
//! fast and reading one entry past the table at the wrap; it wraps at 1024
//! and the last step leads back to the first entry.

use super::hp_mod_common::{increment, one_m_cos_table, RingMod, Stepper};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const RATE: usize = 0;
const DEPTH: usize = 1;
const SMOOTHING: usize = 2;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("rateHz", "Frequency", 0.0, 4.0, 2.0).unit("Hz").smooth(100.0),
    ParamSpec::new("depth", "Depth", 0.0, 100.0, 0.0).unit("%").smooth(100.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 50.0).unit("%").kind(ParamKind::Int).smooth(0.0).glides("rateHz"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-tremolo",
    code: 52,
    version: 1,
    label: "Hyperprism Tremolo",
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
    source: "Hyperprism-PPC 1.2.1: Tremolo",
    sound: None,
};

pub struct HpTremolo {
    sample_rate: f32,
    values: [f32; 3],
    ring: RingMod,
    steps: Stepper,
}

impl HpTremolo {
    fn targets(&self) -> (f32, f32) {
        (increment(self.values[RATE], self.sample_rate), self.values[DEPTH] * 0.01)
    }
}

impl Kernel for HpTremolo {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 3];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpTremolo {
            sample_rate,
            values,
            ring: RingMod::new(one_m_cos_table()),
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
        self.ring.clear();
        (self.ring.rate, self.ring.depth) = self.targets();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.steps.due() {
            let (rate, depth) = self.targets();
            self.ring.rate = self.steps.step(self.ring.rate, rate);
            self.ring.depth = self.steps.step(self.ring.depth, depth);
        }
        self.ring.tick(frame);
    }
}
