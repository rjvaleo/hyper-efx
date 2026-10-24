//! Hyperprism MS Matrix: Hyperprism-PPC 1.2.1's process 25, "M-S Matrix"
//! (class EHyperStereo, routine `MSMatrix`), as its code does it.
//!
//! It takes the two input channels themselves as mid and side — which one
//! carries the side is the Input button — scales the side by Side Volume,
//! and outputs the sum and the difference, the Output button choosing which
//! side gets the sum:
//!
//! - Input "Side in left": S = L × side volume, M = R; "Side in right": the
//!   other way about.
//! - Output "Sum to left": L = (M + S)·gA, R = (M − S)·gB; "Sum to right": the
//!   other way about.
//!
//! So it decodes M/S to L/R, and since sum and difference are their own
//! inverse up to scale, encodes as well. Stereo Position is a constant-power
//! balance of the two outputs at an overall half gain, gA = ½cos θ,
//! gB = ½sin θ with θ = π/4 · (1 + position / 90): at the centre each output
//! is ×0.354, 9 dB down. At its start values an ordinary L/R sound comes out
//! as the right channel alone on both sides, 9 dB down.
//!
//! Nothing glides. The original computes smoothed copies of its three gains
//! every 45 samples and never uses them, so its pad moves step straight to
//! the new value; that is kept, and the Smoothing slider, which acts on
//! nothing, is left out. Its dead ×0.707 pre-scale (a flag nothing sets) is
//! left out too. A mono input passes unchanged in the original; Mono input
//! does that with the left input. The side volume's range is the original's
//! absolute limits, −400 to 400 %; its window shows 0 to 150.

use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const POSITION: usize = 0;
const SIDE: usize = 1;
const INPUT: usize = 2;
const OUTPUT: usize = 3;
const MONO: usize = 4;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("position", "Stereo Position", -90.0, 90.0, 0.0).unit("°").smooth(0.0),
    ParamSpec::new("sideVolume", "Side Volume", -400.0, 400.0, 0.0).unit("%").smooth(0.0),
    ParamSpec::new("input", "Input", 0.0, 1.0, 0.0).kind(ParamKind::Choice(&["Side in left", "Side in right"])).smooth(0.0),
    ParamSpec::new("output", "Output", 0.0, 1.0, 0.0).kind(ParamKind::Choice(&["Sum to left", "Sum to right"])).smooth(0.0),
    ParamSpec::new("mono", "Mono input", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-ms-matrix",
    code: 60,
    version: 1,
    label: "Hyperprism MS Matrix",
    category: Category::Stereo,
    params: PARAMS,
    panel: Panel {
        // The original: the X/Y pad, position across and side volume up, and
        // the input and output buttons beside it.
        groups: &[
            Group { label: "Matrix", keys: &["position", "sideVolume"] },
            Group { label: "Routing", keys: &["input", "output", "mono"] },
        ],
        primary: &["position", "sideVolume", "input", "output"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "position", x_range: (-90.0, 90.0), y: "sideVolume", y_range: (0.0, 150.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: MS Matrix",
    sound: None,
};

pub struct HpMsMatrix {
    values: [f32; 5],
    ga: f32,
    gb: f32,
    side: f32,
}

impl HpMsMatrix {
    /// `SetCenter`.
    fn centre(&mut self) {
        let q = core::f32::consts::FRAC_PI_4 as f64;
        let theta = (q + (self.values[POSITION] as f64 * q) as f32 as f64 / 90.0) as f32 as f64;
        self.ga = (theta.cos() as f32 as f64 * 0.5) as f32;
        self.gb = theta.sin() as f32 * 0.5;
    }

    /// `SetSideVolume`.
    fn side_volume(&mut self) {
        self.side = (0.01 * self.values[SIDE] as f64) as f32;
    }
}

impl Kernel for HpMsMatrix {
    fn new(_sample_rate: f32) -> Self {
        let mut values = [0.0; 5];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpMsMatrix { values, ga: 0.0, gb: 0.0, side: 0.0 };
        k.centre();
        k.side_volume();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        self.values[index] = spec.clamp(value);
        match index {
            POSITION => self.centre(),
            SIDE => self.side_volume(),
            _ => {}
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {}

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.values[MONO] > 0.5 {
            frame[1] = frame[0];
            return;
        }
        let (s, m) = if self.values[INPUT] < 0.5 { (frame[0], frame[1]) } else { (frame[1], frame[0]) };
        let s = s * self.side;
        if self.values[OUTPUT] < 0.5 {
            frame[0] = (s + m) * self.ga;
            frame[1] = (m - s) * self.gb;
        } else {
            frame[0] = (m - s) * self.ga;
            frame[1] = (s + m) * self.gb;
        }
    }
}
