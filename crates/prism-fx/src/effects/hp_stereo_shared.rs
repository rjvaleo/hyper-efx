//! What Hyperprism-PPC 1.2.1's stereo, pan and dynamics processes share: the
//! stepped parameter smoothing, the tables, and the one-pole coefficient
//! formula of the Compressor and Noise Gate.
//!
//! Hyperprism works in 16-bit units (±32767). Here a float ±1 is ±32768 of
//! them, which is the scale its thresholds and quantisers are measured on.
//!
//! The original's numbers hold at 44.1 kHz. At another rate the sample counts
//! — the 45-sample smoothing step, buffer and block lengths — scale by
//! sr / 44100, so times in milliseconds stay what they were.

/// A float of 1.0 in the original's 16-bit units.
pub(crate) const FULL_SCALE: f32 = 32_768.0;

/// The rate the original's sample counts are for.
pub(crate) const ORIGINAL_RATE: f32 = 44_100.0;

/// The smoothing countdown's period at 44.1 kHz.
const SMOOTHING_PERIOD: f32 = 45.0;

/// `frames` at 44.1 kHz, as frames at `sample_rate`, at least one.
pub(crate) fn scaled(frames: f32, sample_rate: f32) -> usize {
    ((frames * sample_rate / ORIGINAL_RATE).round() as usize).max(1)
}

/// The Smoothing (%) slider's one-pole coefficient a and 1 − a, as the
/// original rounds them: a = 0.0001 × int(10^(0.04 × (100 − s))).
pub(crate) fn smoothing_coefficients(percent: f32) -> (f32, f32) {
    let field = (10f64.powf(0.01 * 4.0 * (100.0 - percent as f64))) as i32;
    let a = 0.0001 * field as f64;
    (a as f32, (1.0 - a) as f32)
}

/// The original's smoothing: a countdown that fires on every 45th sample, on
/// which each smoothed value takes `v = a·target + (1 − a)·v`. Between firings
/// the values hold. It counts samples itself, so the host's block size does
/// not move it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stepper {
    period: u32,
    count: u32,
    a: f32,
    b: f32,
}

impl Stepper {
    pub(crate) fn new(sample_rate: f32, percent: f32) -> Self {
        let period = scaled(SMOOTHING_PERIOD, sample_rate) as u32;
        let (a, b) = smoothing_coefficients(percent);
        Stepper { period, count: period - 1, a, b }
    }

    pub(crate) fn set_smoothing(&mut self, percent: f32) {
        (self.a, self.b) = smoothing_coefficients(percent);
    }

    /// The countdown as it is after `Initialize`: the first firing is on the
    /// 45th sample.
    pub(crate) fn restart(&mut self) {
        self.count = self.period - 1;
    }

    /// One sample: true when the smoothed values move on this one.
    #[inline]
    pub(crate) fn fires(&mut self) -> bool {
        if self.count == 0 {
            self.count = self.period - 1;
            true
        } else {
            self.count -= 1;
            false
        }
    }

    /// One step of a smoothed value toward its target.
    #[inline]
    pub(crate) fn toward(&self, value: f32, target: f32) -> f32 {
        self.a * target + self.b * value
    }
}

/// `CalcLowPassParameter(t, r, fs)`: the Compressor's and Noise Gate's
/// one-pole coefficient. A root of (1 − r²)a² − B·a + C = 0 with
/// B = −2r²(1 + cos w), C = −2r²(1 − cos w), w = 2π / (0.001·t·fs) clamped to
/// π. For long times a ≈ w²/4, so the time constant grows with the square of
/// t; under w = π it is 1.1547, over one. Zero or less gives 1.
pub(crate) fn low_pass_coefficient(t: f64, r: f64, fs: f64) -> f64 {
    if t <= 0.0 {
        return 1.0;
    }
    let pi = core::f64::consts::PI;
    let w = ((2.0 * pi) / (0.001 * t * fs)).min(pi);
    let r2 = r * r;
    let a = 1.0 - r2;
    let b = r2 * -2.0 * (1.0 + w.cos());
    let c = r2 * -2.0 * (1.0 - w.cos());
    (2.0 * c) / (b - (b * b - 4.0 * a * c).sqrt())
}

/// `SinBuffer`: one cycle, from the original's 2π constant, a hair over.
pub(crate) fn sin_table<const N: usize>() -> [f32; N] {
    let step = 6.283_185_349 / N as f64;
    core::array::from_fn(|i| (i as f64 * step).sin() as f32)
}

/// `QuarterSinBuffer`: a quarter cycle, 0 up to just under 1.
pub(crate) fn quarter_sin_table<const N: usize>() -> [f32; N] {
    let step = core::f64::consts::FRAC_PI_2 / N as f64;
    core::array::from_fn(|i| (i as f64 * step).sin() as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothing_matches_the_original_table() {
        assert_eq!(smoothing_coefficients(0.0).0, 1.0);
        assert_eq!(smoothing_coefficients(25.0).0, 0.1);
        assert_eq!(smoothing_coefficients(30.0).0, 0.063);
        assert_eq!(smoothing_coefficients(50.0).0, 0.01);
        assert_eq!(smoothing_coefficients(100.0).0, 0.0001);
    }

    #[test]
    fn the_step_is_45_samples_at_44k1_and_scales() {
        let mut s = Stepper::new(44_100.0, 50.0);
        let fired: Vec<usize> = (0..200).filter(|_| s.fires()).collect::<Vec<_>>();
        assert_eq!(fired.len(), 4);
        let mut s = Stepper::new(44_100.0, 50.0);
        assert!((0..44).all(|_| !s.fires()) && s.fires());
        assert_eq!(scaled(45.0, 88_200.0), 90);
    }

    #[test]
    fn the_coefficient_matches_the_decoded_values() {
        assert!((low_pass_coefficient(10.0, 0.5, 44_100.0 / 1024.0) - 1.154_700_5).abs() < 1e-6);
        assert!((low_pass_coefficient(100.0, 0.5, 44_100.0 / 1024.0) - 0.484).abs() < 1e-3);
        assert!((low_pass_coefficient(2.5, 0.5, 44_100.0) - 0.000_81).abs() < 1e-5);
        assert_eq!(low_pass_coefficient(0.0, 0.5, 44_100.0), 1.0);
    }
}
