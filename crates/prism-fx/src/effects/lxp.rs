//! The engine the four LXP reverbs share — this crate's own design, not
//! Lexicon's.
//!
//! Lexicon's LXP Native plug-ins are encrypted by their copy protection, so
//! their code was not read. What could be read — the parameters and their
//! text, the 225 factory presets, the panel art, and what the manual says each
//! control is for — fixes the controls. The sound behind them is built here:
//!
//! - **Input.** Each side's input, plus any echo fed back to it, feeds the
//!   echoes, the reflections and the predelay. Predelay holds back only the
//!   reverb, as the manual says.
//! - **Diffusion.** After the predelay, four all-passes in series (six on the
//!   Plate), their gain `diffusion × max`.
//! - **Shape and Spread.** The diffused sound is fed into the tank at eight
//!   points across a window `spread × 2 × size / 343` seconds long, a quarter
//!   millisecond apart even when the window is shut. Their weights are
//!   `e^(β·u)` along the window, `β = 6 × (2 × shape / 64 − 1)`, so a low shape
//!   feeds early, 32 evenly and a high shape late. They are normalised to unit
//!   energy.
//! - **The tank.** Eight lines `size / 343 × ratio` seconds long, mixed by an
//!   8 × 8 Hadamard matrix. Each line has 0–2 all-passes inside the loop, by
//!   algorithm. Then a two-band decay: a one-pole split at Bass XOV, and a gain
//!   on each band for a 60 dB fall in `reverbTime` above the split and in
//!   `reverbTime × bassRt` below it. Then a one-pole low-pass at RT HiCut. So
//!   Reverb Time is the time itself, whatever the size.
//! - **Spin and Wander.** Each line's length wanders: `spin` times a second it
//!   takes a step of up to `wander` milliseconds, kept within 0 to `wander`,
//!   and glides there over the next cycle. Spin at zero holds every line still.
//! - **Tap Slope.** Each line is heard at a quarter, a half, three quarters and
//!   its end, the taps tilted by 2.5 dB per step of slope across the line, so a
//!   positive slope builds and a negative one falls away.
//! - **Early voices.** Reflections are diffused delays: a delay, one
//!   all-pass, a gain, to their own or the other side. Echoes are the same, to
//!   their own side, and they feed back into that side's input, which the
//!   reverb and the reflections also hear.
//! - **Out.** Early Level scales the reflections and echoes, Rvb Level the
//!   tank. The lowest setting of each level is silence. Then a second-order
//!   low-pass at Rolloff, then the wet/dry mix.
//!
//! Coefficients follow their controls every 16 frames, counted from the last
//! reset, so the output does not depend on the host's block size.

use crate::{Descriptor, Kernel, ParamKind, ParamSpec, Smoothed};
use core::marker::PhantomData;

/// The speed of sound, for turning a size into a time.
const SOUND: f32 = 343.0;
/// The lowest setting of a level, which is off.
pub const OFF_DB: f32 = -80.0;
const LINES: usize = 8;
const TAPS: usize = 8;
/// Injection taps sit this far apart even when Spread is shut.
const STAGGER_S: f32 = 0.000_25;
/// How often coefficients follow their controls, in frames.
const CONTROL: u32 = 16;
/// Echo feedback is kept below this, so a full setting rings and does not grow.
const MOST_FEEDBACK: f32 = 0.98;
/// The early voices' own all-passes.
const VOICE_DIFFUSION: f32 = 0.5;
const REFLECTION_AP_S: [f32; 4] = [0.001_31, 0.001_73, 0.002_07, 0.002_53];
const ECHO_AP_S: [f32; 2] = [0.001_67, 0.002_29];
/// Input all-pass lengths, per side.
const INPUT_AP_S: [[f32; 6]; 2] = [
    [0.002_43, 0.003_71, 0.005_17, 0.007_39, 0.009_11, 0.011_03],
    [0.002_61, 0.003_89, 0.005_47, 0.007_73, 0.009_59, 0.011_41],
];
/// The loop all-passes, as shares of their line.
const LOOP_AP: [f32; 2] = [0.067, 0.109];
/// Where each line is heard, as shares of its length.
const OUT_TAPS: [f32; 4] = [0.25, 0.5, 0.75, 1.0];
/// Brings the tank's output to about the input's level at a reverb time of
/// 1.5 s.
const TANK_GAIN: f32 = 0.35;
/// The tank's two outputs: orthogonal rows of the Hadamard matrix.
const OUT_SIGNS: [[f32; LINES]; 2] = [[1., -1., 1., -1., 1., -1., 1., -1.], [1., 1., -1., -1., 1., 1., -1., -1.]];
/// The right side feeds the lines with this sign, the left with +1.
const RIGHT_SIGNS: [f32; LINES] = [1., -1., -1., 1., 1., -1., -1., 1.];

