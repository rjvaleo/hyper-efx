//! Hyperprism Band Reject (Hyperprism-PPC 1.2.1, EFilter2 process 3,
//! Filter2), as its code does it.
//!
//! In 1.2.1 the menu's Band Reject is EFilter2's state-variable filter, not
//! EFilter's own (unreachable) band reject. A plain Chamberlin SVF, one
//! iteration a sample, no oversampling and no saturation:
//!
//! ```text
//! low  = low + F·band
//! high = x − low − q·band
//! band = band + F·high
//! out  = low + high              (= x − q·band before this sample)
//! ```
//!
//! - **Coefficients.** `F = 2·sin(π·f/sr)`, `q = 1/Q`. f is held at 1 Hz or
//!   more, and Q at `(1.35·f/(sr/2 − f))²` or more, which only matters near
//!   Nyquist. The code stores the raised values back into the parameters;
//!   the pad sends both axes on every move, so that is the same as applying
//!   the limits each time, which is what this does.
//! - **Response.** Unity away from f, infinitely deep at f, about f/Q wide at
//!   −3 dB.
//! - **Smoothing.** F and q glide once every 45 samples, the first step on
//!   the 45th, `s = a·target + (1 − a)·s` with the Smoothing setting's `a`
//!   (0.1 at the default 25 %). A reset puts them on their targets and clears
//!   the state.
//!
//! Ranges are the pad window (0–2000 Hz, Q 1–10), starting at 1 kHz and Q 1.
//! The original let a window be typed out to 20000 Hz and Q 99999; that is
//! not offered.
//!
//! Changed from the original:
//! - **Rate.** The 45-sample step is scaled with the rate (49 at 48 kHz), so
//!   a glide takes the same time anywhere. Frequencies are in hertz at the
//!   host's rate, as the code's are at its own.
//!
//! Added (not in the original):
//! - **Smoothing step**, 1–2048 samples at 44.1 kHz (scaled with the rate as
//!   the 45 is), default 45: how often F and q glide a step. The default is the
//!   original exactly.

use super::hp_fr::{Countdown, Glide, SMOOTHING, SMOOTHING_STEP};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamSpec};

const FREQ: usize = 0;
const Q: usize = 1;
const SMOOTH: usize = 2;
const STEP: usize = 3;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("freqHz", "Frequency", 0.0, 2_000.0, 1_000.0).unit("Hz").smooth(9.7),
    ParamSpec::new("q", "Q", 1.0, 10.0, 1.0).smooth(9.7),
    SMOOTHING.glides("freqHz"),
    SMOOTHING_STEP.glides("freqHz"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-band-reject",
    code: 35,
    version: 1,
    label: "Hyperprism Band Reject",
    category: Category::Filter,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Filter", keys: &["freqHz", "q"] },
            Group { label: "Settings", keys: &["smoothing"] },
            Group { label: "Added (not in the original)", keys: &["smoothingStep"] },
        ],
        primary: &["freqHz", "q"],
        displays: &[Display::Curve, Display::Meter],
        pad: Some(PadPair { x: "freqHz", x_range: (0.0, 2_000.0), y: "q", y_range: (1.0, 10.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Band Reject",
    sound: None,
};

#[derive(Clone, Copy, Default)]
struct Svf {
    low: f32,
    band: f32,
}

pub struct HpBandReject {
    sample_rate: f32,
    values: [f32; 4],
    /// F and q: their targets and their glides.
    target: (f32, f32),
    smoothed: (f32, f32),
    sides: [Svf; 2],
    glide: Glide,
    countdown: Countdown,
}

impl HpBandReject {
    /// SetFilterParameters.
    fn retarget(&mut self) {
        let sr = f64::from(self.sample_rate);
        let f = if self.values[FREQ] < 1.0 { 1.0 } else { self.values[FREQ] };
        let floor = ((1.35 * f64::from(f)) / (0.5 * sr - f64::from(f))).powf(2.0);
        let q = if f64::from(self.values[Q]) < floor { floor as f32 } else { self.values[Q] };
        let big_f = (2.0 * (core::f64::consts::PI * f64::from(f) / sr).sin()) as f32;
        self.target = (big_f, (1.0 / f64::from(q)) as f32);
    }
}

impl Kernel for HpBandReject {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 4];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpBandReject {
            sample_rate,
            values,
            target: (0.0, 0.0),
            smoothed: (0.0, 0.0),
            sides: [Svf::default(); 2],
            glide: Glide::new(values[SMOOTH]),
            countdown: Countdown::every(values[STEP], sample_rate),
        };
        k.retarget();
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        self.values[index] = spec.clamp(value);
        if index == SMOOTH {
            self.glide = Glide::new(self.values[SMOOTH]);
        } else if index == STEP {
            self.countdown.set_every(self.values[STEP], self.sample_rate);
        } else {
            self.retarget();
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.smoothed = self.target;
        self.sides = [Svf::default(); 2];
        self.countdown.reset();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.countdown.fires() {
            self.smoothed = (
                self.glide.step(self.target.0, self.smoothed.0),
                self.glide.step(self.target.1, self.smoothed.1),
            );
        }
        let (big_f, q) = (f64::from(self.smoothed.0), f64::from(self.smoothed.1));
        for (x, s) in frame.iter_mut().zip(self.sides.iter_mut()) {
            let low = (big_f * f64::from(s.band) + f64::from(s.low)) as f32;
            let high = -((q * f64::from(s.band) - f64::from((f64::from(*x) - f64::from(low)) as f32)) as f32);
            s.band = (big_f * f64::from(high) + f64::from(s.band)) as f32;
            s.low = low;
            *x = (f64::from(low) + f64::from(high)) as f32;
        }
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
            checksum::<HpBandReject>(44_100.0, 3.0, &[], &[]),
            checksum::<HpBandReject>(44_100.0, 3.0, &[("freqHz", 500.0), ("q", 4.0)], &[("freqHz", 1_500.0)]),
            checksum::<HpBandReject>(
                48_000.0,
                3.0,
                &[("freqHz", 200.0), ("q", 8.0), ("smoothing", 60.0)],
                &[("q", 2.0)],
            ),
        ];
        assert_eq!(got, [10350161641232645261, 9081015865674694557, 14509206055873080460]);
    }

    #[test]
    fn the_smoothing_step_paces_the_glide() {
        crate::effects::hp_fr::render::assert_step_paces_the_glide::<HpBandReject>(
            &[("freqHz", 500.0), ("q", 4.0)],
            &[("freqHz", 1_500.0)],
        );
    }
}
