//! ++bubbler: SoundHack Freesound's granular delay (Tom Erbe, v9.12), as its
//! code does it.
//!
//! Up to eight grains at once read back from a delay line. A grain starts
//! every `grain length / (4 × density)` samples, when a voice is free.
//!
//! - **Grain length** is `grain size × D`, D being the time plus two samples,
//!   give or take the start variation; the start itself waits up to `start
//!   variation × interval`.
//! - **Pitch** is `2^trunc(octave ± octave variation)` times the ratio of a
//!   key picked at random from those held down (the root when none is), equal
//!   tempered or just: 16/15, 9/8, 6/5, 5/4, 4/3, 7/5, 3/2, 8/5, 5/3, 7/4, 15/8.
//!   Octave variation gives whole octaves only.
//! - **Delay.** A grain reads `D × rate ± time variation × D` back, so an
//!   up-pitched grain does not overtake the write.
//! - **Envelope.** A raised cosine over grains up to 0.1 s; longer ones fade
//!   in and out over 50 ms. Every grain is halved. Pan variation balances it
//!   between the left line and the right.
//! - **The line** is fed the input plus feedback through a resonant low-pass
//!   (out of the way above 19 kHz), a DC blocker and ++delay's level stage, so
//!   feedback runs to 200 %. The wet sound is `atan`.
//!
//! A new instance starts where the original's constructor puts it: 190.5 ms,
//! time variation 50 %, grain size 10 %, density 80 %, reversal 50 %.
//!
//! Changed from the original: in and out gain act (it never read them).
//! Reversal is a chance per grain; the original compared a 0–1 random number
//! with the 0–100 dial, so anything from 1 up reversed every grain. A grain
//! reads at its true position; the original mirrored the fraction, which put
//! whole-sample reads one sample early and jittered the rest. Grain times are
//! counted in whole samples, where the original's float clock coarsened after
//! six minutes. A time that lands exactly on its target stops there. Its MIDI
//! keys are the host's to send; its tempo lock waits for the rack to have a
//! tempo. The other controls glide; the original ramped them across each
//! host block.

use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec, Smoothed};
use core::f64::consts::{PI, TAU};

const DELAY: usize = 0;
const TIME_VAR: usize = 1;
const FEEDBACK: usize = 2;
const GRAIN: usize = 3;
const PAN_VAR: usize = 4;
const DENSITY: usize = 5;
const START_VAR: usize = 6;
const OCTAVE: usize = 7;
const OCTAVE_VAR: usize = 8;
const REVERSE: usize = 9;
const JUST: usize = 10;
const LOWPASS: usize = 11;
const RESONANCE: usize = 12;
const KEY0: usize = 13;
const MIX: usize = 25;
const IN_DB: usize = 26;
const OUT_DB: usize = 27;
const COUNT: usize = 28;

const VOICES: usize = 8;
const CLIP: f32 = 1.5;
const WINDOW: usize = 512;
/// The line: about 44 s at 48 kHz, as the original's 2^21.
const LINE_S: f32 = 43.0;
/// The original never seeded its generator; this one starts here.
const SEED: u32 = 0x2545_F491;

const JUST_RATIOS: [f64; 12] = [
    1.0,
    16.0 / 15.0,
    9.0 / 8.0,
    6.0 / 5.0,
    5.0 / 4.0,
    4.0 / 3.0,
    7.0 / 5.0,
    3.0 / 2.0,
    8.0 / 5.0,
    5.0 / 3.0,
    7.0 / 4.0,
    15.0 / 8.0,
];

const fn key(key: &'static str, label: &'static str) -> ParamSpec {
    ParamSpec::new(key, label, 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0)
}

