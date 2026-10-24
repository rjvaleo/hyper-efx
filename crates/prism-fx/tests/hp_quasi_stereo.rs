//! Hyperprism Quasi Stereo (EQuasiStereo), as its code does it: a delayed,
//! high-passed copy of the mid added to the left and taken from the right.

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn quasi(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-quasi-stereo", SR).expect("hp-quasi-stereo is in the catalogue");
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

fn noise(n: usize, seed: u32) -> Vec<f32> {
    let mut x = seed | 1;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x as f32 / u32::MAX as f32) - 0.5
        })
        .collect()
}

fn sine(hz: f32, n: usize) -> Vec<f32> {
    (0..n).map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / SR).sin()).collect()
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// The high-pass's first output for a unit impulse: 2α.
fn two_alpha(fc: f64) -> f32 {
    let w = (std::f64::consts::TAU * fc / SR as f64) as f32 as f64;
    let s = w.sin() as f32 as f64;
    let c = w.cos() as f32 as f64;
    let beta = (0.5 * ((1.0 - s / 2f64.sqrt()) / (1.0 + s / 2f64.sqrt()))) as f32;
    let gamma = ((0.5 + beta as f64) * c) as f32;
    let alpha = (0.25 * (0.5 + beta as f64 + gamma as f64)) as f32;
    2.0 * alpha
}

#[test]
fn the_sum_of_the_two_sides_is_the_input() {
    let (x, y) = (noise(8_192, 3), noise(8_192, 5));
    let (l, r) = run(&mut quasi(&[("depth", 100.0), ("lowCutHz", 300.0), ("delayMs", 7.0)]), &x, &y);
    for i in 0..x.len() {
        assert!(((l[i] + r[i]) - (x[i] + y[i])).abs() < 1e-5, "frame {i}");
    }
}

#[test]
fn the_copy_arrives_exactly_delay_time_late() {
    // 25 ms at 44.1 kHz is int(1102.5) = 1102 samples.
    let mut x = vec![0.0; 2_048];
    x[0] = 1.0;
    let (l, r) = run(&mut quasi(&[("depth", 100.0), ("lowCutHz", 10.0)]), &x, &x);
    let side: Vec<f32> = (0..x.len()).map(|i| l[i] - x[i]).collect();
    assert!(side[..1_102].iter().all(|s| *s == 0.0), "nothing before the delay");
    assert!((side[1_102] - two_alpha(10.0)).abs() < 1e-6, "{}", side[1_102]);
    assert!((r[1_102] + side[1_102]).abs() < 1e-6, "taken from the right");
}

#[test]
fn no_delay_time_injects_at_once() {
    let mut x = vec![0.0; 64];
    x[0] = 1.0;
    let (l, _) = run(&mut quasi(&[("depth", 50.0), ("lowCutHz", 1000.0), ("delayMs", 0.0)]), &x, &x);
    assert!((l[0] - 1.0 - 0.5 * two_alpha(1000.0)).abs() < 1e-6, "{}", l[0]);
}

#[test]
fn the_low_cut_is_a_butterworth_high_pass_three_db_down_at_the_corner() {
    let side = |hz: f32| {
        let x = sine(hz, 44_100);
        let (l, _) = run(&mut quasi(&[("depth", 100.0), ("lowCutHz", 1000.0)]), &x, &x);
        let d: Vec<f32> = (0..x.len()).map(|i| l[i] - x[i]).collect();
        peak(&d[22_050..]) / 0.5
    };
    assert!((side(1000.0) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01, "{}", side(1000.0));
    assert!((side(10_000.0) - 1.0).abs() < 0.03, "{}", side(10_000.0));
    // 12 dB an octave: a decade below is 40 dB down.
    assert!((20.0 * side(100.0).log10() - -40.0).abs() < 1.0, "{}", side(100.0));
}

#[test]
fn the_low_cut_never_goes_under_ten_hertz() {
    let mut x = vec![0.0; 1_200];
    x[0] = 1.0;
    let (l, _) = run(&mut quasi(&[("depth", 100.0), ("lowCutHz", 0.0)]), &x, &x);
    assert!((l[1_102] - two_alpha(0.000_226_75 * 44_100.0)).abs() < 1e-6);
}

#[test]
fn a_mono_input_injects_with_the_opposite_sign() {
    let mut x = vec![0.0; 1_200];
    x[0] = 1.0;
    let right = vec![0.3; 1_200];
    let (l, r) = run(&mut quasi(&[("depth", 100.0), ("lowCutHz", 10.0), ("mono", 1.0)]), &x, &right);
    assert!((l[1_102] - -two_alpha(10.0)).abs() < 1e-6, "{}", l[1_102]);
    assert!((r[1_102] - two_alpha(10.0)).abs() < 1e-6, "{}", r[1_102]);
    assert_eq!(r[1], 0.0, "the right input is not used");
}

#[test]
fn at_no_depth_it_passes_the_sound() {
    let (x, y) = (noise(2_048, 9), noise(2_048, 13));
    let (l, r) = run(&mut quasi(&[("depth", 0.0)]), &x, &y);
    assert_eq!((l, r), (x, y));
}
