//! DC: MultiGrainLive's DC offset remover (`fx::shape::DcOffset`), moved here
//! unchanged.
//!
//! Each side runs a one-pole low pass at the chosen corner (1–60 Hz) and has
//! that low pass taken away from it — a gentle high pass, so an offset that
//! drifts is followed rather than assumed constant. The coefficient is
//! 2π·f/rate, capped at 0.5.

use crate::{Category, Descriptor, Group, Kernel, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[ParamSpec::new("hz", "Below", 1.0, 60.0, 5.0).log().unit("Hz").smooth(0.0)];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "dc",
    code: 68,
    version: 1,
    label: "DC",
    category: Category::Filter,
    params: PARAMS,
    panel: Panel {
        groups: &[Group { label: "Filter", keys: &["hz"] }],
        primary: &["hz"],
        displays: &[],
        pad: None,
    },
    source: "MultiGrainLive: DC (fx::shape::DcOffset)",
    sound: None,
};

pub struct Dc {
    sample_rate: f32,
    hz: f32,
    k: f32,
    state: [f32; 2],
}

impl Dc {
    fn derive(&mut self) {
        self.k = (std::f32::consts::TAU * self.hz.clamp(1.0, 60.0) / self.sample_rate).min(0.5);
    }
}

impl Kernel for Dc {
    fn new(sample_rate: f32) -> Self {
        let mut k = Dc { sample_rate: sample_rate.max(1.0), hz: PARAMS[0].default, k: 0.0, state: [0.0; 2] };
        k.derive();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(spec) = PARAMS.get(index) {
            self.hz = spec.clamp(value);
            self.derive();
        }
    }

    fn param(&self, index: usize) -> f32 {
        if index == 0 {
            self.hz
        } else {
            0.0
        }
    }

    fn reset(&mut self) {
        self.state = [0.0; 2];
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        for (s, state) in frame.iter_mut().zip(self.state.iter_mut()) {
            *state += self.k * (*s - *state);
            *s -= *state;
        }
    }
}
