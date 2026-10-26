//! Doubler: MultiGrainLive's `Doubler` (`ShapeKind::Doubler`), the
//! double-tracking delay of Part 2 of Jon Dattorro's "Effect Design", sample
//! for sample.
//!
//! One 200 ms line per side, read `delay + variation·sin(φ + 1.7·side)` ms
//! back (at least 1 ms) by a slow sine whose phase runs in f64; the output is
//! `dry·x + wet·delayed`.

use super::mgl_dattorro_common::{advance, spec, store, whole_rate, Delay};
use crate::{Category, Descriptor, Display, Group, Kernel, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[
    spec("delayMs", "Delay", 5.0, 100.0, 32.0).log().unit("ms"),
    spec("variationMs", "Variation", 0.0, 20.0, 3.0).unit("ms"),
    spec("rateHz", "Rate", 0.05, 5.0, 0.35).log().unit("Hz"),
    spec("wet", "Wet", 0.0, 1.0, 0.65),
    spec("dry", "Dry", 0.0, 1.0, 1.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "doubler",
    code: 93,
    version: 1,
    label: "Doubler",
    category: Category::Delay,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Delay", keys: &["delayMs", "variationMs", "rateHz"] },
            Group { label: "Mix", keys: &["wet", "dry"] },
        ],
        primary: &["delayMs", "variationMs"],
        displays: &[Display::Meter],
        pad: None,
    },
    source: "MultiGrainLive: Doubler (dattorro.rs, Doubler)",
    sound: None,
};

pub struct MglDoubler {
    p: [f32; 5],
    d: [Delay; 2],
    phase: f64,
    sr: u32,
}

impl Kernel for MglDoubler {
    fn new(sample_rate: f32) -> Self {
        let sr = whole_rate(sample_rate);
        let n = (sr as usize / 5).max(8);
        MglDoubler { p: [32.0, 3.0, 0.35, 0.65, 1.0], d: [Delay::new(n), Delay::new(n)], phase: 0.0, sr }
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        store(PARAMS, &mut self.p, index, value);
    }

    fn param(&self, index: usize) -> f32 {
        self.p.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        for d in &mut self.d {
            d.clear();
        }
        self.phase = 0.0;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        for (ch, out) in frame.iter_mut().enumerate() {
            let x = *out;
            let phase = self.phase as f32 + ch as f32 * 1.7;
            let delay = (self.p[0] + self.p[1] * phase.sin()).max(1.0) * self.sr as f32 / 1000.0;
            let y = self.d[ch].tap(delay);
            self.d[ch].push(x);
            *out = x * self.p[4] + y * self.p[3];
        }
        self.phase = advance(self.phase, self.p[2], self.sr);
    }

    fn tail_seconds(&self) -> f32 {
        (self.p[0] + self.p[1]) / 1000.0
    }
}
