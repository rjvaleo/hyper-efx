//! What Hyperprism-PPC 1.2.1's filters and reverbs share: CEffect's Smoothing
//! setting and its 45-sample stepped glide, EFilter's coefficient laws and its
//! biquad section, and EReverb's single-memory engine. The effects themselves
//! are `hp_low_pass` … `hp_hall`; this module holds no descriptor.
//!
//! Arithmetic follows the decompile's precision: coefficients and state are
//! `f32`, the products and sums the code does in double are done in `f64` and
//! rounded to `f32` where the code rounds.

use crate::ParamSpec;

/// The rate Hyperprism's fixed sample counts were written for.
pub(crate) const HP_RATE: f64 = 44_100.0;

/// CEffect's "Smoothing (%)" setting: a whole-number slider, 25 when the
/// effect is made.
pub(crate) const SMOOTHING: ParamSpec =
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 25.0).unit("%").kind(crate::ParamKind::Int).smooth(0.0);

/// `a` for a Smoothing setting: `0.0001·int(10^(0.04·(100 − s)))`, so 25 %
/// is 0.1, 0 % is 1 (no glide) and 100 % is 0.0001.
pub(crate) fn smoothing_coef(percent: f32) -> f64 {
    let s = percent.round() as i32;
    let field = 10f64.powf(0.01 * 4.0 * f64::from(100 - s)) as i32;
    0.0001 * f64::from(field)
}

/// The glide's `a` and `1 − a`, as the routines hold them: floats.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Glide {
    a: f32,
    b: f32,
}

impl Glide {
    pub(crate) fn new(percent: f32) -> Self {
        let a = smoothing_coef(percent);
        Glide { a: a as f32, b: (1.0 - a) as f32 }
    }

    /// One step of `s = a·target + (1 − a)·s`.
    #[inline]
    pub(crate) fn step(&self, target: f32, value: f32) -> f32 {
        self.a * target + self.b * value
    }
}

/// The countdown that paces every glide: read, then decremented, and when it
/// read zero the glide steps and it reloads. It starts at 44, so the first
/// step is the 45th sample and then every 45th. Scaled with the rate, so a
/// glide takes the same time at any rate.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Countdown {
    reload: i32,
    count: i32,
}

impl Countdown {
    pub(crate) fn new(sample_rate: f32) -> Self {
        let every = (45.0 * f64::from(sample_rate) / HP_RATE).round().max(1.0) as i32;
        Countdown { reload: every - 1, count: every - 1 }
    }

    pub(crate) fn reset(&mut self) {
        self.count = self.reload;
    }

    /// Whether this sample's glide steps.
    #[inline]
    pub(crate) fn fires(&mut self) -> bool {
        let fire = self.count < 1;
        self.count -= 1;
        if fire {
            self.count = self.reload;
        }
        fire
    }
}

// ---- EFilter ---------------------------------------------------------------------

/// EFilter's three numbers: the Motorola-style α, β, γ of one section.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Abg {
    pub alpha: f32,
    pub beta: f32,
    pub gamma: f32,
}

impl Abg {
    /// Each one glided a step toward `target`.
    #[inline]
    pub(crate) fn glided(&self, glide: &Glide, target: &Abg) -> Abg {
        Abg {
            alpha: glide.step(target.alpha, self.alpha),
            beta: glide.step(target.beta, self.beta),
            gamma: glide.step(target.gamma, self.gamma),
        }
    }
}

/// The 2π EFilter's low, high and band pass use.
const TWO_PI_EFILTER: f64 = 6.283185308;

/// The low end every EFilter process holds the frequency above, and the top
/// of the pad window as a share of the rate. The first is the code's; the
/// second only acts below 44.1 kHz, where 20 kHz would pass Nyquist.
fn efilter_frequency(f: f32, sample_rate: f32) -> f32 {
    let floor = 0.0003 * f64::from(sample_rate);
    let f = if f64::from(f) < floor { floor as f32 } else { f };
    f.min((20_000.0 * f64::from(sample_rate) / HP_RATE) as f32)
}

