//! Harmonizer: MultiGrainLive's `PitchShifter` (`ShapeKind::Harmonizer`), the
//! windowed delay-line pitch shifter of Part 2, sections 4.4 and 6 of Jon
//! Dattorro's "Effect Design", sample for sample.
//!
//! Each side writes a half-second line; two taps half a window apart sweep it
//! at `1 − 2^(semitones/12)` windows a sample (the phase in f64, starting a
//! quarter in), crossfaded by a raised cosine so each is silent where it
//! jumps. The window is at least 32 samples. The shifted sound is fed back
//! into the line by `feedback`; the output is `dry·x + wet·shifted`.

use super::mgl_dattorro_common::{spec, store, whole_rate, PitchCore};
use crate::{Category, Descriptor, Display, Group, Kernel, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[
    spec("semitones", "Pitch", -24.0, 24.0, 7.0).unit("st"),
    spec("windowMs", "Window", 10.0, 250.0, 60.0).log().unit("ms"),
    spec("feedback", "Feedback", 0.0, 0.95, 0.0),
    spec("wet", "Wet", 0.0, 1.0, 0.5),
    spec("dry", "Dry", 0.0, 1.0, 1.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "harmonizer",
    code: 91,
    version: 1,
    label: "Harmonizer",
    category: Category::Pitch,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Pitch", keys: &["semitones", "windowMs", "feedback"] },
            Group { label: "Mix", keys: &["wet", "dry"] },
        ],
        primary: &["semitones", "windowMs"],
        displays: &[Display::Meter],
        pad: None,
    },
    source: "MultiGrainLive: Harmonizer (dattorro.rs, PitchShifter)",
    sound: None,
};

pub struct MglHarmonizer {
    p: [f32; 5],
    ratio: f32,
    core: PitchCore,
    fb: [f32; 2],
}

impl Kernel for MglHarmonizer {
    fn new(sample_rate: f32) -> Self {
        MglHarmonizer {
            p: [7.0, 60.0, 0.0, 0.5, 1.0],
            ratio: 2f32.powf(7.0 / 12.0),
            core: PitchCore::new(whole_rate(sample_rate)),
            fb: [0.0; 2],
        }
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if store(PARAMS, &mut self.p, index, value) {
            self.ratio = 2f32.powf(self.p[0] / 12.0);
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.p.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.core.clear();
        self.fb = [0.0; 2];
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let input = [frame[0] + self.fb[0] * self.p[2], frame[1] + self.fb[1] * self.p[2]];
        let mut wet = [0.0; 2];
        self.core.frame(&input, &mut wet, self.ratio, self.p[1]);
        for (ch, out) in frame.iter_mut().enumerate() {
            self.fb[ch] = wet[ch];
            *out = *out * self.p[4] + wet[ch] * self.p[3];
        }
    }

    fn tail_seconds(&self) -> f32 {
        let fb = self.p[2];
        let window = self.p[1] / 1000.0;
        if fb <= 1e-3 {
            window
        } else {
            (window * (0.001f32.ln() / fb.ln()).ceil()).min(30.0)
        }
    }
}
