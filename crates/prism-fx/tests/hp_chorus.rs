//! Hyperprism Chorus (ChorusWide): a 0–depth modulated delay, the right LFO a
//! quarter cycle ahead, and fixed taps 1020 and 980 frames back.

use std::f64::consts::PI;

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn chorus(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-chorus", SR).expect("hp-chorus is in the catalogue");
    for &(key, value) in settings {
        set(&mut k, key, value);
    }
    k.reset();
    k
}

fn set(k: &mut Boxed, key: &str, value: f32) {
    let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
    k.set_param(i, value);
}

fn run(k: &mut Boxed, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut a, mut b) = (l.to_vec(), r.to_vec());
    k.process(&mut a, &mut b);
    (a, b)
}

fn click(n: usize, at: usize) -> Vec<f32> {
    let mut x = vec![0.0; n];
    x[at] = 1.0;
    x
}

/// Every sample that is not (near) zero, as (index, value).
fn nonzero(y: &[f32]) -> Vec<(usize, f32)> {
    y.iter().enumerate().filter(|(_, v)| v.abs() > 1e-6).map(|(i, v)| (i, *v)).collect()
}

fn assert_taps(y: &[f32], expected: &[(usize, f32)], what: &str) {
    let got = nonzero(y);
    assert_eq!(got.len(), expected.len(), "{what}: {got:?}");
    for (&(i, v), &(j, w)) in got.iter().zip(expected) {
        assert!(i == j && (v - w).abs() < 1e-6, "{what}: {got:?} against {expected:?}");
    }
}

#[test]
fn a_left_click_comes_back_on_the_right_1020_frames_later() {
    // Depth 0: the modulated tap is this sample, so a click comes out whole on
    // its own side. The left line's fixed tap (1020 frames) is mixed into the
    // right output at wet level and fed back into the left line at 0.25.
    let n = 3_200;
    let mut k = chorus(&[("depth", 0.0)]);
    let (l, r) = run(&mut k, &click(n, 0), &vec![0.0; n]);
    assert_taps(&l, &[(0, 1.0), (1020, 0.125), (2040, 0.031_25), (3060, 0.007_812_5)], "left");
    assert_taps(&r, &[(1020, 0.5), (2040, 0.125), (3060, 0.031_25)], "right");
}

#[test]
fn a_right_click_comes_back_on_the_left_980_frames_later() {
    let n = 3_000;
    let mut k = chorus(&[("depth", 0.0)]);
    let (l, r) = run(&mut k, &vec![0.0; n], &click(n, 0));
    assert_taps(&r, &[(0, 1.0), (980, 0.125), (1960, 0.031_25), (2940, 0.007_812_5)], "right");
    assert_taps(&l, &[(980, 0.5), (1960, 0.125), (2940, 0.031_25)], "left");
}

#[test]
fn the_980_frame_tap_stays_on_the_right_line() {
    // Changed from the original: its 980-frame tap started on a right slot
    // but wrapped to the buffer's first (left) slot, so after 980 frames it
    // read the left line — the left output echoed itself and the left fed the
    // right line. Fixed, a left click long after the start reaches only the
    // right output, 1020 frames on.
    let n = 6_000;
    let mut k = chorus(&[("depth", 0.0)]);
    let (l, r) = run(&mut k, &click(n, 3_000), &vec![0.0; n]);
    assert_taps(&l, &[(3000, 1.0), (4020, 0.125), (5040, 0.031_25)], "left");
    assert_taps(&r, &[(4020, 0.5), (5040, 0.125)], "right");
}

#[test]
fn both_lines_feed_their_fixed_tap_back_at_a_quarter() {
    // Changed from the original: InitializeChorus set both coefficients to
    // 0.25 and then overwrote the left one with 0; the left line's echo
    // recirculates like the right's (the 0.125 at 1020 and 2040 above).
    let n = 2_100;
    let mut k = chorus(&[("depth", 0.0)]);
    let (l, _) = run(&mut k, &click(n, 0), &vec![0.0; n]);
    assert!((l[1020] - 0.125).abs() < 1e-6, "{}", l[1020]);
}