static PARAMS: &[ParamSpec] = &[
    // The defaults are where the original's constructor sets its dials.
    ParamSpec::new("delayMs", "Time", 10.0, 10_000.0, 190.5).log().unit("ms").smooth(0.0),
    ParamSpec::new("timeVar", "Time vari", 0.0, 1.0, 0.5),
    ParamSpec::new("feedback", "Feedback", 0.0, 2.0, 0.0),
    ParamSpec::new("grainSize", "Grain size", 0.0, 0.5, 0.1),
    ParamSpec::new("panVar", "Pan vari", 0.0, 1.0, 0.0),
    ParamSpec::new("density", "Density", 0.0, 2.0, 0.8),
    ParamSpec::new("startVar", "G start vari", 0.0, 1.0, 0.0),
    ParamSpec::new("octave", "Octave", -4.0, 4.0, 0.0).kind(ParamKind::Int),
    ParamSpec::new("octaveVar", "Oct vari", 0.0, 2.0, 0.0),
    ParamSpec::new("reverse", "G reversal", 0.0, 1.0, 0.5),
    ParamSpec::new("just", "Just", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("lowpassHz", "Filter freq", 20.0, 20_000.0, 20_000.0).log().unit("Hz").smooth(0.0),
    ParamSpec::new("resonance", "Resonance", 0.0, 1.0, 0.1).smooth(0.0),
    key("key0", "Root"),
    key("key1", "m2"),
    key("key2", "M2"),
    key("key3", "m3"),
    key("key4", "M3"),
    key("key5", "4th"),
    key("key6", "Tritone"),
    key("key7", "5th"),
    key("key8", "m6"),
    key("key9", "M6"),
    key("key10", "m7"),
    key("key11", "M7"),
    ParamSpec::new("mix", "Wet/dry", 0.0, 1.0, 0.5),
    ParamSpec::new("inDb", "In", -20.0, 20.0, 0.0).unit("dB"),
    ParamSpec::new("outDb", "Out", -20.0, 20.0, 0.0).unit("dB"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "sh-bubbler",
    code: 14,
    version: 1,
    label: "++bubbler",
    category: Category::Granular,
    params: PARAMS,
    panel: Panel {
        // The original: two rows of six dials, then a keyboard of twelve keys.
        groups: &[
            Group { label: "Delay", keys: &["delayMs", "timeVar", "feedback", "lowpassHz", "resonance", "panVar"] },
            Group { label: "Grains", keys: &["density", "grainSize", "startVar", "octave", "octaveVar", "reverse"] },
            Group {
                label: "Keys",
                keys: &[
                    "key0", "key1", "key2", "key3", "key4", "key5", "key6", "key7", "key8", "key9", "key10", "key11",
                    "just",
                ],
            },
            Group { label: "Level", keys: &["mix", "inDb", "outDb"] },
        ],
        primary: &["delayMs", "density", "grainSize", "feedback"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "density", x_range: (0.0, 2.0), y: "grainSize", y_range: (0.0, 0.5) }),
    },
    source: "SoundHack: ++bubbler (Freesound 9.12)",
    sound: None,
};

#[derive(Clone, Copy, Default)]
struct Voice {
    active: bool,
    start: u64,
    end: f64,
    pan: f32,
    /// Long grains fade over 50 ms at each end; short ones are one window.
    long: bool,
    up_end: f64,
    down_start: f64,
    /// Envelope cycles per sample.
    env_rate: f64,
    position: f64,
    rate: f64,
}

#[derive(Clone, Copy, Default)]
struct Line {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
    dc_x: f32,
    dc_y: f32,
}

pub struct ShBubbler {
    sample_rate: f32,
    values: [f32; COUNT],
    time_var: Smoothed,
    feedback: Smoothed,
    grain: Smoothed,
    pan_var: Smoothed,
    density: Smoothed,
    start_var: Smoothed,
    octave: Smoothed,
    octave_var: Smoothed,
    reverse: Smoothed,
    mix: Smoothed,
    in_db: Smoothed,
    out_db: Smoothed,
    /// Time in ms, gliding to its target over 100 ms.
    time: f32,
    time_step: f32,
    lines: [Vec<f32>; 2],
    states: [Line; 2],
    mask: usize,
    write: usize,
    counter: u64,
    last_start: u64,
    voices: [Voice; VOICES],
    keys: [usize; 12],
    key_count: usize,
    seed: u32,
    level: [f32; WINDOW],
    level_sum: f64,
    level_at: usize,
    coefs: [f64; 5],
    dc_pole: f32,
}

impl ShBubbler {
    fn glides(&mut self) -> [(&mut Smoothed, usize); 12] {
        [
            (&mut self.time_var, TIME_VAR),
            (&mut self.feedback, FEEDBACK),
            (&mut self.grain, GRAIN),
            (&mut self.pan_var, PAN_VAR),
            (&mut self.density, DENSITY),
            (&mut self.start_var, START_VAR),
            (&mut self.octave, OCTAVE),
            (&mut self.octave_var, OCTAVE_VAR),
            (&mut self.reverse, REVERSE),
            (&mut self.mix, MIX),
            (&mut self.in_db, IN_DB),
            (&mut self.out_db, OUT_DB),
        ]
    }

    /// The original's generator, and its draws as 0–1 and as −1–1.
    fn draw(&mut self) -> f64 {
        self.seed = self.seed.wrapping_mul(0x0BB3_8435).wrapping_add(0x3619_636B);
        self.seed as f32 as f64 / 4_294_967_296.0
    }

    fn bipolar(&mut self) -> f64 {
        2.0 * self.draw() - 1.0
    }

    fn keys(&mut self) {
        self.key_count = 0;
        for k in 0..12 {
            if self.values[KEY0 + k] > 0.5 {
                self.keys[self.key_count] = k;
                self.key_count += 1;
            }
        }
        if self.key_count == 0 {
            self.keys[0] = 0;
            self.key_count = 1;
        }
    }

    fn filter(&mut self) {
        let sr = self.sample_rate as f64;
        let w = (self.values[LOWPASS] as f64).min(0.5 * sr) / sr;
        let c = if w >= 0.1 { (PI * (0.5 - w)).tan() } else { 1.0 / (w * PI) };
        let d = (1.0 - self.values[RESONANCE] as f64) * 1.314_213_562_373_095 + 0.1;
        let a0 = 1.0 / (d * c + 1.0 + c * c);
        self.coefs = [a0, 2.0 * a0, a0, 2.0 * a0 * (1.0 - c * c), a0 * (1.0 - d * c + c * c)];
    }

    /// The four-point Hermite read at `position`.
    fn read(&self, side: usize, position: f64) -> f32 {
        let line = &self.lines[side];
        let i = position.floor();
        let t = (position - i) as f32;
        let i = i as usize;
        let at = |k: usize| line[i.wrapping_add(k).wrapping_sub(1) & self.mask];
        let (ym1, y0, y1, y2) = (at(0), at(1), at(2), at(3));
        let c1 = 0.5 * (y1 - ym1);
        let c3 = 0.5 * (y2 - y1 - ym1 + y0) + (y0 - y1);
        let c2 = (ym1 - y0) + c1 - c3;
        ((c3 * t + c2) * t + c1) * t + y0
    }

    fn envelope(&self, v: &Voice) -> f32 {
        let now = self.counter as f64;
        let fade = 0.05 * self.sample_rate as f64;
        let e = if !v.long || now <= v.up_end {
            0.5 - 0.5 * ((now - v.start as f64) * TAU * v.env_rate).cos()
        } else if v.down_start > now {
            1.0
        } else {
            0.5 - 0.5 * (v.env_rate * TAU * ((now - v.down_start) - fade)).cos()
        };
        e as f32
    }

    /// The write path: low-pass, DC blocker, level stage.
    fn feed(&mut self, side: usize, x: f32) -> f32 {
        let [b0, b1, b2, a1, a2] = self.coefs;
        let s = &mut self.states[side];
        let x = x as f64;
        let y = b0 * x + b1 * s.x1 + b2 * s.x2 - a1 * s.y1 - a2 * s.y2;
        s.x2 = s.x1;
        s.x1 = x;
        s.y2 = s.y1;
        s.y1 = y;
        let y = if self.values[LOWPASS] > 19_000.0 { x as f32 } else { y as f32 };
        let z = self.dc_pole * s.dc_y + (y - s.dc_x);
        s.dc_y = z;
        s.dc_x = y;
        let a = z.abs();
        self.level_sum += (a - self.level[self.level_at]) as f64;
        self.level[self.level_at] = a;
        self.level_at = (self.level_at + 1) % WINDOW;
        let mean = (self.level_sum / WINDOW as f64) as f32;
        if mean > 3.0 {
            z * 0.33333
        } else if mean > 0.5 {
            z * (((mean * -0.089_017_6 + 0.553_09) * mean - 1.245_09) * mean + 1.493_845)
        } else {
            z
        }
    }

    /// Start a grain in the first free voice, if there is one.
    fn start(&mut self, now: &Now, interval: f64, grain: f64, delay: f64) {
        let Some(slot) = self.voices.iter().position(|v| !v.active) else { return };
        self.last_start = self.counter;
        let sr = self.sample_rate as f64;
        // The draws, in the original's order: start, length, pan, key,
        // octave, delay, reversal.
        let begin = self.counter + (interval * now.start_var * self.draw()) as u64;
        let length = now.start_var * self.bipolar() * grain + grain;
        let pan = (now.pan_var * (self.draw() - 0.5) + 0.5) as f32;
        let end = begin as f64 + length;
        let long = length / sr > 0.1;
        let pick = self.draw();
        let octave = (self.bipolar() * now.octave_var + now.octave) as i32;
        let index = ((self.key_count as f64 * pick) as usize).min(self.key_count - 1);
        let key = self.keys[index];
        let step = if self.values[JUST] > 0.5 { JUST_RATIOS[key] } else { (key as f64 / 12.0).exp2() };
        let mut rate = 2f64.powi(octave) * step;
        let lag = now.time_var * self.bipolar() * delay + delay * rate;
        let position = (self.write as f64 - lag).rem_euclid(self.lines[0].len() as f64);
        if self.draw() < now.reverse {
            rate = -rate;
        }
        self.voices[slot] = Voice {
            active: true,
            start: begin,
            end,
            pan,
            long,
            up_end: if long { begin as f64 + 0.05 * sr } else { 0.0 },
            down_start: if long { end - 0.05 * sr } else { 0.0 },
            env_rate: if long { 10.0 / sr } else { 1.0 / length.max(1.0) },
            position,
            rate,
        };
    }
}

/// The glided values a new grain is drawn from.
struct Now {
    time_var: f64,
    pan_var: f64,
    start_var: f64,
    octave: f64,
    octave_var: f64,
    reverse: f64,
}

impl Kernel for ShBubbler {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; COUNT];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let glide = |i: usize| Smoothed::new(sample_rate, PARAMS[i].smooth_ms);
        let length = ((sample_rate * LINE_S) as usize).next_power_of_two();
        let mut k = ShBubbler {
            sample_rate,
            values,
            time_var: glide(TIME_VAR),
            feedback: glide(FEEDBACK),
            grain: glide(GRAIN),
            pan_var: glide(PAN_VAR),
            density: glide(DENSITY),
            start_var: glide(START_VAR),
            octave: glide(OCTAVE),
            octave_var: glide(OCTAVE_VAR),
            reverse: glide(REVERSE),
            mix: glide(MIX),
            in_db: glide(IN_DB),
            out_db: glide(OUT_DB),
            time: values[DELAY],
            time_step: 0.0,
            lines: [vec![0.0; length], vec![0.0; length]],
            states: [Line::default(); 2],
            mask: length - 1,
            write: 0,
            counter: 0,
            last_start: 0,
            voices: [Voice::default(); VOICES],
            keys: [0; 12],
            key_count: 1,
            seed: SEED,
            level: [0.0; WINDOW],
            level_sum: 0.0,
            level_at: 0,
            coefs: [0.0; 5],
            dc_pole: 1.0 - 126.0 / sample_rate,
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
            DELAY if v != before => self.time_step = (v - self.time) / (self.sample_rate * 0.1),
            LOWPASS | RESONANCE => self.filter(),
            i if (KEY0..KEY0 + 12).contains(&i) => self.keys(),
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
        self.states = [Line::default(); 2];
        self.write = 0;
        self.counter = 0;
        self.last_start = 0;
        self.voices = [Voice::default(); VOICES];
        self.seed = SEED;
        self.level = [0.0; WINDOW];
        self.level_sum = 0.0;
        self.level_at = 0;
        self.keys();
        self.filter();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let db = |d: f32| 10f32.powf(d * 0.05);
        let gain_in = db(self.in_db.next());
        let gain_out = db(self.out_db.next());
        let feedback = self.feedback.next();
        let wet = self.mix.next();
        let grain_size = self.grain.next().max(1e-5) as f64;
        let density = self.density.next() as f64;
        let now = Now {
            time_var: self.time_var.next() as f64,
            pan_var: self.pan_var.next() as f64,
            start_var: self.start_var.next() as f64,
            octave: self.octave.next() as f64,
            octave_var: self.octave_var.next() as f64,
            reverse: self.reverse.next() as f64,
        };
        let x = [frame[0] * gain_in, frame[1] * gain_in];

        let delay = self.time as f64 * 0.001 * self.sample_rate as f64 + 2.0;
        let grain = delay * grain_size;
        let interval = if density <= 0.0 { f64::INFINITY } else { grain / (density * 4.0) };
        if (self.last_start as f64 + interval) < self.counter as f64 {
            self.start(&now, interval, grain, delay);
        }

        let mut sum = [0.0f32; 2];
        let length = self.lines[0].len() as f64;
        for i in 0..VOICES {
            let v = self.voices[i];
            if !v.active || v.start > self.counter {
                continue;
            }
            let env = self.envelope(&v) * 0.5;
            sum[0] += self.read(0, v.position) * env * v.pan;
            sum[1] += self.read(1, v.position) * env * (1.0 - v.pan);
            self.voices[i].position = (v.position + v.rate).rem_euclid(length);
        }

        for s in [1, 0] {
            let fed = self.feed(s, x[s] + feedback * sum[s]);
            self.lines[s][self.write] = fed;
        }
        for (s, out) in frame.iter_mut().enumerate() {
            let wet_sound = sum[s].atan() + 1e-6;
            *out = (((1.0 - wet) * x[s] + wet_sound * wet) * gain_out).clamp(-CLIP, CLIP);
        }

        self.write = (self.write + 1) & self.mask;
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
        let now = self.counter as f64;
        for v in &mut self.voices {
            if now > v.end {
                v.active = false;
            }
        }
        self.counter += 1;
    }

    /// The longest grain's read, and its repeats.
    fn tail_seconds(&self) -> f32 {
        let longest = self.values[DELAY] * (1.0 + self.values[TIME_VAR]) * (1.0 + self.values[GRAIN]) / 1000.0;
        let gain = self.values[FEEDBACK] * 0.5;
        if gain >= 1.0 {
            return 60.0;
        }
        if gain <= 0.0 {
            return longest;
        }
        (longest * (0.001f32.ln() / gain.ln()).ceil()).min(60.0)
    }
}