/// β and γ, with `w` where the textbook has `sin w` in β.
fn efilter_beta_gamma(f: f32, q: f32, sample_rate: f32) -> (f64, f32, f32) {
    let w = TWO_PI_EFILTER * f64::from(f) / f64::from(sample_rate);
    let two_q = (2.0 * f64::from(q)) as f32;
    let beta = (-(0.5 * w - f64::from(q)) / (f64::from(two_q) + w)) as f32;
    let gamma = ((0.5 + f64::from(beta)) * w.cos()) as f32;
    (w, beta, gamma)
}

/// SetLowpassParameters: α = min((½ − β)·sin w/(1 + cos w), ½(½ + β − γ)).
pub(crate) fn lowpass(f: f32, q: f32, sample_rate: f32) -> Abg {
    let f = efilter_frequency(f, sample_rate);
    let q = if f64::from(q) < 0.001 { 0.001 } else { q };
    let (w, beta, gamma) = efilter_beta_gamma(f, q, sample_rate);
    let b = f64::from(beta);
    let a = (((0.5 - b) * w.sin()) / (1.0 + w.cos())) as f32;
    let half = (0.5 * ((0.5 - f64::from(gamma)) + b)) as f32;
    Abg { alpha: if a < half { a } else { half }, beta, gamma }
}

/// SetHighpassParameters: α = √((½ − β)² + γ²).
pub(crate) fn highpass(f: f32, q: f32, sample_rate: f32) -> Abg {
    let f = efilter_frequency(f, sample_rate);
    let q = if f64::from(q) < 0.001 { 0.001 } else { q };
    let (_, beta, gamma) = efilter_beta_gamma(f, q, sample_rate);
    let b = f64::from(beta);
    let gg = (f64::from(gamma) * f64::from(gamma)) as f32;
    let alpha = ((0.5 - b) * (0.5 - b) + f64::from(gg)).sqrt() as f32;
    Abg { alpha, beta, gamma }
}

/// SetBandpassParameters: Q held at 1 or more, α = ½(½ − β).
pub(crate) fn bandpass(f: f32, q: f32, sample_rate: f32) -> Abg {
    let f = efilter_frequency(f, sample_rate);
    let q = if q < 1.0 { 1.0 } else { q };
    let (_, beta, gamma) = efilter_beta_gamma(f, q, sample_rate);
    Abg { alpha: (0.5 * (0.5 - f64::from(beta))) as f32, beta, gamma }
}

/// One section's five coefficients. The output gain is 2 for every section
/// these processes use, and it multiplies the recursion as well.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Coefs {
    a0: f32,
    a1: f32,
    a2: f32,
    b1: f32,
    b2: f32,
}

impl Coefs {
    /// `(α, ±2α, α; γ, −β)`: `sign` +1 for the low-pass numerator, −1 for
    /// the high-pass one.
    pub(crate) fn of(abg: &Abg, sign: f64) -> Coefs {
        Coefs {
            a0: abg.alpha,
            a1: (sign * 2.0 * f64::from(abg.alpha)) as f32,
            a2: abg.alpha,
            b1: abg.gamma,
            b2: -abg.beta,
        }
    }
}

pub(crate) const SECTION_GAIN: f32 = 2.0;