const MAX_PREDELAY_S: f32 = 0.5;
const MAX_VOICE_S: f32 = 1.0;
const MAX_SIZE_M: f32 = 80.0;
const MAX_WANDER_S: f32 = 0.024;

/// What a parameter does. Each algorithm lists one per parameter, in order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Role {
    Predelay,
    Diffusion,
    Shape,
    Spread,
    Size,
    ReverbTime,
    BassRt,
    BassXov,
    RtHiCut,
    Spin,
    Wander,
    TapSlope,
    ReflectionMaster,
    /// A reflection's delay and gain, by voice.
    ReflectionDelay(usize),
    ReflectionGain(usize),
    EchoTimeMaster,
    EchoFeedbackMaster,
    /// An echo's delay, polarity and feedback, by side.
    EchoDelay(usize),
    EchoInvert(usize),
    EchoFeedback(usize),
    Early,
    Reverb,
    Rolloff,
    Mix,
}

/// How one algorithm's tank and voices are built.
pub struct Voicing {
    /// The lines' lengths as shares of the time sound takes to cross the room.
    pub lines: [f32; LINES],
    /// All-passes inside each line's loop, 0 to 2, and their gain.
    pub loop_allpasses: usize,
    pub loop_gain: f32,
    /// Input all-passes, 4 to 6, and their gain at full diffusion.
    pub input_allpasses: usize,
    pub input_gain: f32,
    /// Each reflection's input side and output side: 0 left, 1 right.
    pub reflections: &'static [(usize, usize)],
    pub echoes: bool,
}

/// One of the four reverbs: its descriptor, its parameters' roles, its voicing.
pub trait Algorithm: 'static {
    fn descriptor() -> &'static Descriptor;
    const ROLES: &'static [Role];
    const VOICING: Voicing;
}

/// The controls as the engine reads them, one frame's worth.
#[derive(Clone, Copy, Default)]
struct Settings {
    predelay_ms: f32,
    diffusion: f32,
    shape: f32,
    spread: f32,
    size: f32,
    reverb_time: f32,
    bass_rt: f32,
    bass_xov: f32,
    rt_hi_cut: f32,
    spin: f32,
    wander_ms: f32,
    tap_slope: f32,
    reflection_master: f32,
    reflection_ms: [f32; 4],
    reflection_db: [f32; 4],
    echo_time_master: f32,
    echo_feedback_master: f32,
    echo_ms: [f32; 2],
    echo_invert: [f32; 2],
    echo_db: [f32; 2],
    early_db: f32,
    reverb_db: f32,
    rolloff: f32,
    mix: f32,
}

