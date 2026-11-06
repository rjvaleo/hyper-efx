//! What Hyperprism-PPC 1.2.1's modulation, phaser and pitch processes share:
//! the smoothing slider's law and its stepped countdown, the 1024-entry LFO
//! tables, and the two routines that several processes run (Modulation for
//! Flanger and Vibrato, RingMod for Ring Modulator and Tremolo).
//!
//! Hyperprism's numbers are for 44.1 kHz. Frequencies go through the real
//! sample rate, as the original's did through its rate global; counts of
//! samples (delays, taps, the smoothing interval) are scaled by `sr / 44100`,
//! so at 44.1 kHz every number is the original's.

/// The rate Hyperprism's sample counts are written for.
pub(crate) const HP_RATE: f64 = 44_100.0;

/// Entries in the LFO tables.
pub(crate) const LFO: usize = 1024;

/// CEffect's 2π, a hair above the real one.
const TWO_PI: f64 = 6.283_185_349;

/// A count of samples at 44.1 kHz, at this rate.
pub(crate) fn samples(count: f64, sample_rate: f32) -> f64 {
    count * sample_rate as f64 / HP_RATE
}

/// The Smoothing (%) slider's coefficient: `int(10^(0.04·(100 − s)))·0.0001`,
/// so 0 % is 1 (each step lands), 50 % is 0.01 and 100 % is 0.0001.
pub(crate) fn smoothing_coefficient(percent: f32) -> f64 {
    let s = percent.clamp(0.0, 100.0) as f64;
    0.0001 * 10f64.powf(0.01 * 4.0 * (100.0 - s)).trunc()
}

/// The stepped smoothing: a countdown that fires when it has run below one
/// and then reloads, so the steps come every `reload + 1` samples; at each,
/// every smoothed value takes `v ← a·target + b·v`. It counts samples itself,
/// so the host's block size does not move the steps.
#[derive(Clone, Copy)]
pub(crate) struct Stepper {
    pub a: f32,
    pub b: f32,
    reload: i32,
    start: i32,
    count: i32,
}

impl Stepper {
    /// `reload` and `start` are the original's counts at 44.1 kHz.
    pub fn new(sample_rate: f32, reload: f64, start: f64, percent: f32) -> Self {
        let scale = |n: f64| Self::scaled(n, sample_rate);
        let mut s = Stepper { a: 1.0, b: 0.0, reload: scale(reload), start: scale(start), count: 0 };
        s.set_percent(percent);
        s.reset();
        s
    }

    fn scaled(n: f64, sample_rate: f32) -> i32 {
        samples(n, sample_rate).round().clamp(0.0, i32::MAX as f64) as i32
    }

    /// Steps every `every` samples at 44.1 kHz: the reload and the start are
    /// both `every − 1`, the original's 44 for its 45. A countdown already
    /// past the new reload is brought back to it.
    pub fn set_every(&mut self, sample_rate: f32, every: f32) {
        let n = Self::scaled(every.max(1.0) as f64 - 1.0, sample_rate);
        self.reload = n;
        self.start = n;
        self.count = self.count.min(n);
    }

    pub fn set_percent(&mut self, percent: f32) {
        let a = smoothing_coefficient(percent);
        self.a = a as f32;
        self.b = (1.0 - a) as f32;
    }

    pub fn reset(&mut self) {
        self.count = self.start;
    }

    /// One sample: true when the values step.
    #[inline]
    pub fn due(&mut self) -> bool {
        self.due_unless(false)
    }

    /// As `due`, but a held stepper keeps counting down without stepping or
    /// reloading, and steps on the first sample after it is let go.
    #[inline]
    pub fn due_unless(&mut self, held: bool) -> bool {
        let due = self.count < 1;
        self.count = self.count.saturating_sub(1);
        if due && !held {
            self.count = self.reload;
            true
        } else {
            false
        }
    }

    #[inline]
    pub fn step(&self, value: f32, target: f32) -> f32 {
        self.a * target + self.b * value
    }
}

/// `SinBuffer`: one cycle of `sin(2π·i/n)`.
pub(crate) fn sin_table() -> [f32; LFO] {
    let mut t = [0.0; LFO];
    let step = TWO_PI / LFO as f64;
    for (i, v) in t.iter_mut().enumerate() {
        *v = (i as f64 * step).sin() as f32;
    }
    t
}

