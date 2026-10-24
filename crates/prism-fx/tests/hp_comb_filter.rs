//! Hyperprism Comb Filter (EDelay, `CombFilter`; in 1.05's menu, not 1.2.1's):
//! half of the input plus depth times the input a delay ago, the delay and
//! depth smoothed every second sample.

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn comb_at(sr: f32, settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-comb-filter", sr).expect("hp-comb-filter is in the catalogue");
    for &(key, value) in settings {
        set(&mut k, key, value);
    }
    k
}

fn comb(settings: &[(&str, f32)]) -> Boxed {
    comb_at(SR, settings)
}

fn set(k: &mut Boxed, key: &str, value: f32) {
    let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
    k.set_param(i, value);
}

fn run(k: &mut Boxed, x: &[f32]) -> Vec<f32> {
    let (mut l, mut r) = (x.to_vec(), x.to_vec());
    k.process(&mut l, &mut r);
    assert_eq!(l, r);
    l
}

fn click(n: usize) -> Vec<f32> {
    let mut x = vec![0.0; n];
    x[0] = 1.0;
    x
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
}

fn tone(hz: f32, n: usize) -> Vec<f32> {
    (0..n).map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / SR).sin()).collect()
}

#[test]
fn half_the_input_plus_depth_times_the_delayed_input() {
    // 1 ms is 44.1 samples, read between 44 and 45 by linear interpolation.
    let mut k = comb(&[("delayMs", 1.0), ("depth", 100.0)]);
    let y = run(&mut k, &click(200));
    let f = (0.001 * ((1.0f64 * 44_100.0) as f32) as f64) as f32 - 44.0;
    assert_eq!(y[0], 0.5);
    assert!((y[44] - 0.5 * (1.0 - f)).abs() < 1e-5, "{}", y[44]);
    assert!((y[45] - 0.5 * f).abs() < 1e-5, "{}", y[45]);
    assert_eq!(y.iter().filter(|v| v.abs() > 1e-6).count(), 3, "no feedback");
}

#[test]
fn at_no_depth_it_is_the_input_at_half_level() {
    let x = tone(440.0, 4_000);
    let y = run(&mut comb(&[]), &x);
    assert!(y.iter().zip(&x).all(|(a, b)| *a == 0.5 * b));
}

#[test]
fn the_notches_fall_at_odd_multiples_of_half_the_inverse_delay() {
    // 1 ms: a notch at 500 Hz, a peak of the input's own level at 1 kHz.
    let notch = run(&mut comb(&[("delayMs", 1.0), ("depth", 100.0)]), &tone(500.0, 8_000));
    let peak = run(&mut comb(&[("delayMs", 1.0), ("depth", 100.0)]), &tone(1_000.0, 8_000));
    let level = rms(&tone(500.0, 8_000)[1_000..]);
    assert!(rms(&notch[1_000..]) < 0.02 * level, "{}", rms(&notch[1_000..]));
    assert!((rms(&peak[1_000..]) / level - 1.0).abs() < 0.02, "{}", rms(&peak[1_000..]) / level);
}

#[test]
fn depth_glides_by_the_smoothing_law_every_second_sample() {
    // Smoothing 50 % gives a = 1e-4 × int(10^(0.04 × 50)) = 0.01 per update,
    // and the update runs on every second sample.
    let mut k = comb(&[("delayMs", 1.0)]);
    run(&mut k, &vec![1.0; 1_000]);
    set(&mut k, "depth", 100.0);
    let y = run(&mut k, &vec![1.0; 4_000]);
    let g: Vec<f32> = y.iter().map(|v| 2.0 * v - 1.0).collect();
    assert_eq!(g[0], 0.0);
    assert!((g[1] - 0.01).abs() < 1e-6 && g[2] == g[1], "{} {}", g[1], g[2]);
    assert!((g[3] - 0.0199).abs() < 1e-6 && g[4] == g[3], "{} {}", g[3], g[4]);
    let after = 1.0 - 0.99f32.powi(100);
    assert!((g[199] - after).abs() < 1e-4, "{} against {after}", g[199]);
}

#[test]
fn full_smoothing_is_slow_and_none_is_instant() {
    for (smoothing, first) in [(0.0, 1.0), (100.0, 1e-4)] {
        let mut k = comb(&[("delayMs", 1.0), ("smoothing", smoothing)]);
        run(&mut k, &vec![1.0; 1_000]);
        set(&mut k, "depth", 100.0);
        let y = run(&mut k, &[1.0; 4]);
        assert!((2.0 * y[1] - 1.0 - first).abs() < 1e-6, "smoothing {smoothing}: {}", 2.0 * y[1] - 1.0);
    }
}

#[test]
fn at_48_khz_the_smoothing_takes_the_same_time() {
    let depth_after = |sr: f32, n: usize| {
        let mut k = comb_at(sr, &[("delayMs", 1.0)]);
        run(&mut k, &vec![1.0; 1_000]);
        set(&mut k, "depth", 100.0);
        2.0 * run(&mut k, &vec![1.0; n])[n - 1] - 1.0
    };
    // Ten milliseconds: 220 updates at 44.1 kHz leave 0.99^220 of the move.
    let (a, b) = (depth_after(44_100.0, 441), depth_after(48_000.0, 480));
    assert!((a - (1.0 - 0.99f32.powi(220))).abs() < 1e-4, "{a}");
    assert!((a - b).abs() < 2e-3, "{a} against {b}");
}

#[test]
fn controls_are_the_originals() {
    let d = prism_fx::find("hp-comb-filter").unwrap().descriptor;
    assert_eq!((d.code, d.label, d.source), (44, "Hyperprism Comb Filter", "Hyperprism-PPC 1.2.1: Comb Filter"));
    assert_eq!(d.category, prism_fx::Category::Filter);
    let keys: Vec<&str> = d.params.iter().map(|p| p.key).collect();
    assert_eq!(keys, ["delayMs", "depth", "smoothing"]);
    let p = |key: &str| d.spec(key).unwrap();
    assert_eq!((p("delayMs").min, p("delayMs").max, p("delayMs").default), (0.0, 100.0, 50.0));
    assert_eq!((p("depth").min, p("depth").max, p("depth").default), (0.0, 100.0, 0.0));
    assert_eq!((p("smoothing").min, p("smoothing").max, p("smoothing").default), (0.0, 100.0, 50.0));
    assert!(d.params.iter().all(|s| !s.log));
    let pad = d.panel.pad.unwrap();
    assert_eq!((pad.x, pad.x_range, pad.y, pad.y_range), ("delayMs", (0.0, 100.0), "depth", (0.0, 100.0)));
}
