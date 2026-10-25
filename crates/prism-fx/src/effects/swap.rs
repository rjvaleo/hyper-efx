//! Swap: MultiGrainLive's channel swap (`fx::shape::SwapChannels`), moved
//! here unchanged.
//!
//! Left and right trade places. A signal that is the same on both sides is
//! untouched.

use crate::{Category, Descriptor, Kernel, Panel};

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "swap",
    code: 66,
    version: 1,
    label: "Swap",
    category: Category::Stereo,
    params: &[],
    panel: Panel { groups: &[], primary: &[], displays: &[], pad: None },
    source: "MultiGrainLive: Swap (fx::shape::SwapChannels)",
    sound: None,
};

pub struct Swap;

impl Kernel for Swap {
    fn new(_sample_rate: f32) -> Self {
        Swap
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
        frame.swap(0, 1);
    }
}
