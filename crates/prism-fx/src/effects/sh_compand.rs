//! ++compand: SoundHack Freesound's compressor and expander (Tom Erbe, v9.10),
//! as its code does it.
//!
//! A ratio at or under 1 compresses above the threshold with that slope (0.5
//! is 2:1, 0.025 is 40:1); over 1 it expands below the threshold (2 is 1:2).
//! The curve is a table of 8192 gains over 0–1, read with interpolation and
//! averaged with the last gain each sample.
//!
//! - **Knee.** 12 dB × knee wide, centred on the threshold. In it the
//!   compression gain is scaled by how far into the knee the level is, which
//!   lifts the level slightly just below the threshold.
//! - **Detector.** Peak, or RMS over `response` samples, clamped to full
//!   scale. It falls 20 dB per release time and, here, rises at most 20 dB per
//!   attack time.
//! - **Linked.** Both sides follow the louder side's detector and share one
//!   gain.
//!
//! Changed from the original: the attack works — its coefficient came out
//! infinite, so the detector jumped to every peak. The expansion knee takes
//! its top in dB, as the compression knee does (it took the linear level, so
//! the gain stepped at the knee's foot and boosted inside it). Each side has
//! its own RMS window, which drops what it held when it shortens (the
//! original's was shared by both sides and kept stale values in its sum). The
//! stereo output clips at ±1.5 as the mono one did. In and out gain glide.

use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec, Smoothed};

const RATIO: usize = 0;
const THRESHOLD: usize = 1;
const ATTACK: usize = 2;
const RELEASE: usize = 3;
const KNEE: usize = 4;
const RESPONSE: usize = 5;
const RMS: usize = 6;
const LINKED: usize = 7;
const IN_DB: usize = 8;
const OUT_DB: usize = 9;

