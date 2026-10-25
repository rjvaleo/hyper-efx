//! Width: MultiGrainLive's stereo width (`fx::shape::Width`), moved here
//! unchanged.
//!
//! The pair is split into mid and side, the side is scaled, and the two are
//! put back together. At 0 both channels carry the mid only; at 1 the input
//! comes back as it went in; up to 2 the side is pushed beyond the recording,
//! which spreads the image and thins whatever sat in the centre. Changes land
//! at once, as in the original.

use crate::{Category, Descriptor, Group, Kernel, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[ParamSpec::new("width", "Width", 0.0, 2.0, 1.0).unit("x").smooth(0.0)];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "width",
    code: 67,
    version: 1,
    label: "Width",
    category: Category::Stereo,
    params: PARAMS,
    panel: Panel {
        groups: &[Group { label: "Width", keys: &["width"] }],
        primary: &["width"],
        displays: &[],
        pad: None,
    },
    source: "MultiGrainLive: Width (fx::shape::Width)",
    sound: None,
};

pub struct Width {
    width: f32,
}

impl Kernel for Width {
    fn new(_sample_rate: f32) -> Self {
        Width { width: PARAMS[0].default }
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(spec) = PARAMS.get(index) {
            self.width = spec.clamp(value);
        }
    }

    fn param(&self, index: usize) -> f32 {
        if index == 0 {
            self.width
        } else {
            0.0
        }
    }

    fn reset(&mut self) {}

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let w = self.width.clamp(0.0, 2.0);
        let (l, r) = (frame[0], frame[1]);
        let mid = (l + r) * 0.5;
        let side = (l - r) * 0.5 * w;
        frame[0] = mid + side;
        frame[1] = mid - side;
    }
}
