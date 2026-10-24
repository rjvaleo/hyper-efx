//! Hyperprism Phaser: four first-order allpasses on one coefficient, swept by
//! a triangle in log frequency, the dry sound subtracted.

use prism_fx::effects::HpPhaser;
use prism_fx::Kernel;
use std::f64::consts::PI;

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn phaser(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-phaser", SR).expect("hp-phaser is in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.reset();
    k
}

/// The kernel itself, for the sweep it reports.
fn probe(settings: &[(&str, f32)]) -> HpPhaser {
    let mut k = HpPhaser::new(SR);
    for &(key, value) in settings {
        k.set_param(HpPhaser::descriptor().index_of(key).unwrap(), value);
    }
    k.reset();
    k
}

fn sine(hz: f64, n: usize) -> Vec<f32> {
    (0..n).map(|i| (2.0 * PI * hz * i as f64 / SR as f64).sin() as f32 * 0.5).collect()
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / x.len() as f64).sqrt()
}

/// The gain the code's structure gives at a frozen sweep: each stage is
/// (g − z⁻¹)/(1 − g·z⁻¹), the chain's input takes fb × its last output, and
/// the output is 0.5·chain − 0.5·dry.
fn formula(hz: f64, base: f64, fb: f64) -> f64 {
    let w = (PI * base / SR as f64) as f32 as f64;
    let g = (1.0 - w) / (1.0 + w);
    let om = 2.0 * PI * hz / SR as f64;
    let z1 = (om.cos(), -om.sin()); // z⁻¹
    let mul = |a: (f64, f64), b: (f64, f64)| (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0);
    let div = |a: (f64, f64), b: (f64, f64)| {
        let d = b.0 * b.0 + b.1 * b.1;
        ((a.0 * b.0 + a.1 * b.1) / d, (a.1 * b.0 - a.0 * b.1) / d)
    };
    let h = div((g - z1.0, -z1.1), (1.0 - g * z1.0, -g * z1.1));
    let h4 = mul(mul(h, h), mul(h, h));
    let loop_ = mul((fb * z1.0, fb * z1.1), h4);
    let chain = div(h4, (1.0 - loop_.0, -loop_.1));
    let y = (0.5 * chain.0 - 0.5, 0.5 * chain.1);
    (y.0 * y.0 + y.1 * y.1).sqrt()
}

/// The kernel's gain at a frequency, measured on a held sine.
fn measured(settings: &[(&str, f32)], hz: f64) -> f64 {
    let x = sine(hz, 22_050);
    let mut k = phaser(settings);
    let (mut l, mut r) = (x.clone(), x.clone());
    k.process(&mut l, &mut r);
    rms(&l[11_025..]) / rms(&x[11_025..])
}

#[test]
fn at_a_frozen_sweep_the_response_is_the_four_allpass_formula() {
    // Rate 0 holds the sweep at the base frequency.
    for fb in [0.0, 20.0, 60.0] {
        let settings = [("rateHz", 0.0), ("baseHz", 1_000.0), ("feedback", fb)];
        for hz in [100.0, 300.0, 700.0, 1_500.0, 3_000.0, 8_000.0, 15_000.0] {
            let (got, want) = (measured(&settings, hz), formula(hz, 1_000.0, fb as f64 / 100.0));
            assert!((got - want).abs() < 0.01 + 0.02 * want, "fb {fb} %, {hz} Hz: {got} vs {want}");
        }
    }
}

#[test]
fn the_notch_sits_where_the_formula_puts_it() {
    let base = 1_000.0;
    let grid = |from: f64, to: f64, steps: usize, f: &dyn Fn(f64) -> f64| {
        (0..=steps).map(|i| from * (to / from).powf(i as f64 / steps as f64)).map(|hz| (hz, f(hz))).fold((0.0, f64::MAX), |m, p| if p.1 < m.1 { p } else { m }).0
    };
    let at = grid(200.0, 15_000.0, 100_000, &|hz| formula(hz, base, 0.0));
    // Four stages at −90° each: where tan(πf/SR) = w, just under the base.
    let w = (PI * base / SR as f64) as f32 as f64;
    assert!((at - w.atan() * SR as f64 / PI).abs() < 0.5, "{at}");
    let settings = [("rateHz", 0.0), ("baseHz", base as f32), ("feedback", 0.0)];
    let found = grid(at * 0.95, at * 1.05, 100, &|hz| measured(&settings, hz));
    assert!((found - at).abs() / at < 0.01, "notch at {found} Hz, the formula says {at}");
}

