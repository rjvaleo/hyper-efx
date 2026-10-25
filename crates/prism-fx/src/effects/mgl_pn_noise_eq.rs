//! Equalized PN noise: MultiGrainLive's `PnNoise` equalised
//! (`ShapeKind::PnNoiseEq`), from Part 3, section 8 of Jon Dattorro's
//! "Effect Design", sample for sample, register for register.
//!
//! A 24-bit shift register (taps 0, 1, 3, 4, never left at zero) clocked by
//! a phase that gains `clock / rate` a sample and steps the register each time
//! it passes one. The whole register, read as a number from −1 to 1, times the
//! level, is held between clocks and mixed into both sides alike.
//!
//! As in the original, a fresh one starts its register at 1 whatever the seed
//! control says; setting the seed, or a reset, starts it from the seed.
//!
//! The equaliser: each new value has a one-pole low-pass of the values
//! (coefficient 0.35) taken from it, and the difference is scaled by 1.35,
//! tilting the held steps' spectrum up.

use super::mgl_dattorro_common::{seeded, shift, spec, store, whole_rate};
use crate::{Category, Descriptor, Display, Group, Kernel, Panel, ParamSpec};

const SEED: usize = 3;

static PARAMS: &[ParamSpec] = &[
    spec("clockHz", "Clock", 1.0, 48000.0, 48000.0).log().unit("Hz"),
    spec("level", "Level", 0.0, 1.0, 0.1),
    spec("mix", "Mix", 0.0, 1.0, 0.25),
    spec("seed", "Seed", 1.0, 16777215.0, 1.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "pn-noise-eq",
    code: 85,
    version: 1,
    label: "Equalized PN noise",
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
    source: "MultiGrainLive: Equalized PN noise (dattorro.rs, PnNoise)",
    sound: None,
};

pub struct MglPnNoiseEq {
    p: [f32; 4],
    sr: u32,
    reg: u32,
    phase: f32,
    last: f32,
    lp: f32,
}

impl Kernel for MglPnNoiseEq {
    fn new(sample_rate: f32) -> Self {
        MglPnNoiseEq { p: [48000.0, 0.1, 0.25, 1.0], sr: whole_rate(sample_rate), reg: 1, phase: 0.0, last: 0.0, lp: 0.0 }
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    /// A new seed restarts the register at once, as the original's does.
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
        self.last = 0.0;
        self.lp = 0.0;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        self.phase += self.p[0] / self.sr.max(1) as f32;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
            self.reg = shift(self.reg);
            let mut n = (self.reg as f32 / 8388607.5) - 1.0;
            self.lp += 0.35 * (n - self.lp);
            n = (n - self.lp) * 1.35;
            self.last = n * self.p[1];
        }
        for x in frame.iter_mut() {
            *x = *x * (1.0 - self.p[2]) + self.last * self.p[2];
        }
    }
}
