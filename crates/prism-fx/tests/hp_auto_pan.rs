//! Hyperprism Auto-Pan (ESpatializer Sweeping Pan, process 28), as its code
//! does it: the input summed to mono and panned by a sine LFO about the
//! Position, with a constant-power quarter-sine law read from a table.

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn auto_pan_at(sr: f32, settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-auto-pan", sr).expect("hp-auto-pan is in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.reset();
    k
}

fn auto_pan(settings: &[(&str, f32)]) -> Boxed {
    auto_pan_at(SR, settings)
}

fn run(k: &mut Boxed, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut a, mut b) = (l.to_vec(), r.to_vec());
    k.process(&mut a, &mut b);
    (a, b)
}

/// The LFO's period in samples, from its phase accumulator: an f32 that
/// gains Hz × 1024 / SR a sample and wraps at 1023.
fn period_of(hz: f32, sr: f32, n: usize) -> f64 {
    let inc = (hz * 1024.0) / sr;
    let (mut p, mut wraps) = (0.0f32, 0u64);
    for _ in 0..n {
        p += inc;
        while p >= 1023.0 {
            p -= 1023.0;
            wraps += 1;
        }
    }
    1023.0 * n as f64 / (wraps as f64 * 1023.0 + p as f64)
}

/// The pan table: a quarter sine of 1024 entries.
fn q(i: usize) -> f32 {
    (std::f64::consts::FRAC_PI_2 / 1024.0 * i as f64).sin() as f32
}

#[test]
fn at_no_depth_it_is_a_static_constant_power_pan_of_the_mono_sum() {
    // Position 50 % is the centre: both sides read entry 511, −3.02 dB.
    let (l, r) = run(&mut auto_pan(&[("depth", 0.0), ("position", 50.0)]), &[0.8; 64], &[0.2; 64]);
    for i in 0..64 {
        assert!((l[i] - 0.5 * q(511)).abs() < 1e-7 && (r[i] - 0.5 * q(511)).abs() < 1e-7, "{} {}", l[i], r[i]);
    }
    assert!((20.0 * q(511).log10() - -3.0103).abs() < 0.03);
}

#[test]
fn the_position_starts_at_a_quarter_as_the_window_does() {
    // Position 25 % is a pan of 0.25: left reads entry 767, right 255.
    let k = auto_pan(&[]);
    let i = k.descriptor().index_of("position").unwrap();
    assert_eq!(k.param(i), 25.0);
    let (l, r) = run(&mut auto_pan(&[("depth", 0.0)]), &[1.0; 16], &[1.0; 16]);
    assert!((l[0] - q(767)).abs() < 1e-7 && (r[0] - q(255)).abs() < 1e-7, "{} {}", l[0], r[0]);
}

#[test]
fn position_offsets_the_swing_and_the_pan_clamps_at_the_edges() {
    // Depth 100 % about position 100 %: the pan runs 0.5 … 1.5, clamped to 1.
    let x = vec![1.0; 44_100];
    let (l, r) = run(&mut auto_pan(&[("depth", 100.0), ("position", 100.0), ("rateHz", 2.0)]), &x, &x);
    let max_l = l.iter().fold(0.0f32, |m, s| m.max(*s));
    assert!((max_l - q(511)).abs() < 1e-3, "{max_l}");
    let at_right = r.iter().filter(|s| **s == q(1023)).count();
    assert!(at_right > 15_000, "held at the right edge for a third of each cycle: {at_right}");
}

#[test]
fn the_lfo_starts_at_the_centre_and_moves_right_first() {
    let x = vec![1.0; 2_000];
    let (l, r) = run(&mut auto_pan(&[("depth", 100.0), ("position", 50.0), ("rateHz", 5.0)]), &x, &x);
    assert_eq!(l[0], r[0]);
    assert!(r[1_000] > l[1_000]);
}

#[test]
fn the_rate_is_the_frequency_times_1024_over_1023() {
    // The phase advances Hz × 1024 / SR table steps but wraps at 1023, so the
    // LFO runs 0.1 % fast. Measure it from the right-hand gain's upward
    // crossings of the centre over 20 seconds.
    let hz = 5.0;
    let n = (20.0 * SR) as usize;
    let x = vec![1.0; n];
    let (l, r) = run(&mut auto_pan(&[("depth", 100.0), ("position", 50.0), ("rateHz", hz)]), &x, &x);
    let crossings: Vec<usize> = (1..n).filter(|&i| r[i - 1] <= l[i - 1] && r[i] > l[i]).collect();
    let period = (crossings[crossings.len() - 1] - crossings[0]) as f64 / (crossings.len() - 1) as f64;
    let expected = period_of(hz, SR, n);
    assert!((period - expected).abs() < 0.5, "period {period}, expected {expected}");
    // 0.1 % fast: 8.6 samples short of a true 5 Hz.
    assert!((SR as f64 / hz as f64 - period - 8.6).abs() < 1.0, "{period}");
}

#[test]
fn the_rate_holds_in_hertz_at_another_sample_rate() {
    let sr = 96_000.0;
    let n = (10.0 * sr) as usize;
    let x = vec![1.0; n];
    let (l, r) = run(&mut auto_pan_at(sr, &[("depth", 100.0), ("position", 50.0), ("rateHz", 3.0)]), &x, &x);
    let crossings: Vec<usize> = (1..n).filter(|&i| r[i - 1] <= l[i - 1] && r[i] > l[i]).collect();
    let period = (crossings[crossings.len() - 1] - crossings[0]) as f64 / (crossings.len() - 1) as f64;
    let expected = period_of(3.0, sr, n);
    assert!((period - expected).abs() < 1.0, "period {period}, expected {expected}");
    assert!((period / sr as f64 - 1023.0 / 1024.0 / 3.0).abs() < 1e-4, "{period}");
}

#[test]
fn a_rate_change_glides_in_forty_five_sample_steps() {
    // Smoothing 0 % lands on the 45th sample; until then the old rate holds.
    let mut k = auto_pan(&[("depth", 100.0), ("position", 50.0), ("rateHz", 0.0), ("smoothing", 0.0)]);
    let i = k.descriptor().index_of("rateHz").unwrap();
    k.set_param(i, 20.0);
    let (l, r) = run(&mut k, &[1.0; 60], &[1.0; 60]);
    assert!((0..=44).all(|n| l[n] == r[n]), "still at the centre through sample 44");
    assert!(r[45] > l[45]);
}
