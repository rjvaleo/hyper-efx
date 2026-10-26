//! Regalia-Mitra EQ: MultiGrainLive's `MusicalFilter` in its boost/cut
//! mode (`ShapeKind::Regalia`), from Part 1 of Jon Dattorro's "Effect
//! Design", sample for sample.
//!
//! The original swaps the Regalia-Mitra lattice for the peaking biquad with
//! the same normalised response (`A = 10^(gain/40)`), which sidesteps the
//! lattice's fixed-point truncation worry. Bandwidth in hertz becomes
//! `Q = centre / bandwidth`; a dry/wet blend follows. A non-finite filter
//! output is passed on as silence.

use super::mgl_dattorro_common::{ring_seconds, spec, store, whole_rate, FilterMode, Musical};
use crate::{Category, Descriptor, Display, Group, Kernel, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[
    spec("frequency", "Frequency", 20.0, 20_000.0, 1000.0).log().unit("Hz"),
    spec("bandwidth", "Bandwidth", 1.0, 10_000.0, 200.0).log().unit("Hz"),
    spec("gainDb", "Gain", -24.0, 24.0, 6.0).unit("dB"),
    spec("mix", "Dry / wet", 0.0, 1.0, 1.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "regalia",
    code: 79,
    version: 1,
    label: "Regalia-Mitra EQ",
    category: Category::Filter,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Filter", keys: &["frequency", "bandwidth", "gainDb"] },
            Group { label: "Mix", keys: &["mix"] },
        ],
        primary: &["frequency", "bandwidth"],
        displays: &[Display::Curve],
        pad: None,
    },
    source: "MultiGrainLive: Regalia-Mitra EQ (dattorro.rs, MusicalFilter)",
    sound: None,
};

pub struct MglRegalia {
    values: [f32; 4],
    filter: Musical,
}

impl MglRegalia {
    fn apply(&mut self) {
        let f = &mut self.filter;
        f.frequency = self.values[0];
        f.bandwidth = self.values[1];
        f.gain_db = self.values[2];
        f.mix = self.values[3];
        f.update();
    }
}

impl Kernel for MglRegalia {
    fn new(sample_rate: f32) -> Self {
        let mut k = MglRegalia {
            values: [1000.0, 200.0, 6.0, 1.0],
            filter: Musical::new(FilterMode::Regalia, whole_rate(sample_rate)),
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
