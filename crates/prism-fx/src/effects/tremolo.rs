//! Tremolo, and Auto-Pan as its stereo phase.
//!
//! A gain LFO. Auto-Pan is the same tremolo with its right side half a cycle
//! behind its left, so one effect covers Hyperprism's two processes: at a stereo
//! phase of 1 the two gains always sum to the same level, and the sound moves
//! between the speakers instead of pulsing.
//!
//! Every shape starts at the top of its cycle, so a tremolo that has just been
//! inserted or reset starts at full level.

use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec, Smoothed};

const RATE: usize = 0;
const DEPTH: usize = 1;
const SHAPE: usize = 2;
const STEREO: usize = 3;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("rateHz", "Rate", 0.05, 20.0, 4.0).log().unit("Hz"),
    ParamSpec::new("depth", "Depth", 0.0, 1.0, 0.5),
    ParamSpec::new("shape", "Shape", 0.0, 2.0, 0.0).kind(ParamKind::Choice(&["Sine", "Triangle", "Square"])),
    ParamSpec::new("stereo", "Stereo phase", 0.0, 1.0, 0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "tremolo",
    code: 1,
    version: 1,
    label: "Tremolo",
    category: Category::Modulation,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "LFO", keys: &["rateHz", "shape"] },
            Group { label: "Amount", keys: &["depth", "stereo"] },
        ],
        primary: &["rateHz", "depth"],
        displays: &[Display::Curve],
        // Hyperprism's pad: frequency across, depth up.
        pad: Some(PadPair { x: "rateHz", x_range: (0.05, 20.0), y: "depth", y_range: (0.0, 1.0) }),
    },
    source: "Hyperprism: Tremolo and Auto-Pan",
    sound: None,
};

/// How long the square's edges take, so a full-depth square does not click.
const SQUARE_EDGE_SECONDS: f32 = 0.002;

pub struct Tremolo {
    sample_rate: f32,
    values: [f32; 4],
    rate: Smoothed,
    depth: Smoothed,
    /// Glided too: a moved shape morphs through its neighbours.
    shape: Smoothed,
    stereo: Smoothed,
    /// Where the left side is in its cycle, 0–1. In f64 so slow rates stay
    /// accurate over a long session.
    phase: f64,
}

impl Tremolo {
    /// The gain at a point in the cycle: 1 at the top, 1 − depth at the bottom.
    fn gain(phase: f32, depth: f32, shape: f32, edge: f32) -> f32 {
        // Distance from the bottom of the cycle, 0–0.5. Every shape is symmetric
        // about the bottom, which is what makes the stereo sum level.
        let d = (phase - 0.5).abs();
        let triangle = 2.0 * d;
        let wave = if shape <= 1.0 {
            // A raised cosine, written as sin² from the bottom: 0.5 + 0.5·cos
            // loses its precision near −1 and flattens the bottom to zero over
            // several frames, where this stays exact.
            let sine = (core::f32::consts::PI * d).sin().powi(2);
            sine + (triangle - sine) * shape
        } else {
            let square = ((d - 0.25) / edge + 0.5).clamp(0.0, 1.0);
            triangle + (square - triangle) * (shape - 1.0)
        };
        1.0 - depth * (1.0 - wave)
    }

    fn land(&mut self) {
        self.rate.jump(self.values[RATE]);
        self.depth.jump(self.values[DEPTH]);
        self.shape.jump(self.values[SHAPE]);
        self.stereo.jump(self.values[STEREO]);
    }
}

impl Kernel for Tremolo {
    fn new(sample_rate: f32) -> Self {
        let glide = |i: usize| Smoothed::new(sample_rate, PARAMS[i].smooth_ms);
        let mut k = Tremolo {
            sample_rate,
            values: [PARAMS[RATE].default, PARAMS[DEPTH].default, PARAMS[SHAPE].default, PARAMS[STEREO].default],
            rate: glide(RATE),
            depth: glide(DEPTH),
            shape: glide(SHAPE),
            stereo: glide(STEREO),
            phase: 0.0,
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
        match index {
            RATE => self.rate.set(v),
            DEPTH => self.depth.set(v),
            SHAPE => self.shape.set(v),
            _ => self.stereo.set(v),
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.land();
        self.phase = 0.0;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let rate = self.rate.next();
        let depth = self.depth.next();
        let shape = self.shape.next();
        let stereo = self.stereo.next();
        let edge = (SQUARE_EDGE_SECONDS * rate).clamp(1e-6, 0.25);

        let left = self.phase as f32;
        let mut right = left - 0.5 * stereo;
        if right < 0.0 {
            right += 1.0;
        }
        frame[0] *= Self::gain(left, depth, shape, edge);
        frame[1] *= Self::gain(right, depth, shape, edge);

        self.phase += f64::from(rate) / f64::from(self.sample_rate);
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
    }
}
