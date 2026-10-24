//! Hyperprism Doppler (EDelay, `Doppler`; in 1.05's menu, not 1.2.1's): a
//! source on a plane around the listener, heard a distance over the speed of
//! sound late and panned by its bearing.

use std::f32::consts::FRAC_1_SQRT_2;

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn doppler(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-doppler", SR).expect("hp-doppler is in the catalogue");
    for &(key, value) in settings {
        set(&mut k, key, value);
    }
    k
}

fn set(k: &mut Boxed, key: &str, value: f32) {
    let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
    k.set_param(i, value);
}

fn run(k: &mut Boxed, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (l.to_vec(), r.to_vec());
    k.process(&mut l, &mut r);
    (l, r)
}

fn click(n: usize) -> Vec<f32> {
    let mut x = vec![0.0; n];
    x[0] = 1.0;
    x
}

fn loudest(x: &[f32]) -> usize {
    (0..x.len()).max_by(|&a, &b| x[a].abs().total_cmp(&x[b].abs())).unwrap()
}

#[test]
fn the_delay_is_the_distance_over_340_metres_a_second() {
    // 34 m ahead: 0.1 s, 4410 samples, and the same level both sides.
    let mut k = doppler(&[("xM", 0.0), ("yM", 34.0)]);
    let (l, r) = run(&mut k, &click(8_000), &click(8_000));
    assert_eq!(loudest(&l), 4_410);
    assert!((l[4_410] - FRAC_1_SQRT_2).abs() < 1e-3, "{}", l[4_410]);
    assert!((r[4_410] - FRAC_1_SQRT_2).abs() < 1e-3, "{}", r[4_410]);
}

#[test]
fn the_bearing_pans_by_sine_and_cosine() {
    // Hard right at x = +100, y = 0; hard left at x = −100.
    let mut k = doppler(&[("xM", 100.0), ("yM", 0.0)]);
    let (l, r) = run(&mut k, &click(20_000), &click(20_000));
    assert!(l.iter().all(|v| v.abs() < 1e-6));
    assert!(r[loudest(&r)] > 0.5);
    let mut k = doppler(&[("xM", -100.0), ("yM", 0.0)]);
    let (l, r) = run(&mut k, &click(20_000), &click(20_000));
    assert!(r.iter().all(|v| v.abs() < 1e-6));
    assert!(l[loudest(&l)] > 0.5);
}

#[test]
fn both_inputs_are_summed_to_one_line() {
    let mut k = doppler(&[("xM", 0.0), ("yM", 34.0)]);
    let (l, r) = run(&mut k, &click(8_000), &vec![0.0; 8_000]);
    assert!((l[4_410] - 0.5 * FRAC_1_SQRT_2).abs() < 1e-3);
    assert!((r[4_410] - 0.5 * FRAC_1_SQRT_2).abs() < 1e-3);
}

#[test]
fn the_delay_follows_its_target_by_a_44th_every_sample() {
    // With no smoothing the position lands at once; the delay then moves
    // (target − delay) / 44 a sample, recomputed every second sample.
    const K: f32 = 1e-5;
    let ramp: Vec<f32> = (0..40_000).map(|i| i as f32 * K).collect();
    let mut k = doppler(&[("xM", 0.0), ("yM", 34.0), ("smoothing", 0.0)]);
    let n0 = 20_000;
    run(&mut k, &ramp[..n0], &ramp[..n0]);
    set(&mut k, "yM", 68.0);
    let (l, _) = run(&mut k, &ramp[n0..n0 + 2_000], &ramp[n0..n0 + 2_000]);
    let target = (44_100.0f64 * (34.0f64 / 340.0 * 2.0)) as f32;
    let mut d = (44_100.0f64 * (34.0f64 / 340.0)) as f32;
    let mut inc = 0.0f32;
    for (i, v) in l.iter().enumerate() {
        // The update runs on odd samples counted from the start.
        if (n0 + i) % 2 == 1 {
            inc = (target - d) / 44.0;
        }
        let measured = (n0 + i) as f32 - v / (FRAC_1_SQRT_2 * K);
        assert!((measured - d).abs() < 0.1, "sample {i}: {measured} against {d}");
        d += inc;
    }
}

#[test]
fn coming_closer_raises_the_pitch() {
    let tone: Vec<f32> = (0..30_000).map(|i| 0.5 * (std::f32::consts::TAU * 1000.0 * i as f32 / SR).sin()).collect();
    let mut k = doppler(&[("xM", 0.0), ("yM", 100.0)]);
    let n0 = 20_000;
    run(&mut k, &tone[..n0], &tone[..n0]);
    set(&mut k, "yM", 10.0);
    let (l, _) = run(&mut k, &tone[n0..n0 + 2_205], &tone[n0..n0 + 2_205]);
    let crossings = l.windows(2).filter(|p| (p[0] < 0.0) != (p[1] < 0.0)).count() as f32;
    assert!(crossings / 2.0 / 0.05 > 1_100.0, "{} Hz", crossings / 2.0 / 0.05);
}

#[test]
fn reset_starts_at_the_position_without_a_sweep() {
    // The original seeded the read from the x position, not the delay.
    let mut k = doppler(&[("xM", 0.0), ("yM", 68.0)]);
    run(&mut k, &vec![0.3; 10_000], &vec![0.3; 10_000]);
    set(&mut k, "yM", 34.0);
    k.reset();
    let (l, _) = run(&mut k, &click(8_000), &click(8_000));
    assert_eq!(loudest(&l), 4_410);
}

#[test]
fn controls() {
    let d = prism_fx::find("hp-doppler").unwrap().descriptor;
    assert_eq!((d.code, d.label, d.source), (45, "Hyperprism Doppler", "Hyperprism-PPC 1.2.1: Doppler"));
    assert_eq!(d.category, prism_fx::Category::Stereo);
    let keys: Vec<&str> = d.params.iter().map(|p| p.key).collect();
    assert_eq!(keys, ["xM", "yM", "smoothing"]);
    let p = |key: &str| d.spec(key).unwrap();
    assert_eq!((p("smoothing").min, p("smoothing").max, p("smoothing").default), (0.0, 100.0, 50.0));
    assert!(d.params.iter().all(|s| !s.log));
    let pad = d.panel.pad.unwrap();
    assert_eq!((pad.x, pad.y), ("xM", "yM"));
}
