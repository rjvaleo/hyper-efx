//! Hyperprism Frequency Modulator: the input frequency-modulates a
//! raised-cosine oscillator whose level follows the input's envelope.

use std::f64::consts::PI;

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn fm(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-frequency-modulator", SR).expect("hp-frequency-modulator is in the catalogue");
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

fn one_m_cos(phase: f64) -> f64 {
    0.5 * (1.0 - (2.0 * PI * phase / 1024.0).cos())
}

fn rising_through(y: &[f32], level: f32) -> usize {
    y.windows(2).filter(|w| w[0] < level && w[1] >= level).count()
}

#[test]
fn the_output_is_1_4_times_the_envelope_times_the_carrier() {
    let mut k = fm(&[("rateHz", 500.0), ("depth", 0.0)]);
    let n = 20_000;
    let x = vec![0.5; n];
    let (y, _) = run(&mut k, &x, &x);
    // The phase in single precision, as the original keeps it.
    let inc = 500.0f32 * 1024.0 / SR;
    let mut ph = 0.0f32;
    for (i, &got) in y.iter().enumerate() {
        let env = 0.5 * (1.0 - 0.999f64.powi(i as i32 + 1));
        let expected = 1.4 * env * one_m_cos(ph as f64);
        ph += inc;
        if ph >= 1024.0 {
            ph -= 1024.0;
        }
        assert!((got as f64 - expected).abs() < 1e-4, "sample {i}: {got} vs {expected}");
    }
}

#[test]
fn the_input_is_not_passed_through() {
    // At rate 0 and depth 0 the carrier sits at the bottom of its table.
    let mut k = fm(&[("rateHz", 0.0), ("depth", 0.0)]);
    let x: Vec<f32> = (0..4000).map(|i| ((i as f32) * 0.03).sin()).collect();
    let (l, r) = run(&mut k, &x, &x);
    assert!(l.iter().chain(&r).all(|&v| v == 0.0));
}

#[test]
fn the_input_moves_the_carrier_frequency() {
    // Depth 10 % is 1e-3 table steps per 16-bit unit: a steady 0.5 (16 384)
    // adds 16.384 steps a sample, 705.6 Hz.
    let mut k = fm(&[("rateHz", 0.0), ("depth", 10.0)]);
    let n = 10_000 + 44_100;
    let x = vec![0.5; n];
    let (y, _) = run(&mut k, &x, &x);
    let cycles = rising_through(&y[10_000..], 0.35);
    assert!((cycles as i64 - 706).abs() <= 1, "{cycles} cycles in a second");
}

#[test]
fn the_carrier_is_exactly_its_frequency() {
    // Changed from the original: the phase wrapped at 1023, not 1024, running
    // the carrier 1024/1023 fast, and its last table step read one past the
    // table. Ten seconds at 1000 Hz is 10 000 cycles, not 10 009.8.
    let mut k = fm(&[("rateHz", 1_000.0), ("depth", 0.0)]);
    let n = 10_000 + 441_000;
    let x = vec![0.5; n];
    let (y, _) = run(&mut k, &x, &x);
    let cycles = rising_through(&y[10_000..], 0.35);
    assert!((cycles as i64 - 10_000).abs() <= 1, "{cycles} cycles");
}

#[test]
fn the_ranges_are_the_originals() {
    let d = prism_fx::find("hp-frequency-modulator").unwrap().descriptor;
    let rate = d.spec("rateHz").unwrap();
    let depth = d.spec("depth").unwrap();
    assert!(!rate.log && !depth.log);
    assert_eq!((rate.min, rate.max, rate.default), (0.0, 1000.0, 500.0));
    assert_eq!((depth.min, depth.max, depth.default), (0.0, 10.0, 0.0));
    assert_eq!(d.label, "Hyperprism Frequency Modulator");
}
