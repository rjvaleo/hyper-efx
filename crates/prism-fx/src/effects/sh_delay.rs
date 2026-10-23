//! ++delay: SoundHack Delay Trio's delay (Tom Erbe, v9.11), as its code does
//! it.
//!
//! One line per side, read with a four-point Hermite interpolator, and an LFO
//! that moves the read: `d = time·(1 + lfo·depth)·(1 − auto) + 2` samples, so a
//! still line echoes the set time plus one sample.
//!
//! - **LFO.** Sine, triangle, a square summed from odd harmonics up to the
//!   25th, ramp up, ramp down, or a random value that slides to a new target
//!   each cycle. The right side runs `phase` degrees ahead.
//! - **Auto time.** The mean of |L + R| over 512 samples shortens the delay:
//!   `auto = min(6 × amount × level, 1)`.
//! - **Two heads.** A second read half an LFO cycle on, the two crossfaded by
//!   triangles so each head is silent where its ramp jumps: a pitch shifter.
//! - **The loop.** `s = y·feedback + x`, then a resonant low-pass (out of the
//!   way above 19 kHz), a 20 Hz-and-up high-pass, a DC blocker once feedback
//!   passes 100 %, and a level stage over the last 512 values of |s| that
//!   leans on the loop above 0.5 and cuts it to a third above 3. That is what
//!   lets feedback run to 200 %. The wet sound is `atan(y)`.
//! - **Freeze.** The line feeds itself, unfiltered, and the input is ignored.
//!
//! Changed from the original: *Feedback +* is positive feedback (the original's
//! switch was the wrong way round). Frozen, the right side reads its own delay
//! and gets the out gain. Two heads on the random shape each slide to their
//! own targets (the original stepped the second head every sample, a buzz).
//! A time that lands exactly on its target stops there. The tempo locks wait
//! for the rack to have a tempo. The other controls glide; the original ramped
//! them across each host block.

use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec, Smoothed};
use core::f64::consts::PI;

const DELAY: usize = 0;
const FEEDBACK: usize = 1;
const RATE: usize = 2;
const DEPTH: usize = 3;
const SHAPE: usize = 4;
const PHASE: usize = 5;
const AUTO: usize = 6;
const LOWPASS: usize = 7;
const HIGHPASS: usize = 8;
const RESONANCE: usize = 9;
const INVERT: usize = 10;
const FREEZE: usize = 11;
const TWO_HEADS: usize = 12;
const MIX: usize = 13;
const IN_DB: usize = 14;
const OUT_DB: usize = 15;

