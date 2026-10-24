//! Measuring tools for the Hyperprism filter and reverb tests: build a kernel
//! with settings, run it, and read a sine's gain through it.

#![allow(dead_code)]

/// Hyperprism's own rate: its numbers are exact here.
pub const SR: f32 = 44_100.0;

pub type Boxed = Box<dyn prism_fx::DynKernel>;

/// The effect `id` at `sr`, with `settings` applied and then a reset, so every
/// glide starts on its target.
pub fn make(id: &str, sr: f32, settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make(id, sr).unwrap_or_else(|| panic!("{id} is in the catalogue"));
    set(&mut k, settings);
    k.reset();
    k
}

/// Set controls by key, without a reset.
pub fn set(k: &mut Boxed, settings: &[(&str, f32)]) {
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
}

pub fn run(k: &mut Boxed, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut a, mut b) = (l.to_vec(), r.to_vec());
    k.process(&mut a, &mut b);
    (a, b)
}

pub fn impulse(n: usize) -> Vec<f32> {
    let mut x = vec![0.0; n];
    x[0] = 1.0;
    x
}

pub fn sine(hz: f64, frames: usize, sr: f32, amp: f64) -> Vec<f32> {
    let w = std::f64::consts::TAU * hz / sr as f64;
    (0..frames).map(|n| (amp * (w * n as f64).sin()) as f32).collect()
}

/// The amplitude of `hz` in `y`, by projection. Exact for a whole number of
/// cycles: `hz` a whole number and `y` one second long.
pub fn amplitude(y: &[f32], hz: f64, sr: f32) -> f64 {
    let w = std::f64::consts::TAU * hz / sr as f64;
    let (mut re, mut im) = (0.0, 0.0);
    for (n, &v) in y.iter().enumerate() {
        re += v as f64 * (w * n as f64).cos();
        im += v as f64 * (w * n as f64).sin();
    }
    2.0 * (re * re + im * im).sqrt() / y.len() as f64
}

/// The gain in dB of a sine at `hz` through the left side: two seconds in,
/// measured over the second.
pub fn gain_db(id: &str, sr: f32, settings: &[(&str, f32)], hz: f64) -> f64 {
    let mut k = make(id, sr, settings);
    let frames = 2 * sr as usize;
    let amp = 0.25;
    let x = sine(hz, frames, sr, amp);
    let (y, _) = run(&mut k, &x, &x);
    20.0 * (amplitude(&y[sr as usize..], hz, sr) / amp).log10()
}

/// |H(e^jw)| in dB of `y = g·(b0·x + b1·x1 + b2·x2 + c1·y1 + c2·y2)`: the
/// Hyperprism section, whose output gain multiplies the recursion too.
pub fn section_db(g: f64, b: [f64; 3], c: [f64; 2], w: f64) -> f64 {
    let z1 = (w.cos(), -w.sin());
    let z2 = ((2.0 * w).cos(), -(2.0 * w).sin());
    let num = (g * (b[0] + b[1] * z1.0 + b[2] * z2.0), g * (b[1] * z1.1 + b[2] * z2.1));
    let den = (1.0 - g * (c[0] * z1.0 + c[1] * z2.0), -g * (c[0] * z1.1 + c[1] * z2.1));
    20.0 * ((num.0 * num.0 + num.1 * num.1) / (den.0 * den.0 + den.1 * den.1)).sqrt().log10()
}

/// EFilter's β and γ for a frequency and Q: β uses `w` where the textbook has
/// `sin w`.
pub fn beta_gamma(f: f64, q: f64, sr: f64) -> (f64, f64, f64) {
    let w = std::f64::consts::TAU * f / sr;
    let beta = (q - w / 2.0) / (2.0 * q + w);
    (w, beta, (0.5 + beta) * w.cos())
}

pub fn w_of(hz: f64, sr: f32) -> f64 {
    std::f64::consts::TAU * hz / sr as f64
}

/// The first index from which two signals differ.
pub fn first_difference(a: &[f32], b: &[f32]) -> Option<usize> {
    a.iter().zip(b).position(|(x, y)| x != y)
}