impl Settings {
    fn read(roles: &[Role], values: &[f32]) -> Self {
        let mut s = Settings { echo_time_master: 1.0, ..Settings::default() };
        for (role, &v) in roles.iter().zip(values) {
            match *role {
                Role::Predelay => s.predelay_ms = v,
                Role::Diffusion => s.diffusion = v,
                Role::Shape => s.shape = v,
                Role::Spread => s.spread = v,
                Role::Size => s.size = v,
                Role::ReverbTime => s.reverb_time = v,
                Role::BassRt => s.bass_rt = v,
                Role::BassXov => s.bass_xov = v,
                Role::RtHiCut => s.rt_hi_cut = v,
                Role::Spin => s.spin = v,
                Role::Wander => s.wander_ms = v,
                Role::TapSlope => s.tap_slope = v,
                Role::ReflectionMaster => s.reflection_master = v,
                Role::ReflectionDelay(i) => s.reflection_ms[i] = v,
                Role::ReflectionGain(i) => s.reflection_db[i] = v,
                Role::EchoTimeMaster => s.echo_time_master = v,
                Role::EchoFeedbackMaster => s.echo_feedback_master = v,
                Role::EchoDelay(i) => s.echo_ms[i] = v,
                Role::EchoInvert(i) => s.echo_invert[i] = v,
                Role::EchoFeedback(i) => s.echo_db[i] = v,
                Role::Early => s.early_db = v,
                Role::Reverb => s.reverb_db = v,
                Role::Rolloff => s.rolloff = v,
                Role::Mix => s.mix = v,
            }
        }
        s
    }
}

// ---- the controls all four share ----------------------------------------------
//
// Ranges are not in anything readable; each is wide enough for every factory
// preset, and set where the presets and the manual point (see the spec).

pub const fn predelay(default: f32) -> ParamSpec {
    ParamSpec::new("predelayMs", "Predelay", 0.0, 500.0, default).unit("ms").smooth(60.0)
}
pub const fn diffusion(default: f32) -> ParamSpec {
    ParamSpec::new("diffusion", "Diffusion", 0.0, 1.0, default).smooth(20.0)
}
pub const fn shape(default: f32) -> ParamSpec {
    ParamSpec::new("shape", "Shape", 0.0, 64.0, default).kind(ParamKind::Int).smooth(20.0)
}
pub const fn spread(default: f32) -> ParamSpec {
    ParamSpec::new("spread", "Spread", 0.0, 1.0, default).smooth(20.0)
}
pub const fn size(default: f32) -> ParamSpec {
    ParamSpec::new("sizeM", "Size", 2.0, MAX_SIZE_M, default).unit("m").smooth(200.0)
}
pub const fn reverb_time(default: f32) -> ParamSpec {
    ParamSpec::new("reverbTimeS", "Reverb Time", 0.1, 20.0, default).log().unit("s").smooth(20.0)
}
pub const fn bass_rt(default: f32) -> ParamSpec {
    ParamSpec::new("bassRt", "BassRT", 0.2, 4.0, default).unit("x").smooth(20.0)
}
pub const fn bass_xov(default: f32) -> ParamSpec {
    ParamSpec::new("bassXovHz", "Bass XOV", 20.0, 5_000.0, default).log().unit("Hz").smooth(20.0)
}
pub const fn rt_hi_cut(default: f32) -> ParamSpec {
    ParamSpec::new("rtHiCutHz", "RT HiCut", 100.0, 20_000.0, default).log().unit("Hz").smooth(20.0)
}
pub const fn spin(default: f32) -> ParamSpec {
    ParamSpec::new("spinHz", "Spin", 0.0, 5.0, default).unit("Hz").smooth(20.0)
}
pub const fn wander(default: f32) -> ParamSpec {
    ParamSpec::new("wanderMs", "Wander", 0.0, MAX_WANDER_S * 1000.0, default).unit("ms").smooth(20.0)
}
pub const fn tap_slope(default: f32) -> ParamSpec {
    ParamSpec::new("tapSlope", "Tap Slope", -10.0, 10.0, default).kind(ParamKind::Int).smooth(20.0)
}
pub const fn reflection_master(default: f32) -> ParamSpec {
    ParamSpec::new("rflTimeMaster", "Rfl Time Mast", 0.0, 1.0, default).smooth(60.0)
}
/// A voice's delay: a reflection's or an echo's.
pub const fn voice_delay(key: &'static str, label: &'static str, default: f32) -> ParamSpec {
    ParamSpec::new(key, label, 0.0, MAX_VOICE_S * 1000.0, default).unit("ms").smooth(60.0)
}
/// A level whose bottom is off.
pub const fn level(key: &'static str, label: &'static str, default: f32) -> ParamSpec {
    ParamSpec::new(key, label, OFF_DB, 0.0, default).unit("dB").smooth(10.0)
}
pub const fn echo_master(key: &'static str, label: &'static str, default: f32) -> ParamSpec {
    ParamSpec::new(key, label, 0.0, 1.0, default).smooth(60.0)
}
pub const fn polarity(key: &'static str, label: &'static str, default: f32) -> ParamSpec {
    ParamSpec::new(key, label, 0.0, 1.0, default).kind(ParamKind::Choice(&["Normal", "Inverted"])).smooth(0.0)
}
pub const fn rolloff(default: f32) -> ParamSpec {
    ParamSpec::new("rolloffHz", "Rolloff", 100.0, 20_000.0, default).log().unit("Hz").smooth(20.0)
}
pub const fn mix(default: f32) -> ParamSpec {
    ParamSpec::new("mix", "Mix", 0.0, 1.0, default).smooth(10.0)
}

