//! White chorus: MultiGrainLive's `ModDelay` in its chorus mode
//! (`ShapeKind::WhiteChorus`), from Part 2 of Jon Dattorro's "Effect
//! Design", sample for sample.
//!
//! One 160 ms line per side, read `delay + depth·sin(φ)` ms back (never under
//! 0.05 ms) by a sine LFO whose phase runs in f64; the right side reads
//! `stereo·π` further round. The read is linear, or with the all-pass control
//! at 0.5 or more a first-order all-pass interpolator, clamped to ±8 and
//! reset to silence if it goes non-finite. What is read goes back into the
//! line times the feedback, and out times wet, beside the dry input.

use super::mgl_dattorro_common::{spec, store, whole_rate, ModDelay, ModMode};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

static PARAMS: &[ParamSpec] = &[
    spec("delayMs", "Delay", 0.1, 100.0, 14.0).log().unit("ms"),
    spec("depthMs", "Depth", 0.0, 50.0, 7.0).unit("ms"),
    spec("rateHz", "Rate", 0.01, 20.0, 0.55).log().unit("Hz"),
    spec("feedback", "Feedback", -0.98, 0.98, 0.12),
    spec("wet", "Wet", 0.0, 1.0, 0.8),
    spec("dry", "Dry", 0.0, 1.0, 1.0),
    spec("stereo", "Stereo phase", 0.0, 1.0, 0.5),
    spec("allpass", "All-pass interp", 0.0, 1.0, 0.0).kind(ParamKind::Toggle),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "white-chorus",
    code: 81,
    version: 1,
    label: "White chorus",
    category: Category::Modulation,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Delay", keys: &["delayMs", "feedback", "allpass"] },
            Group { label: "LFO", keys: &["rateHz", "depthMs", "stereo"] },
            Group { label: "Mix", keys: &["wet", "dry"] },
        ],
        primary: &["rateHz", "depthMs", "feedback"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "rateHz", x_range: (0.01, 10.0), y: "depthMs", y_range: (0.0, 20.0) }),
    },
    source: "MultiGrainLive: White chorus (dattorro.rs, ModDelay)",
    sound: None,
};

pub struct MglWhiteChorus(ModDelay);

impl Kernel for MglWhiteChorus {
    fn new(sample_rate: f32) -> Self {
        MglWhiteChorus(ModDelay::new(ModMode::Chorus, whole_rate(sample_rate)))
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        store(PARAMS, &mut self.0.p, index, value);
    }

    fn param(&self, index: usize) -> f32 {
        self.0.p.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.0.clear();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        self.0.tick(frame);
    }

    /// The longest delay, until the feedback has died 60 dB.
    fn tail_seconds(&self) -> f32 {
        let p = &self.0.p;
        let longest = (p[0] + p[1]) / 1000.0;
        let g = p[3].abs();
        if g <= 1e-3 {
            longest
        } else {
            (longest * (0.001f32.ln() / g.ln()).ceil()).min(30.0)
        }
    }
}
