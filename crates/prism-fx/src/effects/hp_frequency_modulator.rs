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
//!
//! Added (not in the original):
//! - **Smoothing step**, 1–2048 samples at 44.1 kHz (scaled with the rate as
//!   the 45 is), default 45: how often carrier and depth glide a step. A
//!   change restarts the count. The default is the original exactly.
//! - **Follow time**, 1–500 ms, default 22.68 ms: the envelope's time
//!   constant, `1/(0.001·44100)` s in the original's fixed 0.001. Shorter
//!   follows the input's level faster (more of its waveform rides on the
//!   carrier), longer smooths it. The default is the original exactly.

use super::hp_fr::SMOOTHING_STEP;
use super::hp_mod_common::{advance, increment, lookup, one_m_cos_table, Stepper, HP_RATE, LFO};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const RATE: usize = 0;
const DEPTH: usize = 1;
const SMOOTHING: usize = 2;
const STEP: usize = 3;
const FOLLOW: usize = 4;

/// The envelope's time constant in the original, in ms: its 0.001 a sample
/// at 44.1 kHz.
const FOLLOW_MS: f32 = 22.675_737;

/// Hyperprism's sample scale: 16-bit integer units.
const UNITS: f32 = 32_768.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("rateHz", "Frequency", 0.0, 1000.0, 500.0).unit("Hz").smooth(100.0),
    ParamSpec::new("depth", "Depth", 0.0, 10.0, 0.0).unit("%").smooth(100.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 50.0).unit("%").kind(ParamKind::Int).smooth(0.0).glides("rateHz"),
    SMOOTHING_STEP.glides("rateHz"),
    ParamSpec::new("followMs", "Follow time", 1.0, 500.0, FOLLOW_MS).unit("ms").smooth(0.0),
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
            Group { label: "Added (not in the original)", keys: &["smoothingStep", "followMs"] },
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
    values: [f32; 5],
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
    /// The stepper for the Smoothing step: a count of `step − 1`, the
    /// original's 44 at the default.
    fn stepper(sample_rate: f32, step: f32, percent: f32) -> Stepper {
        let reload = f64::from(step) - 1.0;
        Stepper::new(sample_rate, reload, reload, percent)
    }

    /// The envelope's coefficient: 0.001 at 44.1 kHz and the default, and
    /// the same time at any rate.
    fn follow(&mut self) {
        let ms = self.values[FOLLOW];
        let (c, keep) = if ms == FOLLOW_MS {
            (0.001, 0.999)
        } else {
            let c = 1000.0 / (f64::from(ms) * HP_RATE);
            (c, 1.0 - c)
        };
        self.attack = if self.sample_rate as f64 == HP_RATE {
            c as f32
        } else {
            (1.0 - keep.powf(HP_RATE / self.sample_rate as f64)) as f32
        };
        self.keep = 1.0 - self.attack;
    }

    fn targets(&self) -> (f32, f32) {
        (increment(self.values[RATE], self.sample_rate), self.values[DEPTH] * 1e-4)
    }
}

impl Kernel for HpFrequencyModulator {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 5];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpFrequencyModulator {
            sample_rate,
            values,
            table: one_m_cos_table(),
            steps: Self::stepper(sample_rate, values[STEP], values[SMOOTHING]),
            rate: 0.0,
            depth: 0.0,
            phase: [0.0; 2],
            env: [0.0; 2],
            attack: 0.0,
            keep: 0.0,
            deviation: (HP_RATE / sample_rate as f64) as f32 * UNITS,
        };
        k.follow();
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        let v = spec.clamp(value);
        let changed = v != self.values[index];
        self.values[index] = v;
        match index {
            SMOOTHING => self.steps.set_percent(v),
            STEP if changed => self.steps = Self::stepper(self.sample_rate, v, self.values[SMOOTHING]),
            FOLLOW => self.follow(),
            _ => {}
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
        // The envelope falls 60 dB in about 7000 samples at 44.1 kHz, and
        // in proportion at other follow times.
        (0.2 * (self.values[FOLLOW] / FOLLOW_MS)).max(0.2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::hp_fr::render::checksum;

    /// The renders before any control was added, at the defaults and at two
    /// other settings, one at 48 kHz: the added controls at their defaults
    /// leave every bit as it was.
    #[test]
    fn defaults_render_as_the_original() {
        let got = [
            checksum::<HpFrequencyModulator>(44_100.0, 3.0, &[], &[]),
            checksum::<HpFrequencyModulator>(44_100.0, 3.0, &[("rateHz", 200.0), ("depth", 3.0)], &[("rateHz", 800.0)]),
            checksum::<HpFrequencyModulator>(48_000.0, 3.0, &[("depth", 8.0), ("smoothing", 20.0)], &[("depth", 1.0)]),
        ];
        assert_eq!(got, [12536195243350908990, 10956369245920007391, 9583764100551506492]);
    }

    /// Samples until the carrier is within 1 % of a move from 200 to 800 Hz.
    fn settles_in(step: f32) -> usize {
        let mut k = HpFrequencyModulator::new(44_100.0);
        let d = HpFrequencyModulator::descriptor();
        k.set_param(d.index_of("rateHz").unwrap(), 200.0);
        k.set_param(d.index_of("smoothingStep").unwrap(), step);
        k.reset();
        let from = k.rate;
        k.set_param(d.index_of("rateHz").unwrap(), 800.0);
        let to = k.targets().0;
        (1..10 * 44_100)
            .find(|_| {
                let mut frame = [0.0, 0.0];
                k.tick(&mut frame);
                (k.rate - to).abs() < 0.01 * (to - from)
            })
            .unwrap()
    }

    #[test]
    fn the_smoothing_step_paces_the_glide() {
        // At 50 % (a = 0.01) the glide takes 459 steps to come within 1 %.
        assert_eq!(settles_in(45.0), 459 * 45);
        assert_eq!(settles_in(1.0), 459);
        assert_eq!(settles_in(400.0), 459 * 400);
    }

    /// The output's level 50 ms after a tone stops, against its level as it
    /// stops, in dB.
    fn release_db(follow_ms: f32) -> f64 {
        let mut k = HpFrequencyModulator::new(44_100.0);
        k.set_param(HpFrequencyModulator::descriptor().index_of("followMs").unwrap(), follow_ms);
        k.reset();
        let out: Vec<f32> = (0..44_100)
            .map(|i| {
                let tone = 0.5 * (core::f64::consts::TAU * 300.0 * i as f64 / 44_100.0).sin() as f32;
                let x = if i < 22_050 { tone } else { 0.0 };
                let mut frame = [x, x];
                k.tick(&mut frame);
                frame[0]
            })
            .collect();
        let level = |at: usize| (out[at..at + 441].iter().map(|s| f64::from(s * s)).sum::<f64>() / 441.0).sqrt();
        20.0 * (level(22_050 + 2_205) / level(22_050 - 441)).log10()
    }

    #[test]
    fn the_follow_time_sets_how_fast_the_envelope_falls() {
        // A one-pole falls 8.69 dB a time constant.
        for ms in [FOLLOW_MS, 10.0, 100.0] {
            let want = -8.686 * 50.0 / f64::from(ms);
            let got = release_db(ms);
            assert!((got - want).abs() < 0.15 * want.abs() + 0.5, "{ms} ms: {got:.1} dB, want {want:.1}");
        }
        // The default is the original's 0.001 a sample.
        let k = HpFrequencyModulator::new(44_100.0);
        assert_eq!((k.attack, k.keep), (0.001, 1.0 - 0.001));
    }
}