/// `OneMCosBuffer`: one cycle of `0.5·(1 − cos(2π·i/n))`, from 0 up to 1 and back.
pub(crate) fn one_m_cos_table() -> [f32; LFO] {
    let mut t = [0.0; LFO];
    let step = TWO_PI / LFO as f64;
    for (i, v) in t.iter_mut().enumerate() {
        *v = (0.5 * (1.0 - (i as f64 * step).cos())) as f32;
    }
    t
}

// ---- added LFO shapes and stereo offsets (not in the original) -------------------

/// One cycle of a triangle in the sine's phase: 0, up to 1 at a quarter, down
/// to −1 at three quarters, back to 0.
pub(crate) fn triangle_table() -> [f32; LFO] {
    core::array::from_fn(|i| {
        let x = i as f32 / LFO as f32;
        if x < 0.25 {
            4.0 * x
        } else if x < 0.75 {
            2.0 - 4.0 * x
        } else {
            4.0 * x - 4.0
        }
    })
}

/// One cycle of a square in the sine's phase: 1 for the first half, −1 for
/// the second.
pub(crate) fn square_table() -> [f32; LFO] {
    core::array::from_fn(|i| if i < LFO / 2 { 1.0 } else { -1.0 })
}

/// A sine-phased table made unipolar the way `OneMCosBuffer` is the sine's:
/// `0.5·(1 + t(i − n/4))`, from 0 at the start up to 1 halfway.
fn raised(t: [f32; LFO]) -> [f32; LFO] {
    core::array::from_fn(|i| 0.5 * (1.0 + t[(i + LFO - LFO / 4) % LFO]))
}

/// The LFO shape control's table: 0 the original's own (`OneMCosBuffer` when
/// `unipolar`, else `SinBuffer`), 1 a triangle, 2 a square, each in the
/// original's phase and range.
pub(crate) fn shape_table(shape: f32, unipolar: bool) -> [f32; LFO] {
    match (shape.round() as i32, unipolar) {
        (1, true) => raised(triangle_table()),
        (1, false) => triangle_table(),
        (2, true) => raised(square_table()),
        (2, false) => square_table(),
        (_, true) => one_m_cos_table(),
        (_, false) => sin_table(),
    }
}

/// A phase in degrees as table steps, 360° to the whole table.
#[inline]
pub(crate) fn degrees(deg: f32) -> f32 {
    deg / 360.0 * LFO as f32
}

/// A table read at a phase in table units, linearly between neighbours, the
/// last entry leading back to the first.
#[inline]
pub(crate) fn lookup(t: &[f32; LFO], phase: f32) -> f32 {
    let i = phase as usize;
    let f = phase - i as f32;
    if i < LFO - 1 {
        t[i] + f * (t[i + 1] - t[i])
    } else {
        t[LFO - 1] + f * (t[0] - t[LFO - 1])
    }
}

/// A phase moved on and brought back into 0–1024.
#[inline]
pub(crate) fn advance(phase: f32, by: f32) -> f32 {
    let mut p = phase + by;
    if p >= LFO as f32 {
        p -= LFO as f32;
    }
    if !(0.0..LFO as f32).contains(&p) {
        p = if p.is_finite() { p.rem_euclid(LFO as f32) } else { 0.0 };
        if p >= LFO as f32 {
            p = 0.0;
        }
    }
    p
}

/// The pad's frequency as a table step per sample: `v·1024/SR`.
#[inline]
pub(crate) fn increment(hz: f32, sample_rate: f32) -> f32 {
    hz * LFO as f32 / sample_rate
}

// ---- the delay line Modulation and Chorus read --------------------------------

/// EModulation's buffer 0: stereo frames, 32 768 of them at 44.1 kHz.
pub(crate) struct Line {
    frames: Vec<[f32; 2]>,
    write: usize,
}

impl Line {
    pub fn new(sample_rate: f32) -> Self {
        let n = samples(32_768.0, sample_rate).round().max(256.0) as usize;
        Line { frames: vec![[0.0; 2]; n], write: 0 }
    }

