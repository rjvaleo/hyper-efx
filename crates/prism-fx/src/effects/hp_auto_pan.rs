//! Hyperprism Auto-Pan: Hyperprism-PPC 1.2.1's process 28, "Sweeping Pan"
//! (class ESpatializer, routine `SweepingPan`), as its code does it.
//!
//! The input is summed to mono, ½(L + R), and panned by a sine LFO about a
//! fixed Position: pan = ½(lfo × depth + position) + ½, clamped to 0…1, with
//! position running −1…+1 over the slider's 0–100 %. The pan law is constant
//! power, read without interpolation from a 1024-entry quarter-sine table:
//! left = Q[int((1 − pan) × 1023)], right = Q[int(pan × 1023)]. So the centre
//! reads entry 511 on both sides, 0.706 (−3.02 dB), and at no depth it is a
//! static pan of the mono sum rather than unity.
//!
//! The LFO is a 1024-entry sine table read with linear interpolation. Its
//! phase starts at 0 — the centre, moving right first — and advances
//! Hz × 1024 / SR table steps a sample, but wraps at 1023, so the LFO runs
//! 1024/1023 fast (0.1 %). Depth and rate glide by Hyperprism's smoothing:
//! every 45th sample, `a·target + (1 − a)·value`, a from the Smoothing slider.
//! Position does not glide.
//!
//! Position starts at 25 % — a pan offset of −0.5, left of centre. The
//! constructor sets 50 %, but opening the window reads the settings dialog
//! back into the effect (`InitializeAdditionalSettings` calls
//! `GetAdditionalSettings`, which calls `ApplyAdditionalSettings`), so the
//! slider resource's 25 is what the process starts with. Smoothing starts at
//! the slider's 30 % (a = 0.063) the same way.
//!
//! Not reproduced: the original's first moments after choosing the process,
//! when the rate glides in from 1 Hz (a value slot nothing else writes)
//! because the effect is set up before the window sends its start values.
//! Here a reset lands every glide on its target. The routine's other three
//! siblings — Stereo Tremolo, Crossing Pan and Hyper Pan — are empty in the
//! original and have no algorithm to build. The window's rate is 0–20 Hz; the
//! original would take typed rates to ±32767 Hz.

use super::hp_stereo_shared::{quarter_sin_table, sin_table, Stepper};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const RATE: usize = 0;
const DEPTH: usize = 1;
const POSITION: usize = 2;
const SMOOTHING: usize = 3;

const TABLE: usize = 1024;
/// Where the phase wraps: one entry short of the table.
const WRAP: f32 = (TABLE - 1) as f32;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("rateHz", "Frequency", 0.0, 20.0, 5.0).unit("Hz").smooth(0.0),
    ParamSpec::new("depth", "Depth", 0.0, 100.0, 50.0).unit("%").smooth(0.0),
    ParamSpec::new("position", "Position", 0.0, 100.0, 25.0).unit("%").kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 30.0).unit("%").kind(ParamKind::Int).smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-auto-pan",
    code: 57,
    version: 1,
    label: "Hyperprism Auto-Pan",
    category: Category::Stereo,
    params: PARAMS,
    panel: Panel {
        // The original: the X/Y pad, frequency across and depth up, and the
        // position and smoothing sliders in its settings.
        groups: &[
            Group { label: "Sweep", keys: &["rateHz", "depth"] },
            Group { label: "Settings", keys: &["position", "smoothing"] },
        ],
        primary: &["rateHz", "depth"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "rateHz", x_range: (0.0, 20.0), y: "depth", y_range: (0.0, 100.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Auto-Pan",
    sound: None,
};

pub struct HpAutoPan {
    sample_rate: f32,
    values: [f32; 4],
    sine: [f32; TABLE],
    quarter: [f32; TABLE],
    /// Targets: table steps a sample, depth as a fraction.
    inc: f32,
    depth: f32,
    position: f32,
    inc_s: f32,
    depth_s: f32,
    phase: f32,
    step: Stepper,
}

impl HpAutoPan {
    fn targets(&mut self) {
        self.inc = (self.values[RATE] * TABLE as f32) / self.sample_rate;
        self.depth = (0.01 * self.values[DEPTH] as f64) as f32;
        self.position = (0.02 * self.values[POSITION] as f64 - 1.0) as f32;
    }
}

impl Kernel for HpAutoPan {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 4];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpAutoPan {
            sample_rate,
            values,
            sine: sin_table(),
            quarter: quarter_sin_table(),
            inc: 0.0,
            depth: 0.0,
            position: 0.0,
            inc_s: 0.0,
            depth_s: 0.0,
            phase: 0.0,
            step: Stepper::new(sample_rate, values[SMOOTHING]),
        };
        k.targets();
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        self.values[index] = spec.clamp(value);
        match index {
            SMOOTHING => self.step.set_smoothing(self.values[SMOOTHING]),
            _ => self.targets(),
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.inc_s = self.inc;
        self.depth_s = self.depth;
        // The original starts half a cycle on for a negative rate, which the
        // window's range never gives.
        self.phase = if self.inc < 0.0 { 512.0 } else { 0.0 };
        self.step.restart();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.step.fires() {
            self.depth_s = self.step.toward(self.depth_s, self.depth);
            self.inc_s = self.step.toward(self.inc_s, self.inc);
        }
        let i = self.phase as usize;
        let f = self.phase - i as f32;
        let lfo = if i + 1 < TABLE {
            f * (self.sine[i + 1] - self.sine[i]) + self.sine[i]
        } else {
            // The last entry wraps to the first. The original added the value
            // one past the end of its table here; its phase never gets here
            // (it wraps at 1023), and neither does this one.
            f * (self.sine[0] - self.sine[TABLE - 1]) + self.sine[TABLE - 1]
        };
        let pan = ((0.5 * ((lfo * self.depth_s + self.position) as f64) + 0.5) as f32).clamp(0.0, 1.0);
        let m = (0.5 * (frame[0] + frame[1]) as f64) as f32;
        let span = WRAP as f64;
        frame[0] = self.quarter[((1.0 - pan as f64) * span) as usize] * m;
        frame[1] = self.quarter[(pan as f64 * span) as usize] * m;
        self.phase += self.inc_s;
        while self.phase >= WRAP {
            self.phase -= WRAP;
        }
        while self.phase < 0.0 {
            self.phase += WRAP;
        }
    }
}
