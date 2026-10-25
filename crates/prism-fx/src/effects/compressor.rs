//! Compressor: the compressor in MultiGrainLive's rack (`fx::comp::Compressor`),
//! sample for sample, with the rack's keys and ranges.
//!
//! Feed-forward, with one detector for both sides: each frame's louder side,
//! in dB (−120 for silence), goes through a static curve, and the gain
//! reduction it asks for is smoothed — by the attack while the reduction
//! deepens, by the release while it eases — before both sides take it.
//!
//! - **Curve.** Unity below the threshold; above it, `threshold + over/ratio`.
//!   With a knee, the corner is a quadratic across `kneeDb` centred on the
//!   threshold.
//! - **Times** are one-pole time constants: `exp(−1/(ms/1000 · rate))`.
//! - **Makeup** gain after the reduction.
//!
//! The rack builds its own starting compressor at 1:1, so it is inert until
//! moved; this one starts at the settings' defaults, 4:1 at −18 dB, as a
//! compressor saved without a ratio does. The original worked its time
//! constants out once per block; here when a control moves, which is the same
//! wherever a host moves it between blocks.

use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamSpec};

const THRESHOLD: usize = 0;
const RATIO: usize = 1;
const ATTACK: usize = 2;
const RELEASE: usize = 3;
const KNEE: usize = 4;
const MAKEUP: usize = 5;

/// The detector's level for silence.
const MIN_DB: f32 = -120.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("thresholdDb", "Threshold", -60.0, 0.0, -18.0).unit("dB").smooth(0.0),
    ParamSpec::new("ratio", "Ratio", 1.0, 20.0, 4.0).unit(":1").smooth(0.0),
    ParamSpec::new("attackMs", "Attack", 0.05, 500.0, 10.0).log().unit("ms").smooth(0.0),
    ParamSpec::new("releaseMs", "Release", 5.0, 3000.0, 120.0).log().unit("ms").smooth(0.0),
    ParamSpec::new("kneeDb", "Knee", 0.0, 24.0, 6.0).unit("dB").smooth(0.0),
    ParamSpec::new("makeupDb", "Makeup", -24.0, 24.0, 0.0).unit("dB").smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "compressor",
    code: 101,
    version: 1,
    label: "Compressor",
    category: Category::Dynamics,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Curve", keys: &["thresholdDb", "ratio", "kneeDb"] },
            Group { label: "Times", keys: &["attackMs", "releaseMs"] },
            Group { label: "Level", keys: &["makeupDb"] },
        ],
        primary: &["thresholdDb", "ratio", "attackMs", "releaseMs"],
        displays: &[Display::Meter],
        // Hyperprism's Compressor, as the rack's pad has it: volume across,
        // compression up.
        pad: Some(PadPair { x: "thresholdDb", x_range: (-60.0, 0.0), y: "ratio", y_range: (1.0, 20.0) }),
    },
    source: "MultiGrainLive: rack compressor (fx::comp::Compressor)",
    sound: None,
};

pub struct Compressor {
    sample_rate: f32,
    values: [f32; 6],
    attack: f32,
    release: f32,
    makeup: f32,
    /// The smoothed gain reduction in dB, never above zero.
    envelope_db: f32,
}

impl Compressor {
    /// One-pole coefficient for a time constant.
    fn coeff(&self, ms: f32) -> f32 {
        let t = (ms.max(0.01) / 1000.0) * self.sample_rate;
        (-1.0 / t).exp()
    }

    fn derive(&mut self) {
        self.attack = self.coeff(self.values[ATTACK]);
        self.release = self.coeff(self.values[RELEASE]);
        self.makeup = 10f32.powf(self.values[MAKEUP] / 20.0);
    }

    /// The static curve: level in, level out, both in dB.
    fn curve(&self, level_db: f32) -> f32 {
        let threshold = self.values[THRESHOLD];
        let ratio = self.values[RATIO].max(1.0);
        let knee = self.values[KNEE].max(0.0);
        let over = level_db - threshold;
        if knee > 0.0 && over > -knee / 2.0 && over < knee / 2.0 {
            let x = over + knee / 2.0;
            level_db + (1.0 / ratio - 1.0) * x * x / (2.0 * knee)
        } else if over <= 0.0 {
            level_db
        } else {
            threshold + over / ratio
        }
    }
}

impl Kernel for Compressor {
    fn new(sample_rate: f32) -> Self {
        let mut values = [0.0; 6];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = Compressor {
            // The original took a whole number of hertz, at least one.
            sample_rate: sample_rate.max(1.0),
            values,
            attack: 0.0,
            release: 0.0,
            makeup: 1.0,
            envelope_db: 0.0,
        };
        k.derive();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        self.values[index] = spec.clamp(value);
        self.derive();
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.envelope_db = 0.0;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let mut peak = 0f32;
        for s in frame.iter() {
            let a = s.abs();
            if a > peak {
                peak = a;
            }
        }
        let level_db = if peak > 0.0 { 20.0 * peak.log10() } else { MIN_DB };
        let target_db = (self.curve(level_db) - level_db).min(0.0);
        let c = if target_db < self.envelope_db { self.attack } else { self.release };
        self.envelope_db = target_db + c * (self.envelope_db - target_db);
        if !self.envelope_db.is_finite() {
            self.envelope_db = 0.0;
        }
        let g = 10f32.powf(self.envelope_db / 20.0) * self.makeup;
        frame[0] *= g;
        frame[1] *= g;
    }
}
