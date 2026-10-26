//! Invert: MultiGrainLive's polarity flip (`fx::shape::Invert`), moved here
//! unchanged.
//!
//! Every sample is negated, on both sides. On its own it sounds like nothing;
//! against a copy of the same signal it cancels it.

use crate::{Category, Descriptor, Kernel, Panel};

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "invert",
    code: 65,
    version: 1,
    label: "Invert",
    category: Category::Utility,
    params: &[],
    panel: Panel { groups: &[], primary: &[], displays: &[], pad: None },
    source: "MultiGrainLive: Invert (fx::shape::Invert)",
    sound: None,
};

pub struct Invert;

impl Kernel for Invert {
    fn new(_sample_rate: f32) -> Self {
        Invert
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, _index: usize, _value: f32) {}

    fn param(&self, _index: usize) -> f32 {
        0.0
    }

    fn reset(&mut self) {}

    fn tick(&mut self, frame: &mut [f32; 2]) {
        for s in frame.iter_mut() {
            *s = -*s;
        }
    }
}
