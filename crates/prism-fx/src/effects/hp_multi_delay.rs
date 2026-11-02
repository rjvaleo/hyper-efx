//! Hyperprism Multi Delay: Hyperprism-PPC 1.2.1's Multi Delay, as its code
//! does it (class EDelay, process 1, routine `DelayMultiTap`).
//!
//! One stereo line, no feedback, three whole-sample taps: at the pad's two
//! times d1 and d2 and at their sum, each with its own attenuation
//! (0–36 dB), `out = in·(1 − mix) + mix·(g1·t1 + g2·t2 + g3·t3)`. Each side
//! runs its own line through the same taps.
//!
//! Each time glides by one one-pole step a sample, `s ← a·int(t) + b·s`, toward
//! the whole-sample target, held to at least one sample, and the tap is the
//! whole part of `s`. "Param. Speed Ch." sets `a` by EchoTranz's law (the
//! default 50 % is a 22.7 s time constant at 44.1 kHz; 100 % jumps). The taps
//! move a sample at a time, so a moving time steps the pitch.
//!
//! The arithmetic is the original's, fused multiply-adds included. One
//! consequence is kept: `a + b` falls a hair under one, so at some settings
//! the glide settles just under its whole-sample target and the tap reads a
//! sample short (100 ms at 50 % reads 4409 samples back, not 4410). At rates
//! other than 44.1 kHz the pole is scaled so each glide takes the same time.
//!
//! Changed from the original: settings made before the first frame land at
//! once, as the original's initialisation copies the target times into the
//! glides.
//!
//! Added (not in the original): Feedback, 0–100 %, default 0. The line takes
//! `in + feedback·sum / (g1 + g2 + g3)`, the taps' sum over their total gain,
//! so the loop never gains more than `feedback` and the attenuations shape
//! the regeneration as they shape the taps. At 0 it is the original exactly.

use super::hp_delay_law::{samples, speed, Line};
use super::hp_echo::fraction;
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const DELAY1: usize = 0;
const ATT1: usize = 2;
const MIX: usize = 5;
const SPEED: usize = 6;
const FEEDBACK: usize = 7;

/// The line's ceiling under feedback.
const LOOP_LIMIT: f32 = 4.0;

const LONGEST_MS: f32 = 1000.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("delay1Ms", "Delay 1", 0.0, LONGEST_MS, 500.0).unit("ms").smooth(0.0),
    ParamSpec::new("delay2Ms", "Delay 2", 0.0, LONGEST_MS, 500.0).unit("ms").smooth(0.0),
    ParamSpec::new("att1Db", "Delay 1 att.", 0.0, 36.0, 0.0).unit("dB").kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("att2Db", "Delay 2 att.", 0.0, 36.0, 0.0).unit("dB").kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("att3Db", "Delay 3 att.", 0.0, 36.0, 0.0).unit("dB").kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("mix", "Mix", 0.0, 100.0, 50.0).unit("%").smooth(0.0),
    ParamSpec::new("speed", "Param. Speed Ch.", 1.0, 100.0, 50.0)
        .unit("%")
        .kind(ParamKind::Int)
        .smooth(0.0)
        .glides("delay1Ms"),
    ParamSpec::new("feedback", "Feedback", 0.0, 100.0, 0.0).unit("%").smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-multi-delay",
    code: 43,
    version: 1,
    label: "Hyperprism Multi Delay",
    category: Category::Delay,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Pad", keys: &["delay1Ms", "delay2Ms"] },
            Group { label: "Taps", keys: &["att1Db", "att2Db", "att3Db"] },
            Group { label: "Additional settings", keys: &["feedback", "speed", "mix"] },
        ],
        primary: &["delay1Ms", "delay2Ms", "mix"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "delay1Ms", x_range: (0.0, LONGEST_MS), y: "delay2Ms", y_range: (0.0, LONGEST_MS) }),
    },
    source: "Hyperprism-PPC 1.2.1: Multi Delay",
    sound: None,
};

pub struct HpMultiDelay {
    sample_rate: f32,
    values: [f32; 8],
    line: Line,
    /// The two gliding times, in samples (the original's doubles at +0x78, +0x80).
    times: [f64; 2],
    /// Their targets (`p[0]`, `p[1]`).
    targets: [f32; 2],
    gains: [f32; 3],
    a: f64,
    b: f64,
    mix: f32,
    dry: f32,
    /// Feedback over the taps' total gain.
    regen: f32,
    started: bool,
}

impl HpMultiDelay {
    fn apply(&mut self) {
        for (i, target) in self.targets.iter_mut().enumerate() {
            *target = samples(self.values[DELAY1 + i], self.sample_rate);
        }
        for (i, gain) in self.gains.iter_mut().enumerate() {
            *gain = 10f64.powf(-(self.values[ATT1 + i] as i32) as f64 / 20.0) as f32;
        }
        (self.a, self.b) = speed(self.values[SPEED], self.sample_rate);
        self.mix = fraction(self.values[MIX]);
        self.dry = (1.0 - self.mix as f64) as f32;
        self.regen = fraction(self.values[FEEDBACK]) / self.gains.iter().sum::<f32>();
        if !self.started {
            self.times = [self.targets[0] as f64, self.targets[1] as f64];
        }
    }
}

impl Kernel for HpMultiDelay {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 8];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let longest = 2 * (samples(LONGEST_MS, sample_rate) as usize).max(1);
        let mut k = HpMultiDelay {
            sample_rate,
            values,
            line: Line::new(longest),
            times: [0.0; 2],
            targets: [0.0; 2],
            gains: [1.0; 3],
            a: 1.0,
            b: 0.0,
            mix: 0.0,
            dry: 0.0,
            regen: 0.0,
            started: false,
        };
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        self.values[index] = spec.clamp(value);
        self.apply();
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.line.clear();
        self.started = false;
        self.apply();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        self.started = true;
        let mut taps = [0usize; 2];
        for ((time, target), tap) in self.times.iter_mut().zip(self.targets).zip(taps.iter_mut()) {
            let whole = (target as i32) as f64;
            let mut s = self.a.mul_add(whole, self.b * *time);
            if s < 1.0 {
                s = 1.0;
            }
            *time = s;
            *tap = s as i32 as usize;
        }
        let one = self.line.at(taps[0]);
        let two = self.line.at(taps[1]);
        let three = self.line.at((taps[0] + taps[1]).min(self.line.longest()));
        let [g1, g2, g3] = self.gains;
        let mut written = *frame;
        for (ch, s) in frame.iter_mut().enumerate() {
            let sum = three[ch].mul_add(g3, one[ch].mul_add(g1, two[ch] * g2));
            if self.regen > 0.0 {
                written[ch] = sum.mul_add(self.regen, *s).clamp(-LOOP_LIMIT, LOOP_LIMIT);
            }
            *s = s.mul_add(self.dry, self.mix * sum);
        }
        // At zero feedback the line takes the input, as the original's does.
        self.line.push(written);
    }

    fn tail_seconds(&self) -> f32 {
        let [a, b] = self.times;
        let [c, d] = self.targets;
        let longest = ((a + b) as f32).max(c + d) / self.sample_rate;
        // Each pass round the loop loses at least the feedback's share.
        let gain = fraction(self.values[FEEDBACK]);
        if gain >= 1.0 {
            return 60.0;
        }
        if gain <= 0.0 {
            return longest;
        }
        (longest * (0.001f32.ln() / gain.ln()).ceil()).min(60.0)
    }
}
