//! Hyperprism High Pass (Hyperprism-PPC 1.2.1, EFilter process 1,
//! FilterSingle), as its code does it.
//!
//! One direct-form biquad in the α/β/γ form, X Frequency and Y Q, both linear
//! on the pad.
//!
//! - **Coefficients.** β and γ as the Low Pass (`β = (q − w/2)/(2q + w)`,
//!   `γ = (½ + β)·cos w`), then `α = √((½ − β)² + γ²)`.
//! - **Section.** `y = 2·(α·x − 2α·x1 + α·x2 + γ·y1 − β·y2)`: +6 dB passband,
//!   and a resonance that grows with Q (+20 dB at Q 5, +26 dB at Q 10).
//! - **Low Q.** q is held at 0.001 or more. Down there β → −½ and the section
//!   rises toward Nyquist, +43 dB at 22 kHz with the pad at its bottom edge.
//!   That is finite and it is how the original sounds, so it is kept.
//! - **Limits and smoothing** as the Low Pass: f ≥ 0.0003·sr; α, β, γ glide
//!   a step every 45 samples by the Smoothing setting's `a`.
//!
//! Ranges are the pad window (0–20000 Hz, Q 0–10), starting at 20 Hz and Q 1.
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
    ParamSpec::new("freqHz", "Frequency", 0.0, 20_000.0, 20.0).unit("Hz").smooth(9.7),
    ParamSpec::new("q", "Q", 0.0, 10.0, 1.0).smooth(9.7),
    SMOOTHING.glides("freqHz"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-high-pass",
    code: 33,
    version: 1,
    label: "Hyperprism High Pass",
    category: Category::Filter,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Filter", keys: &["freqHz", "q"] },
            Group { label: "Settings", keys: &["smoothing"] },
        ],
        primary: &["freqHz", "q"],
        displays: &[Display::Curve, Display::Meter],
        pad: Some(PadPair { x: "freqHz", x_range: (0.0, 20_000.0), y: "q", y_range: (0.0, 10.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: High Pass",
    sound: None,
};

pub struct HpHighPass(EFilterCore);

impl Kernel for HpHighPass {
    fn new(sample_rate: f32) -> Self {
        HpHighPass(EFilterCore::new(EFilterProcess::High, PARAMS, sample_rate))
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
