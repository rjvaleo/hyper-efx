//! Leslie speaker: MultiGrainLive's `Leslie` (`ShapeKind::Leslie`), a rotating
//! horn after Part 2 of Jon Dattorro's "Effect Design", sample for sample.
//!
//! The two sides are averaged into one 50 ms line. Each side reads it at
//! `distance + doppler·(1 + osc)` ms, where `osc` is the rotor's sine pushed
//! `∓width·π/2` apart for left and right, and is scaled by
//! `1 − am + am·(0.5 + 0.5·osc)`. The rotor's phase runs in f64. As in the
//! original, the longest settings ask for more than the line holds and read
//! round it.

use super::mgl_dattorro_common::{advance, spec, store, whole_rate, Delay};
use crate::{Category, Descriptor, Display, Group, Kernel, Panel, ParamSpec};
use core::f32::consts::PI;

static PARAMS: &[ParamSpec] = &[
    spec("rateHz", "Rate", 0.1, 12.0, 1.2).log().unit("Hz"),
    spec("depthMs", "Doppler", 0.0, 8.0, 2.0).unit("ms"),
    spec("amDepth", "Amplitude", 0.0, 1.0, 0.35),
    spec("width", "Stereo width", 0.0, 1.0, 1.0),
    spec("wet", "Wet", 0.0, 1.0, 1.0),
    spec("dry", "Dry", 0.0, 1.0, 0.0),
    spec("baseMs", "Distance", 0.5, 40.0, 3.0).unit("ms"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "leslie",
    code: 89,
    version: 1,
    label: "Leslie speaker",
    category: Category::Modulation,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Rotor", keys: &["rateHz", "depthMs", "amDepth", "baseMs"] },
            Group { label: "Output", keys: &["width", "wet", "dry"] },
        ],
        primary: &["rateHz", "depthMs", "amDepth"],
        displays: &[Display::Meter],
        pad: None,
    },
    source: "MultiGrainLive: Leslie speaker (dattorro.rs, Leslie)",
    sound: None,
};

pub struct MglLeslie {
    p: [f32; 7],
    d: Delay,
    phase: f64,
    sr: u32,
}

impl Kernel for MglLeslie {
    fn new(sample_rate: f32) -> Self {
        let sr = whole_rate(sample_rate);
        MglLeslie { p: [1.2, 2.0, 0.35, 1.0, 1.0, 0.0, 3.0], d: Delay::new((sr as usize / 20).max(8)), phase: 0.0, sr }
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
        self.d.clear();
        self.phase = 0.0;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let p = self.p;
        self.d.push((frame[0] + frame[1]) / 2.0);
        let ph = self.phase as f32;
        for (ch, out) in frame.iter_mut().enumerate() {
            let side = if ch == 0 { -1.0 } else { 1.0 };
            let osc = (ph + side * p[3] * PI * 0.5).sin();
            let delay = (p[6] + p[1] * (1.0 + osc)) * self.sr as f32 / 1000.0;
            let y = self.d.tap(delay) * (1.0 - p[2] + p[2] * (0.5 + 0.5 * osc));
            *out = *out * p[5] + y * p[4];
        }
        self.phase = advance(self.phase, p[0], self.sr);
    }

    fn tail_seconds(&self) -> f32 {
        0.06
    }
}