/// One section's memory: `y = 2·(a0·x + a1·x1 + a2·x2 + b1·y1 + b2·y2)`,
/// summed in the code's order.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Section {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Section {
    #[inline]
    pub(crate) fn run(&mut self, c: &Coefs, x: f32) -> f32 {
        let t = (f64::from(c.a1) * f64::from(self.x1)) as f32;
        let t = (f64::from(c.a0) * f64::from(x) + f64::from(t)) as f32;
        let t = (f64::from(c.a2) * f64::from(self.x2) + f64::from(t)) as f32;
        let t = (f64::from(c.b1) * f64::from(self.y1) + f64::from(t)) as f32;
        let t = (f64::from(c.b2) * f64::from(self.y2) + f64::from(t)) as f32;
        let y = SECTION_GAIN * t;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Which EFilter routine runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EFilterProcess {
    /// FilterSingle, process 0: one section, low-pass numerator.
    Low,
    /// FilterSingle, process 1: one section, high-pass numerator.
    High,
    /// FilterDual, process 2: both numerators on the same input, summed, × ½.
    Band,
}

pub(crate) const FREQ: usize = 0;
pub(crate) const Q: usize = 1;
pub(crate) const SMOOTH: usize = 2;

/// Low Pass, High Pass and Band Pass: X frequency, Y Q, and Smoothing.
pub(crate) struct EFilterCore {
    process: EFilterProcess,
    params: &'static [ParamSpec],
    sample_rate: f32,
    values: [f32; 3],
    target: Abg,
    smoothed: Abg,
    first: Coefs,
    second: Coefs,
    /// Per side: the section, and FilterDual's second section.
    sides: [[Section; 2]; 2],
    glide: Glide,
    countdown: Countdown,
}

impl EFilterCore {
    pub(crate) fn new(process: EFilterProcess, params: &'static [ParamSpec], sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 3];
        for (v, spec) in values.iter_mut().zip(params) {
            *v = spec.default;
        }
        let mut k = EFilterCore {
            process,
            params,
            sample_rate,
            values,
            target: Abg::default(),
            smoothed: Abg::default(),
            first: Coefs::default(),
            second: Coefs::default(),
            sides: [[Section::default(); 2]; 2],
            glide: Glide::new(values[SMOOTH]),
            countdown: Countdown::new(sample_rate),
        };
        k.retarget();
        k.reset();
        k
    }

    fn retarget(&mut self) {
        let (f, q, sr) = (self.values[FREQ], self.values[Q], self.sample_rate);
        self.target = match self.process {
            EFilterProcess::Low => lowpass(f, q, sr),
            EFilterProcess::High => highpass(f, q, sr),
            EFilterProcess::Band => bandpass(f, q, sr),
        };
    }

    fn load(&mut self) {
        let s = self.smoothed;
        let (first, second) = match self.process {
            EFilterProcess::Low => (Coefs::of(&s, 1.0), Coefs::default()),
            EFilterProcess::High => (Coefs::of(&s, -1.0), Coefs::default()),
            EFilterProcess::Band => (Coefs::of(&s, 1.0), Coefs::of(&s, -1.0)),
        };
        self.first = first;
        self.second = second;
    }

    pub(crate) fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = self.params.get(index) else { return };
        self.values[index] = spec.clamp(value);
        if index == SMOOTH {
            self.glide = Glide::new(self.values[SMOOTH]);
        } else {
            self.retarget();
        }
    }

    pub(crate) fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    /// The process's (re)initialisation: glides on their targets, state zero.
    pub(crate) fn reset(&mut self) {
        self.smoothed = self.target;
        self.load();
        self.sides = [[Section::default(); 2]; 2];
        self.countdown.reset();
    }

    #[inline]
    pub(crate) fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.countdown.fires() {
            self.smoothed = self.smoothed.glided(&self.glide, &self.target);
            self.load();
        }
        for (x, side) in frame.iter_mut().zip(self.sides.iter_mut()) {
            *x = match self.process {
                EFilterProcess::Low | EFilterProcess::High => side[0].run(&self.first, *x),
                EFilterProcess::Band => {
                    let s1 = side[0].run(&self.first, *x);
                    let s2 = side[1].run(&self.second, *x);
                    ((f64::from(s1) + f64::from(s2)) as f32) * 0.5
                }
            };
        }
    }
}

// ---- EReverb ---------------------------------------------------------------------

/// Which EReverb routine runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Room {
    /// LargeHallMono.
    Hall,
    /// MediumRoomMono.
    Medium,
}

