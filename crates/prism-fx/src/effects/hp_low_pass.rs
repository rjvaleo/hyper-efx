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
//!
//! Added (not in the original):
//! - **Smoothing step**, 1–2048 samples at 44.1 kHz (scaled with the rate as
//!   the 45 is), default 45: how often the glide steps. The default is the
//!   original exactly.
//! - **Output gain**, −24 to +12 dB, default +6.02 dB (`20·log₁₀ 2`, the
//!   section's built-in gain of 2): the passband level. The gain of 2 that
//!   multiplies the recursion stays where it is, since it shapes the poles;
//!   the control scales only the output, by `10^((dB − 6.02)/20)`, gliding
//!   with the coefficients. The default is the original exactly.

use super::hp_fr::{EFilterCore, EFilterProcess, OUTPUT_GAIN, SMOOTHING, SMOOTHING_STEP};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("freqHz", "Frequency", 0.0, 20_000.0, 20_000.0).unit("Hz").smooth(9.7),
    ParamSpec::new("q", "Q", 0.0, 10.0, 1.0).smooth(9.7),
    SMOOTHING.glides("freqHz"),
    SMOOTHING_STEP.glides("freqHz"),
    OUTPUT_GAIN,
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
            Group { label: "Added (not in the original)", keys: &["smoothingStep", "outputDb"] },
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::hp_fr::render::checksum;
    use crate::effects::hp_fr::render::{rms, settled};
    use crate::effects::hp_fr::OUTPUT_GAIN;

    /// The renders before any control was added, at the defaults and at two
    /// other settings, one at 48 kHz: the added controls at their defaults
    /// leave every bit as it was.
    #[test]
    fn defaults_render_as_the_original() {
        let got = [
            checksum::<HpLowPass>(44_100.0, 3.0, &[], &[]),
            checksum::<HpLowPass>(44_100.0, 3.0, &[("freqHz", 1_000.0), ("q", 5.0)], &[("freqHz", 300.0)]),
            checksum::<HpLowPass>(
                48_000.0,
                3.0,
                &[("freqHz", 5_000.0), ("q", 0.5), ("smoothing", 60.0)],
                &[("q", 8.0)],
            ),
        ];
        assert_eq!(got, [2541648777998183818, 8751267158301971790, 16077833013817603346]);
    }

    #[test]
    fn the_smoothing_step_paces_the_glide() {
        crate::effects::hp_fr::render::assert_step_paces_the_glide::<HpLowPass>(
            &[("freqHz", 2_000.0)],
            &[("freqHz", 300.0)],
        );
    }

    #[test]
    fn the_output_gain_sets_the_passband_level() {
        let at = |db: f32| {
            let out = settled::<HpLowPass>(44_100.0, 1.0, &[("freqHz", 5_000.0), ("outputDb", db)]);
            rms(&out, 0, 22_050..44_100)
        };
        let original = at(OUTPUT_GAIN.default);
        for db in [-24.0, -12.0, 0.0, 12.0] {
            let shift = 20.0 * (at(db) / original).log10();
            let want = f64::from(db - OUTPUT_GAIN.default);
            assert!((shift - want).abs() < 0.01, "{db} dB: shifted {shift:.3}, want {want:.3}");
        }
        // The passband itself, in a band the filter passes, is the set gain.
        let tone = |db: f32| {
            let mut k = HpLowPass::new(44_100.0);
            let d = HpLowPass::descriptor();
            k.set_param(d.index_of("freqHz").unwrap(), 5_000.0);
            k.set_param(d.index_of("outputDb").unwrap(), db);
            k.reset();
            let mut peak = 0f32;
            for i in 0..44_100 {
                let x = 0.25 * (core::f64::consts::TAU * 100.0 * i as f64 / 44_100.0).sin() as f32;
                let mut frame = [x, x];
                k.tick(&mut frame);
                if i > 22_050 {
                    peak = peak.max(frame[0].abs());
                }
            }
            20.0 * f64::from(peak / 0.25).log10()
        };
        assert!((tone(OUTPUT_GAIN.default) - 6.02).abs() < 0.1);
        assert!((tone(0.0) - 0.0).abs() < 0.1, "{}", tone(0.0));
        assert!((tone(-12.0) + 12.0).abs() < 0.1, "{}", tone(-12.0));
    }
}
