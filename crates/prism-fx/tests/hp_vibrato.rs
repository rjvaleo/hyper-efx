//! Hyperprism Vibrato: the Modulation routine wet only, its depth a pitch
//! deviation that the rate does not change.

use std::f64::consts::PI;

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn vibrato(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-vibrato", SR).expect("hp-vibrato is in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.reset();
    k
}

/// Delay per sample, read from a ramp: wet only, a ramp delayed by d is n − d
/// exactly under linear interpolation.
fn delays(k: &mut Boxed, n: usize) -> Vec<f64> {
    let x: Vec<f32> = (0..n).map(|i| i as f32).collect();
    let (mut l, mut r) = (x.clone(), x.clone());
    k.process(&mut l, &mut r);
    assert_eq!(l, r);
    (0..n).map(|i| i as f64 - l[i] as f64).collect()
}

#[test]
fn the_pitch_deviation_does_not_depend_on_the_rate() {
    // Depth v gives a delay swing D = v·SR/(1024·f), whose steepest slope is
    // π·v/1024 at every rate: ±3.07 % at v = 10.
    let v = 10.0;
    for hz in [0.5f32, 1.0, 2.0, 4.0] {
        let d = delays(&mut vibrato(&[("rateHz", hz), ("depth", v)]), 30_000);
        let w = 100;
        let start = (v as f64 * SR as f64 / (1024.0 * hz as f64)) as usize + 10;
        let steepest = (start..d.len() - w).map(|i| (d[i + w] - d[i]) / w as f64).fold(0.0f64, |m, s| m.max(s.abs()));
        let expected = PI * v as f64 / 1024.0;
        assert!((steepest - expected).abs() / expected < 0.01, "{hz} Hz: {steepest} vs {expected}");
    }
}

#[test]
fn the_delay_swings_from_zero_to_v_sr_over_1024_f() {
    let hz = 4.0;
    let d = delays(&mut vibrato(&[("rateHz", hz), ("depth", 10.0)]), 12_000);
    let swing = 10.0 * SR as f64 / (1024.0 * hz as f64);
    let top = d[200..11_025].iter().cloned().fold(0.0, f64::max);
    assert!((top - swing).abs() < 0.05, "{top} vs {swing}");
    // The bottom, a cycle on, is no delay at all.
    assert!(d[11_025].abs() < 0.05, "{}", d[11_025]);
}

#[test]
fn the_swing_stops_at_16384_samples() {
    // 0.01 Hz at v = 10 asks for 43 066 samples; the original caps it.
    let mut k = vibrato(&[("rateHz", 0.01), ("depth", 10.0)]);
    let n = 30_000;
    let d = delays(&mut k, n);
    // Near the bottom, d = D·0.5(1 − cos 2πft); at t = n samples:
    let ph = (0.01 * 1024.0 / SR as f64 * n as f64) % 1024.0;
    let expected = 16_384.0 * 0.5 * (1.0 - (2.0 * PI * ph / 1024.0).cos());
    assert!((d[n - 1] - expected).abs() < 0.5, "{} vs {expected}", d[n - 1]);
}

#[test]
fn rate_and_depth_at_zero_pass_the_sound_through() {
    // Changed from the original: depth 0 over rate 0 was 0/0, a NaN that
    // stuck in the smoother and sent the read outside the line. It is no
    // delay at all.
    let mut k = vibrato(&[("rateHz", 0.0), ("depth", 0.0)]);
    let x: Vec<f32> = (0..2000).map(|i| ((i as f32) * 0.02).sin()).collect();
    let (mut l, mut r) = (x.clone(), x.clone());
    k.process(&mut l, &mut r);
    assert_eq!(l, x);
    assert_eq!(r, x);
    // And any depth at rate 0 holds the LFO at the bottom: no delay either.
    let mut k = vibrato(&[("rateHz", 0.0), ("depth", 5.0)]);
    let (mut l, mut r) = (x.clone(), x.clone());
    k.process(&mut l, &mut r);
    assert_eq!(l, x);
}

#[test]
fn the_ranges_are_the_originals() {
    let d = prism_fx::find("hp-vibrato").unwrap().descriptor;
    let rate = d.spec("rateHz").unwrap();
    let depth = d.spec("depth").unwrap();
    assert!(!rate.log && !depth.log);
    assert_eq!((rate.min, rate.max, rate.default), (0.0, 4.0, 2.0));
    assert_eq!((depth.min, depth.max, depth.default), (0.0, 10.0, 0.0));
    assert_eq!(d.label, "Hyperprism Vibrato");
}
