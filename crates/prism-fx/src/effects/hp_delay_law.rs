//! What Hyperprism-PPC 1.2.1's two delay classes (EDelay and EDelay2) share:
//! how a time in ms becomes samples, the "Param. Speed Ch." and "Smoothing"
//! laws, the rule for other sample rates, and the line itself.
//!
//! The original ran at 44.1 kHz (or the 22 and 11 kHz rates below it) and
//! counted everything in samples. Here every number is the original's at
//! 44.1 kHz; at another rate a count of samples is scaled by rate / 44 100 and
//! a per-step pole is raised to 44 100 / rate, so each takes the same time.

/// The rate the original's numbers are for.
pub(crate) const ORIGINAL_RATE: f64 = 44_100.0;

/// A time in ms as the original's SetParameter makes it samples:
/// `0.001 × (float)(ms × rate)`, kept as a float.
pub(crate) fn samples(ms: f32, sample_rate: f32) -> f32 {
    (0.001 * ((ms as f64 * sample_rate as f64) as f32) as f64) as f32
}

/// A count of samples at 44.1 kHz, at this rate.
pub(crate) fn count(n: i32, sample_rate: f32) -> i32 {
    if sample_rate as f64 == ORIGINAL_RATE {
        n
    } else {
        ((n as f64 * sample_rate as f64 / ORIGINAL_RATE).round() as i32).max(1)
    }
}

/// A one-pole's `(a, b)` per step at 44.1 kHz, at this rate.
fn at_rate(a: f64, b: f64, sample_rate: f32) -> (f64, f64) {
    if sample_rate as f64 == ORIGINAL_RATE {
        (a, b)
    } else {
        let b = b.powf(ORIGINAL_RATE / sample_rate as f64);
        (1.0 - b, b)
    }
}

/// "Param. Speed Ch." (EDelay: EchoTranz, Multi Delay), 1–100: the smoother's
/// `(a, b)` per sample. `N = int(10^(4v/100))`, `a = 1e-8 × N`, `b = 1 − a`,
/// except that 100 gives `N = 0`, which means `a = 1`: no glide at all.
pub(crate) fn speed(v: f32, sample_rate: f32) -> (f64, f64) {
    let v = v as i32;
    let n = if v == 100 { 0 } else { 10f64.powf(4.0 * v as f64 / 100.0) as i32 };
    if n == 0 {
        (1.0, 0.0)
    } else {
        let a = 1e-8 * n as f64;
        at_rate(a, 1.0 - a, sample_rate)
    }
}

/// CEffect's "Smoothing (%)", 0–100 (Comb Filter, Doppler): `(a, b)` per
/// update, as the floats the routines use. `N = int(10^(0.04 × (100 − s)))`,
/// `a = 1e-4 × N`: 1 at no smoothing, 1e-4 at full.
pub(crate) fn smoothing(s: f32, sample_rate: f32) -> (f32, f32) {
    let n = 10f64.powf(0.01 * 4.0 * (100 - s as i32) as f64) as i32;
    let a = 0.0001 * n as f64;
    let (a, b) = at_rate(a, 1.0 - a, sample_rate);
    (a as f32, b as f32)
}

/// A stereo line read by age: age 1 is the frame written last; age 0 is the
/// slot about to be written, which holds the oldest frame, as the original's
/// pointer arithmetic gives when a tap lands on the write pointer.
pub(crate) struct Line {
    frames: Vec<[f32; 2]>,
    write: usize,
}

impl Line {
    /// Room for ages up to `longest` and one more for interpolation.
    pub(crate) fn new(longest: usize) -> Self {
        Line { frames: vec![[0.0; 2]; longest + 2], write: 0 }
    }

    pub(crate) fn clear(&mut self) {
        self.frames.fill([0.0; 2]);
        self.write = 0;
    }

    #[inline]
    pub(crate) fn at(&self, age: usize) -> [f32; 2] {
        let len = self.frames.len();
        self.frames[(self.write + len - age % len) % len]
    }

    #[inline]
    pub(crate) fn push(&mut self, frame: [f32; 2]) {
        self.frames[self.write] = frame;
        self.write = (self.write + 1) % self.frames.len();
    }

    /// The longest age that reads a frame of its own.
    pub(crate) fn longest(&self) -> usize {
        self.frames.len() - 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_speed_law_at_its_marks() {
        let near = |v: f32, a: f64| (speed(v, 44_100.0).0 / a - 1.0).abs() < 1e-12;
        assert!(near(50.0, 1e-6) && near(25.0, 1e-7) && near(75.0, 1e-5) && near(1.0, 1e-8));
        assert!(near(99.0, 9.12e-5), "int(10^3.96) = 9120");
        assert_eq!(speed(100.0, 44_100.0), (1.0, 0.0));
    }

    #[test]
    fn the_smoothing_law_at_its_marks() {
        let near = |s: f32, a: f32| (smoothing(s, 44_100.0).0 / a - 1.0).abs() < 1e-6;
        assert!(near(50.0, 0.01) && near(0.0, 1.0) && near(100.0, 1e-4) && near(75.0, 0.001));
    }

    #[test]
    fn other_rates_keep_the_time() {
        let (_, b) = speed(75.0, 48_000.0);
        let (_, b0) = speed(75.0, 44_100.0);
        assert!((b.powf(48_000.0) - b0.powf(44_100.0)).abs() < 1e-9);
        assert_eq!(count(4096, 48_000.0), 4458);
        assert_eq!(count(4096, 44_100.0), 4096);
    }

    #[test]
    fn the_line_reads_by_age() {
        let mut line = Line::new(3);
        for v in 1..=4 {
            line.push([v as f32, 0.0]);
        }
        assert_eq!(line.at(1)[0], 4.0);
        assert_eq!(line.at(3)[0], 2.0);
    }
}