/// The original's pages, as panel groups, for the controls all four share.
pub const REVERB_KEYS: &[&str] = &["sizeM", "reverbTimeS", "bassRt", "bassXovHz", "rtHiCutHz"];
pub const MODULATION_KEYS: &[&str] = &["shape", "spread", "spinHz", "wanderMs", "tapSlope"];
pub const LEVELS_KEYS: &[&str] = &["predelayMs", "diffusion", "earlyDb", "reverbDb", "rolloffHz", "mix"];
/// The soft row every factory preset ships with.
pub const SOFT_ROW: &[&str] = &["predelayMs", "reverbTimeS", "rolloffHz", "diffusion", "rtHiCutHz", "mix"];

/// A level in dB as a gain, with the bottom of the range silent.
fn gain(db: f32) -> f32 {
    if db <= OFF_DB {
        0.0
    } else {
        10f32.powf(db * 0.05)
    }
}

/// A delay line. `tap(0)` is the newest sample pushed.
struct Line {
    buf: Vec<f32>,
    mask: usize,
    at: usize,
}

impl Line {
    fn new(longest: f32) -> Self {
        let length = (longest.max(1.0) as usize + 8).next_power_of_two();
        Line { buf: vec![0.0; length], mask: length - 1, at: 0 }
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
        self.at = 0;
    }

    fn push(&mut self, x: f32) {
        self.at = (self.at + 1) & self.mask;
        self.buf[self.at] = x;
    }

    fn tap(&self, k: usize) -> f32 {
        self.buf[self.at.wrapping_sub(k) & self.mask]
    }

    /// `d` samples back, read straight between neighbours.
    fn linear(&self, d: f32) -> f32 {
        let d = d.clamp(0.0, (self.mask - 4) as f32);
        let i = d as usize;
        let a = self.tap(i);
        a + (self.tap(i + 1) - a) * (d - i as f32)
    }

    /// `d` samples back, at least one, by a four-point Hermite curve.
    fn hermite(&self, d: f32) -> f32 {
        let d = d.clamp(1.0, (self.mask - 4) as f32);
        let i = d as usize;
        let t = d - i as f32;
        let (xm1, x0, x1, x2) = (self.tap(i - 1), self.tap(i), self.tap(i + 1), self.tap(i + 2));
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * t + c2) * t + c1) * t + x0
    }
}

/// A Schroeder all-pass whose delay may be fractional and may move.
struct Allpass(Line);

impl Allpass {
    fn new(longest: f32) -> Self {
        Allpass(Line::new(longest))
    }

    /// `m` is the delay in samples, at least one.
    fn run(&mut self, x: f32, m: f32, g: f32) -> f32 {
        let d = self.0.linear(m.max(1.0) - 1.0);
        let w = x - g * d;
        self.0.push(w);
        d + g * w
    }
}

