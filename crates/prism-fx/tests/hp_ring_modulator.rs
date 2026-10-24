//! Hyperprism Ring Modulator: the RingMod routine over a sine table.

use std::f64::consts::PI;

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn ring(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-ring-modulator", SR).expect("hp-ring-modulator is in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.reset();
    k
}

/// The carrier's phase at each sample, in single precision as the original
/// keeps it.
fn phases(hz: f32, n: usize) -> Vec<f64> {
    let inc = hz * 1024.0 / SR;
    let mut ph = 0.0f32;
    (0..n)
        .map(|_| {
            let now = ph as f64;
            ph += inc;
            if ph >= 1024.0 {
                ph -= 1024.0;
            }
            now
        })
        .collect()
}

fn run(k: &mut Boxed, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut a, mut b) = (l.to_vec(), r.to_vec());
    k.process(&mut a, &mut b);
    (a, b)
}

#[test]
fn the_output_is_dry_times_one_minus_depth_plus_ring_times_depth() {
    let (hz, depth) = (440.0, 30.0);
    let mut k = ring(&[("rateHz", hz), ("depth", depth)]);
    let n = 22_050;
    let l: Vec<f32> = (0..n).map(|i| 0.6 * ((i as f32) * 0.003).cos()).collect();
    let r: Vec<f32> = (0..n).map(|i| -0.4 * ((i as f32) * 0.007).sin()).collect();
    let (yl, yr) = run(&mut k, &l, &r);
    let ph = phases(hz, n);
    let d = depth as f64 * 0.01;
    for i in 0..n {
        let s = (2.0 * PI * ph[i] / 1024.0).sin();
        let g = (1.0 - d) + d * s;
        assert!((yl[i] as f64 - l[i] as f64 * g).abs() < 1e-4, "left, sample {i}");
        assert!((yr[i] as f64 - r[i] as f64 * g).abs() < 1e-4, "right, sample {i}");
    }
}

#[test]
fn at_full_depth_it_is_a_pure_ring_modulator() {
    let mut k = ring(&[("rateHz", 1_000.0), ("depth", 100.0)]);
    let x = vec![1.0; 4410];
    let (y, _) = run(&mut k, &x, &x);
    let ph = phases(1_000.0, y.len());
    for (i, &v) in y.iter().enumerate() {
        let s = (2.0 * PI * ph[i] / 1024.0).sin();
        assert!((v as f64 - s).abs() < 1e-4, "sample {i}");
    }
}

#[test]
fn stereo_does_not_invert_the_dry_sound() {
    // Changed from the original: its stereo loop multiplied the dry sound by
    // (d − 1), not (1 − d). At depth 0 that turned the sound upside down.
    let mut k = ring(&[("rateHz", 2_000.0), ("depth", 0.0)]);
    let x: Vec<f32> = (0..1000).map(|i| ((i as f32) * 0.05).sin()).collect();
    let (l, r) = run(&mut k, &x, &x);
    for i in 0..x.len() {
        assert!((r[i] - x[i]).abs() < 1e-6 && (l[i] - x[i]).abs() < 1e-6, "sample {i}");
    }
}

#[test]
fn the_carrier_is_exactly_its_frequency() {
    // Changed from the original: the 1023 wrap made the carrier 1024/1023
    // fast — 440.43 Hz for 440. Ten seconds at 440 Hz is 4400 cycles.
    let mut k = ring(&[("rateHz", 440.0), ("depth", 100.0)]);
    let x = vec![1.0; 441_000];
    let (y, _) = run(&mut k, &x, &x);
    let rising = y.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
    assert!((rising as i64 - 4400).abs() <= 1, "{rising} cycles");
}

#[test]
fn the_ranges_are_the_originals() {
    let d = prism_fx::find("hp-ring-modulator").unwrap().descriptor;
    let rate = d.spec("rateHz").unwrap();
    let depth = d.spec("depth").unwrap();
    assert!(!rate.log && !depth.log);
    assert_eq!((rate.min, rate.max, rate.default), (0.0, 4000.0, 2000.0));
    assert_eq!((depth.min, depth.max, depth.default), (0.0, 100.0, 0.0));
    assert_eq!(d.label, "Hyperprism Ring Modulator");
}
