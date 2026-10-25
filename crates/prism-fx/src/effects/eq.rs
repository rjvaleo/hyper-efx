//! EQ: the parametric EQ in MultiGrainLive's rack (`fx::eq::Eq` on
//! `fx::biquad`), sample for sample, every setting `EqSettings` carries as a
//! control.
//!
//! Eight nodes in a fixed order, each an RBJ-cookbook biquad in direct form I
//! with its own type — high-pass, low shelf, bell, notch, high shelf or
//! low-pass — and its own switch. Out of the box: a rumble high-pass (off at
//! 20 Hz and below), a low shelf at 100 Hz, a bell at 1 kHz, three more bells
//! switched off at 250 Hz, 500 Hz and 2 kHz, a high shelf at 8 kHz, and a
//! low-pass at 20 kHz, switched off.
//!
//! - **Keys** are the rack's own, `band.<n>.<freq|q|gainDb|enabled|mode>`,
//!   with its ranges: 10 Hz–24 kHz, Q 0.05–18, ±24 dB. Frequencies are kept
//!   under 0.49 of the sample rate.
//! - **The end nodes** have no Q or gain of their own, as in the original —
//!   they run at Q 0.707 and 0 dB whatever their type — so they offer only a
//!   frequency, a type and a switch. The first node passes everything while
//!   its frequency is 20 Hz or below, whatever its type; the rack stores 0 Hz
//!   for that, which sounds the same as this control's 10 Hz floor.
//! - **Notch** is the bell's formula, as it was in the original: it cuts as
//!   deep as its gain says. Gain acts for the shelves, the bell and the notch,
//!   not the passes.
//! - **Flat.** While no node has anything to do — every one off, at 0 dB, or a
//!   pass out of the way — the sound passes untouched and the filters are
//!   emptied, so a fresh EQ is exactly transparent.
//!
//! The original rebuilt its coefficients at the start of a block when the
//! settings had changed; here they are rebuilt when a control moves, which is
//! the same wherever a host moves it between blocks.

use crate::{Category, Descriptor, Display, Group, Kernel, Panel, ParamKind, ParamSpec};
use core::f32::consts::PI;

const BANDS: usize = 8;
const FREQ_MIN: f32 = 10.0;
const FREQ_MAX: f32 = 24_000.0;
/// At or below this the first node's high-pass does nothing.
const HP_OFF: f32 = 20.0;
/// At or above this a low-pass does nothing.
const LP_OFF: f32 = 20_000.0;
/// The Q and gain of the end nodes, which have none of their own.
const END_Q: f32 = 0.707;

const HIGH_PASS: usize = 0;
const LOW_SHELF: usize = 1;
const BELL: usize = 2;
const NOTCH: usize = 3;
const HIGH_SHELF: usize = 4;
const LOW_PASS: usize = 5;
const MODES: &[&str] = &["High-pass", "Low shelf", "Bell", "Notch", "High shelf", "Low-pass"];

const fn freq(key: &'static str, label: &'static str, default: f32) -> ParamSpec {
    ParamSpec::new(key, label, FREQ_MIN, FREQ_MAX, default).log().unit("Hz").smooth(0.0)
}
const fn q(key: &'static str, label: &'static str, default: f32) -> ParamSpec {
    ParamSpec::new(key, label, 0.05, 18.0, default).log().smooth(0.0)
}
const fn gain(key: &'static str, label: &'static str) -> ParamSpec {
    ParamSpec::new(key, label, -24.0, 24.0, 0.0).unit("dB").smooth(0.0)
}
const fn enabled(key: &'static str, label: &'static str, on: bool) -> ParamSpec {
    ParamSpec::new(key, label, 0.0, 1.0, if on { 1.0 } else { 0.0 }).kind(ParamKind::Toggle).smooth(0.0)
}
const fn mode(key: &'static str, label: &'static str, default: usize) -> ParamSpec {
    ParamSpec::new(key, label, 0.0, 5.0, default as f32).kind(ParamKind::Choice(MODES)).smooth(0.0)
}