/// A second-order low-pass, in f64 so it holds together low in the range.
#[derive(Clone, Copy, Default)]
struct Lowpass {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    state: [[f64; 4]; 2],
}

impl Lowpass {
    fn tune(&mut self, hz: f32, sample_rate: f32) {
        let k = (core::f64::consts::PI * (hz.min(0.45 * sample_rate) / sample_rate) as f64).tan();
        let root2 = core::f64::consts::SQRT_2;
        let a0 = 1.0 / (1.0 + root2 * k + k * k);
        self.b0 = k * k * a0;
        self.b1 = 2.0 * self.b0;
        self.b2 = self.b0;
        self.a1 = 2.0 * (k * k - 1.0) * a0;
        self.a2 = (1.0 - root2 * k + k * k) * a0;
    }

    fn run(&mut self, side: usize, x: f32) -> f32 {
        let [x1, x2, y1, y2] = self.state[side];
        let x = x as f64;
        let y = self.b0 * x + self.b1 * x1 + self.b2 * x2 - self.a1 * y1 - self.a2 * y2;
        let y = if y.abs() < 1e-30 { 0.0 } else { y };
        self.state[side] = [x, x1, y, y1];
        y as f32
    }
}

/// One line's wanderer: where its offset glides from and to, and how far
/// through the glide it is.
#[derive(Clone, Copy, Default)]
struct Wanderer {
    from: f32,
    to: f32,
    phase: f32,
}

/// A tank line and what sits in its loop.
struct TankLine {
    line: Line,
    allpasses: [Allpass; 2],
    /// The bass split and the high cut's states.
    low: f32,
    high: f32,
    /// This line's mid and bass gains per pass.
    mid_gain: f32,
    bass_gain: f32,
    wander: Wanderer,
}

/// One of the LXP reverbs, as `A` voices it.
pub struct Lxp<A: Algorithm> {
    sample_rate: f32,
    values: Vec<f32>,
    glides: Vec<Smoothed>,
    current: Vec<f32>,
    clock: u32,
    seed: u32,
    // Per side: the reverb's input history, the predelayed and diffused sound
    // the taps read, the voices' history, and the echoes.
    predelay: [Line; 2],
    inputs: [[Allpass; 6]; 2],
    spread: [Line; 2],
    source: [Line; 2],
    echo: [Line; 2],
    echo_ap: [Allpass; 2],
    reflection_ap: [Allpass; 4],
    tank: [TankLine; LINES],
    rolloff: Lowpass,
    // Coefficients, set at the control rate.
    input_g: f32,
    tap_times: [f32; TAPS],
    tap_weights: [f32; TAPS],
    out_weights: [f32; 4],
    bass_coef: f32,
    high_coef: f32,
    _algorithm: PhantomData<fn() -> A>,
}

