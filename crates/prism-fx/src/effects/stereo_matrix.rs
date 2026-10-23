//! Stereo Matrix: Hyperprism's four stereo processes in one effect.
//!
//! Pan, Stereo Dynamics, More Stereo and MS Matrix are all gains and a
//! rotation on mid and side, so they share one chain:
//!
//! 1. split into mid (L+R)/2 and side (L−R)/2;
//! 2. take the side's low end out below `monoBelowHz` — More Stereo's low cut,
//!    which keeps the bass in the middle when the rest is widened;
//! 3. scale mid and side (Stereo Dynamics' two gains, More Stereo's level);
//! 4. rotate the two, MS Matrix's stereo position: at 90° mid and side trade
//!    places;
//! 5. back to left and right, then Pan's balance and gain.
//!
//! At its defaults it is transparent, and the low cut at its lowest setting
//! is off rather than merely low.

use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamSpec, Smoothed};

const PAN: usize = 0;
const GAIN: usize = 1;
const MID: usize = 2;
const SIDE: usize = 3;
const ROTATE: usize = 4;
const MONO_BELOW: usize = 5;

/// At this setting the low cut is out of the chain.
const MONO_OFF_HZ: f32 = 20.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("pan", "Pan", -1.0, 1.0, 0.0),
    ParamSpec::new("gainDb", "Gain", -96.0, 12.0, 0.0).unit("dB"),
    ParamSpec::new("midGain", "Mid", 0.0, 2.0, 1.0),
    ParamSpec::new("sideGain", "Side", 0.0, 3.0, 1.0),
    ParamSpec::new("rotate", "Position", -90.0, 90.0, 0.0).unit("°"),
    ParamSpec::new("monoBelowHz", "Mono below", MONO_OFF_HZ, 6_000.0, MONO_OFF_HZ).log().unit("Hz"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "stereo-matrix",
    code: 5,
    version: 1,
    label: "Stereo Matrix",
    category: Category::Stereo,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Width", keys: &["midGain", "sideGain", "monoBelowHz"] },
            Group { label: "Position", keys: &["rotate"] },
            Group { label: "Balance", keys: &["pan", "gainDb"] },
        ],
        primary: &["rotate", "sideGain", "pan"],
        displays: &[Display::Scope],
        // MS Matrix's pad: stereo position across, side level up.
        pad: Some(PadPair { x: "rotate", x_range: (-90.0, 90.0), y: "sideGain", y_range: (0.0, 1.5) }),
    },
    source: "Hyperprism: Pan, Stereo Dynamics, More Stereo, MS Matrix",
    sound: None,
};

/// A second-order high-pass (RBJ), for the side's low cut.
#[derive(Default)]
struct HighPass {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
    hz: f32,
}

impl HighPass {
    fn tune(&mut self, hz: f32, sample_rate: f32) {
        if hz == self.hz {
            return;
        }
        self.hz = hz;
        let w = core::f32::consts::TAU * (hz / sample_rate).min(0.49);
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * core::f32::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        self.b0 = (1.0 + cos) / 2.0 / a0;
        self.b1 = -(1.0 + cos) / a0;
        self.b2 = self.b0;
        self.a1 = -2.0 * cos / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    fn clear(&mut self) {
        (self.x1, self.x2, self.y1, self.y2) = (0.0, 0.0, 0.0, 0.0);
    }
}

pub struct StereoMatrix {
    sample_rate: f32,
    values: [f32; 6],
    pan: Smoothed,
    gain_db: Smoothed,
    mid: Smoothed,
    side: Smoothed,
    rotate: Smoothed,
    mono_below: Smoothed,
    low_cut: HighPass,
}

impl StereoMatrix {
    fn glides(&mut self) -> [&mut Smoothed; 6] {
        [&mut self.pan, &mut self.gain_db, &mut self.mid, &mut self.side, &mut self.rotate, &mut self.mono_below]
    }

    fn land(&mut self) {
        let values = self.values;
        for (glide, v) in self.glides().into_iter().zip(values) {
            glide.jump(v);
        }
    }
}

impl Kernel for StereoMatrix {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let glide = |i: usize| Smoothed::new(sample_rate, PARAMS[i].smooth_ms);
        let mut values = [0.0; 6];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = StereoMatrix {
            sample_rate,
            values,
            pan: glide(PAN),
            gain_db: glide(GAIN),
            mid: glide(MID),
            side: glide(SIDE),
            rotate: glide(ROTATE),
            mono_below: glide(MONO_BELOW),
            low_cut: HighPass::default(),
        };
        k.land();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        let v = spec.clamp(value);
        self.values[index] = v;
        self.glides()[index].set(v);
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.land();
        self.low_cut.clear();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let pan = self.pan.next();
        let gain = 10f32.powf(self.gain_db.next() / 20.0);
        let mid_gain = self.mid.next();
        let side_gain = self.side.next();
        let (sin, cos) = self.rotate.next().to_radians().sin_cos();
        let mono_below = self.mono_below.next();

        let mut mid = 0.5 * (frame[0] + frame[1]);
        let mut side = 0.5 * (frame[0] - frame[1]);
        if mono_below > MONO_OFF_HZ {
            self.low_cut.tune(mono_below, self.sample_rate);
            side = self.low_cut.run(side);
        }
        mid *= mid_gain;
        side *= side_gain;
        let (mid, side) = (mid * cos - side * sin, mid * sin + side * cos);

        frame[0] = (mid + side) * (1.0 - pan).min(1.0) * gain;
        frame[1] = (mid - side) * (1.0 + pan).min(1.0) * gain;
    }
}
