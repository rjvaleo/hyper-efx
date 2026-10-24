//! Hyperprism Echo: Hyperprism-PPC 1.2.1's Echo, as its code does it (class
//! EDelay2, process 2, routine `Delay2`). Single Delay is the same routine
//! with no feedback; its engine is here too.
//!
//! One stereo line, read at a whole-sample tap: no interpolation, so a still
//! time is an exact copy of the input that many samples back.
//! `out = in·(1 − mix) + mix·y` and the line takes `in + feedback·y`.
//!
//! **A time change is a walk of crossfades, never a glide.** A step counter
//! fires every `N + 1` samples. When it fires with a new time pending, the
//! next tap becomes the current one and a new next tap is set `speed` % of the
//! remaining way to the target ("Param. Speed Ch.", 1–100, a step fraction
//! here). The step lasts `N = max(4096, |distance|)` samples, over which
//! `y = cos θ · old + sin θ · new` with θ running from 0 to π/2: an
//! equal-power crossfade. So 1000 ms to 500 ms at 50 % is steps to 750, 625,
//! 562.5 ms ... lasting 22 050, 11 025, 5 512, then 4 096 samples each, and the
//! pitch never moves. A target reached exactly goes idle, and the counter then
//! polls every 1025 samples (2049 from a fresh start), so a new time waits up
//! to that long to begin. At speed 100 the time jumps at once. Mix and
//! feedback are not smoothed. Both sides use the same taps.
//!
//! The arithmetic is the original's, fused multiply-adds included, so taps
//! land on the same samples. At rates other than 44.1 kHz the 4096, 1024 and
//! 2048-sample counts are scaled by rate / 44 100.
//!
//! Defaults: Param. Speed Ch. 50 % and Mix 50 % are the window's slider
//! values, which the original reads as its defaults; the edit fields beside
//! them show 100 and 0.
//!
//! Changed from the original:
//! - Setting the delay no longer also sets the feedback (Single Delay: the
//!   mix) to 0.01 × the delay in ms; its SetParameter lacked a `break`.
//! - A target of 0 ms is reached (the original used 0 to mean "no target", so
//!   0 ms was ignored), and the next tap is held to one sample like the
//!   current one (the original's read the oldest sample in the line).
//! - A step between two taps on the same sample passes that sample through.
//!   The original summed it with itself at cos θ + sin θ, a swell of up to
//!   3 dB, in each of a walk's last, sub-sample steps, and between a new time
//!   being set and the first step, which also began with a jump in level.
//! - A step too small to move the float lands on the target; the original
//!   could stall a hair short and step forever.
//! - A jump at speed 100 moves the next tap too, so a later walk does not
//!   start from the time before the jump.
//! - Feedback stops at 100 % (the original's field went to 32 767 %), and the
//!   line is held within ±4 (12 dB over full scale): a crossfade between
//!   neighbouring taps lifts the loop gain above one, which ran away at short
//!   times and full feedback.
//! - Settings made before the first frame land at once, as the original's
//!   initialisation copies the target time into its taps.

use super::hp_delay_law::{count, samples, Line};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};
use core::f64::consts::FRAC_PI_2;

const DELAY: usize = 0;
const FEEDBACK: usize = 1;
const MIX: usize = 2;
const SPEED: usize = 3;

