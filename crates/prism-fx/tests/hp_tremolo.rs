//! Hyperprism Tremolo: the RingMod routine over a raised-cosine table.

use std::f64::consts::PI;

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn tremolo(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-tremolo", SR).expect("hp-tremolo is in the catalogue");
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

/// The LFO table's shape at a phase in table units.
fn one_m_cos(phase: f64) -> f64 {
    0.5 * (1.0 - (2.0 * PI * phase / 1024.0).cos())
}

/// The phase at each sample, kept in single precision as the original keeps
/// it, wrapping at `wrap`.
fn phases(hz: f32, n: usize, wrap: f32) -> Vec<f64> {
    let inc = hz * 1024.0 / SR;
    let mut ph = 0.0f32;
    (0..n)
        .map(|_| {
            let now = ph as f64;
            ph += inc;
            if ph >= wrap {
                ph -= wrap;
            }
            now
        })
        .collect()
}

#[test]
fn the_gain_is_one_minus_depth_plus_depth_times_the_raised_cosine() {
    let (hz, depth) = (3.0, 60.0);
    let mut k = tremolo(&[("rateHz", hz), ("depth", depth)]);
    let n = 44_100;
    let l: Vec<f32> = (0..n).map(|i| 0.5 + 0.3 * ((i as f32) * 0.01).sin()).collect();
    let r: Vec<f32> = l.iter().map(|s| -0.7 * s).collect();
    let (yl, yr) = run(&mut k, &l, &r);
    let ph = phases(hz, n, 1024.0);
    let d = depth as f64 * 0.01;
    for i in 0..n {
        let g = 1.0 - d + d * one_m_cos(ph[i]);
        assert!((yl[i] as f64 - l[i] as f64 * g).abs() < 2e-4, "left, sample {i}: {} vs {}", yl[i], l[i] as f64 * g);
        assert!((yr[i] as f64 - r[i] as f64 * g).abs() < 2e-4, "right, sample {i}");
    }
}

#[test]
fn the_cycle_starts_at_the_bottom() {
    let mut k = tremolo(&[("rateHz", 2.0), ("depth", 100.0)]);
    let (y, _) = run(&mut k, &[1.0; 4], &[1.0; 4]);
    assert!(y[0].abs() < 1e-6, "gain 1 − d at the start: {}", y[0]);
}

#[test]
fn stereo_keeps_the_dry_term_positive() {
    // Changed from the original: its stereo loop used (d − 1)·x, so stereo at
    // half depth swung between −0.5 and 0. Fixed, both sides follow the mono law.
    let mut k = tremolo(&[("rateHz", 2.0), ("depth", 50.0)]);
    let x = vec![1.0; 44_100];
    let (l, r) = run(&mut k, &x, &x);
    assert_eq!(l, r);
    assert!(r.iter().all(|&g| (0.5 - 1e-4..=1.0 + 1e-4).contains(&g)), "gain stays in 1 − d … 1");
}

#[test]
fn one_cycle_is_exactly_1024_table_steps() {
    // Changed from the original: it wrapped the phase at 1023 (and read one
    // past the table there), so it ran 1024/1023 fast. At 2 Hz the cycle is
    // 22 050 samples (22 048 once the phase's single-precision steps have
    // added up, as in the original); the original's was 22 028.5.
    let mut k = tremolo(&[("rateHz", 2.0), ("depth", 100.0)]);
    let x = vec![1.0; 40_000];
    let (y, _) = run(&mut k, &x, &x);
    let bottom = (11_025..33_075).min_by(|&a, &b| y[a].total_cmp(&y[b])).unwrap();
    let lowest = |ph: &[f64]| (11_025..33_075).min_by(|&a, &b| one_m_cos(ph[a]).total_cmp(&one_m_cos(ph[b]))).unwrap();
    let at_1024 = lowest(&phases(2.0, 40_000, 1024.0));
    let at_1023 = lowest(&phases(2.0, 40_000, 1023.0));
    assert!((at_1024 as i64 - 22_050).abs() <= 3 && (at_1023 as i64 - 22_028).abs() <= 3, "{at_1024} {at_1023}");
    assert!((bottom as i64 - at_1024 as i64).abs() <= 1, "the next bottom is at {bottom}, not {at_1024}");
    let top = (0..22_050).max_by(|&a, &b| y[a].total_cmp(&y[b])).unwrap();
    assert!((top as i64 - 11_025).abs() <= 1, "the top is at {top}");
}

#[test]
fn smoothing_steps_every_45_samples_with_a_from_the_slider() {
    // a = int(10^(0.04·(100 − s)))·0.0001: s = 0 → 1, 25 → 0.1, 50 → 0.01.
    for (s, a) in [(0.0, 1.0f32), (25.0, 0.1), (50.0, 0.01)] {
        let mut k = tremolo(&[("rateHz", 0.0), ("depth", 0.0), ("smoothing", s)]);
        set(&mut k, "depth", 100.0);
        let x = vec![1.0; 200];
        let (y, _) = run(&mut k, &x, &x);
        // Rate 0 holds the LFO at the bottom, so the gain is 1 − depth.
        let d1 = a;
        let d2 = a + (1.0 - a) * d1;
        let d3 = a + (1.0 - a) * d2;
        for (range, depth) in [(0..44, 0.0), (44..89, d1), (89..134, d2), (134..179, d3)] {
            for i in range {
                assert!((y[i] - (1.0 - depth)).abs() < 1e-5, "s {s}, sample {i}: {} vs {}", y[i], 1.0 - depth);
            }
        }
    }
}

#[test]
fn the_pad_is_linear_and_the_ranges_are_the_originals() {
    let d = prism_fx::find("hp-tremolo").unwrap().descriptor;
    let rate = d.spec("rateHz").unwrap();
    let depth = d.spec("depth").unwrap();
    assert!(!rate.log && !depth.log);
    assert_eq!((rate.min, rate.max, rate.default), (0.0, 4.0, 2.0));
    assert_eq!((depth.min, depth.max, depth.default), (0.0, 100.0, 0.0));
    let s = d.spec("smoothing").unwrap();
    assert_eq!((s.min, s.max, s.default), (0.0, 100.0, 50.0));
    assert_eq!(d.label, "Hyperprism Tremolo");
    assert_eq!(d.source, "Hyperprism-PPC 1.2.1: Tremolo");
}
