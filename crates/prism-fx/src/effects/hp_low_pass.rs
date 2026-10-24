//! Hyperprism Low Pass (Hyperprism-PPC 1.2.1, EFilter process 0,
//! FilterSingle), as its code does it.
//!
//! One direct-form biquad in the Motorola DSP56000 α/β/γ form, X Frequency
//! and Y Q, both linear on the pad.
//!
//! - **Coefficients.** `w = 2πf/sr`, `β = (q − w/2)/(2q + w)` — `w` where the
//!   cookbook has `sin w` — and `γ = (½ + β)·cos w`. Then
//!   `α = min((½ − β)·sin w/(1 + cos w), ½(½ + β − γ))`. The second term is
//!   twice the textbook low-pass α, so the passband is +6 dB; the first takes
//!   over as Q rises and pins the resonant peak at +6 dB, so more Q lowers
//!   everything but the peak instead of raising the peak.
//! - **Section.** `y = 2·(α·x + 2α·x1 + α·x2 + γ·y1 − β·y2)`: the output
//!   gain of 2 multiplies the recursion as well. Each side has its own
//!   memory and both use the same coefficients.
//! - **Limits.** f is held at 0.0003·sr (13.2 Hz) or more, q at 0.001 or more.
//! - **Smoothing.** α, β and γ glide, not f and q: `s = a·target + (1 − a)·s`
//!   once every 45 samples, the first step on the 45th, with
//!   `a = 0.0001·int(10^(0.04·(100 − Smoothing)))` — 0.1, about 10 ms, at the
//!   default 25 %. A reset puts them on their targets and clears the memory.
//!
//! Ranges are Hyperprism's pad window (0–20000 Hz, Q 0–10), starting at
//! 20 kHz and Q 1. The original let a window be typed out to 32767; above
//! Nyquist its α diverges, so that is not offered.
//!
//! Changed from the original:
//! - **Rate.** The 45-sample step is scaled with the rate (49 at 48 kHz), so
//!   a glide takes the same time anywhere. Frequencies are in hertz at the
//!   host's rate, as the code's are at its own; below 44.1 kHz the frequency
//!   is also held under 20 kHz × sr/44100, the pad window's share of the rate.

use super::hp_fr::{EFilterCore, EFilterProcess, SMOOTHING};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("freqHz", "Frequency", 0.0, 20_000.0, 20_000.0).unit("Hz").smooth(9.7),
    ParamSpec::new("q", "Q", 0.0, 10.0, 1.0).smooth(9.7),
    SMOOTHING.glides("freqHz"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-low-pass",
    code: 32,
    version: 1,
    label: "Hyperprism Low Pass",
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
    source: "Hyperprism-PPC 1.2.1: Low Pass",
    sound: None,
};

pub struct HpLowPass(EFilterCore);

impl Kernel for HpLowPass {
    fn new(sample_rate: f32) -> Self {
        HpLowPass(EFilterCore::new(EFilterProcess::Low, PARAMS, sample_rate))
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
