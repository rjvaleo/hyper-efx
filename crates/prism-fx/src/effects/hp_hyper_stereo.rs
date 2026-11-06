//! Hyperprism Hyper Stereo: Hyperprism-PPC 1.2.1's process 23 (class
//! EHyperStereo, routine `HyperStereo`), as its code does it. The program has
//! no window for this process (no PPob 8023), so its menu cannot reach it;
//! the routine is complete, and this is it.
//!
//! From an L/R input it forms M = ½(L + R) and S = ½(L − R); with Input set to
//! M/S it takes the left channel as M and the right as S. Then
//! L = gM·M + gS·S and R = gM·M − gS·S, each gain a percentage of unity. X is
//! the side gain and Y the mid gain. Both glide by Hyperprism's smoothing:
//! every 45th sample, `a·target + (1 − a)·value`. A mono input passes
//! unchanged; Mono input does that with the left input.
//!
//! With no window there are no window ranges or start values. The ranges here
//! are the class's absolute limits, which this process shares with MS
//! Matrix: −90 to 90 % for X and −400 to 400 % for Y, so the side gain stops
//! short of unity. Both gains start at 0, as the class sets them, so the
//! effect starts silent; mid 100 % and side 0 % is the mono sum. Smoothing
//! starts at the effect's own default, 25 % (a = 0.1), since no settings
//! dialog ever loads.
//!
//! Added (not in the original):
//! - **Smoothing step**, 1–2048 samples at 44.1 kHz (scaled with the rate as
//!   the 45 is), default 45: how often both gains glide a step. The default is
//!   the original exactly.

use super::hp_fr::SMOOTHING_STEP;
use super::hp_stereo_shared::Stepper;
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const SIDE: usize = 0;
const MID: usize = 1;
const INPUT: usize = 2;
const SMOOTHING: usize = 3;
const MONO: usize = 4;
const STEP: usize = 5;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("sideGain", "Side", -90.0, 90.0, 0.0).unit("%").smooth(0.0),
    ParamSpec::new("midGain", "Mid", -400.0, 400.0, 0.0).unit("%").smooth(0.0),
    ParamSpec::new("input", "Input", 0.0, 1.0, 0.0).kind(ParamKind::Choice(&["L/R", "M/S"])).smooth(0.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 25.0).unit("%").kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("mono", "Mono input", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
    SMOOTHING_STEP,
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-hyper-stereo",
    code: 61,
    version: 1,
    label: "Hyperprism Hyper Stereo",
    category: Category::Stereo,
    params: PARAMS,
    panel: Panel {
        // No original window: the pad as the class maps it, side across and
        // mid up, with the input button the class listens for.
        groups: &[
            Group { label: "Gains", keys: &["sideGain", "midGain"] },
            Group { label: "Settings", keys: &["input", "smoothing", "mono"] },
            Group { label: "Added (not in the original)", keys: &["smoothingStep"] },
        ],
        primary: &["sideGain", "midGain", "input"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "sideGain", x_range: (-90.0, 90.0), y: "midGain", y_range: (-400.0, 400.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Hyper Stereo",
    sound: None,
};

pub struct HpHyperStereo {
    sample_rate: f32,
    values: [f32; 6],
    /// Targets: mid gain, side gain.
    target: [f32; 2],
    smoothed: [f32; 2],
    step: Stepper,
}

impl HpHyperStereo {
    fn targets(&mut self) {
        let gain = |percent: f32| (0.01 * percent as f64) as f32;
        self.target = [gain(self.values[MID]), gain(self.values[SIDE])];
    }
}

impl Kernel for HpHyperStereo {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 6];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let step = Stepper::every(values[STEP], sample_rate, values[SMOOTHING]);
        let mut k = HpHyperStereo { sample_rate, values, target: [0.0; 2], smoothed: [0.0; 2], step };
        k.targets();
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        self.values[index] = spec.clamp(value);
        match index {
            SIDE | MID => self.targets(),
            SMOOTHING => self.step.set_smoothing(self.values[SMOOTHING]),
            STEP => self.step.set_every(self.values[STEP], self.sample_rate),
            _ => {}
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.smoothed = self.target;
        self.step.restart();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        // The mono path never reaches the countdown in the original.
        if self.values[MONO] > 0.5 {
            frame[1] = frame[0];
            return;
        }
        if self.step.fires() {
            for (s, t) in self.smoothed.iter_mut().zip(self.target) {
                *s = self.step.toward(*s, t);
            }
        }
        let (l, r) = (frame[0], frame[1]);
        let (m, s) = if self.values[INPUT] < 0.5 {
            ((0.5 * (l + r) as f64) as f32, (0.5 * (l - r) as f64) as f32)
        } else {
            (l, r)
        };
        let [gm, gs] = self.smoothed;
        let side = gs * s;
        frame[0] = (gm as f64 * m as f64 + side as f64) as f32;
        frame[1] = (gm as f64 * m as f64 - side as f64) as f32;
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
            checksum::<HpHyperStereo>(44_100.0, 3.0, &[], &[]),
            checksum::<HpHyperStereo>(44_100.0, 3.0, &[("sideGain", 60.0), ("midGain", 120.0)], &[("sideGain", -40.0)]),
            checksum::<HpHyperStereo>(
                48_000.0,
                3.0,
                &[("input", 1.0), ("midGain", -200.0), ("smoothing", 50.0)],
                &[("midGain", 300.0)],
            ),
        ];
        assert_eq!(got, [2307696701297496229, 2703270182231127609, 8891148128338008449]);
    }

    #[test]
    fn the_smoothing_step_paces_the_glide() {
        crate::effects::hp_fr::render::assert_step_paces_the_glide::<HpHyperStereo>(
            &[("sideGain", 60.0), ("midGain", 100.0)],
            &[("sideGain", -60.0)],
        );
    }
}
