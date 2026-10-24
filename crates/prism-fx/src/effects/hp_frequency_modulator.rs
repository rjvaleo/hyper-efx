//! Hyperprism Frequency Modulator: Hyperprism-PPC 1.2.1's FrequencyMod
//! routine (EModulation, process index 6), as its code does it. 1.2.1's
//! factory never reaches it; 1.05's menu had it as Frequency Modulator.
//!
//! The input frequency-modulates a raised-cosine oscillator (a 1024-entry
//! `0.5·(1 − cos)` table, linearly interpolated) whose level follows the
//! input's envelope. Per side:
//!
//! ```text
//! env   = 0.001·|x| + 0.999·env
//! y     = 1.4 · env · lfo(phase)
//! phase += x·depth + rate
//! ```
//!
//! The input itself is not passed through, and the unipolar carrier leaves a
//! DC component of `0.7·env`. The phase step takes the input in Hyperprism's
//! 16-bit units (±32 768), so a full-scale input at depth 10 % moves the
//! carrier about 1.4 kHz.
//!
//! - **Pad.** X is the carrier, 0–1000 Hz (`v·1024/SR` table steps); Y the
//!   depth, 0–10 % (`v·1e-4` table steps per 16-bit unit). Both linear,
//!   starting at 500 Hz and 0.
//! - **Smoothing.** The additional setting, 0–100 %, default 50 % (the
//!   window's slider): `a = int(10^(0.04·(100 − s)))·0.0001`, a step of
//!   carrier and depth every 45th sample.
//! - Away from 44.1 kHz the envelope keeps its 22.7 ms time and the input's
//!   deviation its hertz. The right envelope starts at 0.25 (16-bit units),
//!   left over from the chorus setup it shares, as in the original.
//!
//! Changed from the original: the phase wrapped at 1023, not 1024 — the
//! carrier 1024/1023 sharp — and its last table step read one past the
//! table. It wraps into 0–1024 both ways and the last step leads back to the
//! first entry.

use super::hp_mod_common::{advance, increment, lookup, one_m_cos_table, Stepper, HP_RATE, LFO};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const RATE: usize = 0;
const DEPTH: usize = 1;
const SMOOTHING: usize = 2;

/// Hyperprism's sample scale: 16-bit integer units.
const UNITS: f32 = 32_768.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("rateHz", "Frequency", 0.0, 1000.0, 500.0).unit("Hz").smooth(100.0),
    ParamSpec::new("depth", "Depth", 0.0, 10.0, 0.0).unit("%").smooth(100.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 50.0).unit("%").kind(ParamKind::Int).smooth(0.0).glides("rateHz"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-frequency-modulator",
    code: 53,
    version: 1,
    label: "Hyperprism Frequency Modulator",
    category: Category::Modulation,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Pad", keys: &["rateHz", "depth"] },
            Group { label: "Additional settings", keys: &["smoothing"] },
        ],
        primary: &["rateHz", "depth"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "rateHz", x_range: (0.0, 1000.0), y: "depth", y_range: (0.0, 10.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Frequency Modulator",
    sound: None,
};

pub struct HpFrequencyModulator {
    sample_rate: f32,
    values: [f32; 3],
    table: [f32; LFO],
    steps: Stepper,
    rate: f32,
    depth: f32,
    phase: [f32; 2],
    env: [f32; 2],
    /// The envelope's coefficients: 0.001 and 0.999 at 44.1 kHz.
    attack: f32,
    keep: f32,
    /// The input's phase step, kept in hertz away from 44.1 kHz.
    deviation: f32,
}

impl HpFrequencyModulator {
    fn targets(&self) -> (f32, f32) {
        (increment(self.values[RATE], self.sample_rate), self.values[DEPTH] * 1e-4)
    }
}

impl Kernel for HpFrequencyModulator {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 3];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let attack = if sample_rate as f64 == HP_RATE { 0.001 } else { (1.0 - 0.999f64.powf(HP_RATE / sample_rate as f64)) as f32 };
        let mut k = HpFrequencyModulator {
            sample_rate,
            values,
            table: one_m_cos_table(),
            steps: Stepper::new(sample_rate, 44.0, 44.0, values[SMOOTHING]),
            rate: 0.0,
            depth: 0.0,
            phase: [0.0; 2],
            env: [0.0; 2],
            attack,
            keep: 1.0 - attack,
            deviation: (HP_RATE / sample_rate as f64) as f32 * UNITS,
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
        self.phase = [0.0; 2];
        // P20 and P21 as InitializeChorus leaves them: 0, and 0.25 in 16-bit units.
        self.env = [0.0, 0.25 / UNITS];
        (self.rate, self.depth) = self.targets();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.steps.due() {
            let (rate, depth) = self.targets();
            self.rate = self.steps.step(self.rate, rate);
            self.depth = self.steps.step(self.depth, depth);
        }
        for (c, s) in frame.iter_mut().enumerate() {
            let x = *s;
            self.env[c] = x.abs() * self.attack + self.env[c] * self.keep;
            let lfo = lookup(&self.table, self.phase[c]);
            *s = self.env[c] * 1.4 * lfo;
            self.phase[c] = advance(self.phase[c], x * self.deviation * self.depth + self.rate);
        }
    }

    fn tail_seconds(&self) -> f32 {
        // The envelope falls 60 dB in about 7000 samples at 44.1 kHz.
        0.2
    }
}