/// LargeHallMono's 25 pointers, as ages behind the write point (tap 16).
/// Taps 20 and 24 are advanced but never read.
const HALL_TAPS: [u32; 25] = [
    3, 334, 335, 846, 1768, 5623, 1769, 4501, 7121, 12413, 7122, 10455, 10456, 11779, 1, 12420, 0, 851, 7001, 12415,
    10379, 1002, 657, 12421, 10409,
];

/// MediumRoomMono's 25 pointers. All are read.
const MEDIUM_TAPS: [u32; 25] = [
    3, 1541, 4, 369, 370, 1345, 1742, 3066, 6645, 6646, 6647, 8356, 6648, 7078, 1, 13106, 0, 1600, 6007, 8446, 9318,
    1599, 6000, 8453, 9345,
];

/// An in-place allpass: its gain as a share of Diffusion, and the taps of its
/// head and tail.
type Allpass = (f64, usize, usize);

/// LargeHallMono's chain, in the code's order. 3 holds 4; 5 holds 6 and 7.
const HALL_ALLPASSES: [Allpass; 7] =
    [(0.3, 0, 1), (0.3, 2, 3), (0.5, 4, 5), (0.25, 6, 7), (0.5, 8, 9), (0.25, 10, 11), (0.25, 12, 13)];

/// MediumRoomMono's chain, in the code's order: four in the first half (1
/// holds 2 and 3), then the mid injection, then two in the second (5 holds 6).
const MEDIUM_ALLPASSES: [Allpass; 6] = [(0.3, 0, 1), (0.7, 2, 3), (0.5, 4, 5), (0.5, 6, 7), (0.3, 10, 11), (0.6, 12, 13)];

/// Allpass gains are held below this. See the Medium Room's doc comment.
pub(crate) const ALLPASS_LIMIT: f32 = 0.99;

pub(crate) const MIX: usize = 0;
pub(crate) const FEEDBACK: usize = 1;
pub(crate) const DIFFUSION: usize = 2;
pub(crate) const BRIGHTNESS: usize = 3;
pub(crate) const REVERB_SMOOTH: usize = 4;

/// Mix and Feedback on the pad, then the window's settings.
pub(crate) static REVERB_PARAMS: &[ParamSpec] = &[
    ParamSpec::new("mix", "Mix", 0.0, 100.0, 50.0).unit("%").smooth(9.7),
    ParamSpec::new("feedback", "Feedback", 0.0, 100.0, 20.0).unit("%").smooth(9.7),
    ParamSpec::new("diffusion", "Diffusion", 0.0, 150.0, 85.0).unit("%").kind(crate::ParamKind::Int).smooth(0.0),
    ParamSpec::new("brightness", "Brightness", 0.0, 100.0, 50.0).unit("%").kind(crate::ParamKind::Int).smooth(0.0),
    SMOOTHING.glides("mix"),
];

/// Sample ages at another rate: each scaled and rounded, except that ages
/// one or two apart in the original — the joins between stages — stay that
/// far apart. At 44.1 kHz every age is the original's.
fn scale_ages(ages: &[u32; 25], sample_rate: f32) -> [usize; 25] {
    let ratio = f64::from(sample_rate) / HP_RATE;
    let mut sorted = *ages;
    sorted.sort_unstable();
    let mut scaled_sorted = [0usize; 25];
    for i in 0..25 {
        scaled_sorted[i] = if i == 0 {
            (f64::from(sorted[0]) * ratio).round() as usize
        } else {
            let gap = sorted[i] - sorted[i - 1];
            let prev = scaled_sorted[i - 1];
            if gap <= 2 {
                prev + gap as usize
            } else {
                ((f64::from(sorted[i]) * ratio).round() as usize).max(prev + 1)
            }
        };
    }
    let mut out = [0usize; 25];
    for (o, age) in out.iter_mut().zip(ages) {
        let at = sorted.iter().position(|a| a == age).unwrap_or(0);
        *o = scaled_sorted[at];
    }
    out
}

