//! Multi Delay: two taps on one line, with their own times.
//!
//! Hyperprism's Multi Delay put its two delay times on the pad's two axes. Here
//! the taps share one line and one feedback path: what goes back into the line
//! is the average of the two taps, so each tap's repeats come back through the
//! other as well, at every sum of the two times. The taps are panned apart by
//! `spread` — at 0 both are in the middle, at 1 the first is hard left and the
//! second hard right — so the pair is heard as two.
//!
//! The line is fed the mid of the input. A moved time glides with a quarter of
//! a second's time constant, so the taps bend in pitch like tape rather than
//! jumping, and the pad can be swept without clicks.

use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamSpec, Smoothed};

const DELAY1: usize = 0;
const DELAY2: usize = 1;
const FEEDBACK: usize = 2;
const SPREAD: usize = 3;
const WET: usize = 4;
const DRY: usize = 5;

/// The longest time either tap reaches, which sizes the line.
const MAX_MS: f32 = 2_000.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("delay1Ms", "Delay 1", 1.0, MAX_MS, 250.0).log().unit("ms").smooth(250.0),
    ParamSpec::new("delay2Ms", "Delay 2", 1.0, MAX_MS, 375.0).log().unit("ms").smooth(250.0),
    ParamSpec::new("feedback", "Feedback", 0.0, 0.95, 0.3),
    ParamSpec::new("spread", "Spread", 0.0, 1.0, 0.5),
    ParamSpec::new("wet", "Wet", 0.0, 1.0, 0.5),
    ParamSpec::new("dry", "Dry", 0.0, 1.0, 1.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "multi-delay",
    code: 4,
    version: 1,
    label: "Multi Delay",
    category: Category::Delay,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Taps", keys: &["delay1Ms", "delay2Ms", "spread"] },
            Group { label: "Repeats", keys: &["feedback"] },
            Group { label: "Levels", keys: &["wet", "dry"] },
        ],
        primary: &["delay1Ms", "delay2Ms", "feedback"],
        displays: &[Display::Diagram],
        // Hyperprism's pad: one time across, the other up, each to a second.
        pad: Some(PadPair { x: "delay1Ms", x_range: (1.0, 1_000.0), y: "delay2Ms", y_range: (1.0, 1_000.0) }),
    },
    source: "Hyperprism: Multi Delay",
    sound: None,
};

pub struct MultiDelay {
    sample_rate: f32,
    values: [f32; 6],
    delay1: Smoothed,
    delay2: Smoothed,
    feedback: Smoothed,
    spread: Smoothed,
    wet: Smoothed,
    dry: Smoothed,
    line: Vec<f32>,
    write: usize,
}

impl MultiDelay {
    /// The line `delay` samples back, linearly interpolated.
    fn tap(&self, delay: f32) -> f32 {
        let n = self.line.len();
        let p = (self.write as f32 - delay).rem_euclid(n as f32);
        let i = p.floor() as usize;
        let f = p - i as f32;
        let a = self.line[i % n];
        a + (self.line[(i + 1) % n] - a) * f
    }

    fn glides(&mut self) -> [&mut Smoothed; 6] {
        [&mut self.delay1, &mut self.delay2, &mut self.feedback, &mut self.spread, &mut self.wet, &mut self.dry]
    }

    fn land(&mut self) {
        let values = self.values;
        for (glide, v) in self.glides().into_iter().zip(values) {
            glide.jump(v);
        }
    }
}

impl Kernel for MultiDelay {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let glide = |i: usize| Smoothed::new(sample_rate, PARAMS[i].smooth_ms);
        let mut values = [0.0; 6];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = MultiDelay {
            sample_rate,
            values,
            delay1: glide(DELAY1),
            delay2: glide(DELAY2),
            feedback: glide(FEEDBACK),
            spread: glide(SPREAD),
            wet: glide(WET),
            dry: glide(DRY),
            line: vec![0.0; (MAX_MS * 0.001 * sample_rate) as usize + 4],
            write: 0,
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
        self.line.fill(0.0);
        self.write = 0;
        self.land();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let per_ms = self.sample_rate / 1000.0;
        let d1 = self.delay1.next() * per_ms;
        let d2 = self.delay2.next() * per_ms;
        let feedback = self.feedback.next();
        let spread = self.spread.next();
        let wet = self.wet.next();
        let dry = self.dry.next();

        let t1 = self.tap(d1);
        let t2 = self.tap(d2);
        let mid = 0.5 * (frame[0] + frame[1]);
        self.line[self.write] = mid + feedback * 0.5 * (t1 + t2);
        self.write = (self.write + 1) % self.line.len();

        // Tap 1 leans left and tap 2 right; each keeps its level as it moves.
        let near = 0.5 * (1.0 + spread);
        let far = 0.5 * (1.0 - spread);
        frame[0] = frame[0] * dry + wet * (t1 * near + t2 * far);
        frame[1] = frame[1] * dry + wet * (t1 * far + t2 * near);
    }

    /// Until the repeats are 60 dB down, at the longer tap's pace.
    fn tail_seconds(&self) -> f32 {
        let longest = self.values[DELAY1].max(self.values[DELAY2]) / 1000.0;
        let gain = self.values[FEEDBACK];
        if gain <= 0.0 {
            return longest;
        }
        longest * (0.001f32.ln() / gain.ln()).ceil()
    }
}
