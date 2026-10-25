//! All-pass diffuser: MultiGrainLive's `AllpassDiffuser`
//! (`ShapeKind::AllpassDiffuser`), the lattice all-pass of Part 1, section
//! 1.3.3 of Jon Dattorro's "Effect Design", on its own, sample for sample.
//!
//! One 125 ms line per side, read `delay` ms back (at least one sample) with
//! linear interpolation: `y = z − g·x`, and `x + g·y` goes into the line. The
//! output is `dry·x + wet·y`.

use super::mgl_dattorro_common::{ap_mod, spec, store, whole_rate, Delay};
use crate::{Category, Descriptor, Display, Group, Kernel, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[
    spec("delayMs", "Delay", 0.1, 100.0, 12.0).log().unit("ms"),
    spec("coefficient", "Coefficient", -0.999, 0.999, 0.625),
    spec("wet", "Wet", 0.0, 1.0, 1.0),
    spec("dry", "Dry", 0.0, 1.0, 0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "allpass-diffuser",
    code: 87,
    version: 1,
    label: "All-pass diffuser",
    category: Category::Reverb,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "All-pass", keys: &["delayMs", "coefficient"] },
            Group { label: "Mix", keys: &["wet", "dry"] },
        ],
        primary: &["delayMs", "coefficient"],
        displays: &[Display::Meter],
        pad: None,
    },
    source: "MultiGrainLive: All-pass diffuser (dattorro.rs, AllpassDiffuser)",
    sound: None,
};

pub struct MglAllpassDiffuser {
    p: [f32; 4],
    d: [Delay; 2],
    sr: u32,
}

impl Kernel for MglAllpassDiffuser {
    fn new(sample_rate: f32) -> Self {
        let sr = whole_rate(sample_rate);
        let n = (sr as usize / 8).max(2);
        MglAllpassDiffuser { p: [12.0, 0.625, 1.0, 0.0], d: [Delay::new(n), Delay::new(n)], sr }
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
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let delay = (self.p[0] * self.sr as f32 / 1000.0).max(1.0);
        for (ch, out) in frame.iter_mut().enumerate() {
            let x = *out;
            let y = ap_mod(&mut self.d[ch], x, self.p[1], delay);
            *out = x * self.p[3] + y * self.p[2];
        }
    }

    fn tail_seconds(&self) -> f32 {
        let g = self.p[1].abs().max(1e-3);
        (self.p[0] / 1000.0 * (0.001f32.ln() / g.ln()).ceil()).min(30.0)
    }
}
