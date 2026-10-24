//! Hyperprism Pan (EDynamics Balance, process 21), as its code does it: a
//! constant-power balance with whole-dB gain, smoothed in 45-sample steps.

const SR: f32 = 44_100.0;
/// The original's π/4, a double a little short of the true value.
const QUARTER_PI: f64 = 0.785_398_141_75;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn pan_at(sr: f32, settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-pan", sr).expect("hp-pan is in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.reset();
    k
}

fn pan(settings: &[(&str, f32)]) -> Boxed {
    pan_at(SR, settings)
}

fn set(k: &mut Boxed, key: &str, value: f32) {
    let i = k.descriptor().index_of(key).unwrap();
    k.set_param(i, value);
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
            (x as f32 / u32::MAX as f32) * 2.0 - 1.0
        })
        .collect()
}

fn law(balance: f32) -> (f32, f32) {
    let theta = QUARTER_PI * (1.0 + (balance * 0.01f32) as f64);
    (theta.cos() as f32, theta.sin() as f32)
}

#[test]
fn the_centre_is_three_db_down_on_both_sides() {
    let (l, r) = run(&mut pan(&[]), &[0.5; 256], &[0.5; 256]);
    for s in l.iter().chain(&r) {
        assert!((20.0 * (s / 0.5).log10() - -3.0103).abs() < 0.001, "{s}");
    }
}

#[test]
fn the_balance_follows_the_constant_power_law_sample_by_sample() {
    let (x, y) = (noise(2_048, 7), noise(2_048, 11));
    for balance in [-100.0, -63.0, -20.0, 0.0, 35.0, 100.0] {
        let (gl, gr) = law(balance);
        let (l, r) = run(&mut pan(&[("balance", balance)]), &x, &y);
        for i in 0..x.len() {
            assert!((l[i] - x[i] * gl).abs() < 1e-6, "balance {balance}, frame {i}");
            assert!((r[i] - y[i] * gr).abs() < 1e-6, "balance {balance}, frame {i}");
        }
        assert!((gl * gl + gr * gr - 1.0).abs() < 1e-6, "constant power at {balance}");
    }
}

#[test]
fn it_is_a_balance_so_the_left_never_reaches_the_right() {
    let (l, r) = run(&mut pan(&[("balance", 60.0)]), &[1.0; 64], &[0.0; 64]);
    assert!(r.iter().all(|s| *s == 0.0));
    assert!(l.iter().all(|s| *s > 0.0));
}

#[test]
fn the_gain_is_whole_decibels_truncated_toward_zero() {
    let at = |db: f32| run(&mut pan(&[("balance", -100.0), ("gainDb", db)]), &[1.0; 8], &[0.0; 8]).0[7];
    let whole = |db: i32| 10f64.powf(db as f64 / 20.0) as f32;
    assert!((at(-6.7) - whole(-6)).abs() < 1e-6, "{}", at(-6.7));
    assert!((at(-0.9) - 1.0).abs() < 1e-6);
    assert!((at(-96.0) - whole(-96)).abs() < 1e-9);
}

#[test]
fn moves_glide_in_steps_of_forty_five_samples() {
    // Smoothing 50 % gives a = 0.01, applied on the 45th sample and every
    // 45 after; between, the gains hold.
    let mut k = pan(&[("smoothing", 50.0)]);
    set(&mut k, "balance", 100.0);
    let (l, r) = run(&mut k, &[1.0; 200], &[1.0; 200]);
    let (c, _) = law(0.0);
    let (t_l, t_r) = law(100.0);
    assert!(l[..44].iter().all(|s| *s == c), "held until the 45th sample");
    let first_l = 0.01f32 * t_l + (1.0 - 0.01f64) as f32 * c;
    let first_r = 0.01f32 * t_r + (1.0 - 0.01f64) as f32 * c;
    assert!((l[44] - first_l).abs() < 1e-7 && (r[44] - first_r).abs() < 1e-7, "{} {}", l[44], r[44]);
    assert!(l[44..89].iter().all(|s| *s == l[44]));
    assert!(l[89] < l[88]);
}

#[test]
fn no_smoothing_lands_on_the_forty_fifth_sample() {
    let mut k = pan(&[("smoothing", 0.0)]);
    set(&mut k, "balance", -100.0);
    let (_, r) = run(&mut k, &[1.0; 50], &[1.0; 50]);
    let (c, _) = law(0.0);
    assert!(r[..44].iter().all(|s| *s == c));
    assert!(r[44].abs() < 1e-7, "{}", r[44]);
}

#[test]
fn the_step_scales_with_the_sample_rate() {
    let mut k = pan_at(88_200.0, &[("smoothing", 0.0)]);
    set(&mut k, "balance", -100.0);
    let (_, r) = run(&mut k, &[1.0; 100], &[1.0; 100]);
    assert!(r[88] > 0.5 && r[89].abs() < 1e-7, "{} {}", r[88], r[89]);
}