/// Named for what each node is out of the box, as the rack's menus name them.
static PARAMS: &[ParamSpec] = &[
    freq("band.0.freq", "High-pass frequency", FREQ_MIN),
    enabled("band.0.enabled", "High-pass on/off", true),
    mode("band.0.mode", "High-pass type", HIGH_PASS),
    freq("band.1.freq", "Low shelf frequency", 100.0),
    q("band.1.q", "Low shelf Q", 0.7),
    gain("band.1.gainDb", "Low shelf gain"),
    enabled("band.1.enabled", "Low shelf on/off", true),
    mode("band.1.mode", "Low shelf type", LOW_SHELF),
    freq("band.2.freq", "Bell 2 frequency", 1000.0),
    q("band.2.q", "Bell 2 Q", 1.0),
    gain("band.2.gainDb", "Bell 2 gain"),
    enabled("band.2.enabled", "Bell 2 on/off", true),
    mode("band.2.mode", "Bell 2 type", BELL),
    freq("band.3.freq", "Bell 3 frequency", 250.0),
    q("band.3.q", "Bell 3 Q", 1.0),
    gain("band.3.gainDb", "Bell 3 gain"),
    enabled("band.3.enabled", "Bell 3 on/off", false),
    mode("band.3.mode", "Bell 3 type", BELL),
    freq("band.4.freq", "Bell 4 frequency", 500.0),
    q("band.4.q", "Bell 4 Q", 1.0),
    gain("band.4.gainDb", "Bell 4 gain"),
    enabled("band.4.enabled", "Bell 4 on/off", false),
    mode("band.4.mode", "Bell 4 type", BELL),
    freq("band.5.freq", "Bell 5 frequency", 2000.0),
    q("band.5.q", "Bell 5 Q", 1.0),
    gain("band.5.gainDb", "Bell 5 gain"),
    enabled("band.5.enabled", "Bell 5 on/off", false),
    mode("band.5.mode", "Bell 5 type", BELL),
    freq("band.6.freq", "High shelf frequency", 8000.0),
    q("band.6.q", "High shelf Q", 0.7),
    gain("band.6.gainDb", "High shelf gain"),
    enabled("band.6.enabled", "High shelf on/off", true),
    mode("band.6.mode", "High shelf type", HIGH_SHELF),
    freq("band.7.freq", "Low-pass frequency", LP_OFF),
    enabled("band.7.enabled", "Low-pass on/off", false),
    mode("band.7.mode", "Low-pass type", LOW_PASS),
];

/// Where each node's controls sit in `PARAMS`.
struct Layout {
    freq: usize,
    q: Option<usize>,
    gain: Option<usize>,
    enabled: usize,
    mode: usize,
}

const fn middle(band: usize) -> Layout {
    let at = 3 + (band - 1) * 5;
    Layout { freq: at, q: Some(at + 1), gain: Some(at + 2), enabled: at + 3, mode: at + 4 }
}

const LAYOUT: [Layout; BANDS] = [
    Layout { freq: 0, q: None, gain: None, enabled: 1, mode: 2 },
    middle(1),
    middle(2),
    middle(3),
    middle(4),
    middle(5),
    middle(6),
    Layout { freq: 33, q: None, gain: None, enabled: 34, mode: 35 },
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "eq",
    code: 100,
    version: 1,
    label: "EQ",
    category: Category::Filter,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "High-pass", keys: &["band.0.freq", "band.0.enabled", "band.0.mode"] },
            Group {
                label: "Low shelf",
                keys: &["band.1.freq", "band.1.q", "band.1.gainDb", "band.1.enabled", "band.1.mode"],
            },
            Group { label: "Bell 2", keys: &["band.2.freq", "band.2.q", "band.2.gainDb", "band.2.enabled", "band.2.mode"] },
            Group { label: "Bell 3", keys: &["band.3.freq", "band.3.q", "band.3.gainDb", "band.3.enabled", "band.3.mode"] },
            Group { label: "Bell 4", keys: &["band.4.freq", "band.4.q", "band.4.gainDb", "band.4.enabled", "band.4.mode"] },
            Group { label: "Bell 5", keys: &["band.5.freq", "band.5.q", "band.5.gainDb", "band.5.enabled", "band.5.mode"] },
            Group {
                label: "High shelf",
                keys: &["band.6.freq", "band.6.q", "band.6.gainDb", "band.6.enabled", "band.6.mode"],
            },
            Group { label: "Low-pass", keys: &["band.7.freq", "band.7.enabled", "band.7.mode"] },
        ],
        primary: &["band.1.gainDb", "band.2.freq", "band.2.gainDb", "band.6.gainDb"],
        displays: &[Display::Curve],
        pad: None,
    },
    source: "MultiGrainLive: rack EQ (fx::eq::Eq, RBJ cookbook biquads)",
    sound: None,
};

/// Direct-form-I coefficients, normalised by a0.
#[derive(Clone, Copy)]
struct Coeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Coeffs {
    const IDENTITY: Coeffs = Coeffs { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    fn normalise(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        if a0.abs() < 1e-20 || !a0.is_finite() {
            return Coeffs::IDENTITY;
        }
        Coeffs { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    /// `cos ω₀`, `α` and `A`, with the frequency kept under 0.49 of the rate.
    fn terms(freq: f32, q: f32, gain_db: f32, sr: f32) -> (f32, f32, f32) {
        let f = freq.clamp(1.0, sr * 0.49);
        let q = q.max(0.05);
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * f / sr;
        let alpha = w0.sin() / (2.0 * q);
        (w0.cos(), alpha, a)
    }

    fn peaking(freq: f32, q: f32, gain_db: f32, sr: f32) -> Self {
        let (cos_w0, alpha, a) = Coeffs::terms(freq, q, gain_db, sr);
        Coeffs::normalise(
            1.0 + alpha * a,
            -2.0 * cos_w0,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cos_w0,
            1.0 - alpha / a,
        )
    }

    fn low_shelf(freq: f32, q: f32, gain_db: f32, sr: f32) -> Self {
        let (cos_w0, alpha, a) = Coeffs::terms(freq, q, gain_db, sr);
        let sq = 2.0 * a.sqrt() * alpha;
        Coeffs::normalise(
            a * ((a + 1.0) - (a - 1.0) * cos_w0 + sq),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0),
            a * ((a + 1.0) - (a - 1.0) * cos_w0 - sq),
            (a + 1.0) + (a - 1.0) * cos_w0 + sq,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0),
            (a + 1.0) + (a - 1.0) * cos_w0 - sq,
        )
    }

    fn high_shelf(freq: f32, q: f32, gain_db: f32, sr: f32) -> Self {
        let (cos_w0, alpha, a) = Coeffs::terms(freq, q, gain_db, sr);
        let sq = 2.0 * a.sqrt() * alpha;
        Coeffs::normalise(
            a * ((a + 1.0) + (a - 1.0) * cos_w0 + sq),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0),
            a * ((a + 1.0) + (a - 1.0) * cos_w0 - sq),
            (a + 1.0) - (a - 1.0) * cos_w0 + sq,
            2.0 * ((a - 1.0) - (a + 1.0) * cos_w0),
            (a + 1.0) - (a - 1.0) * cos_w0 - sq,
        )
    }

