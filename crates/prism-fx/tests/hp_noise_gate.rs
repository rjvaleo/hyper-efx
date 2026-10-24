//! Hyperprism Noise Gate (ENoiseGate, process 37), as its code does it: a
//! power detector on the mid against 10^(dB/5), hysteresis by detector reset,
//! and "Attack" setting the fade out while "Release" sets the fade in.

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn gate(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-noise-gate", SR).expect("hp-noise-gate is in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.reset();
    k
}

fn run(k: &mut Boxed, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut a, mut b) = (l.to_vec(), r.to_vec());
    k.process(&mut a, &mut b);
    (a, b)
}

fn sine(amp: f32, n: usize) -> Vec<f32> {
    (0..n).map(|i| amp * (std::f32::consts::TAU * 440.0 * i as f32 / SR).sin()).collect()
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// Hyperprism's one-pole coefficient for a time of `t` in its units, r = ½.
fn coefficient(t: f64) -> f32 {
    if t <= 0.0 {
        return 1.0;
    }
    let w = (2.0 * std::f64::consts::PI / (0.001 * t * SR as f64)).min(std::f64::consts::PI);
    let r2 = 0.25;
    let a = 1.0 - r2;
    let b = r2 * -2.0 * (1.0 + w.cos());
    let c = r2 * -2.0 * (1.0 - w.cos());
    (2.0 * c / (b - (b * b - 4.0 * a * c).sqrt())) as f32
}

#[test]
fn the_threshold_is_power_against_ten_to_the_db_over_five() {
    // A sine's mean detector reading is 2.83 × A² / 2. At −20 dB on the dial
    // (1e-4 in power) a sine opens at about −41.5 dBFS, twice the dial.
    let thr = 1e-4f32;
    let open_at = (thr / std::f32::consts::SQRT_2).sqrt();
    let level = |amp: f32| {
        let x = sine(amp, 44_100);
        let (l, _) = run(&mut gate(&[("thresholdDb", -20.0), ("attackMs", 100.0), ("releaseMs", 100.0)]), &x, &x);
        peak(&l[22_050..]) / amp
    };
    assert!(level(open_at * 1.5) > 0.99, "{}", level(open_at * 1.5));
    assert!(level(open_at / 1.5) < 1e-3, "{}", level(open_at / 1.5));
    assert!((20.0 * open_at.log10() - -41.5).abs() < 0.1);
}

#[test]
fn the_detector_is_the_mid() {
    // Opposite channels cancel in the mid, so the gate never opens.
    let x = sine(0.5, 22_050);
    let neg: Vec<f32> = x.iter().map(|s| -s).collect();
    let (l, _) = run(&mut gate(&[("thresholdDb", -60.0), ("attackMs", 20.0)]), &x, &neg);
    assert!(peak(&l[11_025..]) < 1e-6);
}

#[test]
fn attack_sets_the_fade_out_by_its_coefficient_each_sample() {
    // Loud, then quiet: once the open detector falls to the threshold the
    // gain falls by (1 − a) a sample, a from the attack time ÷ 100.
    let mut x = vec![0.5f32; 2_000];
    x.extend(vec![0.001f32; 30_000]);
    let (l, _) = run(&mut gate(&[("thresholdDb", -20.0), ("attackMs", 300.0), ("releaseMs", 900.0)]), &x, &x);
    let closing = (2_001..x.len()).find(|&i| l[i] < 0.999 * 0.001).expect("it closes");
    let a = coefficient(0.01 * 300.0);
    for i in closing + 2..closing + 400 {
        let ratio = l[i] / l[i - 1];
        assert!((ratio - (1.0 - a)).abs() < 1e-5, "frame {i}: {ratio} vs {}", 1.0 - a);
    }
}

#[test]
fn release_sets_the_fade_in_toward_one() {
    // Quiet, then loud: on opening the gain rises as g = (1 − a)g + a, a from
    // the release time ÷ 100, from where the fade out left it.
    let mut x = vec![0.0f32; 20_000];
    x.extend(vec![0.5f32; 20_000]);
    let (l, _) = run(&mut gate(&[("thresholdDb", -20.0), ("attackMs", 50.0), ("releaseMs", 700.0)]), &x, &x);
    let a = coefficient(0.01 * 700.0);
    let g: Vec<f32> = l[20_000..].iter().map(|s| s / 0.5).collect();
    // The closed gain has faded to the smallest subnormal, where a multiply
    // by (1 − a) rounds back to itself: it never quite reaches zero.
    assert!(g[0] > 0.0 && g[0] < 1e-40);
    let opening = g.iter().position(|v| *v > 1e-20).expect("it opens");
    for i in opening + 1..opening + 2_000 {
        let expect = (1.0 - a) * g[i - 1] + a;
        assert!((g[i] - expect).abs() < 1e-5, "frame {i}: {} vs {expect}", g[i]);
    }
}

#[test]
fn it_starts_open_and_closes_on_the_first_quiet_sample() {
    // The start threshold, 0 dB, is 1.0 in power: everything under about
    // −1.5 dBFS is quiet.
    let x = vec![0.5f32; 8];
    let (l, _) = run(&mut gate(&[]), &x, &x);
    let a = coefficient(0.01 * 250.0);
    assert_eq!(l[0], 0.5);
    assert!((l[1] - 0.5 * (1.0 - a)).abs() < 1e-7, "{}", l[1]);
}

#[test]
fn no_attack_time_closes_in_one_sample() {
    let mut x = vec![0.5f32; 1_000];
    x.extend(vec![0.001f32; 1_000]);
    let (l, _) = run(&mut gate(&[("thresholdDb", -20.0), ("attackMs", 0.0)]), &x, &x);
    let first_zero = l.iter().position(|s| *s == 0.0).expect("it closes");
    assert!(first_zero > 1_000 && l[first_zero..].iter().all(|s| *s == 0.0));
}

#[test]
fn short_times_overshoot_but_stay_finite() {
    // Under 4.5 on the dial the coefficient is 1.1547, so the fade alternates
    // in sign as it falls: the original's formula, kept.
    let mut x = vec![0.5f32; 1_000];
    x.extend(vec![0.001f32; 1_000]);
    let (l, _) = run(&mut gate(&[("thresholdDb", -20.0), ("attackMs", 3.0)]), &x, &x);
    let closing = (1_001..x.len()).find(|&i| l[i] < 0.999 * 0.001).unwrap();
    assert!(l[closing] < 0.0 && l[closing + 1] > 0.0);
    assert!(l.iter().all(|s| s.is_finite()));
}