impl<A: Algorithm> Lxp<A> {
    fn params() -> &'static [ParamSpec] {
        A::descriptor().params
    }

    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32
    }

    /// Each line's length now, in samples, before it wanders.
    fn base(&self, s: &Settings, i: usize) -> f32 {
        s.size / SOUND * A::VOICING.lines[i] * self.sample_rate
    }

    /// Follow the controls: the coefficients that need more than a multiply.
    fn control(&mut self, s: &Settings) {
        let sr = self.sample_rate;
        let v = &A::VOICING;
        self.input_g = s.diffusion * v.input_gain;

        // Injection: eight taps across the spread window, weighted by shape.
        let window = s.spread * 2.0 * s.size / SOUND * sr;
        let beta = 6.0 * (2.0 * s.shape / 64.0 - 1.0);
        let mut energy = 0.0;
        for k in 0..TAPS {
            let u = k as f32 / (TAPS - 1) as f32;
            self.tap_times[k] = window * u + k as f32 * STAGGER_S * sr;
            self.tap_weights[k] = (beta * u).exp();
            energy += self.tap_weights[k] * self.tap_weights[k];
        }
        let norm = 1.0 / energy.sqrt();
        self.tap_weights.iter_mut().for_each(|w| *w *= norm);

        // Output taps tilted by the slope.
        let mut energy = 0.0;
        for (j, w) in self.out_weights.iter_mut().enumerate() {
            *w = 10f32.powf(2.5 * s.tap_slope * (OUT_TAPS[j] - 0.625) / 20.0);
            energy += *w * *w;
        }
        let norm = 1.0 / energy.sqrt();
        self.out_weights.iter_mut().for_each(|w| *w *= norm);

        // The decay: each line's loop, all-passes included, falls 60 dB in
        // the reverb time, and in bassRt times it below the split.
        for i in 0..LINES {
            let base = self.base(s, i);
            let loop_length = base * (1.0 + LOOP_AP[..v.loop_allpasses].iter().sum::<f32>());
            let t = &mut self.tank[i];
            t.mid_gain = 10f32.powf(-3.0 * loop_length / (s.reverb_time * sr));
            t.bass_gain = 10f32.powf(-3.0 * loop_length / (s.reverb_time * s.bass_rt * sr));
        }
        let pole = |hz: f32| 1.0 - (-core::f32::consts::TAU * hz.min(0.49 * sr) / sr).exp();
        self.bass_coef = pole(s.bass_xov);
        self.high_coef = pole(s.rt_hi_cut);
        self.rolloff.tune(s.rolloff, sr);
    }

    /// Move each line's wanderer on by a frame.
    fn wander(&mut self, s: &Settings) {
        let step = s.spin / self.sample_rate;
        if step <= 0.0 {
            return;
        }
        let reach = s.wander_ms * 0.001 * self.sample_rate;
        for i in 0..LINES {
            let w = &mut self.tank[i].wander;
            w.phase += step;
            if w.phase >= 1.0 {
                w.phase -= 1.0;
                w.from = w.to;
                let jump = (2.0 * self.random() - 1.0) * reach;
                let w = &mut self.tank[i].wander;
                w.to = (w.from + jump).clamp(0.0, reach);
            }
        }
    }

    /// One frame of the reverb: the tank's left and right outputs.
    fn tank(&mut self, s: &Settings, input: [f32; 2]) -> [f32; 2] {
        let sr = self.sample_rate;
        let v = &A::VOICING;

        // Predelay, then the input all-passes, into the spread line.
        let held = s.predelay_ms * 0.001 * sr;
        let mut fed = [[0.0f32; TAPS]; 2];
        for side in 0..2 {
            self.predelay[side].push(input[side]);
            let mut x = self.predelay[side].linear(held);
            for (k, ap) in self.inputs[side][..v.input_allpasses].iter_mut().enumerate() {
                x = ap.run(x, INPUT_AP_S[side][k] * sr, self.input_g);
            }
            self.spread[side].push(x);
            for (k, f) in fed[side].iter_mut().enumerate() {
                *f = self.spread[side].linear(self.tap_times[k]) * self.tap_weights[k];
            }
        }

        // Read each line at its wandering length, through its loop.
        let mut ends = [0.0f32; LINES];
        let mut out = [0.0f32; 2];
        for i in 0..LINES {
            let base = self.base(s, i);
            let t = &mut self.tank[i];
            let w = t.wander;
            let length = base + w.from + (w.to - w.from) * w.phase;
            let mut y = t.line.hermite(length - 1.0);
            for (k, ap) in t.allpasses[..v.loop_allpasses].iter_mut().enumerate() {
                y = ap.run(y, base * LOOP_AP[k], v.loop_gain);
            }
            t.low += self.bass_coef * (y - t.low);
            let y = t.mid_gain * (y - t.low) + t.bass_gain * t.low;
            t.high += self.high_coef * (y - t.high);
            if t.high.abs() < 1e-25 {
                t.high = 0.0;
            }
            if t.low.abs() < 1e-25 {
                t.low = 0.0;
            }
            ends[i] = t.high;
            let mut heard = self.out_weights[3] * t.high;
            for (j, weight) in self.out_weights[..3].iter().enumerate() {
                heard += weight * t.line.tap((base * OUT_TAPS[j]) as usize);
            }
            out[0] += OUT_SIGNS[0][i] * heard;
            out[1] += OUT_SIGNS[1][i] * heard;
        }

        // The Hadamard mix, and the new sound in, written back.
        let mixed = hadamard(ends);
        for i in 0..LINES {
            let x = mixed[i] + fed[0][i] + RIGHT_SIGNS[i] * fed[1][i];
            self.tank[i].line.push(x);
        }
        [out[0] * TANK_GAIN, out[1] * TANK_GAIN]
    }
}