    pub fn clear(&mut self) {
        self.frames.fill([0.0; 2]);
        self.write = 0;
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn write(&mut self, side: usize, value: f32) {
        self.frames[self.write][side] = value;
    }

    /// The frame `back` frames before the write head, on one side.
    pub fn back(&self, side: usize, back: usize) -> f32 {
        let n = self.frames.len();
        self.frames[(self.write + n - back % n) % n][side]
    }

    /// The modulated read: `d` frames back, between the frame `int(d)` back
    /// and the one before it. Delay 0 is the frame just written.
    pub fn read(&self, side: usize, d: f64) -> f32 {
        let n = self.frames.len();
        let d = if d.is_finite() { d.clamp(0.0, (n - 2) as f64) } else { 0.0 };
        let k = d as usize;
        let f = (d as f32) - k as f32;
        let p = (self.write + n - k) % n;
        let older = if p == 0 { n - 1 } else { p - 1 };
        let here = self.frames[p][side];
        here + f * (self.frames[older][side] - here)
    }

    pub fn step(&mut self) {
        self.write += 1;
        if self.write == self.frames.len() {
            self.write = 0;
        }
    }
}

// ---- the Modulation routine (Flanger, Vibrato) ----------------------------------

/// The Modulation routine: a delay swept from `centre` to `centre + depth`
/// frames on a raised cosine, the line fed back from the previous sample's
/// wet sound, and `wet·P2 + dry·P3` out. Both sides run the same LFO; the
/// added `offset` (table steps, 0 in the original) puts the right side's
/// ahead of the left's.
pub(crate) struct Modulation {
    pub line: Line,
    table: [f32; LFO],
    pub wet: f32,
    pub dry: f32,
    pub feedback: f32,
    /// The smoothed rate (table steps per sample) and depth (frames).
    pub rate: f32,
    pub depth: f32,
    /// The right LFO's lead over the left, in table steps.
    pub offset: f32,
    phase: f32,
    last_wet: [f32; 2],
}

impl Modulation {
    pub fn new(sample_rate: f32, wet: f32, dry: f32, feedback: f32) -> Self {
        Modulation {
            line: Line::new(sample_rate),
            table: one_m_cos_table(),
            wet,
            dry,
            feedback,
            rate: 0.0,
            depth: 0.0,
            offset: 0.0,
            phase: 0.0,
            last_wet: [0.0; 2],
        }
    }

    pub fn set_table(&mut self, table: [f32; LFO]) {
        self.table = table;
    }

    pub fn clear(&mut self) {
        self.line.clear();
        self.phase = 0.0;
        self.last_wet = [0.0; 2];
    }

    #[inline]
    pub fn tick(&mut self, frame: &mut [f32; 2]) {
        let x = *frame;
        for (c, &x) in x.iter().enumerate() {
            self.line.write(c, x + self.feedback * self.last_wet[c]);
        }
        // At no offset the right phase is the left's to the bit.
        let phases = [self.phase, advance(self.phase, self.offset)];
        for (c, &x) in x.iter().enumerate() {
            let lfo = lookup(&self.table, phases[c]);
            let d = lfo as f64 * self.depth as f64;
            let wet = self.line.read(c, d);
            frame[c] = wet * self.wet + x * self.dry;
            self.last_wet[c] = wet;
        }
        self.phase = advance(self.phase, self.rate);
        self.line.step();
    }
}

// ---- the RingMod routine (Ring Modulator, Tremolo) -------------------------------

/// The RingMod routine: `y = x·(1 − d) + lfo·(x·d)` on both sides, one phase.
/// The added `offset` (table steps, 0 in the original) reads the right side's
/// LFO that far ahead.
pub(crate) struct RingMod {
    table: [f32; LFO],
    pub rate: f32,
    pub depth: f32,
    pub offset: f32,
    phase: f32,
}

impl RingMod {
    pub fn new(table: [f32; LFO]) -> Self {
        RingMod { table, rate: 0.0, depth: 0.0, offset: 0.0, phase: 0.0 }
    }

    pub fn set_table(&mut self, table: [f32; LFO]) {
        self.table = table;
    }

    pub fn clear(&mut self) {
        self.phase = 0.0;
    }

    #[inline]
    pub fn tick(&mut self, frame: &mut [f32; 2]) {
        let lfo = lookup(&self.table, self.phase);
        let right = if self.offset == 0.0 { lfo } else { lookup(&self.table, advance(self.phase, self.offset)) };
        let lfos = [lfo, right];
        let (d, dry) = (self.depth, 1.0 - self.depth);
        for (x, lfo) in frame.iter_mut().zip(lfos) {
            *x = *x * dry + lfo * (*x * d);
        }
        self.phase = advance(self.phase, self.rate);
    }
}

/// What the effects' own tests share: a fixed stereo input, a render through
/// a kernel, and a checksum of every output bit.
#[cfg(test)]
pub(crate) mod check {
    use crate::Kernel;

