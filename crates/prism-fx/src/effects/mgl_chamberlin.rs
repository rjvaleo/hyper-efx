//! Chamberlin filter: MultiGrainLive's `Chamberlin` (`ShapeKind::Chamberlin`),
//! the state-variable filter from Part 1 of Jon Dattorro's "Effect Design",
//! sample for sample.
//!
//! The input is driven into `tanh`, then four state-variable filters — low,
//! band, high and notch, each with its own switch, frequency, Q and level —
//! run side by side on it. Each one iterates twice per sample with
//! `f = 2·sin(π·fc / 2·fs)` (at most 0.99, the centre at most 0.45·fs) and a
//! damping of `2·(1 − r)` held inside `2/f − f/2`, where `r` is the Q mapped
//! from 0.2–10 onto 0–1. The levels sum, and the sum goes through `tanh`.
//! A filter that is off, or at a level of 0.0001 or less, is skipped and keeps
//! its state.
//!
//! The original works `f` and the damping out every sample from controls that
//! only change between blocks; here they are worked out when a control moves.

use super::mgl_dattorro_common::{spec, store, whole_rate};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamSpec};
use core::f32::consts::PI;

static PARAMS: &[ParamSpec] = &[
    spec("lowOn", "Low on", 0.0, 1.0, 1.0),
    spec("lowFreq", "Low frequency", 20.0, 18_000.0, 1000.0).log().unit("Hz"),
    spec("lowQ", "Low Q", 0.2, 10.0, 0.7),
    spec("lowAmp", "Low amplitude", 0.0, 1.0, 1.0),
    spec("bandOn", "Band on", 0.0, 1.0, 0.0),
    spec("bandFreq", "Band frequency", 20.0, 18_000.0, 1000.0).log().unit("Hz"),
    spec("bandQ", "Band Q", 0.2, 10.0, 1.0),
    spec("bandAmp", "Band amplitude", 0.0, 1.0, 0.0),
    spec("highOn", "High on", 0.0, 1.0, 0.0),
    spec("highFreq", "High frequency", 20.0, 18_000.0, 1000.0).log().unit("Hz"),
    spec("highQ", "High Q", 0.2, 10.0, 0.7),
    spec("highAmp", "High amplitude", 0.0, 1.0, 0.0),
    spec("notchOn", "Notch on", 0.0, 1.0, 0.0),
    spec("notchFreq", "Notch frequency", 20.0, 18_000.0, 1000.0).log().unit("Hz"),
    spec("notchQ", "Notch Q", 0.2, 10.0, 1.0),
    spec("notchAmp", "Notch amplitude", 0.0, 1.0, 0.0),
    spec("drive", "Drive", 0.25, 8.0, 1.0).log().unit("x"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "chamberlin",
    code: 80,
    version: 1,
    label: "Chamberlin filter",
    category: Category::Filter,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Low", keys: &["lowOn", "lowFreq", "lowQ", "lowAmp"] },
            Group { label: "Band", keys: &["bandOn", "bandFreq", "bandQ", "bandAmp"] },
            Group { label: "High", keys: &["highOn", "highFreq", "highQ", "highAmp"] },
            Group { label: "Notch", keys: &["notchOn", "notchFreq", "notchQ", "notchAmp"] },
            Group { label: "Drive", keys: &["drive"] },
        ],
        primary: &["lowFreq", "lowQ", "drive"],
        displays: &[Display::Curve],
        pad: Some(PadPair { x: "lowFreq", x_range: (20.0, 18_000.0), y: "lowQ", y_range: (0.2, 10.0) }),
    },
    source: "MultiGrainLive: Chamberlin filter (dattorro.rs, Chamberlin)",
    sound: None,
};

/// One filter's coefficients, as the original works them out.
#[derive(Clone, Copy, Default)]
struct Mode {
    on: bool,
    f: f32,
    damp: f32,
    amp: f32,
}

pub struct MglChamberlin {
    p: [f32; 17],
    sr: u32,
    modes: [Mode; 4],
    low: [[f32; 2]; 4],
    band: [[f32; 2]; 4],
}

impl MglChamberlin {
    fn apply(&mut self) {
        let sr = self.sr.max(1) as f32;
        for (mode, m) in self.modes.iter_mut().enumerate() {
            let base = mode * 4;
            let p = &self.p;
            m.on = !(p[base] < 0.5 || p[base + 3] <= 0.0001);
            m.f = (2.0 * (PI * p[base + 1].min(sr * 0.45) / (2.0 * sr)).sin()).min(0.99);
            let resonance = ((p[base + 2] - 0.2) / 9.8).clamp(0.0, 1.0);
            m.damp = (2.0 * (1.0 - resonance).max(0.02)).min(2.0 / m.f - m.f * 0.5);
            m.amp = p[base + 3];
        }
    }
}

impl Kernel for MglChamberlin {
    fn new(sample_rate: f32) -> Self {
        let mut k = MglChamberlin {
            p: [
                1.0, 1000.0, 0.7, 1.0, 0.0, 1000.0, 1.0, 0.0, 0.0, 1000.0, 0.7, 0.0, 0.0, 1000.0, 1.0, 0.0, 1.0,
            ],
            sr: whole_rate(sample_rate),
            modes: [Mode::default(); 4],
            low: [[0.0; 2]; 4],
            band: [[0.0; 2]; 4],
        };
        k.apply();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if store(PARAMS, &mut self.p, index, value) {
            self.apply();
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.p.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.low = [[0.0; 2]; 4];
        self.band = [[0.0; 2]; 4];
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        for (ch, out) in frame.iter_mut().enumerate() {
            let x = (*out * self.p[16]).tanh();
            let mut sum = 0.0;
            for (mode, m) in self.modes.iter().enumerate() {
                if !m.on {
                    continue;
                }
                let (low, band) = (&mut self.low[mode][ch], &mut self.band[mode][ch]);
                let mut hi = 0.0;
                for _ in 0..2 {
                    *low += m.f * *band;
                    hi = x - *low - m.damp * *band;
                    *band += m.f * hi;
                }
                let value = match mode {
                    0 => *low,
                    1 => *band,
                    2 => hi,
                    _ => hi + *low,
                };
                sum += value * m.amp;
            }
            *out = sum.tanh();
        }
    }

    fn tail_seconds(&self) -> f32 {
        0.5
    }
}
