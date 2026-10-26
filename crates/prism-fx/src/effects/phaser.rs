//! Phaser: MultiGrainLive's `fx::phaser::Phaser`, sample for sample.
//!
//! Julius Smith's all-pass phaser (ICMC 1984): instead of a flanger's delay
//! line, whose notches are locked to multiples of one frequency, a chain of
//! second-order all-pass sections, one per notch, summed with the dry input.
//! Each notch sits where the chain's phase passes π.
//!
//! - **Notches.** One to eight sections, the first at **Centre**, each next
//!   one **Spread** times higher: geometric, not harmonic.
//! - **Notch width** is every section's pole radius (kept under 0.9995);
//!   nearer one is narrower.
//! - **Notch depth** is the gain on the chain where it meets the input;
//!   negative puts notches where the peaks were.
//! - **Sweep.** A sine LFO at **Rate** moves the whole comb of notches up and
//!   down by half **Sweep** octaves each way, retuning every section every
//!   sample. The right side's LFO runs **Stereo** of a cycle ahead.
//! - **Feedback** sends the chain's last output back to its input.
//!
//! Section frequencies are kept between 20 Hz and 0.49 of the sample rate;
//! a section whose output stops being finite clears itself and outputs zero.
//! The original read its controls once per block; here a control acts from
//! the next sample, which is the same wherever a host moves it between blocks.

use crate::{Category, Descriptor, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};
use core::f32::consts::PI;

const NOTCHES: usize = 0;
const CENTRE: usize = 1;
const SPREAD: usize = 2;
const RESONANCE: usize = 3;
const DEPTH: usize = 4;
const RATE: usize = 5;
const SWEEP: usize = 6;
const STEREO: usize = 7;
const FEEDBACK: usize = 8;
const WET: usize = 9;
const DRY: usize = 10;

const MAX_SECTIONS: usize = 8;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("notches", "Notches", 1.0, 8.0, 4.0).kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("centreHz", "Centre", 40.0, 8000.0, 500.0).log().unit("Hz").smooth(0.0),
    ParamSpec::new("spread", "Spread", 1.0, 3.0, 1.8).smooth(0.0),
    ParamSpec::new("resonance", "Notch width", 0.0, 0.999, 0.7).smooth(0.0),
    ParamSpec::new("depth", "Notch depth", -1.0, 1.0, 0.7).smooth(0.0),
    ParamSpec::new("rateHz", "Rate", 0.0, 10.0, 0.35).unit("Hz").smooth(0.0),
    ParamSpec::new("sweep", "Sweep", 0.0, 4.0, 1.5).unit("oct").smooth(0.0),
    ParamSpec::new("stereo", "Stereo", 0.0, 0.5, 0.25).smooth(0.0),
    ParamSpec::new("feedback", "Feedback", -0.95, 0.95, 0.0).smooth(0.0),
    ParamSpec::new("wet", "Wet", 0.0, 1.0, 0.5).smooth(0.0),
    ParamSpec::new("dry", "Dry", 0.0, 1.0, 1.0).smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "phaser",
    code: 99,
    version: 1,
    label: "Phaser",
    category: Category::Modulation,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Notches", keys: &["notches", "centreHz", "spread", "resonance", "depth"] },
            Group { label: "Sweep", keys: &["rateHz", "sweep", "stereo"] },
            Group { label: "Mix", keys: &["feedback", "wet", "dry"] },
        ],
        primary: &["centreHz", "rateHz", "sweep", "depth"],
        displays: &[],
        pad: Some(PadPair { x: "rateHz", x_range: (0.0, 10.0), y: "depth", y_range: (0.0, 1.0) }),
    },
    source: "MultiGrainLive: Phaser (fx::phaser::Phaser, after J. O. Smith 1984)",
    sound: None,
};

/// One second-order all-pass: `(a₂ + a₁z⁻¹ + z⁻²) / (1 + a₁z⁻¹ + a₂z⁻²)`,
/// `a₁ = −2r·cos θ`, `a₂ = r²`.
#[derive(Clone, Copy, Default)]
struct Section {
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Section {
    #[inline]
    fn tune(&mut self, hz: f32, radius: f32, sample_rate: f32) {
        let theta = (2.0 * PI * hz / sample_rate).clamp(1e-4, PI - 1e-4);
        let r = radius.clamp(0.0, 0.9995);
        self.a1 = -2.0 * r * theta.cos();
        self.a2 = r * r;
    }

    #[inline]
    fn step(&mut self, x: f32) -> f32 {
        let y = self.a2 * x + self.a1 * self.x1 + self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        if y.is_finite() {
            y
        } else {
            self.clear();
            0.0
        }
    }

    fn clear(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

pub struct Phaser {
    sample_rate: f32,
    values: [f32; 11],
    sections: [[Section; MAX_SECTIONS]; 2],
    feedback: [f32; 2],
    /// The LFO's place in its cycle, 0–1.
    phase: f64,
}

impl Kernel for Phaser {
    fn new(sample_rate: f32) -> Self {
        let mut values = [0.0; 11];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        Phaser {
            // The original took a whole number of hertz, at least one.
            sample_rate: sample_rate.max(1.0),
            values,
            sections: [[Section::default(); MAX_SECTIONS]; 2],
            feedback: [0.0; 2],
            phase: 0.0,
        }
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(spec) = PARAMS.get(index) {
            self.values[index] = spec.clamp(value);
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        for s in self.sections.iter_mut().flatten() {
            s.clear();
        }
        self.feedback = [0.0; 2];
        self.phase = 0.0;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let sr = self.sample_rate;
        let v = &self.values;
        let count = (v[NOTCHES].round() as usize).clamp(1, MAX_SECTIONS);
        let (centre, spread, radius) = (v[CENTRE], v[SPREAD], v[RESONANCE]);
        let (depth, rate, sweep, stereo) = (v[DEPTH], v[RATE], v[SWEEP], v[STEREO]);
        let (feedback, wet, dry) = (v[FEEDBACK], v[WET], v[DRY]);
        let nyquist = sr * 0.49;

        for (ch, out) in frame.iter_mut().enumerate() {
            let lfo = ((self.phase as f32 + ch as f32 * stereo) * 2.0 * PI).sin();
            let shift = 2f32.powf(lfo * sweep * 0.5);
            let x = *out;
            let mut s = x + self.feedback[ch] * feedback;
            for (i, section) in self.sections[ch][..count].iter_mut().enumerate() {
                let hz = (centre * spread.powi(i as i32) * shift).clamp(20.0, nyquist);
                section.tune(hz, radius, sr);
                s = section.step(s);
            }
            self.feedback[ch] = s;
            let y = x + s * depth;
            *out = x * dry + y * wet;
        }
        self.phase = (self.phase + rate as f64 / sr as f64).rem_euclid(1.0);
    }
}
