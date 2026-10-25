//! Detune: MultiGrainLive's `Detune` (`ShapeKind::Detune`), two of the
//! windowed pitch shifters of Part 2 of Jon Dattorro's "Effect Design" pulled
//! opposite ways, sample for sample.
//!
//! Two shifters, one up by `cents` and one down by as much (ratio `r` and
//! `1/r`, `r = 2^(cents/1200)`), each the harmonizer's two raised-cosine taps
//! on a half-second line. In stereo the left hears the one going up and the
//! right the one going down; the output is `dry·x + wet·shifted`.

use super::mgl_dattorro_common::{spec, store, whole_rate, PitchCore};
use crate::{Category, Descriptor, Display, Group, Kernel, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[
    spec("cents", "Detune", 1.0, 100.0, 12.0).log().unit("ct"),
    spec("windowMs", "Window", 10.0, 250.0, 80.0).log().unit("ms"),
    spec("wet", "Wet", 0.0, 1.0, 0.65),
    spec("dry", "Dry", 0.0, 1.0, 1.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "detune",
    code: 92,
    version: 1,
    label: "Detune",
    category: Category::Pitch,
    params: PARAMS,
    panel: Panel {
        groups: &[Group { label: "Detune", keys: &["cents", "windowMs"] }, Group { label: "Mix", keys: &["wet", "dry"] }],
        primary: &["cents", "windowMs"],
        displays: &[Display::Meter],
        pad: None,
    },
    source: "MultiGrainLive: Detune (dattorro.rs, Detune)",
    sound: None,
};

pub struct MglDetune {
    p: [f32; 4],
    ratio: f32,
    up: PitchCore,
    down: PitchCore,
}

impl Kernel for MglDetune {
    fn new(sample_rate: f32) -> Self {
        let sr = whole_rate(sample_rate);
        MglDetune { p: [12.0, 80.0, 0.65, 1.0], ratio: 2f32.powf(12.0 / 1200.0), up: PitchCore::new(sr), down: PitchCore::new(sr) }
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if store(PARAMS, &mut self.p, index, value) {
            self.ratio = 2f32.powf(self.p[0] / 1200.0);
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.p.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.up.clear();
        self.down.clear();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let input = *frame;
        let (mut u, mut d) = ([0.0; 2], [0.0; 2]);
        self.up.frame(&input, &mut u, self.ratio, self.p[1]);
        self.down.frame(&input, &mut d, 1.0 / self.ratio, self.p[1]);
        frame[0] = frame[0] * self.p[3] + u[0] * self.p[2];
        frame[1] = frame[1] * self.p[3] + d[1] * self.p[2];
    }

    fn tail_seconds(&self) -> f32 {
        self.p[1] / 1000.0
    }
}