#[test]
fn the_sweep_is_a_triangle_in_log_frequency_from_base_to_base_times_depth_squared() {
    // Rate 2 is P0 = 7.792; one cycle is 4·SR/P0 = 22 638.6 samples (1.948 Hz),
    // between 440 Hz and 440 × 5² = 11 kHz.
    let mut k = probe(&[("rateHz", 2.0), ("depth", 5.0), ("baseHz", 440.0)]);
    let mut frame = [0.0f32; 2];
    let w: Vec<f64> = (0..120_000).map(|_| { k.tick(&mut frame); k.sweep() as f64 }).collect();
    let wmin = (PI * 440.0 / SR as f64) as f32 as f64;
    let top = w.iter().cloned().fold(0.0, f64::max);
    let bottom = w.iter().cloned().fold(f64::MAX, f64::min);
    assert!((top / (wmin * 25.0) - 1.0).abs() < 1e-3, "top {top} vs {}", wmin * 25.0);
    assert!((bottom / wmin - 1.0).abs() < 1e-3, "bottom {bottom} vs {wmin}");
    let peaks: Vec<usize> = (1..w.len() - 1).filter(|&i| w[i] > w[i - 1] && w[i] >= w[i + 1]).collect();
    assert!(peaks.len() >= 4, "{peaks:?}");
    let period = (peaks[peaks.len() - 1] - peaks[0]) as f64 / (peaks.len() - 1) as f64;
    let expected = 4.0 * SR as f64 / (3.896 * 2.0);
    assert!((period - expected).abs() < 3.0, "period {period} vs {expected}");
    // Straight lines in log frequency: equal ratios each sample.
    let i = peaks[0] + 2000;
    let (a, b) = ((w[i] / w[i - 1]).ln(), (w[i + 1000] / w[i + 999]).ln());
    assert!((a - b).abs() < 1e-6, "{a} vs {b}");
}

#[test]
fn the_first_step_uses_depth_squared() {
    // Changed from the original: its initialisers set the sweep up with
    // 2^depth, while the routine uses depth² from its first smoothing step.
    // Both use depth² here.
    let mut k = probe(&[("rateHz", 2.0), ("depth", 5.0), ("baseHz", 440.0)]);
    let w0 = k.sweep() as f64;
    let mut frame = [0.0f32; 2];
    k.tick(&mut frame);
    let ratio = k.sweep() as f64 / w0;
    let p0 = (2.0f32 * 3.896) as f64;
    let want = 25f64.powf(p0 / SR as f64 * 0.5);
    let not = 32f64.powf(p0 / SR as f64 * 0.5);
    assert!((ratio - want).abs() < 1e-6, "{ratio} vs {want} (2^depth would give {not})");
}

#[test]
fn depth_below_one_holds_the_sweep_at_the_base() {
    // Changed from the original: below depth 1 the top of its sweep fell
    // under the bottom, so the sweep climbed for ever and went to NaN (and
    // depth 0 divided by zero). Such a depth sweeps nowhere.
    for depth in [0.0, 0.5, 0.99, 1.0] {
        let mut k = probe(&[("rateHz", 4.0), ("depth", depth), ("baseHz", 440.0)]);
        let w0 = k.sweep();
        for i in 0..220_500 {
            let mut frame = [((i as f32) * 0.1).sin(), ((i as f32) * 0.13).cos()];
            k.tick(&mut frame);
            assert!(frame[0].is_finite() && frame[1].is_finite(), "depth {depth}, sample {i}");
        }
        assert_eq!(k.sweep(), w0, "depth {depth}");
    }
}

#[test]
fn the_dry_sound_is_subtracted() {
    // At DC every stage is −1 and the chain +1, so 0.5·chain − 0.5·dry is 0.
    let mut k = phaser(&[("rateHz", 0.0), ("feedback", 0.0)]);
    let (mut l, mut r) = (vec![0.5f32; 20_000], vec![0.5f32; 20_000]);
    k.process(&mut l, &mut r);
    assert!(l[19_999].abs() < 1e-4, "{}", l[19_999]);
    assert_eq!(l, r);
}

#[test]
fn the_ranges_are_the_originals() {
    let d = prism_fx::find("hp-phaser").unwrap().descriptor;
    let check = |key: &str, min: f32, max: f32, default: f32| {
        let s = d.spec(key).unwrap_or_else(|| panic!("{key}"));
        assert!(!s.log, "{key}");
        assert_eq!((s.min, s.max, s.default), (min, max, default), "{key}");
    };
    check("rateHz", 0.0, 4.0, 2.0);
    check("depth", 0.0, 10.0, 5.0);
    check("feedback", 0.0, 100.0, 20.0);
    check("baseHz", 0.0, 10_000.0, 440.0);
    check("smoothing", 0.0, 100.0, 50.0);
    assert_eq!(d.label, "Hyperprism Phaser");
}
