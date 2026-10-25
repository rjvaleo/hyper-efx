//! Damping filter: MultiGrainLive's `DampingFilter`
//! (`ShapeKind::DampingFilter`), the one-pole low-pass Jon Dattorro's plate
//! damps with ("Effect Design", Part 1), on its own, sample for sample.
//!
//! `z += a·(x − z)` per side with `a = 1 − e^(−2π·fc/fs)`; the output is
//! `dry·x + wet·z`.

use super::mgl_dattorro_common::{damping_coefficient, spec, store, whole_rate};
use crate::{Category, Descriptor, Display, Group, Kernel, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[
    spec("cutoffHz", "Cutoff", 20.0, 20000.0, 6000.0).log().unit("Hz"),
    spec("wet", "Wet", 0.0, 1.0, 1.0),
    spec("dry", "Dry", 0.0, 1.0, 0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "damping-filter",
    code: 88,
    version: 1,
    label: "Damping filter",
    category: Category::Filter,
    params: PARAMS,
    panel: Panel {
        groups: &[Group { label: "Filter", keys: &["cutoffHz"] }, Group { label: "Mix", keys: &["wet", "dry"] }],
        primary: &["cutoffHz"],
        displays: &[Display::Curve],
        pad: None,
    },
    source: "MultiGrainLive: Damping filter (dattorro.rs, DampingFilter)",
    sound: None,
};

pub struct MglDampingFilter {
    p: [f32; 3],
    sr: u32,
    a: f32,
    z: [f32; 2],
}

impl Kernel for MglDampingFilter {
    fn new(sample_rate: f32) -> Self {
        let sr = whole_rate(sample_rate);
        MglDampingFilter { p: [6000.0, 1.0, 0.0], sr, a: damping_coefficient(6000.0, sr), z: [0.0; 2] }
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if store(PARAMS, &mut self.p, index, value) {
            self.a = damping_coefficient(self.p[0], self.sr);
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.p.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.z = [0.0; 2];
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        for (ch, out) in frame.iter_mut().enumerate() {
            let x = *out;
            self.z[ch] += self.a * (x - self.z[ch]);
            *out = x * self.p[2] + self.z[ch] * self.p[1];
        }
    }

    fn tail_seconds(&self) -> f32 {
        (1.1 / self.p[0]).min(1.0)
    }
}