/// Hall and Medium Room: one circular memory, every pointer a fixed age
/// behind the write point.
pub(crate) struct ReverbCore {
    room: Room,
    sample_rate: f32,
    values: [f32; 5],
    taps: [usize; 25],
    buf: Vec<f32>,
    mask: usize,
    write: usize,
    /// The targets, as SetParameter stores them: mix and feedback × 0.01.
    mix: f32,
    feedback: f32,
    mix_s: f32,
    feedback_s: f32,
    /// Diffusion × 0.01, and the one-pole's c and 1 − c.
    diffusion: f32,
    c: f32,
    one_minus_c: f32,
    /// The input one-pole and the loop one-pole.
    lpi: f32,
    lpf: f32,
    glide: Glide,
    countdown: Countdown,
}

impl ReverbCore {
    pub(crate) fn new(room: Room, sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let taps = scale_ages(if room == Room::Hall { &HALL_TAPS } else { &MEDIUM_TAPS }, sample_rate);
        let size = (taps.iter().max().copied().unwrap_or(0) + 2).next_power_of_two();
        let mut values = [0.0; 5];
        for (v, spec) in values.iter_mut().zip(REVERB_PARAMS) {
            *v = spec.default;
        }
        let mut k = ReverbCore {
            room,
            sample_rate,
            values,
            taps,
            buf: vec![0.0; size],
            mask: size - 1,
            write: 0,
            mix: 0.0,
            feedback: 0.0,
            mix_s: 0.0,
            feedback_s: 0.0,
            diffusion: 0.0,
            c: 0.0,
            one_minus_c: 0.0,
            lpi: 0.0,
            lpf: 0.0,
            glide: Glide::new(values[REVERB_SMOOTH]),
            countdown: Countdown::new(sample_rate),
        };
        for i in 0..REVERB_PARAMS.len() {
            k.apply(i);
        }
        k.reset();
        k
    }

    /// Store one value where the code keeps it.
    fn apply(&mut self, index: usize) {
        let v = f64::from(self.values[index]);
        match index {
            MIX => self.mix = (0.01 * v) as f32,
            FEEDBACK => self.feedback = (0.01 * v) as f32,
            DIFFUSION => self.diffusion = (0.01 * v) as f32,
            BRIGHTNESS => {
                // At 44.1 kHz c and 1 − c are the code's; elsewhere the pole
                // keeps its frequency: (1 − c)^(44100/sr).
                let c = (0.01 * v) as f32;
                let one_minus_c = (1.0 - f64::from(c)) as f32;
                if (f64::from(self.sample_rate) - HP_RATE).abs() < 1e-6 {
                    self.c = c;
                    self.one_minus_c = one_minus_c;
                } else {
                    self.one_minus_c = f64::from(one_minus_c).powf(HP_RATE / f64::from(self.sample_rate)) as f32;
                    self.c = (1.0 - f64::from(self.one_minus_c)) as f32;
                }
            }
            _ => self.glide = Glide::new(self.values[REVERB_SMOOTH]),
        }
    }

