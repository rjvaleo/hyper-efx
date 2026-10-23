//! ++phasor: SoundHack Freesound's phaser (Tom Erbe, v9.10), as its code does
//! it.
//!
//! Eight identical second-order allpass sections in a chain, all at one swept
//! frequency `f`: pole radius `r = e^(−π·bandwidth·f/sr)`, `a1 = −2r·cos(2πf/sr)`,
//! `a2 = r²`. The switches tap the chain after one, two, four and eight sections
//! (labelled 360, 720, 1080 and 1440, for the phase each reaches).
//!
//! - **Sweep.** `f = frequency × 4^(lfo × depth)`: two octaves each way at full
//!   depth. One LFO serves both sides. Sine starts at the centre going up; the
//!   triangle starts at the bottom.
//! - **Feedback.** The chain's input is `atan(dry + 0.5·feedback·previous out)`
//!   plus a millionth, so dry and feedback saturate together. The previous
//!   output is the final mixed one, and both sides' loops take it from the
//!   left — so a sound on the left alone reaches the right through feedback.
//! - **Output.** `atan((dry + Σ taps on) × out gain / (1 + taps on))`, clipped
//!   at ±1.5.
//!
//! Changed from the original: *random* works — a new target each cycle and a
//! straight slide to it. The original's random path is unreachable, so
//! choosing it kept the previous shape. Frequency and bandwidth glide; the
//! original stepped them at each host block. Its tempo lock waits for the
//! rack to have a tempo.

use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec, Smoothed};

const RATE: usize = 0;
const FREQ: usize = 1;
const DEPTH: usize = 2;
const FEEDBACK: usize = 3;
const SHAPE: usize = 4;
const BANDWIDTH: usize = 5;
const TAP1: usize = 6;
const TAP2: usize = 7;
const TAP4: usize = 8;
const TAP8: usize = 9;
const IN_DB: usize = 10;
const OUT_DB: usize = 11;

const SECTIONS: usize = 8;
const CLIP: f32 = 1.5;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("rateHz", "LFO speed", 0.01, 10.0, 1.0).log().unit("Hz"),
    ParamSpec::new("freqHz", "Frequency", 300.0, 3_000.0, 500.0).log().unit("Hz"),
    ParamSpec::new("depth", "LFO depth", 0.0, 1.0, 0.5),
    ParamSpec::new("feedback", "Feedback", 0.0, 1.0, 0.5),
    ParamSpec::new("shape", "LFO shape", 0.0, 2.0, 0.0).kind(ParamKind::Choice(&["Sine", "Triangle", "Random"])).smooth(0.0),
    ParamSpec::new("bandwidth", "Bandwidth", 0.1, 10.0, 1.0).log(),
    ParamSpec::new("out360", "360 out", 0.0, 1.0, 1.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("out720", "720 out", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("out1080", "1080 out", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("out1440", "1440 out", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("inDb", "In", -20.0, 20.0, 0.0).unit("dB"),
    ParamSpec::new("outDb", "Out", -20.0, 20.0, 0.0).unit("dB"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "sh-phasor",
    code: 10,
    version: 1,
    label: "++phasor",
    category: Category::Modulation,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "LFO", keys: &["rateHz", "depth", "shape"] },
            Group { label: "Phaser", keys: &["freqHz", "bandwidth", "feedback"] },
            Group { label: "Taps", keys: &["out360", "out720", "out1080", "out1440"] },
            Group { label: "Level", keys: &["inDb", "outDb"] },
        ],
        primary: &["rateHz", "freqHz", "depth", "feedback"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "rateHz", x_range: (0.01, 10.0), y: "depth", y_range: (0.0, 1.0) }),
    },
    source: "SoundHack: ++phasor (Freesound 9.10)",
    sound: None,
};

#[derive(Clone, Copy, Default)]
struct Section {
    w1: f32,
    w2: f32,
}

pub struct ShPhasor {
    sample_rate: f32,
    values: [f32; 12],
    rate: Smoothed,
    freq: Smoothed,
    depth: Smoothed,
    feedback: Smoothed,
    bandwidth: Smoothed,
    in_db: Smoothed,
    out_db: Smoothed,
    chains: [[Section; SECTIONS]; 2],
    /// The last output, which both loops take from the left.
    previous: f32,
    phase: f64,
    /// The random shape's current value, and its slide per sample.
    wander: f32,
    slide: f32,
    seed: u32,
}