    /// `seconds` of a 220 Hz sine on the left, 331 Hz on the right, each with
    /// noise from a fixed seed, at `sample_rate`.
    pub fn input(seconds: f32, sample_rate: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (seconds * sample_rate) as usize;
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut noise = move || {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            ((seed >> 40) as f32 / (1u64 << 24) as f32) - 0.5
        };
        let mut left = Vec::with_capacity(n);
        let mut right = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f64 / sample_rate as f64;
            left.push(0.5 * (std::f64::consts::TAU * 220.0 * t).sin() as f32 + 0.2 * noise());
            right.push(0.5 * (std::f64::consts::TAU * 331.0 * t).sin() as f32 + 0.2 * noise());
        }
        (left, right)
    }

    /// The input through `k`, with `later` set (by index) halfway through.
    pub fn render<K: Kernel>(
        k: &mut K,
        input: &(Vec<f32>, Vec<f32>),
        later: &[(usize, f32)],
    ) -> (Vec<f32>, Vec<f32>) {
        let (mut left, mut right) = input.clone();
        let half = left.len() / 2;
        for (i, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            if i == half {
                for &(index, value) in later {
                    k.set_param(index, value);
                }
            }
            let mut frame = [*l, *r];
            k.tick(&mut frame);
            (*l, *r) = (frame[0], frame[1]);
        }
        (left, right)
    }

    /// FNV-1a over the bits of every sample, left then right.
    pub fn checksum(out: &(Vec<f32>, Vec<f32>)) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for v in out.0.iter().chain(out.1.iter()) {
            for b in v.to_bits().to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        h
    }

    /// The checksums of three renders of `seconds` at `sample_rate`: at the
    /// defaults; with `pad` set and the kernel reset; and with `moved` set
    /// halfway through a render at the defaults.
    pub fn checksums<K: Kernel>(
        sample_rate: f32,
        seconds: f32,
        pad: &[(usize, f32)],
        moved: &[(usize, f32)],
    ) -> [u64; 3] {
        let x = input(seconds, sample_rate);
        let defaults = checksum(&render(&mut K::new(sample_rate), &x, &[]));
        let mut k = K::new(sample_rate);
        for &(i, v) in pad {
            k.set_param(i, v);
        }
        k.reset();
        let set = checksum(&render(&mut k, &x, &[]));
        let moving = checksum(&render(&mut K::new(sample_rate), &x, moved));
        [defaults, set, moving]
    }

    /// Mean square of a run of samples.
    pub fn power(x: &[f32]) -> f64 {
        x.iter().map(|&v| v as f64 * v as f64).sum::<f64>() / x.len().max(1) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slider_law_lands_on_its_round_numbers() {
        assert_eq!(smoothing_coefficient(0.0), 1.0);
        assert_eq!(smoothing_coefficient(25.0), 0.1);
        assert!((smoothing_coefficient(50.0) - 0.01).abs() < 1e-15);
        assert!((smoothing_coefficient(100.0) - 0.0001).abs() < 1e-15);
    }

    #[test]
    fn the_countdown_fires_every_reload_plus_one() {
        let mut s = Stepper::new(44_100.0, 44.0, 44.0, 50.0);
        let fired: Vec<usize> = (0..200).filter(|_| s.due()).collect();
        assert_eq!(fired, vec![44, 89, 134, 179]);
    }

    #[test]
    fn the_added_shapes_keep_the_originals_phase_and_range() {
        assert_eq!(shape_table(0.0, true), one_m_cos_table());
        assert_eq!(shape_table(0.0, false), sin_table());
        for unipolar in [true, false] {
            let original = shape_table(0.0, unipolar);
            for shape in [1.0, 2.0] {
                let t = shape_table(shape, unipolar);
                let (lo, hi) = t.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
                assert_eq!((lo, hi), if unipolar { (0.0, 1.0) } else { (-1.0, 1.0) });
                // Each peaks where the original does; all but the bipolar
                // square start where it starts.
                let peak = if unipolar { LFO / 2 } else { LFO / 4 };
                assert_eq!(t[peak], 1.0);
                if unipolar || shape == 1.0 {
                    assert_eq!(t[0], original[0]);
                }
            }
        }
        let mut s = Stepper::new(44_100.0, 44.0, 44.0, 50.0);
        s.set_every(44_100.0, 10.0);
        let fired: Vec<usize> = (0..40).filter(|_| s.due()).collect();
        assert_eq!(fired, vec![9, 19, 29, 39]);
    }

    #[test]
    fn the_table_wraps_from_its_last_entry_to_its_first() {
        let t = one_m_cos_table();
        assert_eq!(lookup(&t, 1023.5), 0.5 * (t[1023] + t[0]));
        assert_eq!(advance(1023.9, 0.2), 1023.9 + 0.2 - 1024.0);
    }
}
