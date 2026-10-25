//! Ring mod: MultiGrainLive's ring modulator (`fx::shape::RingMod`), moved
//! here unchanged.
//!
//! Both sides are multiplied by one sine oscillator and blended with the dry
//! signal by the mix. The oscillator's frequency can be swept: the sweep adds
//! its hertz-per-second to an offset every sample, and when the swept
//! frequency leaves 0.01 Hz…45 % of the rate the offset snaps back to zero, so
//! a sweep repeats rather than sticking at an end. The phase advances after
//! each frame, from the frequency that frame used. The original also had
//! square, saw and triangle shapes, but nothing could select them from a
//! rack, so only the sine is here.

use crate::{Category, Descriptor, Group, Kernel, PadPair, Panel, ParamSpec};

const HZ: usize = 0;
const MIX: usize = 1;
const SWEEP: usize = 2;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("hz", "Frequency", 0.1, 8000.0, 220.0).log().unit("Hz").smooth(0.0),
    ParamSpec::new("mix", "Mix", 0.0, 1.0, 1.0).smooth(0.0),
    ParamSpec::new("sweep", "Sweep", -2000.0, 2000.0, 0.0).unit("Hz/s").smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "ring",
    code: 69,
    version: 1,
    label: "Ring mod",
    category: Category::Modulation,
    params: PARAMS,
    panel: Panel {
        groups: &[Group { label: "Oscillator", keys: &["hz", "sweep", "mix"] }],
        primary: &["hz", "mix"],
        displays: &[],
        pad: Some(PadPair { x: "hz", x_range: (0.1, 4_000.0), y: "mix", y_range: (0.0, 1.0) }),
    },
    source: "MultiGrainLive: Ring mod (fx::shape::RingMod)",
    sound: None,
};

pub struct Ring {
    sample_rate: f32,
    values: [f32; 3],
    phase: f32,
    swept: f32,
}

impl Kernel for Ring {
    fn new(sample_rate: f32) -> Self {
        let mut values = [0.0; 3];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        Ring { sample_rate: sample_rate.max(1.0), values, phase: 0.0, swept: 0.0 }
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(spec) = PARAMS.get(index) {
            self.values[index] = spec.clamp(value);
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.swept = 0.0;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let sr = self.sample_rate;
        let base = self.values[HZ];
        let mix = self.values[MIX].clamp(0.0, 1.0);
        let hz = (base + self.swept).clamp(0.01, sr * 0.45);
        let m = (std::f32::consts::TAU * self.phase).sin();
        for s in frame.iter_mut() {
            *s = *s * (1.0 - mix) + *s * m * mix;
        }
        self.phase = (self.phase + hz / sr).fract();
        self.swept += self.values[SWEEP] / sr;
        if (base + self.swept) > sr * 0.45 || (base + self.swept) < 0.01 {
            self.swept = 0.0;
        }
    }
}
