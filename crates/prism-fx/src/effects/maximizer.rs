//! Maximizer: MultiGrainLive's one-knob channel compressor
//! (`fx::master::Maximizer`), moved here unchanged.
//!
//! One knob, the amount, sets everything: a ratio of 20^amount, a knee of
//! 10 − 9·amount dB, an attack of 41 − 40·amount ms and a release of
//! 440 − 400·amount ms. The detector is the louder side's peak in dB. A slow
//! estimate of the material's level jumps up to any louder peak and falls
//! back over 250 ms, seeded from the first frame with any level in it.
//!
//! - **Auto compression** puts the threshold 6 − 21·amount dB from that
//!   estimate (held to −60…0 dB); off, it is −24·amount dB.
//! - **Auto level** walks the makeup gain, over 900 ms, toward whatever puts
//!   the compressed level at the ceiling (0–30 dB); off, the makeup is what
//!   the curve takes from a full-scale signal.
//!
//! The gain reduction follows its target at the attack or release rate, and
//! the output is finally clamped to the ceiling. At an amount of 0.0001 or
//! less it does nothing and its state waits. The ceiling and the two switches
//! were the channel strip's settings; here, as on MultiGrainLive's module,
//! they are controls like the amount.

use crate::{Category, Descriptor, Display, Group, Kernel, Panel, ParamKind, ParamSpec};

const AMOUNT: usize = 0;
const CEILING: usize = 1;
const AUTO_LEVEL: usize = 2;
const AUTO_COMP: usize = 3;

const MIN_DB: f32 = -120.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("amount", "Amount", 0.0, 1.0, 0.5).smooth(0.0),
    ParamSpec::new("ceilingDb", "Ceiling", -24.0, 0.0, -0.3).unit("dB").smooth(0.0),
    ParamSpec::new("autoLevel", "Auto level", 0.0, 1.0, 1.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("autoComp", "Auto compression", 0.0, 1.0, 1.0).kind(ParamKind::Toggle).smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "maximizer",
    code: 75,
    version: 1,
    label: "Maximizer",
    category: Category::Dynamics,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Maximizer", keys: &["amount", "ceilingDb"] },
            Group { label: "Auto", keys: &["autoLevel", "autoComp"] },
        ],
        primary: &["amount"],
        displays: &[Display::Meter],
        pad: None,
    },
    source: "MultiGrainLive: Maximizer (fx::master::Maximizer)",
    sound: None,
};

/// One-pole smoothing coefficient for a time constant.
fn coeff(ms: f32, sample_rate: f32) -> f32 {
    let t = (ms.max(0.01) / 1000.0) * sample_rate;
    (-1.0 / t).exp()
}

fn to_db(x: f32) -> f32 {
    if x > 1e-9 {
        20.0 * x.log10()
    } else {
        MIN_DB
    }
}

pub struct Maximizer {
    sample_rate: f32,
    values: [f32; 4],
    // From the controls.
    active: bool,
    ratio: f32,
    knee: f32,
    under_db: f32,
    fixed_db: f32,
    atk: f32,
    rel: f32,
    slow: f32,
    walk: f32,
    ceiling_lin: f32,
    // State.
    envelope_db: f32,
    level_db: f32,
    makeup_db: f32,
    started: bool,
}

impl Maximizer {
    fn derive(&mut self) {
        let amount = self.values[AMOUNT];
        let a = amount.clamp(0.0, 1.0);
        self.active = amount > 1e-4;
        self.ratio = 20f32.powf(a).max(1.0);
        self.knee = (9.0 * (1.0 - a) + 1.0).max(0.0);
        self.under_db = 6.0 - 21.0 * a;
        self.fixed_db = -24.0 * a;
        let sr = self.sample_rate;
        self.atk = coeff(40.0 * (1.0 - a) + 1.0, sr);
        self.rel = coeff(400.0 * (1.0 - a) + 40.0, sr);
        self.slow = coeff(250.0, sr);
        self.walk = coeff(900.0, sr);
        self.ceiling_lin = 10f32.powf(self.values[CEILING] / 20.0);
    }

    /// The static curve: level in, level out, both dB.
    fn curve(&self, level_db: f32, threshold_db: f32) -> f32 {
        let (ratio, knee) = (self.ratio, self.knee);
        let over = level_db - threshold_db;
        if knee > 0.0 && over > -knee / 2.0 && over < knee / 2.0 {
            let x = over + knee / 2.0;
            level_db + (1.0 / ratio - 1.0) * x * x / (2.0 * knee)
        } else if over <= 0.0 {
            level_db
        } else {
            threshold_db + over / ratio
        }
    }
}

impl Kernel for Maximizer {
    fn new(sample_rate: f32) -> Self {
        let mut values = [0.0; 4];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = Maximizer {
            // MultiGrainLive's rate is a whole number of hertz.
            sample_rate: (sample_rate.max(1.0) as u32).max(1) as f32,
            values,
            active: false,
            ratio: 1.0,
            knee: 0.0,
            under_db: 0.0,
            fixed_db: 0.0,
            atk: 0.0,
            rel: 0.0,
            slow: 0.0,
            walk: 0.0,
            ceiling_lin: 1.0,
            envelope_db: 0.0,
            level_db: MIN_DB,
            makeup_db: 0.0,
            started: false,
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
        self.envelope_db = 0.0;
        self.level_db = MIN_DB;
        self.makeup_db = 0.0;
        self.started = false;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if !self.active {
            return;
        }
        let ceiling = self.values[CEILING];

        let mut peak = 0f32;
        for s in frame.iter() {
            let a = s.abs();
            if a > peak {
                peak = a;
            }
        }
        let level_db = to_db(peak);

        if !self.started && level_db > MIN_DB + 1.0 {
            self.level_db = level_db;
            self.started = true;
        }
        if level_db > self.level_db {
            self.level_db = level_db;
        } else {
            self.level_db = level_db + self.slow * (self.level_db - level_db);
        }

        let threshold_db = if self.values[AUTO_COMP] >= 0.5 {
            (self.level_db + self.under_db).clamp(-60.0, 0.0)
        } else {
            self.fixed_db
        };

        let target_db = (self.curve(level_db, threshold_db) - level_db).min(0.0);
        let c = if target_db < self.envelope_db { self.atk } else { self.rel };
        self.envelope_db = target_db + c * (self.envelope_db - target_db);
        if !self.envelope_db.is_finite() {
            self.envelope_db = 0.0;
        }

        let want_db = if self.values[AUTO_LEVEL] >= 0.5 {
            (ceiling - (self.level_db + self.envelope_db)).clamp(0.0, 30.0)
        } else {
            (-self.curve(0.0, threshold_db)).clamp(0.0, 30.0)
        };
        self.makeup_db = want_db + self.walk * (self.makeup_db - want_db);
        if !self.makeup_db.is_finite() {
            self.makeup_db = 0.0;
        }

        let g = 10f32.powf((self.envelope_db + self.makeup_db) / 20.0);
        let lin = self.ceiling_lin;
        for s in frame.iter_mut() {
            *s = (*s * g).clamp(-lin, lin);
        }
    }
}
