//! Hyperprism More Stereo (Hyperprism-PPC 1.2.1, EFilter process 4,
//! MoreStereo), as its code does it.
//!
//! A widener that leaves the mid alone and scales only the part of the side
//! above the Low Cut:
//!
//! ```text
//! S  = L − R
//! h  = Butterworth high-pass of S at Low Cut, unity passband
//! L' = L + g·h,   R' = R − g·h,   g = (Stereo Level − 100)/200
//! ```
//!
//! So `L' − R' = S + 2g·HP(S)`: above the cut the side's gain is Stereo
//! Level/100 (0 % is mono there, 100 % no change, 300 % three times as wide),
//! and below it the side is as it was. Low Cut is where the width change
//! starts, not a mono-below.
//!
//! - **High-pass.** The RBJ Butterworth in EFilter's α/β/γ form, with the
//!   true `sin`: `w = 2πf/sr`, `β = ½(1 − sin w/√2)/(1 + sin w/√2)`,
//!   `γ = (½ + β)·cos w`, `α = ¼(½ + β + γ)`, section
//!   `2·(α, −2α, α; γ, −β)`. Low Cut is held within 0.00022675·sr …
//!   0.46031·sr (10 Hz … 20.3 kHz at 44.1 kHz), so its start point, 0, is
//!   10 Hz.
//! - **Smoothing.** α, β, γ and g glide once every 45 samples, the first step
//!   on the 45th, `s = a·target + (1 − a)·s` with the Smoothing setting's `a`
//!   (0.1 at the default 25 %). A reset puts them on their targets and clears
//!   the filter.
//! - **Mono.** The original copied a mono input through; a host here always
//!   hands over two sides, and a centred sound has no side, so it passes
//!   unchanged all the same.
//!
//! Ranges are the pad window (Stereo Level 0–300 %, Low Cut 1–6000 Hz, the
//! Low Cut range reaching down to its start point, 0), starting at 150 % and
//! 0 Hz. The original let a window be typed out to 32767; that is not offered.
//!
//! Changed from the original:
//! - **Rate.** The 45-sample step is scaled with the rate (49 at 48 kHz), so
//!   a glide takes the same time anywhere. Frequencies are in hertz at the
//!   host's rate, as the code's are at its own.

use super::hp_fr::{Abg, Coefs, Countdown, Glide, Section, SMOOTHING};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamSpec};

const LEVEL: usize = 0;
const LOW_CUT: usize = 1;
const SMOOTH: usize = 2;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("level", "Stereo Level", 0.0, 300.0, 150.0).unit("%").smooth(9.7),
    ParamSpec::new("lowCutHz", "Low Cut", 0.0, 6_000.0, 0.0).unit("Hz").smooth(9.7),
    SMOOTHING.glides("level"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-more-stereo",
    code: 36,
    version: 1,
    label: "Hyperprism More Stereo",
    category: Category::Stereo,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Stereo", keys: &["level", "lowCutHz"] },
            Group { label: "Settings", keys: &["smoothing"] },
        ],
        primary: &["level", "lowCutHz"],
        displays: &[Display::Scope, Display::Meter],
        pad: Some(PadPair { x: "level", x_range: (0.0, 300.0), y: "lowCutHz", y_range: (1.0, 6_000.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: More Stereo",
    sound: None,
};

/// SetMoreStereoParameters. Its 2π is the code's, a digit short of `TAU`.
#[allow(clippy::approx_constant)]
fn butterworth(f: f32, sample_rate: f32) -> Abg {
    let sr = f64::from(sample_rate);
    let lo = f64::from((0.00022675 * sr) as f32);
    let hi = f64::from((0.46031 * sr) as f32);
    let f = f64::from(f);
    let f = if f < lo {
        lo
    } else if hi < f {
        hi
    } else {
        f
    };
    let w = f64::from(((6.283185307 * f) / sr) as f32);
    let s = f64::from(w.sin() as f32);
    let c = f64::from(w.cos() as f32);
    let root = 2f64.sqrt();
    let beta = (0.5 * ((1.0 - s / root) / (1.0 + s / root))) as f32;
    let up = 0.5 + f64::from(beta);
    let gamma = (up * c) as f32;
    Abg { alpha: (0.25 * (up + f64::from(gamma))) as f32, beta, gamma }
}

pub struct HpMoreStereo {
    sample_rate: f32,
    values: [f32; 3],
    target: Abg,
    smoothed: Abg,
    /// g, as SetParameter stores it, and its glide.
    gain: f32,
    gain_s: f32,
    coefs: Coefs,
    side: Section,
    glide: Glide,
    countdown: Countdown,
}

impl Kernel for HpMoreStereo {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 3];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpMoreStereo {
            sample_rate,
            values,
            target: Abg::default(),
            smoothed: Abg::default(),
            gain: 0.0,
            gain_s: 0.0,
            coefs: Coefs::default(),
            side: Section::default(),
            glide: Glide::new(values[SMOOTH]),
            countdown: Countdown::new(sample_rate),
        };
        k.set_param(LEVEL, values[LEVEL]);
        k.set_param(LOW_CUT, values[LOW_CUT]);
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        let v = spec.clamp(value);
        self.values[index] = v;
        match index {
            LEVEL => self.gain = ((f64::from(v) - 100.0) / 200.0) as f32,
            LOW_CUT => self.target = butterworth(v, self.sample_rate),
            _ => self.glide = Glide::new(v),
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.smoothed = self.target;
        self.gain_s = self.gain;
        self.coefs = Coefs::of(&self.smoothed, -1.0);
        self.side = Section::default();
        self.countdown.reset();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.countdown.fires() {
            self.smoothed = self.smoothed.glided(&self.glide, &self.target);
            self.gain_s = self.glide.step(self.gain, self.gain_s);
            self.coefs = Coefs::of(&self.smoothed, -1.0);
        }
        let [l, r] = *frame;
        let h = f64::from(self.side.run(&self.coefs, l - r));
        let g = f64::from(self.gain_s);
        frame[0] = (h * g + f64::from(l)) as f32;
        frame[1] = -((h * g - f64::from(r)) as f32);
    }
}
