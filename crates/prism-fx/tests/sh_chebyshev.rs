//! ++chebyshev: SoundHack Freesound's Chebyshev waveshaper, as its code does
//! it.

const SR: f32 = 48_000.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

/// Every partial off (−60 dB) but the ones named.
fn shaper(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("sh-chebyshev", SR).expect("sh-chebyshev is in the catalogue");
    for key in ["t1Db", "t2Db", "t3Db", "t4Db", "t5Db", "t7Db"] {
        k.set_param(k.descriptor().index_of(key).unwrap(), -60.0);
    }
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.reset();
    k
}

fn run(k: &mut Boxed, x: &[f32]) -> Vec<f32> {
    let (mut l, mut r) = (x.to_vec(), x.to_vec());
    k.process(&mut l, &mut r);
    l
}

fn sine(hz: f32, amp: f32) -> Vec<f32> {
    (0..48_000).map(|i| amp * (std::f32::consts::TAU * hz * i as f32 / SR).sin()).collect()
}

fn level_at(x: &[f32], hz: f32) -> f32 {
    let w = std::f64::consts::TAU * hz as f64 / SR as f64;
    let (re, im) = x.iter().enumerate().fold((0.0f64, 0.0f64), |(re, im), (n, &s)| {
        (re + s as f64 * (w * n as f64).cos(), im + s as f64 * (w * n as f64).sin())
    });
    (2.0 * (re * re + im * im).sqrt() / x.len() as f64) as f32
}

#[test]
fn each_polynomial_turns_a_full_scale_sine_into_its_harmonic() {
    for (key, n) in [("t2Db", 2.0), ("t3Db", 3.0), ("t4Db", 4.0), ("t5Db", 5.0), ("t7Db", 7.0)] {
        let y = run(&mut shaper(&[(key, 0.0)]), &sine(200.0, 1.0));
        let (harmonic, fundamental) = (level_at(&y, 200.0 * n), level_at(&y, 200.0));
        assert!(harmonic > 0.95, "{key}: {harmonic}");
        assert!(fundamental < 0.01, "{key}: the fundamental is at {fundamental}");
    }
}

#[test]
fn the_partials_are_mixed_by_their_share() {
    // Σ g·T / Σ g: with T1 and T3 both at 0 dB, each is half.
    let y = run(&mut shaper(&[("t1Db", 0.0), ("t3Db", 0.0)]), &sine(200.0, 1.0));
    assert!((level_at(&y, 200.0) - 0.5).abs() < 0.01, "{}", level_at(&y, 200.0));
    assert!((level_at(&y, 600.0) - 0.5).abs() < 0.01, "{}", level_at(&y, 600.0));
    assert!(y.iter().all(|s| s.abs() <= 1.0 + 1e-4));
}

#[test]
fn a_quiet_sound_brings_out_lower_harmonics() {
    // Below full scale the polynomials are not pure harmonics: T3 of a half
    // scale sine is mostly fundamental.
    let y = run(&mut shaper(&[("t3Db", 0.0)]), &sine(200.0, 0.5));
    assert!(level_at(&y, 200.0) > level_at(&y, 600.0), "{} against {}", level_at(&y, 200.0), level_at(&y, 600.0));
}

#[test]
fn drive_past_full_scale_ends_at_the_clip() {
    // The input is not clamped before the polynomials, so drive runs them
    // away until the ±1.5 clip squares them off.
    let y = run(&mut shaper(&[("t7Db", 0.0), ("inDb", 12.0)]), &sine(200.0, 1.0));
    assert!(y.iter().all(|s| s.abs() <= 1.5));
    assert!(y.iter().filter(|s| s.abs() == 1.5).count() > y.len() / 3);
}

#[test]
fn even_partials_offset_silence_as_the_original_does() {
    // T2(0) is −1: with only the second partial up, silence comes out at −1.
    let y = run(&mut shaper(&[("t2Db", 0.0)]), &[0.0; 256]);
    assert!(y.iter().all(|s| (s + 0.995).abs() < 0.01), "{}", y[0]);
}