/// The modulated delay on each side, read from a ramp on that side alone
/// before any fixed tap has come round: y = 0.5·(n − d) + 0.5·n.
fn modulated_delays(k: impl Fn() -> Boxed, change: Option<(usize, f32)>) -> (Vec<f64>, Vec<f64>) {
    let n = 960;
    let ramp: Vec<f32> = (0..n).map(|i| i as f32).collect();
    let silent = vec![0.0; n];
    let side = |left: bool| {
        let mut kernel = k();
        let (l, r) = if left { (&ramp, &silent) } else { (&silent, &ramp) };
        let (y, z) = match change {
            None => run(&mut kernel, l, r),
            Some((at, hz)) => {
                let (mut y, mut z) = run(&mut kernel, &l[..at], &r[..at]);
                set(&mut kernel, "rateHz", hz);
                let (y2, z2) = run(&mut kernel, &l[at..], &r[at..]);
                y.extend(y2);
                z.extend(z2);
                (y, z)
            }
        };
        let out = if left { y } else { z };
        (0..n).map(|i| 2.0 * (i as f64 - out[i] as f64)).collect::<Vec<f64>>()
    };
    (side(true), side(false))
}

/// The LFO phase, in table units, that gives this delay on the rising half.
fn phase_of(delay: f64, depth: f64) -> f64 {
    (1.0 - 2.0 * delay / depth).clamp(-1.0, 1.0).acos() * 1024.0 / (2.0 * PI)
}

#[test]
fn the_delay_sweeps_from_zero_to_the_depth_with_the_right_a_quarter_ahead() {
    let (hz, depth) = (4.0f64, 100.0f64);
    let (dl, dr) = modulated_delays(|| chorus(&[("rateHz", hz as f32), ("depth", depth as f32)]), None);
    let inc = hz * 1024.0 / SR as f64;
    let shape = |ph: f64| depth * 0.5 * (1.0 - (2.0 * PI * ph / 1024.0).cos());
    for i in 200..960 {
        assert!((dl[i] - shape(i as f64 * inc)).abs() < 0.05, "left, sample {i}: {} vs {}", dl[i], shape(i as f64 * inc));
        assert!((dr[i] - shape(256.0 + i as f64 * inc)).abs() < 0.05, "right, sample {i}");
    }
}

#[test]
fn the_quarter_cycle_holds_while_the_rate_moves() {
    // Changed from the original: its stereo loop stepped the right LFO with
    // the rate as it stood at the start of the host block, while the left
    // followed every smoothing step, so the two drifted apart as the rate
    // glided. Fixed, both step together.
    let depth = 100.0;
    let make = || chorus(&[("rateHz", 4.0), ("depth", depth as f32), ("smoothing", 0.0)]);
    let (dl, dr) = modulated_delays(make, Some((150, 0.5)));
    for i in 300..960 {
        let (pl, pr) = (phase_of(dl[i], depth), phase_of(dr[i], depth));
        assert!((pr - pl - 256.0).abs() < 0.5, "sample {i}: left {pl}, right {pr}");
    }
    // And the rate did change: 359 samples at 0.5 Hz are 4.2 table steps
    // (at 4 Hz they would be 33).
    let moved = phase_of(dl[959], depth) - phase_of(dl[600], depth);
    assert!((moved - 359.0 * 0.5 * 1024.0 / 44_100.0).abs() < 0.2, "{moved}");
}

#[test]
fn a_mono_source_takes_the_originals_mono_loop() {
    // One line: its 1020-frame tap fed back at 0.25 and mixed in at wet level,
    // the same on both outputs.
    let n = 2_100;
    let mut k = chorus(&[("depth", 0.0), ("input", 1.0)]);
    let (l, r) = run(&mut k, &click(n, 0), &vec![0.3; n]);
    assert_eq!(l, r);
    assert_taps(&l, &[(0, 1.0), (1020, 0.625), (2040, 0.156_25)], "mono");
}

#[test]
fn the_ranges_are_the_originals() {
    let d = prism_fx::find("hp-chorus").unwrap().descriptor;
    let rate = d.spec("rateHz").unwrap();
    let depth = d.spec("depth").unwrap();
    assert!(!rate.log && !depth.log);
    assert_eq!((rate.min, rate.max, rate.default), (0.0, 4.0, 2.0));
    assert_eq!((depth.min, depth.max, depth.default), (0.0, 100.0, 0.0));
    assert_eq!(d.label, "Hyperprism Chorus");
    assert_eq!(d.source, "Hyperprism-PPC 1.2.1: Chorus");
}
