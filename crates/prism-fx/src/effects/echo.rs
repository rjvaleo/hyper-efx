//! Echo: a delay with damped feedback and a tape-style glide.
//!
//! Moved from MultiGrainLive's `fx::dattorro::Echo`, keeping its keys, ranges
//! and sound: with the settings held, it renders the same bits (the parity test
//! holds it to that). The move adds two things. Wet, dry, feedback and damping
//! glide over 5 ms where MultiGrainLive stepped them at the block edge, and the
//! effect reports its tail.
//!
//! Moving the delay time moves the read pointer, and the pointer's travel *is*
//! the sweep: while it moves, the line is read at a rate other than one sample
//! per sample, which is a pitch shift — the tape-delay glide. The glide is a
//! time constant rather than a fixed rate, so the pointer sets off quickly and
//! eases in, and the bend decays instead of stopping dead. At zero it jumps,
//! which clicks unless the two times happen to line up.

use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamSpec, Smoothed};

const DELAY: usize = 0;
const FEEDBACK: usize = 1;
const DAMPING: usize = 2;
const WET: usize = 3;
const DRY: usize = 4;
const GLIDE: usize = 5;

/// The longest delay, which sizes the line.
const MAX_SECONDS: usize = 4;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("delayMs", "Delay", 1.0, 4_000.0, 350.0).log().unit("ms"),
    ParamSpec::new("feedback", "Feedback", -0.98, 0.98, 0.35),
    ParamSpec::new("damping", "Damping", 0.0, 1.0, 0.2),
    ParamSpec::new("wet", "Wet", 0.0, 1.0, 0.5),
    ParamSpec::new("dry", "Dry", 0.0, 1.0, 1.0),
    ParamSpec::new("glideMs", "Glide", 0.0, 4_000.0, 220.0).unit("ms").glides("delayMs"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "echo",
    code: 3,
    version: 1,
    label: "Echo",
    category: Category::Delay,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Time", keys: &["delayMs", "glideMs"] },
            Group { label: "Repeats", keys: &["feedback", "damping"] },
            Group { label: "Levels", keys: &["wet", "dry"] },
        ],
        primary: &["delayMs", "feedback", "wet"],
        displays: &[Display::Diagram],
        // Time across, repeats up: the pair a tape echo's two big knobs make.
        pad: Some(PadPair { x: "delayMs", x_range: (20.0, 1_500.0), y: "feedback", y_range: (0.0, 0.95) }),
    },
    source: "MultiGrainLive: Echo (fx::dattorro)",
    sound: None,
};

/// One side's delay line. Reads and writes exactly as MultiGrainLive's does.
struct Line {
    buffer: Vec<f32>,
    write: usize,
}

impl Line {
    fn new(len: usize) -> Self {
        Line { buffer: vec![0.0; len.max(2)], write: 0 }
    }

    /// The line `delay` samples back, linearly interpolated.
    fn tap(&self, delay: f32) -> f32 {
        let n = self.buffer.len();
        let p = (self.write as f32 - delay).rem_euclid(n as f32);
        let i = p.floor() as usize;
        let f = p - i as f32;
        self.buffer[i % n] + (self.buffer[(i + 1) % n] - self.buffer[i % n]) * f
    }

    fn push(&mut self, x: f32) {
        self.buffer[self.write] = x;
        self.write = (self.write + 1) % self.buffer.len();
    }

    fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.write = 0;
    }
}

pub struct Echo {
    sample_rate: f32,
    values: [f32; 6],
    feedback: Smoothed,
    damping: Smoothed,
    wet: Smoothed,
    dry: Smoothed,
    lines: [Line; 2],
    /// The damping filter's state in each side's feedback path.
    lowpass: [f32; 2],
    /// Where each read pointer is, in samples. Moved every frame, so a new
    /// time sweeps rather than stepping at a block edge.
    pointer: [f32; 2],
    /// The pointer's goal, from the delay time.
    target: f32,
    /// How far the pointer closes on its goal each frame, from the glide time.
    pull: f32,
    /// Whether a frame has run since `new` or `reset`. Until one has, settings
    /// land at once and the pointer starts at its time, so an effect set up
    /// before it plays does not open with a sweep or a fade it never asked for.
    started: bool,
}

impl Echo {
    fn retarget(&mut self) {
        self.target = self.values[DELAY] * self.sample_rate / 1000.0;
        let glide = self.values[GLIDE];
        self.pull = if glide <= 0.0 { 1.0 } else { 1.0 - (-1.0 / (glide * 0.001 * self.sample_rate).max(1.0)).exp() };
    }

    fn land(&mut self) {
        self.feedback.jump(self.values[FEEDBACK]);
        self.damping.jump(self.values[DAMPING]);
        self.wet.jump(self.values[WET]);
        self.dry.jump(self.values[DRY]);
    }
}

impl Kernel for Echo {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let glide = |i: usize| Smoothed::new(sample_rate, PARAMS[i].smooth_ms);
        let len = sample_rate as usize * MAX_SECONDS + 4;
        let mut values = [0.0; 6];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = Echo {
            sample_rate,
            values,
            feedback: glide(FEEDBACK),
            damping: glide(DAMPING),
            wet: glide(WET),
            dry: glide(DRY),
            lines: [Line::new(len), Line::new(len)],
            lowpass: [0.0; 2],
            pointer: [0.0; 2],
            target: 0.0,
            pull: 1.0,
            started: false,
        };
        k.retarget();
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
            DELAY | GLIDE => self.retarget(),
            FEEDBACK => self.feedback.set(v),
            DAMPING => self.damping.set(v),
            WET => self.wet.set(v),
            _ => self.dry.set(v),
        }
        if !self.started {
            self.land();
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        self.lowpass = [0.0; 2];
        self.land();
        self.started = false;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if !self.started {
            self.pointer = [self.target; 2];
            self.started = true;
        }
        let feedback = self.feedback.next();
        let damping = self.damping.next();
        let wet = self.wet.next();
        let dry = self.dry.next();
        for (side, s) in frame.iter_mut().enumerate() {
            self.pointer[side] += (self.target - self.pointer[side]) * self.pull;
            let x = *s;
            let y = self.lines[side].tap(self.pointer[side]);
            self.lowpass[side] += (1.0 - damping) * (y - self.lowpass[side]);
            self.lines[side].push(x + self.lowpass[side] * feedback);
            *s = x * dry + y * wet;
        }
    }

    /// Until the repeats are 60 dB down. Damping only shortens it.
    fn tail_seconds(&self) -> f32 {
        let delay = self.values[DELAY] / 1000.0;
        let gain = self.values[FEEDBACK].abs();
        if gain <= 0.0 {
            return delay;
        }
        delay * (0.001f32.ln() / gain.ln()).ceil()
    }
}