/// The longest time, in ms.
pub(crate) const LONGEST_MS: f32 = 2000.0;
/// The line's ceiling under feedback.
const LOOP_LIMIT: f32 = 4.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("delayMs", "Delay", 0.0, LONGEST_MS, 1000.0).unit("ms").smooth(0.0),
    ParamSpec::new("feedback", "Feedback", 0.0, 100.0, 0.0).unit("%").smooth(0.0),
    ParamSpec::new("mix", "Mix", 0.0, 100.0, 50.0).unit("%").smooth(0.0),
    ParamSpec::new("speed", "Param. Speed Ch.", 1.0, 100.0, 50.0)
        .unit("%")
        .kind(ParamKind::Int)
        .smooth(0.0)
        .glides("delayMs"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-echo",
    code: 41,
    version: 1,
    label: "Hyperprism Echo",
    category: Category::Delay,
    params: PARAMS,
    panel: Panel {
        // The original: the pad (delay across, feedback up) and its
        // additional-settings sliders.
        groups: &[
            Group { label: "Pad", keys: &["delayMs", "feedback"] },
            Group { label: "Additional settings", keys: &["speed", "mix"] },
        ],
        primary: &["delayMs", "feedback", "mix"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "delayMs", x_range: (0.0, LONGEST_MS), y: "feedback", y_range: (0.0, 100.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Echo",
    sound: None,
};

/// `Delay2`: the line, its two taps and the step counter.
pub(crate) struct Delay2 {
    sample_rate: f32,
    line: Line,
    /// The tap being read, in samples (the original's `p[0]`).
    current: f32,
    /// The tap being crossfaded to (`p[11]`).
    next: f32,
    /// The time being walked to (`p[9]`), while `moving`.
    target: f32,
    moving: bool,
    /// The step length and the counter (`+0x50`, `+0x54`).
    length: i32,
    countdown: i32,
    /// The crossfade's angle per sample, π/2 over the step.
    angle: f64,
    shortest: i32,
    idle: i32,
    first: i32,
    started: bool,
}

impl Delay2 {
    pub(crate) fn new(sample_rate: f32, delay_ms: f32) -> Self {
        let longest = (samples(LONGEST_MS, sample_rate) as usize).max(1);
        let first = count(2048, sample_rate);
        let mut d = Delay2 {
            sample_rate,
            line: Line::new(longest),
            current: 0.0,
            next: 0.0,
            target: 0.0,
            moving: false,
            length: first,
            countdown: 0,
            angle: 0.0,
            shortest: count(4096, sample_rate),
            idle: count(1024, sample_rate),
            first,
            started: false,
        };
        d.reset(delay_ms);
        d
    }

    /// As new, at this time.
    pub(crate) fn reset(&mut self, delay_ms: f32) {
        self.line.clear();
        self.land(delay_ms);
        self.length = self.first;
        self.countdown = 0;
        self.angle = FRAC_PI_2 / self.first as f64;
        self.started = false;
    }

    fn land(&mut self, delay_ms: f32) {
        let d = samples(delay_ms, self.sample_rate);
        self.current = d;
        self.next = d;
        self.target = 0.0;
        self.moving = false;
    }

    /// A new time: a target to walk to, or at speed 100 a jump.
    pub(crate) fn set_delay(&mut self, delay_ms: f32, speed: f32) {
        if !self.started || speed >= 100.0 {
            self.land(delay_ms);
        } else {
            self.target = samples(delay_ms, self.sample_rate);
            self.moving = true;
        }
    }

    /// The longest time the line is reading or walking to, in seconds.
    pub(crate) fn seconds(&self) -> f32 {
        let walk = if self.moving { self.target } else { 0.0 };
        self.current.max(self.next).max(walk) / self.sample_rate
    }

    #[inline]
    pub(crate) fn tick(&mut self, frame: &mut [f32; 2], mix: f32, dry: f32, feedback: f32, step: f32) {
        self.started = true;
        let fire = self.countdown < 1;
        self.countdown -= 1;
        if fire {
            self.countdown = self.length;
            if self.moving {
                let distance = self.target - self.next;
                self.current = self.next;
                if distance == 0.0 {
                    self.moving = false;
                    self.length = self.idle;
                    self.countdown = self.idle;
                } else {
                    let length = (distance as i32).saturating_abs().max(self.shortest);
                    self.angle = FRAC_PI_2 / length as f64;
                    let next = distance.mul_add(step, self.next);
                    self.next = if next == self.next { self.target } else { next };
                    self.length = length;
                    self.countdown = length;
                }
            }
        }
        if self.current < 1.0 {
            self.current = 1.0;
        }
        let tap = self.current as i32 as usize;
        let old = self.line.at(tap);
        let to = (self.next as i32).max(1) as usize;
        let mut written = [0.0f32; 2];
        if self.moving && to != tap {
            let new = self.line.at(to);
            let theta = self.angle * (self.length - self.countdown) as f64;
            let (cos, sin) = (theta.cos(), theta.sin());
            for (ch, s) in frame.iter_mut().enumerate() {
                let x = *s;
                let y = (old[ch] as f64).mul_add(cos, new[ch] as f64 * sin);
                *s = (mix as f64).mul_add(y, (x * dry) as f64) as f32;
                written[ch] = held((feedback as f64).mul_add(y, x as f64) as f32, feedback);
            }
        } else {
            for (ch, s) in frame.iter_mut().enumerate() {
                let x = *s;
                *s = x.mul_add(dry, old[ch] * mix);
                written[ch] = held(old[ch].mul_add(feedback, x), feedback);
            }
        }
        self.line.push(written);
    }
}

/// What the line takes: the original's value, held within the loop limit
/// once there is feedback to run away with.
#[inline]
fn held(v: f32, feedback: f32) -> f32 {
    if feedback > 0.0 {
        v.clamp(-LOOP_LIMIT, LOOP_LIMIT)
    } else {
        v
    }
}

/// A percentage as the original stores it: `(float)(0.01 × v)`.
pub(crate) fn fraction(v: f32) -> f32 {
    (0.01 * v as f64) as f32
}

pub struct HpEcho {
    values: [f32; 4],
    delay: Delay2,
    feedback: f32,
    mix: f32,
    dry: f32,
    step: f32,
}

impl HpEcho {
    fn apply(&mut self) {
        self.feedback = fraction(self.values[FEEDBACK]);
        self.mix = fraction(self.values[MIX]);
        self.dry = (1.0 - self.mix as f64) as f32;
        self.step = fraction(self.values[SPEED]);
    }
}

impl Kernel for HpEcho {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 4];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpEcho { values, delay: Delay2::new(sample_rate, values[DELAY]), feedback: 0.0, mix: 0.0, dry: 0.0, step: 0.0 };
        k.apply();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        let v = spec.clamp(value);
        self.values[index] = v;
        if index == DELAY {
            self.delay.set_delay(v, self.values[SPEED]);
        }
        self.apply();
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.delay.reset(self.values[DELAY]);
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        self.delay.tick(frame, self.mix, self.dry, self.feedback, self.step);
    }

    /// Until the repeats are 60 dB down, or a minute where the loop holds.
    fn tail_seconds(&self) -> f32 {
        let longest = self.delay.seconds();
        let gain = self.feedback;
        if gain >= 1.0 {
            return 60.0;
        }
        if gain <= 0.0 {
            return longest;
        }
        (longest * (0.001f32.ln() / gain.ln()).ceil()).min(60.0)
    }
}
