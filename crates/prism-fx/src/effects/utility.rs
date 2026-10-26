//! Utility: MultiGrainLive's utility strip (`fx::shape::Utility`), moved here
//! unchanged.
//!
//! In order, per frame: a gain in dB (negated when Invert is on), the sides
//! swapped when Swap is on, the mid/side width (as in Width, unclamped past
//! the control's own 0–2), and, when Amp fit is on, the Amp fit machine with
//! the three fit controls. The switches act from half-way up, as in the
//! original. While Amp fit is off its follower holds where it was, and picks
//! up from there when it is switched back on.

use super::fit::FitCore;
use crate::{Category, Descriptor, Group, Kernel, Panel, ParamKind, ParamSpec};

const GAIN_DB: usize = 0;
const INVERT: usize = 1;
const SWAP: usize = 2;
const WIDTH: usize = 3;
const AMP_FIT: usize = 4;
const GRAIN: usize = 5;
const AMOUNT: usize = 6;
const FLOOR: usize = 7;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("gainDb", "Gain", -24.0, 24.0, 0.0).unit("dB").smooth(0.0),
    ParamSpec::new("invert", "Invert", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("swap", "Swap", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("width", "Width", 0.0, 2.0, 1.0).unit("x").smooth(0.0),
    ParamSpec::new("ampFit", "Amp fit", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("grainMs", "Fit grain", 5.0, 500.0, 30.0).log().unit("ms").smooth(0.0).when("ampFit", 1.0),
    ParamSpec::new("amount", "Fit amount", 0.0, 1.0, 0.7).smooth(0.0).when("ampFit", 1.0),
    ParamSpec::new("floorDb", "Fit floor", -80.0, -20.0, -50.0).unit("dB").smooth(0.0).when("ampFit", 1.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "utility",
    code: 74,
    version: 1,
    label: "Utility",
    category: Category::Utility,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Level", keys: &["gainDb", "invert"] },
            Group { label: "Stereo", keys: &["swap", "width"] },
            Group { label: "Amp fit", keys: &["ampFit", "grainMs", "amount", "floorDb"] },
        ],
        primary: &["gainDb", "width"],
        displays: &[],
        pad: None,
    },
    source: "MultiGrainLive: Utility (fx::shape::Utility)",
    sound: None,
};

pub struct Utility {
    sample_rate: f32,
    values: [f32; 8],
    gain: f32,
    fit: FitCore,
}

impl Utility {
    fn derive(&mut self) {
        let v = self.values;
        self.gain = 10f32.powf(v[GAIN_DB] / 20.0) * if v[INVERT] >= 0.5 { -1.0 } else { 1.0 };
        self.fit.configure(self.sample_rate, v[GRAIN], v[AMOUNT], v[FLOOR]);
    }
}

impl Kernel for Utility {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 8];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = Utility {
            sample_rate,
            values,
            gain: 1.0,
            fit: FitCore::new(sample_rate, values[GRAIN], values[AMOUNT], values[FLOOR]),
        };
        k.derive();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(spec) = PARAMS.get(index) {
            self.values[index] = spec.clamp(value);
            self.derive();
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.fit.reset();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        for s in frame.iter_mut() {
            *s *= self.gain;
        }
        if self.values[SWAP] >= 0.5 {
            frame.swap(0, 1);
        }
        let mid = (frame[0] + frame[1]) * 0.5;
        let side = (frame[0] - frame[1]) * 0.5 * self.values[WIDTH];
        frame[0] = mid + side;
        frame[1] = mid - side;
        if self.values[AMP_FIT] >= 0.5 {
            self.fit.tick(frame);
        }
    }
}