const RANDOM: usize = 5;
const CLIP: f32 = 1.5;
/// The envelope and loop-level windows.
const WINDOW: usize = 512;
/// The band-limited square's table, one cycle.
const SQUARE: usize = 4096;
/// The longest delay the line holds: ten seconds at full depth, doubled.
const LONGEST_S: f32 = 20.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("delayMs", "Time", 1.0, 10_000.0, 80.0).log().unit("ms").smooth(0.0),
    ParamSpec::new("feedback", "Feedback", 0.0, 2.0, 0.0),
    ParamSpec::new("rateHz", "LFO freq", 0.01, 10.0, 1.0).log().unit("Hz"),
    ParamSpec::new("depth", "LFO depth", 0.0, 1.0, 0.0),
    ParamSpec::new("shape", "LFO shape", 0.0, 5.0, 0.0)
        .kind(ParamKind::Choice(&["Sine", "Triangle", "Square", "Ramp +", "Ramp −", "Random"]))
        .smooth(0.0),
    ParamSpec::new("phaseDeg", "LFO phase", -180.0, 180.0, 0.0).unit("°"),
    ParamSpec::new("envMod", "Auto time", 0.0, 1.0, 0.0),
    ParamSpec::new("lowpassHz", "Low-pass", 20.0, 20_000.0, 20_000.0).log().unit("Hz").smooth(0.0),
    ParamSpec::new("highpassHz", "High-pass", 20.0, 2_000.0, 20.0).log().unit("Hz").smooth(0.0),
    ParamSpec::new("resonance", "Resonance", 0.0, 1.0, 0.1).smooth(0.0),
    ParamSpec::new("invert", "Feedback −", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("freeze", "Freeze", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("twoHead", "Two heads", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("mix", "Wet/dry", 0.0, 1.0, 0.5),
    ParamSpec::new("inDb", "In", -20.0, 20.0, 0.0).unit("dB"),
    ParamSpec::new("outDb", "Out", -20.0, 20.0, 0.0).unit("dB"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "sh-delay",
    code: 11,
    version: 1,
    label: "++delay",
    category: Category::Delay,
    params: PARAMS,
    panel: Panel {
        // The original: four big dials, six small ones, the switches below.
        groups: &[
            Group { label: "Delay", keys: &["delayMs", "feedback", "invert", "freeze", "envMod"] },
            Group { label: "LFO", keys: &["rateHz", "depth", "shape", "phaseDeg", "twoHead"] },
            Group { label: "Filter", keys: &["lowpassHz", "highpassHz", "resonance"] },
            Group { label: "Level", keys: &["mix", "inDb", "outDb"] },
        ],
        primary: &["delayMs", "feedback", "rateHz", "depth"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "delayMs", x_range: (10.0, 2_000.0), y: "feedback", y_range: (0.0, 1.5) }),
    },
    source: "SoundHack: ++delay (Delay Trio 9.11)",
    sound: None,
};

/// A biquad's coefficients, `y = b0·x + b1·x1 + b2·x2 − a1·y1 − a2·y2`.
#[derive(Clone, Copy, Default)]
struct Coefs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

#[derive(Clone, Copy, Default)]
struct Biquad {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    fn run(&mut self, c: &Coefs, x: f64) -> f64 {
        let y = c.b0 * x + c.b1 * self.x1 + c.b2 * self.x2 - c.a1 * self.y1 - c.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// One random LFO: its value and its slide per sample.
#[derive(Clone, Copy, Default)]
struct Wander {
    value: f32,
    slide: f32,
}

/// One side's state.
#[derive(Clone, Copy, Default)]
struct Side {
    low: Biquad,
    high: Biquad,
    dc_x: f64,
    dc_y: f64,
    /// The LFO phase of each head, 0–1, and each head's random value.
    phase: [f64; 2],
    wander: [Wander; 2],
}

pub struct ShDelay {
    sample_rate: f32,
    values: [f32; 16],
    feedback: Smoothed,
    rate: Smoothed,
    depth: Smoothed,
    phase: Smoothed,
    auto: Smoothed,
    mix: Smoothed,
    in_db: Smoothed,
    out_db: Smoothed,
    /// Time in ms, gliding to its target over 100 ms.
    time: f32,
    time_step: f32,
    lines: [Vec<f32>; 2],
    mask: usize,
    write: usize,
    square: Vec<f32>,
    sides: [Side; 2],
    /// Head one's left phase, which the others follow.
    lfo: f64,
    seed: u32,
    envelope: [f32; WINDOW],
    envelope_sum: f64,
    envelope_at: usize,
    level: [f32; WINDOW],
    level_sum: f64,
    level_at: usize,
    low: Coefs,
    high: Coefs,
    dc_pole: f64,
}

impl ShDelay {
    fn glides(&mut self) -> [(&mut Smoothed, usize); 8] {
        [
            (&mut self.feedback, FEEDBACK),
            (&mut self.rate, RATE),
            (&mut self.depth, DEPTH),
            (&mut self.phase, PHASE),
            (&mut self.auto, AUTO),
            (&mut self.mix, MIX),
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

    /// The filters' coefficients, set when their controls move.
    fn filters(&mut self) {
        let sr = self.sample_rate as f64;
        let nyquist = 0.5 * sr;
        // Low-pass: the cot form, with 1/(πr) for cot(πr) below a tenth of the rate.
        let r = (self.values[LOWPASS] as f64).min(nyquist) / sr;
        let c = if r >= 0.1 { (PI * (0.5 - r)).tan() } else { 1.0 / (r * PI) };
        let d = 0.1 + 1.314_213_562_373_095 * (1.0 - self.values[RESONANCE] as f64);
        let a0 = 1.0 / (d * c + 1.0 + c * c);
        self.low = Coefs { b0: a0, b1: 2.0 * a0, b2: a0, a1: 2.0 * a0 * (1.0 - c * c), a2: (1.0 - d * c + c * c) * a0 };
        let k = (PI * (self.values[HIGHPASS] as f64).min(nyquist) / sr).tan();
        let root2 = core::f64::consts::SQRT_2;
        let a0 = 1.0 / (root2 * k + 1.0 + k * k);
        self.high = Coefs { b0: a0, b1: -2.0 * a0, b2: a0, a1: 2.0 * a0 * (k * k - 1.0), a2: (1.0 - root2 * k + k * k) * a0 };
    }

    /// The LFO's wavetable shapes at phase `p`, −1 to 1.
    fn wave(&self, shape: usize, p: f64) -> f32 {
        match shape {
            0 => (core::f64::consts::TAU * p).sin() as f32,
            1 => triangle(p),
            2 => {
                let at = p * SQUARE as f64;
                let i = (at as usize).min(SQUARE - 1);
                let frac = (at - i as f64) as f32;
                self.square[i] + (self.square[i + 1] - self.square[i]) * frac
            }
            3 => (2.0 * p - 1.0) as f32,
            _ => (1.0 - 2.0 * p) as f32,
        }
    }

    /// The four-point Hermite read `d` samples back, as the original's index
    /// and fraction put it: one sample later than `d`.
    fn read(&self, side: usize, d: f32) -> f32 {
        let line = &self.lines[side];
        let whole = d as usize;
        let i = self.write.wrapping_sub(whole);
        let t = (whole as f32 - d) + 1.0;
        let at = |k: usize| line[i.wrapping_add(k).wrapping_sub(1) & self.mask];
        let (xm1, x0, x1, x2) = (at(0), at(1), at(2), at(3));
        let c1 = 0.5 * (x1 - xm1);
        let c3 = 0.5 * (-x1 - (xm1 - x0) + x2) + (x0 - x1);
        let c2 = (xm1 - x0) + c1 - c3;
        ((c3 * t + c2) * t + c1) * t + x0
    }

    /// The level stage, one ring shared by both sides as in the original.
    fn ride(&mut self, s: f32) -> f32 {
        let a = s.abs();
        self.level_sum += (a - self.level[self.level_at]) as f64;
        self.level[self.level_at] = a;
        self.level_at = (self.level_at + 1) % WINDOW;
        let mean = (self.level_sum / WINDOW as f64) as f32;
        if mean > 3.0 {
            s * 0.33333
        } else if mean > 0.5 {
            s * (((mean * -0.089_017_6 + 0.553_09) * mean - 1.245_09) * mean + 1.493_845)
        } else {
            s
        }
    }
}

fn triangle(p: f64) -> f32 {
    (if p < 0.5 { 4.0 * p - 1.0 } else { 3.0 - 4.0 * p }) as f32
}

fn wrap(p: f64) -> f64 {
    p - p.floor()
}

impl Kernel for ShDelay {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 16];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let glide = |i: usize| Smoothed::new(sample_rate, PARAMS[i].smooth_ms);
        let length = ((sample_rate * LONGEST_S) as usize + 8).next_power_of_two();
        // Odd harmonics to the 25th at 1/k — the 25th at 1/24, as the original
        // has it — peak-normalised, with one entry over for the interpolation.
        let mut square: Vec<f32> = (0..=SQUARE)
            .map(|i| {
                let x = core::f64::consts::TAU * i as f64 / SQUARE as f64;
                (1..=25)
                    .step_by(2)
                    .map(|k| {
                        let c = if k == 25 { 0.041_666_66 } else { 1.0 / k as f64 };
                        (k as f64 * x).sin() * c
                    })
                    .sum::<f64>() as f32
            })
            .collect();
        let peak = square.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        square.iter_mut().for_each(|v| *v /= peak);
        let mut k = ShDelay {
            sample_rate,
            values,
            feedback: glide(FEEDBACK),
            rate: glide(RATE),
            depth: glide(DEPTH),
            phase: glide(PHASE),
            auto: glide(AUTO),
            mix: glide(MIX),
            in_db: glide(IN_DB),
            out_db: glide(OUT_DB),
            time: values[DELAY],
            time_step: 0.0,
            lines: [vec![0.0; length], vec![0.0; length]],
            mask: length - 1,
            write: 0,
            square,
            sides: [Side::default(); 2],
            lfo: 0.0,
            seed: 0x1234_5679,
            envelope: [0.0; WINDOW],
            envelope_sum: 0.0,
            envelope_at: 0,
            level: [0.0; WINDOW],
            level_sum: 0.0,
            level_at: 0,
            low: Coefs::default(),
            high: Coefs::default(),
            dc_pole: 1.0 - 126.0 / sample_rate as f64,
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
        let before = self.values[index];
        self.values[index] = v;
        match index {
            // The original's 100 ms straight glide to a new time.
            DELAY if v != before => self.time_step = (v - self.time) / (self.sample_rate * 0.1),
            LOWPASS | HIGHPASS | RESONANCE => self.filters(),
            _ => {}
        }
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
        self.time = values[DELAY];
        self.time_step = 0.0;
        for line in &mut self.lines {
            line.fill(0.0);
        }
        self.write = 0;
        self.sides = [Side::default(); 2];
        self.lfo = 0.0;
        self.seed = 0x1234_5679;
        self.envelope = [0.0; WINDOW];
        self.envelope_sum = 0.0;
        self.envelope_at = 0;
        self.level = [0.0; WINDOW];
        self.level_sum = 0.0;
        self.level_at = 0;
        self.filters();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let db = |d: f32| 10f32.powf(d * 0.05);
        let gain_in = db(self.in_db.next());
        let gain_out = db(self.out_db.next());
        let feedback = self.feedback.next();
        let rate = self.rate.next();
        let depth = self.depth.next();
        let offset = self.phase.next() as f64 / 360.0;
        let auto = self.auto.next();
        let wet = self.mix.next();
        let x = [frame[0] * gain_in, frame[1] * gain_in];

        // The envelope: a 512-sample mean of |L + R|.
        let e = (x[0] + x[1]).abs();
        self.envelope_sum += (e - self.envelope[self.envelope_at]) as f64;
        self.envelope[self.envelope_at] = e;
        self.envelope_at = (self.envelope_at + 1) % WINDOW;
        let level = (self.envelope_sum / WINDOW as f64) as f32;
        let squeeze = (auto * 6.0 * level).min(1.0);

        // The LFO. Each head's value is read before the phase moves, its
        // crossfade after, as in the original.
        let shape = self.values[SHAPE] as usize;
        let two = self.values[TWO_HEADS] > 0.5;
        let cps = rate as f64 / self.sample_rate as f64;
        let mut lfo_value = [[0.0f32; 2]; 2];
        for (s, values) in lfo_value.iter_mut().enumerate() {
            for (head, v) in values.iter_mut().enumerate() {
                *v = if shape == RANDOM {
                    let w = &mut self.sides[s].wander[head];
                    w.value += w.slide;
                    w.value
                } else {
                    self.wave(shape, self.sides[s].phase[head]) * depth
                };
            }
        }
        self.lfo += cps;
        let lfo = wrap(self.lfo);
        self.lfo = lfo;
        for s in 0..2 {
            for head in 0..2 {
                let p = wrap(lfo + if s == 1 { offset } else { 0.0 } + if head == 1 { 0.5 } else { 0.0 });
                // A wrap: a new random target, reached in one cycle.
                if p < self.sides[s].phase[head] {
                    let target = depth * (2.0 * self.random() - 1.0);
                    let w = &mut self.sides[s].wander[head];
                    w.slide = (target - w.value) * cps as f32;
                }
                self.sides[s].phase[head] = p;
            }
        }

        let samples = self.sample_rate * 0.001 * self.time;
        let delay = |v: f32| samples * (1.0 + v) * (1.0 - squeeze) + 2.0;
        let gain = if self.values[INVERT] > 0.5 { -feedback } else { feedback };

        for (s, out) in frame.iter_mut().enumerate() {
            let near = delay(lfo_value[s][0]);
            let y = if self.values[FREEZE] >= 0.5 {
                // The line feeds itself from a whole-sample read.
                let held = self.lines[s][self.write.wrapping_sub(near as usize) & self.mask];
                self.lines[s][self.write] = held;
                *out = (gain_out * ((1.0 - wet) * x[s] + held * wet)).clamp(-CLIP, CLIP);
                continue;
            } else if two {
                let side = &self.sides[s];
                let (one, other) = (self.read(s, near), self.read(s, delay(lfo_value[s][1])));
                ((triangle(side.phase[1]) + 1.0) * other + (triangle(side.phase[0]) + 1.0) * one) * 0.5
            } else {
                self.read(s, near)
            };

            let low = self.low;
            let high = self.high;
            let side = &mut self.sides[s];
            let mut v = (y * gain + x[s]) as f64;
            let filtered = side.low.run(&low, v);
            if self.values[LOWPASS] <= 19_000.0 {
                v = filtered;
            }
            v = side.high.run(&high, v);
            if gain > 1.0 {
                let blocked = self.dc_pole * side.dc_y + (v - side.dc_x);
                side.dc_x = v;
                side.dc_y = blocked;
                v = blocked;
            }
            let v = self.ride(v as f32);
            self.lines[s][self.write] = v;
            let wet_sound = y.atan() + 1e-6;
            *out = (gain_out * ((1.0 - wet) * x[s] + wet_sound * wet)).clamp(-CLIP, CLIP);
        }
        self.write = (self.write + 1) & self.mask;

        // Time: a straight glide that stops at its target.
        if self.time_step != 0.0 {
            let target = self.values[DELAY];
            let next = self.time + self.time_step;
            if (self.time - target) * (next - target) > 0.0 {
                self.time = next;
            } else {
                self.time = target;
                self.time_step = 0.0;
            }
        }
    }

    /// Until the repeats are 60 dB down, or a minute where the loop holds.
    fn tail_seconds(&self) -> f32 {
        let longest = self.values[DELAY] * (1.0 + self.values[DEPTH]) / 1000.0;
        let gain = self.values[FEEDBACK];
        if self.values[FREEZE] >= 0.5 || gain >= 1.0 {
            return 60.0;
        }
        if gain <= 0.0 {
            return longest;
        }
        (longest * (0.001f32.ln() / gain.ln()).ceil()).min(60.0)
    }
}