    pub(crate) fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = REVERB_PARAMS.get(index) else { return };
        let v = spec.clamp(value);
        let changed = v != self.values[index];
        self.values[index] = v;
        self.apply(index);
        // ApplyAdditionalSettings clears both one-poles when the window's
        // Diffusion or Brightness is applied.
        if changed && (index == DIFFUSION || index == BRIGHTNESS) {
            self.lpi = 0.0;
            self.lpf = 0.0;
        }
    }

    pub(crate) fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    /// The process's (re)initialisation: the memory and one-poles zeroed,
    /// mix and feedback on their targets.
    pub(crate) fn reset(&mut self) {
        self.buf.iter_mut().for_each(|s| *s = 0.0);
        self.write = 0;
        self.lpi = 0.0;
        self.lpf = 0.0;
        self.mix_s = self.mix;
        self.feedback_s = self.feedback;
        self.countdown.reset();
    }

    #[inline]
    fn at(&self, tap: usize) -> usize {
        (self.write + self.mask + 1 - self.taps[tap]) & self.mask
    }

    #[inline]
    fn get(&self, tap: usize) -> f32 {
        self.buf[self.at(tap)]
    }

    #[inline]
    fn put(&mut self, tap: usize, value: f32) {
        let i = self.at(tap);
        self.buf[i] = value;
    }

    /// `t ← t − g·h; h ← h + g·t`, in place.
    #[inline]
    fn allpass(&mut self, &(share, head, tail): &Allpass) {
        let g = ((share * f64::from(self.diffusion)) as f32).min(ALLPASS_LIMIT);
        let (h, t) = (self.at(head), self.at(tail));
        self.buf[t] += -self.buf[h] * g;
        self.buf[h] += self.buf[t] * g;
    }

    #[inline]
    pub(crate) fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.countdown.fires() {
            self.mix_s = self.glide.step(self.mix, self.mix_s);
            self.feedback_s = self.glide.step(self.feedback, self.feedback_s);
        }
        let [dry_l, dry_r] = *frame;
        let input = (0.5 * f64::from(dry_l + dry_r)) as f32;
        self.lpi = self.c * input + self.one_minus_c * self.lpi;
        self.put(16, self.lpi);
        if self.room == Room::Medium {
            self.put(9, self.lpi);
        }
        let back = self.get(15);
        self.lpf = (f64::from(self.feedback_s) * f64::from(self.c * back) + f64::from(self.one_minus_c * self.lpf)) as f32;
        let i = self.at(14);
        self.buf[i] += self.lpf;

        let mix = f64::from(self.mix_s);
        let dry = 1.0 - mix;
        let t = |k: &Self, tap: usize| f64::from(k.get(tap));
        let (wet_l, wet_r) = match self.room {
            Room::Hall => {
                for ap in &HALL_ALLPASSES {
                    self.allpass(ap);
                }
                (
                    0.1 * t(self, 19) + (0.6 * t(self, 17) - 0.2 * t(self, 18)),
                    -(0.1 * t(self, 23) - (-0.6 * t(self, 21) + 0.2 * t(self, 22))),
                )
            }
            Room::Medium => {
                for ap in &MEDIUM_ALLPASSES[..4] {
                    self.allpass(ap);
                }
                // The halves join through the unsmoothed Feedback.
                let i = self.at(9);
                self.buf[i] += self.get(8) * self.feedback;
                for ap in &MEDIUM_ALLPASSES[4..] {
                    self.allpass(ap);
                }
                (
                    0.4 * t(self, 20) + -0.5 * t(self, 19) + -0.5 * t(self, 17) + 0.5 * t(self, 18),
                    0.4 * t(self, 24) + 0.5 * t(self, 23) + 0.5 * t(self, 21) + 0.5 * t(self, 22),
                )
            }
        };
        frame[0] = (dry * f64::from(dry_l) + mix * wet_l) as f32;
        frame[1] = (dry * f64::from(dry_r) + mix * wet_r) as f32;
        self.write = (self.write + 1) & self.mask;
    }

    /// The loop's 60 dB decay, after the last tap: the Hall's loop gain is
    /// Feedback, the Medium Room's Feedback squared.
    pub(crate) fn tail_seconds(&self) -> f32 {
        let (loop_len, longest) = match self.room {
            Room::Hall => (12_419.0, 12_421.0),
            Room::Medium => (13_104.0, 13_106.0),
        };
        let fb = f64::from(self.feedback);
        let gain = if self.room == Room::Hall { fb } else { fb * fb };
        let tail = if gain <= 1e-3 {
            0.0
        } else if gain >= 0.999 {
            60.0 * HP_RATE
        } else {
            loop_len * (0.001f64.ln() / gain.ln())
        };
        ((longest + tail) / HP_RATE).min(60.0) as f32
    }
}
