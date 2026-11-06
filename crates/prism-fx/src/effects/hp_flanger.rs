//! Hyperprism Flanger: Hyperprism-PPC 1.2.1's Flanger process (EModulation,
//! process 2), as its code does it.
//!
//! The Modulation routine: one delay line per side (32 768 frames at
//! 44.1 kHz), read `d = depth·0.5·(1 − cos)` frames back from a 1024-entry
//! raised-cosine table, linearly between the frames `int(d)` and `int(d) + 1`
//! back. The sweep runs from exactly 0 — the newest sample, the dry sound
//! itself — up to the depth, and both sides sweep in phase.
//!
//! - **Feedback.** The line takes `x + 0.25·w`, where `w` is the previous
//!   sample's wet sound, so the loop is one sample longer than the delay.
//! - **Mix.** `0.5·wet + 0.5·dry`.
//! - **Pad.** X is the frequency, 0–4 Hz (`v·1024/SR` table steps a sample);
//!   Y is the depth, 0–100 samples at 44.1 kHz (the same time at other
//!   rates). Both linear, from the window, starting at 2 Hz and 0.
//! - **Smoothing.** The additional setting, 0–100 %, default 50 % (the
//!   window's slider): `a = int(10^(0.04·(100 − s)))·0.0001`, and every 45th
//!   sample rate and depth step `v ← a·target + (1 − a)·v`.
//!
//! Added (not in the original):
//!
//! - **Feedback**, 0–95 %, default 25 %: the `0.25` the line takes of the
//!   previous wet sample. At 25 % it is the original exactly.
//! - **Mix**, 0–100 % wet, default 50 %: wet `m` and dry `1 − m` in place of
//!   the fixed 0.5 and 0.5. At 50 % it is the original exactly.
//! - **Smoothing step**, 1–2048 samples at 44.1 kHz (scaled with the rate as
//!   the depth is), default 45: how often the smoothing steps. At 45 it is
//!   the original exactly.

