//! Hyperprism Noise Gate: Hyperprism-PPC 1.2.1's process 37 (class ENoiseGate,
//! routine `NoiseGate`), as its code does it.
//!
//! - **Detector.** The power of the mid: m = ½(L + R) in full-scale units
//!   (the original divides its 16-bit samples by 32768), det = 2.8284272·m².
//! - **Threshold.** 10^(dB/5), compared with that power. So the dial is
//!   effectively doubled in dBFS: at −20 dB a sine opens at about −41.5 dBFS.
//!   The start value, 0 dB, is 1.0 in power, which only a signal over about
//!   −1.5 dBFS reaches.
//! - **Two states, two detectors.** Closed: the gain falls as g ← (1 − aA)·g
//!   and a detector rises at the release speed, env ← aR·det + (1 − aR)·env;
//!   when it passes the threshold the gate opens, its open detector reset to
//!   1.0. Open: the gain rises as g ← (1 − aR)·g + aR and the open detector
//!   follows at the attack speed; when it falls to the threshold the gate
//!   closes, its closed detector reset to 0. The resets are the hysteresis:
//!   the fall from 1.0 holds the gate open a while. It always closes to
//!   silence.
//! - **Times.** a = `CalcLowPassParameter(ms ÷ 100, ½)` at the audio rate: the
//!   time constant grows with the square of the dial (250 → 28 ms,
//!   1000 → 447 ms), and under 4.5 the coefficient is 1.1547, past one, so a
//!   fade alternates in sign as it dies — the original's formula, kept.
//!
//! "Attack" sets the fade out and the open detector's fall; "Release" the fade
//! in and the closed detector's rise — the reverse of the usual names. The
//! code uses them that way consistently on both the gain and the detectors,
//! so it reads as the design rather than a slip, and it is kept.
//!
//! It starts open with its open detector at 0, so it closes on the first
//! quiet sample. The window's Smoothing slider is left out: the routine never
//! reads it. A mono input in the original is the same as equal sides here.

use super::hp_stereo_shared::low_pass_coefficient;
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const ATTACK: usize = 0;
const THRESHOLD: usize = 1;
const RELEASE: usize = 2;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("attackMs", "Attack", 0.0, 1_000.0, 250.0).unit("ms").smooth(0.0),
    ParamSpec::new("thresholdDb", "Threshold", -96.0, 0.0, 0.0).unit("dB").smooth(0.0),
    ParamSpec::new("releaseMs", "Release", 0.0, 1_000.0, 250.0).unit("ms").kind(ParamKind::Int).smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-noise-gate",
    code: 62,
    version: 1,
    label: "Hyperprism Noise Gate",
    category: Category::Dynamics,
    params: PARAMS,
    panel: Panel {
        // The original: the X/Y pad, attack across and threshold up, and the
        // release slider in its settings.
        groups: &[
            Group { label: "Gate", keys: &["attackMs", "thresholdDb"] },
            Group { label: "Settings", keys: &["releaseMs"] },
        ],
        primary: &["attackMs", "thresholdDb", "releaseMs"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "attackMs", x_range: (0.0, 1_000.0), y: "thresholdDb", y_range: (-96.0, 0.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Noise Gate",
    sound: None,
};

pub struct HpNoiseGate {
    sample_rate: f32,
    values: [f32; 3],
    threshold: f32,
    /// Attack and release coefficients and their complements.
    a_attack: f32,
    b_attack: f32,
    a_release: f32,
    b_release: f32,
    open: bool,
    gain_open: f32,
    gain_closed: f32,
    env_open: f32,
    env_closed: f32,
}

impl HpNoiseGate {
    fn coefficients(&self, ms: f32) -> (f32, f32) {
        let a = low_pass_coefficient(0.01 * ms as f64, 0.5, self.sample_rate as f64) as f32;
        (a, (1.0 - a as f64) as f32)
    }

    fn update(&mut self) {
        self.threshold = 10f64.powf(0.2 * self.values[THRESHOLD] as f64) as f32;
        (self.a_attack, self.b_attack) = self.coefficients(self.values[ATTACK]);
        (self.a_release, self.b_release) = self.coefficients(self.values[RELEASE]);
    }
}

impl Kernel for HpNoiseGate {
    fn new(sample_rate: f32) -> Self {
        let mut values = [0.0; 3];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpNoiseGate {
            sample_rate: sample_rate.max(1.0),
            values,
            threshold: 1.0,
            a_attack: 0.0,
            b_attack: 0.0,
            a_release: 0.0,
            b_release: 0.0,
            open: true,
            gain_open: 1.0,
            gain_closed: 0.0,
            env_open: 0.0,
            env_closed: 0.0,
        };
        k.update();
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        self.values[index] = spec.clamp(value);
        self.update();
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    /// `InitializeNoiseGate`, and the state counter as `InitializeCounters`
    /// leaves it: open.
    fn reset(&mut self) {
        self.open = true;
        self.gain_open = 1.0;
        self.gain_closed = 0.0;
        self.env_open = 0.0;
        self.env_closed = 1.0;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let (l, r) = (frame[0] as f64, frame[1] as f64);
        let m = 0.5 * (l + r);
        let det = 2.828_427_2 * m * m;
        let gain = if self.open {
            let g = self.b_release * self.gain_open + self.a_release;
            self.gain_open = g;
            let env = self.a_attack as f64 * det + (self.b_attack * self.env_open) as f64;
            self.env_open = env as f32;
            if env <= self.threshold as f64 {
                self.open = false;
                self.gain_closed = self.gain_open;
                self.env_closed = 0.0;
            }
            g
        } else {
            let g = self.b_attack * self.gain_closed;
            self.gain_closed = g;
            let env = self.a_release as f64 * det + (self.b_release * self.env_closed) as f64;
            self.env_closed = env as f32;
            if env > self.threshold as f64 {
                self.open = true;
                self.gain_open = self.gain_closed;
                self.env_open = 1.0;
            }
            g
        };
        frame[0] = (l * gain as f64) as f32;
        frame[1] = (r * gain as f64) as f32;
    }
}