impl ShPhasor {
    fn glides(&mut self) -> [(&mut Smoothed, usize); 7] {
        [
            (&mut self.rate, RATE),
            (&mut self.freq, FREQ),
            (&mut self.depth, DEPTH),
            (&mut self.feedback, FEEDBACK),
            (&mut self.bandwidth, BANDWIDTH),
            (&mut self.in_db, IN_DB),
            (&mut self.out_db, OUT_DB),
        ]
    }

    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32
    }

    /// One side through the chain: its input, and the taps it sums.
    fn side(&mut self, side: usize, dry: f32, a1: f32, a2: f32, feed: f32, gain_out: f32) -> f32 {
        let mut x = (dry + feed).atan() + 1e-6;
        let taps = [self.values[TAP1], self.values[TAP2], 0.0, self.values[TAP4], 0.0, 0.0, 0.0, self.values[TAP8]];
        let on: f32 = taps.iter().sum();
        let mut sum = dry;
        for (k, s) in self.chains[side].iter_mut().enumerate() {
            let feedback = a1 * s.w1;
            let w0 = x - feedback - a2 * s.w2;
            let y = a2 * w0 + feedback + s.w2;
            s.w2 = s.w1;
            s.w1 = w0;
            sum += y * taps[k];
            x = y;
        }
        (sum * gain_out / (1.0 + on)).atan()
    }
}

impl Kernel for ShPhasor {
    fn new(sample_rate: f32) -> Self {
        let mut values = [0.0; 12];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let glide = |i: usize| Smoothed::new(sample_rate, PARAMS[i].smooth_ms);
        let mut k = ShPhasor {
            sample_rate: sample_rate.max(1.0),
            values,
            rate: glide(RATE),
            freq: glide(FREQ),
            depth: glide(DEPTH),
            feedback: glide(FEEDBACK),
            bandwidth: glide(BANDWIDTH),
            in_db: glide(IN_DB),
            out_db: glide(OUT_DB),
            chains: [[Section::default(); SECTIONS]; 2],
            previous: 0.0,
            phase: 0.0,
            wander: 0.0,
            slide: 0.0,
            seed: 0x1234_5679,
        };
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        let v = spec.clamp(value);
        self.values[index] = v;
        for (glide, i) in self.glides() {
            if i == index {
                glide.set(v);
            }
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        let values = self.values;
        for (glide, i) in self.glides() {
            glide.jump(values[i]);
        }
        self.chains = [[Section::default(); SECTIONS]; 2];
        self.previous = 0.0;
        self.phase = 0.0;
        self.wander = 0.0;
        self.slide = 0.0;
        self.seed = 0x1234_5679;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let db = |d: f32| 10f32.powf(d * 0.05);
        let gain_in = db(self.in_db.next());
        let gain_out = db(self.out_db.next());
        let rate = self.rate.next();
        let centre = self.freq.next();
        let depth = self.depth.next();
        let feedback = self.feedback.next();
        let bandwidth = self.bandwidth.next();

        // The LFO, one for both sides.
        let p = self.phase as f32;
        let lfo = match self.values[SHAPE] as usize {
            0 => (core::f32::consts::TAU * p).sin(),
            1 => if p < 0.5 { 4.0 * p - 1.0 } else { 3.0 - 4.0 * p },
            _ => {
                self.wander += self.slide;
                self.wander
            }
        };
        let cycles = rate as f64 / self.sample_rate as f64;
        self.phase += cycles;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            let target = 2.0 * self.random() - 1.0;
            self.slide = (target - self.wander) * cycles as f32;
        }

        let f = centre * 4f32.powf(lfo * depth);
        let pi = core::f64::consts::PI;
        let r = (-(bandwidth as f64 * f as f64) * pi / self.sample_rate as f64).exp() as f32;
        let a1 = ((f as f64 * 2.0 * pi / self.sample_rate as f64).cos() * (-2.0 * r as f64)) as f32;
        let a2 = r * r;

        let feed = self.previous * feedback * 0.5;
        let left = self.side(0, frame[0] * gain_in, a1, a2, feed, gain_out);
        // The right side's loop takes the left's output, as the original's does.
        let right = self.side(1, frame[1] * gain_in, a1, a2, left * feedback * 0.5, gain_out);
        self.previous = left.clamp(-CLIP, CLIP);
        frame[0] = left.clamp(-CLIP, CLIP);
        frame[1] = right.clamp(-CLIP, CLIP);
    }
}