use super::hp_mod_common::{increment, samples, Modulation, Stepper};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const RATE: usize = 0;
const DEPTH: usize = 1;
const SMOOTHING: usize = 2;
const FEEDBACK: usize = 3;
const MIX: usize = 4;
const STEP: usize = 5;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("rateHz", "Frequency", 0.0, 4.0, 2.0).unit("Hz").smooth(100.0),
    ParamSpec::new("depth", "Depth", 0.0, 100.0, 0.0).unit("samples").smooth(100.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 50.0).unit("%").kind(ParamKind::Int).smooth(0.0).glides("rateHz"),
    ParamSpec::new("feedback", "Feedback", 0.0, 95.0, 25.0).unit("%").smooth(0.0),
    ParamSpec::new("mix", "Mix", 0.0, 100.0, 50.0).unit("%").smooth(0.0),
    ParamSpec::new("smoothingStep", "Smoothing step", 1.0, 2048.0, 45.0)
        .unit("samples")
        .kind(ParamKind::Int)
        .smooth(0.0)
        .glides("rateHz"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-flanger",
    code: 49,
    version: 1,
    label: "Hyperprism Flanger",
    category: Category::Modulation,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Pad", keys: &["rateHz", "depth"] },
            Group { label: "Additional settings", keys: &["smoothing"] },
            Group { label: "Added (not in the original)", keys: &["feedback", "mix", "smoothingStep"] },
        ],
        primary: &["rateHz", "depth"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "rateHz", x_range: (0.0, 4.0), y: "depth", y_range: (0.0, 100.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Flanger",
    sound: None,
};

pub struct HpFlanger {
    sample_rate: f32,
    values: [f32; 6],
    modulation: Modulation,
    steps: Stepper,
}

impl HpFlanger {
    fn targets(&self) -> (f32, f32) {
        let depth = samples(self.values[DEPTH] as f64, self.sample_rate) as f32;
        (increment(self.values[RATE], self.sample_rate), depth)
    }

    fn apply(&mut self) {
        let m = &mut self.modulation;
        m.feedback = self.values[FEEDBACK] / 100.0;
        m.wet = self.values[MIX] / 100.0;
        m.dry = 1.0 - m.wet;
        self.steps.set_percent(self.values[SMOOTHING]);
        self.steps.set_every(self.sample_rate, self.values[STEP]);
    }
}

impl Kernel for HpFlanger {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 6];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpFlanger {
            sample_rate,
            values,
            modulation: Modulation::new(sample_rate, 0.5, 0.5, 0.25),
            steps: Stepper::new(sample_rate, 44.0, 44.0, values[SMOOTHING]),
        };
        k.apply();
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        self.values[index] = spec.clamp(value);
        if index >= SMOOTHING {
            self.apply();
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

    fn tail_seconds(&self) -> f32 {
        // Passes of at most 2.3 ms, each losing all but the feedback: at the
        // original's quarter, a few.
        let feedback = self.modulation.feedback;
        if feedback <= 0.0 {
            return 0.05;
        }
        (0.0023 * 0.001f32.ln() / feedback.ln()).max(0.05)
    }
}

#[cfg(test)]
mod tests {
    use super::super::hp_mod_common::check;
    use super::*;

    /// The renders as they were before any control was added, at both rates.
    #[test]
    fn defaults_render_as_the_original() {
        let pad = [(RATE, 1.3), (DEPTH, 60.0), (SMOOTHING, 20.0)];
        let moved = [(RATE, 3.5), (DEPTH, 90.0)];
        let got = [
            check::checksums::<HpFlanger>(44_100.0, 3.0, &pad, &moved),
            check::checksums::<HpFlanger>(48_000.0, 3.0, &pad, &moved),
        ];
        assert_eq!(
            got,
            [[3346986286750801091, 10671915074610586185, 13079813899452551467],
             [18412545448088848179, 15850143374897114819, 12263788079383699513]]
        );
    }

    fn flanger(settings: &[(usize, f32)]) -> HpFlanger {
        let mut k = HpFlanger::new(44_100.0);
        for &(i, v) in settings {
            k.set_param(i, v);
        }
        k.reset();
        k
    }

    /// A click 100 samples before the top of a 2 Hz sweep at full depth: the
    /// first echo, and the second, a loop (101 samples) later.
    fn echoes(settings: &[(usize, f32)]) -> (f32, f32) {
        let mut k = flanger(&[&[(RATE, 2.0), (DEPTH, 100.0)], settings].concat());
        let mut x = vec![0.0f32; 12_000];
        x[10_925] = 1.0;
        let (y, _) = check::render(&mut k, &(x.clone(), x), &[]);
        (y[11_000..11_050].iter().sum(), y[11_100..11_150].iter().sum())
    }

    #[test]
    fn more_feedback_makes_each_echo_more_of_the_last() {
        let (first, second) = echoes(&[]);
        assert!((second / first - 0.25).abs() < 1e-3, "{first} {second}");
        let (first, second) = echoes(&[(FEEDBACK, 75.0)]);
        assert!((second / first - 0.75).abs() < 1e-3, "{first} {second}");
        // And the sweep's comb rings harder: more energy out of the same noise.
        let x = check::input(1.0, 44_100.0);
        let low = check::power(&check::render(&mut flanger(&[(DEPTH, 50.0)]), &x, &[]).0);
        let high = check::power(&check::render(&mut flanger(&[(DEPTH, 50.0), (FEEDBACK, 95.0)]), &x, &[]).0);
        assert!(high > 1.5 * low, "{low} {high}");
    }

    #[test]
    fn mix_moves_from_the_dry_sound_to_the_wet() {
        let x = check::input(0.5, 44_100.0);
        let dry = check::render(&mut flanger(&[(DEPTH, 60.0), (MIX, 0.0)]), &x, &[]);
        assert_eq!(dry, x, "all dry is the input");
        let (first, _) = echoes(&[(MIX, 100.0)]);
        assert!((first - 1.0).abs() < 1e-3, "all wet: the whole echo, {first}");
        let (first, _) = echoes(&[(MIX, 25.0)]);
        assert!((first - 0.25).abs() < 1e-3, "{first}");
    }

    #[test]
    fn the_smoothing_step_sets_how_often_the_glides_move() {
        let fires = |sr: f32, every: f32| {
            let mut k = HpFlanger::new(sr);
            k.set_param(STEP, every);
            k.reset();
            (0..1_000).filter(|_| k.steps.due()).count()
        };
        assert_eq!(fires(44_100.0, 45.0), 22);
        assert_eq!(fires(44_100.0, 10.0), 100);
        assert_eq!(fires(44_100.0, 1.0), 1_000);
        // Scaled with the rate as the original's count is: 9 → 18, every 19.
        assert_eq!(fires(88_200.0, 10.0), 1_000 / 19);
        // A depth move lands sooner with shorter steps.
        let x = check::input(1.0, 44_100.0);
        let moved = [(DEPTH, 100.0)];
        let mut a = flanger(&[(SMOOTHING, 90.0)]);
        let mut b = flanger(&[(SMOOTHING, 90.0), (STEP, 5.0)]);
        let ya = check::render(&mut a, &x, &moved);
        let yb = check::render(&mut b, &x, &moved);
        assert_ne!(ya, yb);
        assert!(b.modulation.depth > a.modulation.depth, "{} {}", a.modulation.depth, b.modulation.depth);
    }
}
