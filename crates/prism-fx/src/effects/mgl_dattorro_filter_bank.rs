//! Dattorro filter bank: MultiGrainLive's `DattorroFilterBank`
//! (`ShapeKind::DattorroFilterBank`), Part 1 of Jon Dattorro's "Effect
//! Design" as one strip, sample for sample.
//!
//! In series, each with its own switch: the notch, the resonator and the
//! Regalia-Mitra EQ (each bandwidth `centre / Q`, each blended by its
//! amplitude; the EQ's gain is `12 dB × amplitude`), then the damping
//! one-pole at `cutoff·√(Q / 0.7)` (held to 20 Hz–20 kHz), blended by its
//! amplitude. The original runs each stage over a whole block before the
//! next; as no stage feeds back into an earlier one, that is the same as
//! running them in turn on each frame.

use super::mgl_dattorro_common::{damping_coefficient, spec, store, whole_rate, FilterMode, Musical};
use crate::{Category, Descriptor, Display, Group, Kernel, Panel, ParamKind, ParamSpec};

static PARAMS: &[ParamSpec] = &[
    spec("notchOn", "Notch", 0.0, 1.0, 1.0).kind(ParamKind::Toggle),
    spec("notchHz", "Notch frequency", 20.0, 20000.0, 500.0).log().unit("Hz"),
    spec("notchQ", "Notch Q", 0.2, 18.0, 4.0),
    spec("notchAmp", "Notch amplitude", 0.0, 1.0, 1.0),
    spec("resonatorOn", "Resonator", 0.0, 1.0, 1.0).kind(ParamKind::Toggle),
    spec("resonatorHz", "Resonator frequency", 20.0, 20000.0, 1200.0).log().unit("Hz"),
    spec("resonatorQ", "Resonator Q", 0.2, 18.0, 5.0),
    spec("resonatorAmp", "Resonator amplitude", 0.0, 1.0, 0.7),
    spec("regaliaOn", "Regalia-Mitra", 0.0, 1.0, 1.0).kind(ParamKind::Toggle),
    spec("regaliaHz", "Regalia frequency", 20.0, 20000.0, 3000.0).log().unit("Hz"),
    spec("regaliaQ", "Regalia Q", 0.2, 18.0, 2.0),
    spec("regaliaAmp", "Regalia amplitude", 0.0, 1.0, 0.7),
    spec("dampingOn", "Damping", 0.0, 1.0, 1.0).kind(ParamKind::Toggle),
    spec("dampingHz", "Damping cutoff", 20.0, 20000.0, 6000.0).log().unit("Hz"),
    spec("dampingQ", "Damping Q", 0.2, 18.0, 0.7),
    spec("dampingAmp", "Damping amplitude", 0.0, 1.0, 1.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "dattorro-filter-bank",
    code: 95,
    version: 1,
    label: "Dattorro filter bank",
    category: Category::Filter,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Notch", keys: &["notchOn", "notchHz", "notchQ", "notchAmp"] },
            Group { label: "Resonator", keys: &["resonatorOn", "resonatorHz", "resonatorQ", "resonatorAmp"] },
            Group { label: "Regalia-Mitra", keys: &["regaliaOn", "regaliaHz", "regaliaQ", "regaliaAmp"] },
            Group { label: "Damping", keys: &["dampingOn", "dampingHz", "dampingQ", "dampingAmp"] },
        ],
        primary: &["notchHz", "resonatorHz", "regaliaHz", "dampingHz"],
        displays: &[Display::Curve],
        pad: None,
    },
    source: "MultiGrainLive: Dattorro filter bank (dattorro.rs, DattorroFilterBank)",
    sound: None,
};

pub struct MglDattorroFilterBank {
    p: [f32; 16],
    sr: u32,
    notch: Musical,
    resonator: Musical,
    regalia: Musical,
    /// The damping stage's coefficient, wet and dry, and its state.
    damping: (f32, f32, f32),
    z: [f32; 2],
}

impl MglDattorroFilterBank {
    fn apply(&mut self) {
        let p = &self.p;
        self.notch.frequency = p[1];
        self.notch.bandwidth = p[1] / p[2];
        self.notch.mix = p[3];
        self.notch.update();
        self.resonator.frequency = p[5];
        self.resonator.bandwidth = p[5] / p[6];
        self.resonator.mix = p[7];
        self.resonator.update();
        self.regalia.frequency = p[9];
        self.regalia.bandwidth = p[9] / p[10];
        self.regalia.gain_db = 12.0 * p[11];
        self.regalia.mix = p[11];
        self.regalia.update();
        let cutoff = (p[13] * (p[14] / 0.7).sqrt()).clamp(20.0, 20000.0);
        self.damping = (damping_coefficient(cutoff, self.sr), p[15], 1.0 - p[15]);
    }
}

impl Kernel for MglDattorroFilterBank {
    fn new(sample_rate: f32) -> Self {
        let sr = whole_rate(sample_rate);
        let mut k = MglDattorroFilterBank {
            p: [1.0, 500.0, 4.0, 1.0, 1.0, 1200.0, 5.0, 0.7, 1.0, 3000.0, 2.0, 0.7, 1.0, 6000.0, 0.7, 1.0],
            sr,
            notch: Musical::new(FilterMode::Notch, sr),
            resonator: Musical::new(FilterMode::Resonator, sr),
            regalia: Musical::new(FilterMode::Regalia, sr),
            damping: (0.0, 0.0, 0.0),
            z: [0.0; 2],
        };
        k.apply();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if store(PARAMS, &mut self.p, index, value) {
            self.apply();
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.p.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.notch.clear();
        self.resonator.clear();
        self.regalia.clear();
        self.z = [0.0; 2];
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let on = |v: f32| v >= 0.5;
        for (ch, out) in frame.iter_mut().enumerate() {
            let mut x = *out;
            if on(self.p[0]) {
                x = self.notch.run(ch, x);
            }
            if on(self.p[4]) {
                x = self.resonator.run(ch, x);
            }
            if on(self.p[8]) {
                x = self.regalia.run(ch, x);
            }
            if on(self.p[12]) {
                let (a, wet, dry) = self.damping;
                self.z[ch] += a * (x - self.z[ch]);
                x = x * dry + self.z[ch] * wet;
            }
            *out = x;
        }
    }

    fn tail_seconds(&self) -> f32 {
        0.5
    }
}
