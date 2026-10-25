//! Single-bit PN noise: MultiGrainLive's `SingleBitPn`
//! (`ShapeKind::SingleBitPn`), from Part 3, section 8 of Jon Dattorro's
//! "Effect Design", sample for sample, register for register.
//!
//! The multibit generator's 24-bit register (taps 0, 1, 3, 4, never zero) and
//! clock, but only its lowest bit is heard: `±level`, held between clocks and
//! mixed into both sides alike.
//!
//! As in the original, a fresh one holds `+0.1` until its first clock however
//! the level is then set, and starts its register at 1 whatever the seed
//! control says; setting the seed restarts the register from it, and a reset
//! restarts it and holds `+level`.

use super::mgl_dattorro_common::{seeded, shift, spec, store, whole_rate};
use crate::{Category, Descriptor, Display, Group, Kernel, Panel, ParamSpec};

const SEED: usize = 3;

static PARAMS: &[ParamSpec] = &[
    spec("clockHz", "Clock", 1.0, 48000.0, 48000.0).log().unit("Hz"),
    spec("level", "Level", 0.0, 1.0, 0.1),
    spec("mix", "Mix", 0.0, 1.0, 1.0),
    spec("seed", "Seed", 1.0, 16777215.0, 1.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "single-bit-pn",
    code: 90,
    version: 1,
    label: "Single-bit PN noise",
    category: Category::Utility,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Noise", keys: &["clockHz", "level", "seed"] },
            Group { label: "Mix", keys: &["mix"] },
        ],
        primary: &["clockHz", "level", "mix"],
        displays: &[Display::Spectrum],
        pad: None,
    },
    source: "MultiGrainLive: Single-bit PN noise (dattorro.rs, SingleBitPn)",
    sound: None,
};

pub struct MglSingleBitPn {
    p: [f32; 4],
    sr: u32,
    reg: u32,
    phase: f32,
    last: f32,
}

impl Kernel for MglSingleBitPn {
    fn new(sample_rate: f32) -> Self {
        MglSingleBitPn { p: [48000.0, 0.1, 1.0, 1.0], sr: whole_rate(sample_rate), reg: 1, phase: 0.0, last: 0.1 }
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if store(PARAMS, &mut self.p, index, value) && index == SEED {
            self.reg = seeded(self.p[SEED]);
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.p.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.reg = seeded(self.p[SEED]);
        self.phase = 0.0;
        self.last = self.p[1];
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        self.phase += self.p[0] / self.sr.max(1) as f32;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
            self.reg = shift(self.reg);
            self.last = if self.reg & 1 == 0 { -self.p[1] } else { self.p[1] };
        }
        for x in frame.iter_mut() {
            *x = *x * (1.0 - self.p[2]) + self.last * self.p[2];
        }
    }
}
