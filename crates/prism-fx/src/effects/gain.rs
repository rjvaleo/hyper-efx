//! Gain: MultiGrainLive's rack gain slot (`fx::Gain`), moved here unchanged.
//!
//! Both sides are multiplied by 10^(dB/20); at 0 dB the input passes
//! untouched. The slot had no parameter table of its own, so this one is
//! assembled from where MultiGrainLive keeps its pieces: the key `db`, the
//! range `GAIN_DB_MIN..GAIN_DB_MAX` (±24 dB, which its `set_param` clamps
//! to), a default of 0 dB as a document without one reads, and "Level", the
//! name its automation lane goes by. (A document could store up to ±48 dB,
//! which the original would play; here such a value lands at ±24.)

use crate::{Category, Descriptor, Group, Kernel, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[ParamSpec::new("db", "Level", -24.0, 24.0, 0.0).unit("dB").smooth(0.0)];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "gain",
    code: 76,
    version: 1,
    label: "Gain",
    category: Category::Utility,
    params: PARAMS,
    panel: Panel {
        groups: &[Group { label: "Level", keys: &["db"] }],
        primary: &["db"],
        displays: &[],
        pad: None,
    },
    source: "MultiGrainLive: Gain (fx::Gain)",
    sound: None,
};

pub struct Gain {
    db: f32,
    g: f32,
}

impl Kernel for Gain {
    fn new(_sample_rate: f32) -> Self {
        let mut k = Gain { db: 0.0, g: 1.0 };
        k.set_param(0, PARAMS[0].default);
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(spec) = PARAMS.get(index) {
            self.db = spec.clamp(value);
            self.g = 10f32.powf(self.db / 20.0);
        }
    }

    fn param(&self, index: usize) -> f32 {
        if index == 0 {
            self.db
        } else {
            0.0
        }
    }

    fn reset(&mut self) {}

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if (self.g - 1.0).abs() < 1e-9 {
            return;
        }
        for s in frame.iter_mut() {
            *s *= self.g;
        }
    }
}