    fn high_pass(freq: f32, q: f32, sr: f32) -> Self {
        let (cos_w0, alpha, _) = Coeffs::terms(freq, q, 0.0, sr);
        Coeffs::normalise(
            (1.0 + cos_w0) / 2.0,
            -(1.0 + cos_w0),
            (1.0 + cos_w0) / 2.0,
            1.0 + alpha,
            -2.0 * cos_w0,
            1.0 - alpha,
        )
    }

    fn low_pass(freq: f32, q: f32, sr: f32) -> Self {
        let (cos_w0, alpha, _) = Coeffs::terms(freq, q, 0.0, sr);
        Coeffs::normalise(
            (1.0 - cos_w0) / 2.0,
            1.0 - cos_w0,
            (1.0 - cos_w0) / 2.0,
            1.0 + alpha,
            -2.0 * cos_w0,
            1.0 - alpha,
        )
    }
}

/// One node's memory, for one side.
#[derive(Clone, Copy, Default)]
struct State {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl State {
    #[inline]
    fn step(&mut self, c: &Coeffs, x: f32) -> f32 {
        let y = c.b0 * x + c.b1 * self.x1 + c.b2 * self.x2 - c.a1 * self.y1 - c.a2 * self.y2;
        // A non-finite value becomes silence; a denormal-sized one, zero.
        let y = if y.is_finite() { if y.abs() < 1e-25 { 0.0 } else { y } } else { 0.0 };
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

pub struct Eq {
    sample_rate: f32,
    values: [f32; 36],
    coeffs: [Coeffs; BANDS],
    states: [[State; BANDS]; 2],
    flat: bool,
}

impl Eq {
    /// Each node's frequency, Q, gain, switch and type, as the original's
    /// `bands()` gave them.
    fn node(&self, band: usize) -> (f32, f32, f32, bool, usize) {
        let l = &LAYOUT[band];
        let v = &self.values;
        (
            v[l.freq],
            l.q.map_or(END_Q, |i| v[i]),
            l.gain.map_or(0.0, |i| v[i]),
            v[l.enabled] >= 0.5,
            v[l.mode] as usize,
        )
    }

    fn rebuild(&mut self) {
        let sr = self.sample_rate;
        let mut flat = true;
        for band in 0..BANDS {
            let (f, q, g, on, mode) = self.node(band);
            flat &= !on
                || match mode {
                    HIGH_PASS => f <= HP_OFF,
                    LOW_PASS => f >= LP_OFF,
                    _ => g.abs() < 1e-6,
                };
            self.coeffs[band] = if !on || (band == 0 && f <= HP_OFF) {
                Coeffs::IDENTITY
            } else {
                let f = if band == 0 { f.max(HP_OFF) } else { f };
                match mode {
                    HIGH_PASS => Coeffs::high_pass(f, q, sr),
                    LOW_SHELF => Coeffs::low_shelf(f, q, g, sr),
                    BELL | NOTCH => Coeffs::peaking(f, q, g, sr),
                    HIGH_SHELF => Coeffs::high_shelf(f, q, g, sr),
                    _ => Coeffs::low_pass(f, q, sr),
                }
            };
        }
        self.flat = flat;
        if flat {
            self.states = [[State::default(); BANDS]; 2];
        }
    }
}

impl Kernel for Eq {
    fn new(sample_rate: f32) -> Self {
        let mut values = [0.0; 36];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = Eq {
            sample_rate: sample_rate.max(1.0),
            values,
            coeffs: [Coeffs::IDENTITY; BANDS],
            states: [[State::default(); BANDS]; 2],
            flat: true,
        };
        k.rebuild();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        let v = spec.clamp(value);
        if v != self.values[index] {
            self.values[index] = v;
            self.rebuild();
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.states = [[State::default(); BANDS]; 2];
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.flat {
            return;
        }
        for (out, states) in frame.iter_mut().zip(self.states.iter_mut()) {
            let mut v = *out;
            for (state, c) in states.iter_mut().zip(&self.coeffs) {
                v = state.step(c, v);
            }
            *out = v;
        }
    }
}
