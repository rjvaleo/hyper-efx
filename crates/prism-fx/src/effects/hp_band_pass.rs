//! Hyperprism Band Pass (Hyperprism-PPC 1.2.1, EFilter process 2,
//! FilterDual), as its code does it.
//!
//! Two biquads **in parallel on the same input**, summed and halved: one with
//! the low-pass numerator `(α, 2α, α)`, one with the high-pass numerator
//! `(α, −2α, α)`, both with `γ` and `−β` in the recursion and an output gain
//! of 2. β and γ are the Low Pass's (`w` for `sin w`); α is the textbook
//! band-pass `½(½ − β)`; Q is held at 1 or more, f at 0.0003·sr or more.
//!
//! The net filter is `2α(1 + z⁻²)/(1 − 2γz⁻¹ + 2βz⁻²)`. Its numerator is
//! `1 + z⁻²`, not a band-pass's `1 − z⁻²`: a zero at a quarter of the rate and
//! none at DC, and a gain at f of `|cot w|` whatever the Q (+16.9 dB at
//! 1 kHz). At Q 1 it is a loud low-pass; at Q 10 a peak over a −3 dB floor.
//! That is what the code does, and it is kept.
//!
//! Only the first section's coefficients glide (every 45 samples, the
//! Smoothing setting's `a`); the second copies them at each step.
//!
//! Ranges are the pad window (0–2000 Hz, Q 1–10), starting at 1 kHz and Q 1.
//! The original let a window be typed out to 32767; that is not offered.
//!
//! Changed from the original:
//! - **Rate.** The 45-sample step is scaled with the rate (49 at 48 kHz), so
//!   a glide takes the same time anywhere. Frequencies are in hertz at the
//!   host's rate, as the code's are at its own; below 44.1 kHz the frequency
//!   is also held under 20 kHz × sr/44100, the pad window's share of the rate.

use super::hp_fr::{EFilterCore, EFilterProcess, SMOOTHING};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("freqHz", "Frequency", 0.0, 2_000.0, 1_000.0).unit("Hz").smooth(9.7),
    ParamSpec::new("q", "Q", 1.0, 10.0, 1.0).smooth(9.7),
    SMOOTHING.glides("freqHz"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-band-pass",
    code: 34,
    version: 1,
    label: "Hyperprism Band Pass",
    category: Category::Filter,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Filter", keys: &["freqHz", "q"] },
            Group { label: "Settings", keys: &["smoothing"] },
        ],
        primary: &["freqHz", "q"],
        displays: &[Display::Curve, Display::Meter],
        pad: Some(PadPair { x: "freqHz", x_range: (0.0, 2_000.0), y: "q", y_range: (1.0, 10.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Band Pass",
    sound: None,
};

pub struct HpBandPass(EFilterCore);

impl Kernel for HpBandPass {
    fn new(sample_rate: f32) -> Self {
        HpBandPass(EFilterCore::new(EFilterProcess::Band, PARAMS, sample_rate))
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        self.0.set_param(index, value)
    }

    fn param(&self, index: usize) -> f32 {
        self.0.param(index)
    }

    fn reset(&mut self) {
        self.0.reset()
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        self.0.tick(frame)
    }
}
