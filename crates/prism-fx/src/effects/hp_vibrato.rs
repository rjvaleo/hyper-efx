//! Hyperprism Vibrato: Hyperprism-PPC 1.2.1's Vibrato process (EModulation,
//! process 4), as its code does it.
//!
//! The Modulation routine wet only (wet 1, dry 0, no feedback): a line per
//! side read `d = D·0.5·(1 − cos)` frames back from a 1024-entry raised-cosine
//! table, linearly interpolated, both sides in phase, the LFO starting at the
//! bottom (no delay).
//!
//! - **Depth.** Y sets `D = v / P0`, where `P0 = f·1024/SR` is the rate's
//!   table step: `D = v·SR/(1024·f)` samples, at most 16 384 (371 ms). The
//!   steepest slope of the delay is `D·π·f/SR = π·v/1024`, so Y is a pitch
//!   deviation the rate does not change: ±3.07 % at v = 10. The original's
//!   pad sends X before Y, so D is taken against the new rate; a move of
//!   either control here does the same. Rate and D are smoothed separately,
//!   so a rate move briefly bends the deviation, as in the original.
//! - **Pad.** X 0–4 Hz, Y 0–10 "%", both linear, starting at 2 Hz and 0.
//! - **Smoothing.** The additional setting, 0–100 %, default 50 % (the
//!   window's slider): `a = int(10^(0.04·(100 − s)))·0.0001`, a step of rate
//!   and D every 45th sample.
//!
//! Changed from the original: at rate 0 and depth 0, `v / P0` was 0/0 — a NaN
//! that passed the 16 384 cap, stuck in the smoother and sent the read
//! outside the line. Depth 0 is no delay at any rate.

use super::hp_mod_common::{increment, samples, Modulation, Stepper};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const RATE: usize = 0;
const DEPTH: usize = 1;
const SMOOTHING: usize = 2;

/// The longest swing, in samples at 44.1 kHz.
const MOST: f64 = 16_384.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("rateHz", "Frequency", 0.0, 4.0, 2.0).unit("Hz").smooth(100.0),
    ParamSpec::new("depth", "Depth", 0.0, 10.0, 0.0).unit("%").smooth(100.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 50.0).unit("%").kind(ParamKind::Int).smooth(0.0).glides("rateHz"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-vibrato",
    code: 51,
    version: 1,
    label: "Hyperprism Vibrato",
    category: Category::Modulation,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Pad", keys: &["rateHz", "depth"] },
            Group { label: "Additional settings", keys: &["smoothing"] },
        ],
        primary: &["rateHz", "depth"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "rateHz", x_range: (0.0, 4.0), y: "depth", y_range: (0.0, 10.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Vibrato",
    sound: None,
};

pub struct HpVibrato {
    sample_rate: f32,
    values: [f32; 3],
    modulation: Modulation,
    steps: Stepper,
}

impl HpVibrato {
    fn targets(&self) -> (f32, f32) {
        let rate = increment(self.values[RATE], self.sample_rate);
        let v = self.values[DEPTH];
        let most = samples(MOST, self.sample_rate) as f32;
        let swing = if v == 0.0 { 0.0 } else { ((v as f64 / rate as f64) as f32).min(most) };
        (rate, swing)
    }
}

impl Kernel for HpVibrato {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 3];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpVibrato {
            sample_rate,
            values,
            modulation: Modulation::new(sample_rate, 1.0, 0.0, 0.0),
            steps: Stepper::new(sample_rate, 44.0, 44.0, values[SMOOTHING]),
        };
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        self.values[index] = spec.clamp(value);
        if index == SMOOTHING {
            self.steps.set_percent(self.values[SMOOTHING]);
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.steps.reset();
        self.modulation.clear();
        (self.modulation.rate, self.modulation.depth) = self.targets();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.steps.due() {
            let (rate, depth) = self.targets();
            self.modulation.rate = self.steps.step(self.modulation.rate, rate);
            self.modulation.depth = self.steps.step(self.modulation.depth, depth);
        }
        self.modulation.tick(frame);
    }
}
