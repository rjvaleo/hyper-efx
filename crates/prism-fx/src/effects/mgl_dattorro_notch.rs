//! Dattorro notch: MultiGrainLive's `MusicalFilter` in its notch mode
//! (`ShapeKind::DattorroNotch`), from Part 1 of Jon Dattorro's "Effect
//! Design", sample for sample.
//!
//! A biquad notch per side whose width is the musical bandwidth in hertz
//! between the half-power points, turned into a Q as `centre / bandwidth`,
//! followed by a dry/wet blend. A non-finite filter output is passed on as
//! silence, as the original does.
//!
//! The gain control is the original's too, and as there it does nothing in
//! this mode: it is marked inert, so documents that store it keep opening.

use super::mgl_dattorro_common::{ring_seconds, spec, store, whole_rate, FilterMode, Musical};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[
    spec("frequency", "Frequency", 20.0, 20_000.0, 1000.0).log().unit("Hz"),
    spec("bandwidth", "Bandwidth", 1.0, 10_000.0, 200.0).log().unit("Hz"),
    // The original shares its table with the Regalia-Mitra EQ, so this has a
    // gain it never reads. Kept so documents that store it still open.
    spec("gainDb", "Gain", -24.0, 24.0, 6.0).unit("dB").inert(),
    spec("mix", "Dry / wet", 0.0, 1.0, 1.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "dattorro-notch",
    code: 77,
    version: 1,
    label: "Dattorro notch",
    category: Category::Filter,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Filter", keys: &["frequency", "bandwidth"] },
            Group { label: "Mix", keys: &["mix"] },
        ],
        primary: &["frequency", "bandwidth"],
        displays: &[Display::Curve],
        pad: Some(PadPair { x: "frequency", x_range: (20.0, 2_000.0), y: "bandwidth", y_range: (10.0, 2_000.0) }),
    },
    source: "MultiGrainLive: Dattorro notch (dattorro.rs, MusicalFilter)",
    sound: None,
};

pub struct MglDattorroNotch {
    values: [f32; 4],
    filter: Musical,
}

impl MglDattorroNotch {
    fn apply(&mut self) {
        let f = &mut self.filter;
        f.frequency = self.values[0];
        f.bandwidth = self.values[1];
        f.gain_db = self.values[2];
        f.mix = self.values[3];
        f.update();
    }
}

impl Kernel for MglDattorroNotch {
    fn new(sample_rate: f32) -> Self {
        let mut k = MglDattorroNotch {
            values: [1000.0, 200.0, 6.0, 1.0],
            filter: Musical::new(FilterMode::Notch, whole_rate(sample_rate)),
        };
        k.apply();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if store(PARAMS, &mut self.values, index, value) {
            self.apply();
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.filter.clear();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        frame[0] = self.filter.run(0, frame[0]);
        frame[1] = self.filter.run(1, frame[1]);
    }

    fn tail_seconds(&self) -> f32 {
        ring_seconds(self.values[1])
    }
}
