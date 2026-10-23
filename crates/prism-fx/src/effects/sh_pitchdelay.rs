//! ++pitchdelay: SoundHack Delay Trio's pitch-shifting delay (Tom Erbe,
//! v9.11), as its code does it.
//!
//! Two read heads drift through the line at `ratio − 1` samples a sample, each
//! wrapping within `time ± depth·time`, half that span apart, and crossfaded
//! by triangles so each is silent where it wraps. Read with the four-point
//! Hermite interpolator, a still head echoes the set time plus one sample.
//!
//! - **Ratio.** Semitones from a table — equal tempered, or just: 16/15, 9/8,
//!   6/5, 5/4, 4/3, 45/32, 3/2, 8/5, 5/3, 9/5, 15/8 and their inverses — times
//!   `2^(octave + cents/1200)`.
//! - **The loop.** `s = y·feedback + x`, always through a DC blocker, then the
//!   level stage ++delay has: it leans on the loop above a mean of 0.5 and
//!   cuts it to a third above 3, so feedback runs to 200 %. Each repeat is
//!   shifted again.
//! - **Wet.** The read clamped to ±1, then `0.9·atan(2y)`.
//!
//! A new instance starts where the original's constructor puts it: just
//! tuning, full depth, all wet, 209.83 ms.
//!
//! Changed from the original: a new time glides over 100 ms. The original
//! worked the glide out and then snapped to the target on the next sample,
//! so moving the time clicked. Its MIDI notes, which set the pitch and
//! octave, are the host's to send; its tempo lock waits for the rack to have a
//! tempo. The other controls glide; the original ramped them across each
//! host block.

use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec, Smoothed};

const DELAY: usize = 0;
const SEMITONES: usize = 1;
const CENTS: usize = 2;
const OCTAVE: usize = 3;
const FEEDBACK: usize = 4;
const DEPTH: usize = 5;
const JUST: usize = 6;
const MIX: usize = 7;
const IN_DB: usize = 8;
const OUT_DB: usize = 9;

const CLIP: f32 = 1.5;
const WINDOW: usize = 512;
/// The longest read: five seconds, plus five more at full depth.
const LONGEST_S: f32 = 10.5;

/// Just ratios from an octave down to an octave up, a semitone apart.
const JUST_RATIOS: [f32; 25] = [
    1.0 / 2.0,
    8.0 / 15.0,
    5.0 / 9.0,
    3.0 / 5.0,
    5.0 / 8.0,
    2.0 / 3.0,
    32.0 / 45.0,
    3.0 / 4.0,
    4.0 / 5.0,
    5.0 / 6.0,
    8.0 / 9.0,
    15.0 / 16.0,
    1.0,
    16.0 / 15.0,
    9.0 / 8.0,
    6.0 / 5.0,
    5.0 / 4.0,
    4.0 / 3.0,
    45.0 / 32.0,
    3.0 / 2.0,
    8.0 / 5.0,
    5.0 / 3.0,
    9.0 / 5.0,
    15.0 / 8.0,
    2.0,
];

