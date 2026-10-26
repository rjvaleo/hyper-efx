//! Gate: MultiGrainLive's live version of Peak's Strip Silence
//! (`fx::shape::Gate`), moved here unchanged.
//!
//! The louder side drives an envelope that rises at the attack's rate and
//! falls at the release's (one-pole, 1 − e^(−1/(time·rate))). While it is at
//! or over the threshold the gate's opening heads for fully open at the
//! attack rate, otherwise for shut at the release rate; both sides are scaled
//! by 1 − depth·(1 − opening), so depth sets how far it closes. It starts
//! shut.

use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamSpec};

const THRESHOLD: usize = 0;
const ATTACK: usize = 1;
const RELEASE: usize = 2;
const DEPTH: usize = 3;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("thresholdDb", "Threshold", -80.0, 0.0, -40.0).unit("dB").smooth(0.0),
    ParamSpec::new("attackMs", "Attack", 0.1, 200.0, 3.0).log().unit("ms").smooth(0.0),
    ParamSpec::new("releaseMs", "Release", 5.0, 2000.0, 120.0).log().unit("ms").smooth(0.0),
    ParamSpec::new("depth", "Depth", 0.0, 1.0, 1.0).smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "gate",
    code: 73,
    version: 1,
    label: "Gate",
    category: Category::Dynamics,
    params: PARAMS,
    panel: Panel {
        groups: &[Group { label: "Gate", keys: &["thresholdDb", "attackMs", "releaseMs", "depth"] }],
        primary: &["thresholdDb", "releaseMs"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "attackMs", x_range: (0.1, 200.0), y: "thresholdDb", y_range: (-80.0, 0.0) }),
    },
    source: "MultiGrainLive: Gate (fx::shape::Gate)",
    sound: None,
};

pub struct Gate {
    sample_rate: f32,
    values: [f32; 4],
    thresh: f32,
    a: f32,
    r: f32,
    depth: f32,
    env: f32,
    open: f32,
}

impl Gate {
    fn derive(&mut self) {
        let sr = self.sample_rate;
        let v = self.values;
        self.thresh = 10f32.powf(v[THRESHOLD].clamp(-80.0, 0.0) / 20.0);
        self.a = 1.0 - (-1.0 / ((v[ATTACK].clamp(0.1, 200.0) / 1000.0) * sr)).exp();
        self.r = 1.0 - (-1.0 / ((v[RELEASE].clamp(5.0, 2000.0) / 1000.0) * sr)).exp();
        self.depth = v[DEPTH].clamp(0.0, 1.0);
    }
}

impl Kernel for Gate {
    fn new(sample_rate: f32) -> Self {
        let mut values = [0.0; 4];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = Gate {
            sample_rate: sample_rate.max(1.0),
            values,
            thresh: 0.0,
            a: 0.0,
            r: 0.0,
            depth: 0.0,
            env: 0.0,
            open: 0.0,
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
        self.env = 0.0;
        self.open = 0.0;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let mut loud = 0f32;
        for s in frame.iter() {
            loud = loud.max(s.abs());
        }
        let k = if loud > self.env { self.a } else { self.r };
        self.env += k * (loud - self.env);

        let want = if self.env >= self.thresh { 1.0 } else { 0.0 };
        let k = if want > self.open { self.a } else { self.r };
        self.open += k * (want - self.open);

        let g = 1.0 - self.depth * (1.0 - self.open);
        for s in frame.iter_mut() {
            *s *= g;
        }
    }
}
