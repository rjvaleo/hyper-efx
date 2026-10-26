//! Dattorro resonator: MultiGrainLive's `MusicalFilter` in its resonator
//! mode (`ShapeKind::DattorroResonator`), from Part 1 of Jon Dattorro's
//! "Effect Design", sample for sample.
//!
//! A constant-peak-gain biquad band-pass per side, its width the bandwidth in
//! hertz between the half-power points (`Q = centre / bandwidth`), then a
//! dry/wet blend. A non-finite filter output is passed on as silence.
//!
//! The gain control is the original's too, and as there it does nothing in
//! this mode: it is marked inert, so documents that store it keep opening.

use super::mgl_dattorro_common::{ring_seconds, spec, store, whole_rate, FilterMode, Musical};
use crate::{Category, Descriptor, Display, Group, Kernel, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[
    spec("frequency", "Frequency", 20.0, 20_000.0, 1000.0).log().unit("Hz"),
    spec("bandwidth", "Bandwidth", 1.0, 10_000.0, 200.0).log().unit("Hz"),
    // The original shares its table with the Regalia-Mitra EQ, so this has a
    // gain it never reads. Kept so documents that store it still open.
    spec("gainDb", "Gain", -24.0, 24.0, 6.0).unit("dB").inert(),
    spec("mix", "Dry / wet", 0.0, 1.0, 1.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "dattorro-resonator",
    code: 78,
    version: 1,
    label: "Dattorro resonator",
    category: Category::Filter,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Filter", keys: &["frequency", "bandwidth"] },
            Group { label: "Mix", keys: &["mix"] },
        ],
        primary: &["frequency", "bandwidth"],
        displays: &[Display::Curve],
        pad: None,
    },
    source: "MultiGrainLive: Dattorro resonator (dattorro.rs, MusicalFilter)",
    sound: None,
};

pub struct MglDattorroResonator {
    values: [f32; 4],
    filter: Musical,
}

impl MglDattorroResonator {
    fn apply(&mut self) {
        let f = &mut self.filter;
        f.frequency = self.values[0];
        f.bandwidth = self.values[1];
        f.gain_db = self.values[2];
        f.mix = self.values[3];
        f.update();
    }
}

impl Kernel for MglDattorroResonator {
    fn new(sample_rate: f32) -> Self {
        let mut k = MglDattorroResonator {
            values: [1000.0, 200.0, 6.0, 1.0],
            filter: Musical::new(FilterMode::Resonator, whole_rate(sample_rate)),
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