/// The 8 × 8 Hadamard matrix times `x`, scaled to keep its energy.
fn hadamard(mut x: [f32; LINES]) -> [f32; LINES] {
    let mut h = 1;
    while h < LINES {
        for i in (0..LINES).step_by(2 * h) {
            for j in i..i + h {
                let (a, b) = (x[j], x[j + h]);
                x[j] = a + b;
                x[j + h] = a - b;
            }
        }
        h *= 2;
    }
    let scale = 1.0 / (LINES as f32).sqrt();
    x.map(|v| v * scale)
}

impl<A: Algorithm> Kernel for Lxp<A> {
    fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let params = Self::params();
        let v = &A::VOICING;
        let longest_line = MAX_SIZE_M / SOUND * v.lines.iter().fold(0.0f32, |m, &r| m.max(r)) * sr;
        let tank_line = || TankLine {
            line: Line::new(longest_line + MAX_WANDER_S * sr + 8.0),
            allpasses: [Allpass::new(longest_line * LOOP_AP[0] + 4.0), Allpass::new(longest_line * LOOP_AP[1] + 4.0)],
            low: 0.0,
            high: 0.0,
            mid_gain: 0.0,
            bass_gain: 0.0,
            wander: Wanderer::default(),
        };
        let inputs = |side: usize| core::array::from_fn(|k| Allpass::new(INPUT_AP_S[side][k] * sr + 4.0));
        let spread = 2.0 * MAX_SIZE_M / SOUND * sr + TAPS as f32 * STAGGER_S * sr + 8.0;
        let mut k = Lxp {
            sample_rate: sr,
            values: params.iter().map(|p| p.default).collect(),
            glides: params.iter().map(|p| Smoothed::new(sr, p.smooth_ms)).collect(),
            current: params.iter().map(|p| p.default).collect(),
            clock: 0,
            seed: 0,
            predelay: [Line::new(MAX_PREDELAY_S * sr), Line::new(MAX_PREDELAY_S * sr)],
            inputs: [inputs(0), inputs(1)],
            spread: [Line::new(spread), Line::new(spread)],
            source: [Line::new(MAX_VOICE_S * sr), Line::new(MAX_VOICE_S * sr)],
            echo: [Line::new(MAX_VOICE_S * sr), Line::new(MAX_VOICE_S * sr)],
            echo_ap: core::array::from_fn(|i| Allpass::new(ECHO_AP_S[i] * sr + 4.0)),
            reflection_ap: core::array::from_fn(|i| Allpass::new(REFLECTION_AP_S[i] * sr + 4.0)),
            tank: core::array::from_fn(|_| tank_line()),
            rolloff: Lowpass::default(),
            input_g: 0.0,
            tap_times: [0.0; TAPS],
            tap_weights: [0.0; TAPS],
            out_weights: [0.0; 4],
            bass_coef: 0.0,
            high_coef: 0.0,
            _algorithm: PhantomData,
        };
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        A::descriptor()
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = Self::params().get(index) else { return };
        let v = spec.clamp(value);
        self.values[index] = v;
        self.glides[index].set(v);
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        for ((glide, &v), now) in self.glides.iter_mut().zip(&self.values).zip(self.current.iter_mut()) {
            glide.jump(v);
            *now = v;
        }
        self.clock = 0;
        self.seed = 0x2545_f491;
        let lines = self.predelay.iter_mut().chain(&mut self.spread).chain(&mut self.source).chain(&mut self.echo);
        lines.for_each(Line::clear);
        let allpasses = self.inputs.iter_mut().flatten().chain(&mut self.echo_ap).chain(&mut self.reflection_ap);
        allpasses.for_each(|ap| ap.0.clear());
        for (i, t) in self.tank.iter_mut().enumerate() {
            t.line.clear();
            t.allpasses.iter_mut().for_each(|ap| ap.0.clear());
            t.low = 0.0;
            t.high = 0.0;
            // The wanderers start out of step, an eighth of a cycle apart.
            t.wander = Wanderer { from: 0.0, to: 0.0, phase: i as f32 / LINES as f32 };
        }
        self.rolloff.state = [[0.0; 4]; 2];
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        for (now, glide) in self.current.iter_mut().zip(self.glides.iter_mut()) {
            *now = glide.next();
        }
        let s = Settings::read(A::ROLES, &self.current);
        if self.clock.is_multiple_of(CONTROL) {
            self.control(&s);
        }
        self.clock = self.clock.wrapping_add(1);
        self.wander(&s);
        let sr = self.sample_rate;
        let dry = *frame;