static PARAMS: &[ParamSpec] = &[
    // 209.83 ms is where the original's constructor sets its time dial.
    ParamSpec::new("delayMs", "Time", 5.0, 5_000.0, 209.83).log().unit("ms").smooth(0.0),
    ParamSpec::new("semitones", "Pitch", -12.0, 12.0, 0.0).kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("cents", "Cents", -100.0, 100.0, 0.0).kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("octave", "Octave", -3.0, 3.0, 0.0).kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("feedback", "Feedback", 0.0, 2.0, 0.0),
    ParamSpec::new("depth", "Depth", 0.01, 1.0, 1.0),
    ParamSpec::new("just", "Just", 0.0, 1.0, 1.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("mix", "Wet/dry", 0.0, 1.0, 1.0),
    ParamSpec::new("inDb", "In", -20.0, 20.0, 0.0).unit("dB"),
    ParamSpec::new("outDb", "Out", -20.0, 20.0, 0.0).unit("dB"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "sh-pitchdelay",
    code: 12,
    version: 1,
    label: "++pitchdelay",
    category: Category::Pitch,
    params: PARAMS,
    panel: Panel {
        // The original: six dials in a row, the tuning switch below.
        groups: &[
            Group { label: "Pitch", keys: &["semitones", "cents", "octave", "just"] },
            Group { label: "Delay", keys: &["delayMs", "feedback", "depth"] },
            Group { label: "Level", keys: &["mix", "inDb", "outDb"] },
        ],
        primary: &["delayMs", "semitones", "feedback", "depth"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "delayMs", x_range: (5.0, 1_000.0), y: "feedback", y_range: (0.0, 1.5) }),
    },
    source: "SoundHack: ++pitchdelay (Delay Trio 9.11)",
    sound: None,
};

#[derive(Clone, Copy, Default)]
struct Side {
    dc_x: f32,
    dc_y: f32,
}

pub struct ShPitchDelay {
    sample_rate: f32,
    values: [f32; 10],
    feedback: Smoothed,
    depth: Smoothed,
    mix: Smoothed,
    in_db: Smoothed,
    out_db: Smoothed,
    /// Time in ms, gliding to its target over 100 ms.
    time: f32,
    time_step: f32,
    /// How far each head drifts a sample: the ratio less one.
    drift: f32,
    lines: [Vec<f32>; 2],
    mask: usize,
    write: usize,
    /// The first head's offset from the set time; the second is `span` on.
    head: f32,
    sides: [Side; 2],
    level: [f32; WINDOW],
    level_sum: f64,
    level_at: usize,
    dc_pole: f32,
}

impl ShPitchDelay {
    fn glides(&mut self) -> [(&mut Smoothed, usize); 5] {
        [
            (&mut self.feedback, FEEDBACK),
            (&mut self.depth, DEPTH),
            (&mut self.mix, MIX),
            (&mut self.in_db, IN_DB),
            (&mut self.out_db, OUT_DB),
        ]
    }

    fn ratio(&mut self) {
        let k = (self.values[SEMITONES] + 12.0).clamp(0.0, 24.0) as usize;
        let step = if self.values[JUST] > 0.5 { JUST_RATIOS[k] } else { ((k as f32 - 12.0) / 12.0).exp2() };
        self.drift = step * (self.values[CENTS] / 1200.0 + self.values[OCTAVE]).exp2() - 1.0;
    }

    /// The four-point Hermite read, as ++delay's: one sample later than `d`.
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

fn triangle(p: f32) -> f32 {
    if p < 0.5 { 4.0 * p - 1.0 } else { 3.0 - 4.0 * p }
}

/// `p` brought into −half…half, a whole span at a time.
fn fold(p: f32, half: f32) -> f32 {
    (p + half).rem_euclid(2.0 * half) - half
}

impl Kernel for ShPitchDelay {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 10];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let glide = |i: usize| Smoothed::new(sample_rate, PARAMS[i].smooth_ms);
        let length = ((sample_rate * LONGEST_S) as usize + 8).next_power_of_two();
        let mut k = ShPitchDelay {
            sample_rate,
            values,
            feedback: glide(FEEDBACK),
            depth: glide(DEPTH),
            mix: glide(MIX),
            in_db: glide(IN_DB),
            out_db: glide(OUT_DB),
            time: values[DELAY],
            time_step: 0.0,
            drift: 0.0,
            lines: [vec![0.0; length], vec![0.0; length]],
            mask: length - 1,
            write: 0,
            head: 0.0,
            sides: [Side::default(); 2],
            level: [0.0; WINDOW],
            level_sum: 0.0,
            level_at: 0,
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
            SEMITONES | CENTS | OCTAVE | JUST => self.ratio(),
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
        self.head = 0.0;
        self.sides = [Side::default(); 2];
        self.level = [0.0; WINDOW];
        self.level_sum = 0.0;
        self.level_at = 0;
        self.ratio();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let db = |d: f32| 10f32.powf(d * 0.05);
        let gain_in = db(self.in_db.next());
        let gain_out = db(self.out_db.next());
        let feedback = self.feedback.next();
        let depth = self.depth.next().max(0.01);
        let wet = self.mix.next();
        let x = [frame[0] * gain_in, frame[1] * gain_in];

        let samples = (self.time * self.sample_rate * 0.001).clamp(0.001 * self.sample_rate, 5.0 * self.sample_rate);
        let half = depth * samples;
        self.head = fold(self.head + self.drift, half);
        let heads = [self.head, fold(self.head + half, half)];
        // Each head is loudest when it reads the set time, silent where it wraps.
        let weight = heads.map(|p| triangle((p + half) / (2.0 * half)) + 1.0);
        let delays = heads.map(|p| samples - p + 2.0);

        for (s, out) in frame.iter_mut().enumerate() {
            let y = (weight[1] * self.read(s, delays[1]) + weight[0] * self.read(s, delays[0])) * 0.5;
            let side = &mut self.sides[s];
            let v = y * feedback + x[s];
            let blocked = self.dc_pole * side.dc_y + (v - side.dc_x);
            side.dc_x = v;
            side.dc_y = blocked;
            let v = self.ride(blocked);
            self.lines[s][self.write] = v;
            let wet_sound = 0.9 * (2.0 * y.clamp(-1.0, 1.0)).atan();
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
    }

    /// Until the repeats are 60 dB down, or a minute where the loop holds.
    fn tail_seconds(&self) -> f32 {
        let longest = self.values[DELAY] * (1.0 + self.values[DEPTH]) / 1000.0;
        let gain = self.values[FEEDBACK];
        if gain >= 1.0 {
            return 60.0;
        }
        if gain <= 0.0 {
            return longest;
        }
        (longest * (0.001f32.ln() / gain.ln()).ceil()).min(60.0)
    }
}