const CLIP: f32 = 1.5;
const TABLE: usize = 8192;
const LONGEST_RMS: usize = 4096;
/// The detector's floor.
const FLOOR: f32 = 1e-5;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("ratio", "Ratio", 0.025, 4.0, 1.0).log().smooth(0.0),
    ParamSpec::new("threshDb", "Threshold", -60.0, 0.0, -20.0).unit("dB").smooth(0.0),
    ParamSpec::new("attackMs", "Attack", 1.0, 100.0, 5.0).log().unit("ms").smooth(0.0),
    ParamSpec::new("releaseS", "Release", 0.1, 2.0, 0.5).log().unit("s").smooth(0.0),
    ParamSpec::new("knee", "Soft knee", 0.0, 1.0, 0.0).smooth(0.0),
    ParamSpec::new("response", "Response", 64.0, 4_096.0, 480.0)
        .log()
        .kind(ParamKind::Int)
        .unit("samples")
        .smooth(0.0)
        .when("rms", 1.0),
    ParamSpec::new("rms", "RMS detect", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("linked", "Linked", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("inDb", "In", -20.0, 20.0, 0.0).unit("dB"),
    ParamSpec::new("outDb", "Out", 0.0, 60.0, 10.0).unit("dB"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "sh-compand",
    code: 16,
    version: 1,
    label: "++compand",
    category: Category::Dynamics,
    params: PARAMS,
    panel: Panel {
        // The original: six dials in a row, the two switches below.
        groups: &[
            Group { label: "Curve", keys: &["ratio", "threshDb", "knee"] },
            Group { label: "Detector", keys: &["attackMs", "releaseS", "rms", "response", "linked"] },
            Group { label: "Level", keys: &["inDb", "outDb"] },
        ],
        primary: &["ratio", "threshDb", "attackMs", "releaseS"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "threshDb", x_range: (-60.0, 0.0), y: "ratio", y_range: (0.025, 4.0) }),
    },
    source: "SoundHack: ++compand (Freesound 9.10)",
    sound: None,
};

/// One side's detector and gain.
#[derive(Clone, Copy)]
struct Side {
    /// The detector's rising ceiling and falling floor.
    rise: f32,
    fall: f32,
    gain: f32,
    window: [f32; LONGEST_RMS],
    sum: f64,
    at: usize,
}

impl Side {
    const FRESH: Side = Side { rise: 1.0, fall: 0.0, gain: 1.0, window: [0.0; LONGEST_RMS], sum: 0.0, at: 0 };

    fn rms(&mut self, x: f32, length: usize) -> f32 {
        if self.at >= length {
            self.at = 0;
        }
        let square = x * x;
        self.sum += (square - self.window[self.at]) as f64;
        self.window[self.at] = square;
        self.at += 1;
        (self.sum.max(0.0) / length as f64).sqrt() as f32
    }

    /// Follow `d`: up no faster than the ceiling rises, down no faster than
    /// the floor falls.
    fn follow(&mut self, d: f32) -> f32 {
        if d < self.fall {
            self.rise = self.fall;
            self.fall
        } else if d > self.rise {
            self.fall = self.rise;
            self.rise
        } else {
            self.rise = d.max(FLOOR);
            self.fall = self.rise;
            d
        }
    }
}

pub struct ShCompand {
    sample_rate: f32,
    values: [f32; 10],
    in_db: Smoothed,
    out_db: Smoothed,
    table: [f32; TABLE],
    rise: f32,
    fall: f32,
    sides: [Side; 2],
}

impl ShCompand {
    /// The gain curve, from the ratio, threshold and knee.
    fn curve(&mut self) {
        let (r, t, k) = (self.values[RATIO], self.values[THRESHOLD], self.values[KNEE]);
        let db = |d: f32| 10f32.powf(d * 0.05);
        let (knee_lo_db, knee_hi_db) = (t - 6.0 * k, t + 6.0 * k);
        let (knee_lo, knee_hi) = (db(knee_lo_db), db(knee_hi_db));
        let width = 12.0 * k;
        for (i, g) in self.table.iter_mut().enumerate() {
            let level = (i + 1) as f32 / TABLE as f32;
            let level_db = 20.0 * level.log10();
            let exponent = if r <= 1.0 {
                if level <= knee_lo {
                    0.0
                } else if level > knee_hi {
                    (t - level_db) * (1.0 - r) * 0.05
                } else {
                    (t - level_db) * (1.0 - r) * 0.05 * ((level_db - knee_lo_db) / width)
                }
            } else if level > knee_hi {
                0.0
            } else if level <= knee_lo {
                (level_db - t) * (r - 1.0) * 0.05
            } else {
                (level_db - t) * (r - 1.0) * ((knee_hi_db - level_db) / width) * 0.05
            };
            *g = 10f32.powf(exponent);
        }
    }

    fn times(&mut self) {
        let sr = self.sample_rate;
        self.rise = 10f32.powf(1.0 / (self.values[ATTACK] * 0.001 * sr));
        self.fall = 0.1f32.powf(1.0 / (self.values[RELEASE] * sr));
    }

    /// Each side's window keeps only what lies within the new length.
    fn response(&mut self) {
        let length = self.values[RESPONSE] as usize;
        for side in &mut self.sides {
            side.sum = side.window[..length].iter().map(|&v| v as f64).sum();
            if side.at >= length {
                side.at = 0;
            }
        }
    }

    /// The gain for detector level `env`, averaged with the last.
    fn gain(&self, env: f32, last: f32) -> f32 {
        let at = env * (TABLE - 1) as f32;
        let i = at as usize;
        if i >= TABLE - 1 {
            self.table[TABLE - 1]
        } else {
            let t = at - i as f32;
            0.5 * (self.table[i] + (self.table[i + 1] - self.table[i]) * t + last)
        }
    }
}

impl Kernel for ShCompand {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 10];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let glide = |i: usize| Smoothed::new(sample_rate, PARAMS[i].smooth_ms);
        let mut k = ShCompand {
            sample_rate,
            values,
            in_db: glide(IN_DB),
            out_db: glide(OUT_DB),
            table: [1.0; TABLE],
            rise: 1.0,
            fall: 1.0,
            sides: [Side::FRESH; 2],
        };
        k.curve();
        k.times();
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        let v = spec.clamp(value);
        let changed = v != self.values[index];
        self.values[index] = v;
        match index {
            RATIO | THRESHOLD | KNEE if changed => self.curve(),
            ATTACK | RELEASE if changed => self.times(),
            RESPONSE if changed => self.response(),
            IN_DB => self.in_db.set(v),
            OUT_DB => self.out_db.set(v),
            _ => {}
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.in_db.jump(self.values[IN_DB]);
        self.out_db.jump(self.values[OUT_DB]);
        self.sides = [Side::FRESH; 2];
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let db = |d: f32| 10f32.powf(d * 0.05);
        let gain_in = db(self.in_db.next());
        let gain_out = db(self.out_db.next());
        let rms = self.values[RMS] > 0.5;
        let length = self.values[RESPONSE] as usize;
        let x = [frame[0] * gain_in, frame[1] * gain_in];

        let mut env = [0.0f32; 2];
        for (s, side) in self.sides.iter_mut().enumerate() {
            side.rise *= self.rise;
            side.fall *= self.fall;
            let d = if rms { side.rms(x[s], length) } else { x[s].abs() };
            env[s] = side.follow(d.min(1.0));
        }

        if self.values[LINKED] > 0.5 {
            // The quieter side's floor is lifted to the louder side's level.
            let loud = env[0].max(env[1]);
            let quiet = if env[1] > env[0] { 0 } else { 1 };
            self.sides[quiet].fall = loud;
            let g = self.gain(loud, self.sides[0].gain);
            self.sides[0].gain = g;
            self.sides[1].gain = g;
        } else {
            for (s, &e) in env.iter().enumerate() {
                self.sides[s].gain = self.gain(e, self.sides[s].gain);
            }
        }

        for (s, out) in frame.iter_mut().enumerate() {
            *out = (self.sides[s].gain * x[s] * gain_out).clamp(-CLIP, CLIP);
        }
    }
}