        // Echoes read before their input is written: they feed it back.
        let mut early = [0.0f32; 2];
        let mut input = dry;
        if A::VOICING.echoes {
            for side in 0..2 {
                let d = (s.echo_ms[side] * s.echo_time_master * 0.001 * sr).max(1.0);
                let e = self.echo[side].linear(d - 1.0);
                let e = self.echo_ap[side].run(e, ECHO_AP_S[side] * sr, VOICE_DIFFUSION);
                let sign = if s.echo_invert[side] >= 0.5 { -1.0 } else { 1.0 };
                let feedback = (gain(s.echo_db[side]) * s.echo_feedback_master).min(MOST_FEEDBACK);
                input[side] = dry[side] + sign * feedback * e;
                early[side] += e;
            }
            for (line, &x) in self.echo.iter_mut().zip(&input) {
                line.push(x);
            }
        }

        // Reflections, from either side to either side.
        for (line, &x) in self.source.iter_mut().zip(&input) {
            line.push(x);
        }
        for (i, &(from, to)) in A::VOICING.reflections.iter().enumerate() {
            let d = s.reflection_ms[i] * s.reflection_master * 0.001 * sr;
            let r = self.source[from].linear(d);
            let r = self.reflection_ap[i].run(r, REFLECTION_AP_S[i] * sr, VOICE_DIFFUSION);
            early[to] += gain(s.reflection_db[i]) * r;
        }

        let tail = self.tank(&s, input);
        let (early_gain, reverb_gain) = (gain(s.early_db), gain(s.reverb_db));
        for side in 0..2 {
            let wet = self.rolloff.run(side, early_gain * early[side] + reverb_gain * tail[side]);
            frame[side] = dry[side] * (1.0 - s.mix) + wet * s.mix;
        }
    }

    /// The tail's 60 dB fall at its slowest band, after the predelay, the
    /// spread and the longest early voice.
    fn tail_seconds(&self) -> f32 {
        let s = Settings::read(A::ROLES, &self.values);
        let voices = s.reflection_ms.iter().fold(0.0f32, |m, &d| m.max(d * s.reflection_master)) * 0.001;
        let echoes = if A::VOICING.echoes {
            let longest = s.echo_ms.iter().fold(0.0f32, |m, &d| m.max(d)) * s.echo_time_master * 0.001;
            let loudest = s.echo_db.iter().fold(0.0f32, |m, &d| m.max(gain(d))) * s.echo_feedback_master;
            let g = loudest.min(MOST_FEEDBACK);
            if g > 0.0 { longest * (1.0 + (0.001f32.ln() / g.ln()).max(0.0)) } else { longest }
        } else {
            0.0
        };
        let start = s.predelay_ms * 0.001 + s.spread * 2.0 * s.size / SOUND;
        (start + s.reverb_time * s.bass_rt.max(1.0) + voices.max(echoes)).min(60.0)
    }
}
