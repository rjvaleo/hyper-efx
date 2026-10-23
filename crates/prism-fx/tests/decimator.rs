//! Decimator: sample-rate reduction by hold and bit-depth reduction, with no
//! anti-alias filter, as HYPERPRISM.md describes it.

use std::collections::BTreeSet;

const SR: f32 = 48_000.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

/// A decimator with these settings, glides landed.
fn decimator(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("decimator", SR).expect("decimator is in the catalogue");
    let d = k.descriptor();
    for &(key, value) in settings {
        k.set_param(d.index_of(key).unwrap_or_else(|| panic!("no control {key}")), value);
    }
    k.reset();
    k
}

/// Full-scale noise on the left, a quieter different noise on the right.
fn input(n: usize) -> (Vec<f32>, Vec<f32>) {
    let mut x = 0x9e37_79b9_u32;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    };
    let left = (0..n).map(|_| next()).collect();
    let right = (0..n).map(|_| next() * 0.3).collect();
    (left, right)
}

fn run(k: &mut Boxed, input: &(Vec<f32>, Vec<f32>)) -> (Vec<f32>, Vec<f32>) {
    let (mut left, mut right) = input.clone();
    k.process(&mut left, &mut right);
    (left, right)
}

fn distinct(samples: &[f32]) -> BTreeSet<u32> {
    samples.iter().map(|s| s.to_bits()).collect()
}

#[test]
fn at_full_rate_and_24_bits_the_audio_is_unchanged() {
    let mut k = decimator(&[]);
    let audio = input(48_000);
    assert_eq!(run(&mut k, &audio), audio);
}

#[test]
fn at_one_bit_the_output_has_two_values() {
    let mut k = decimator(&[("bits", 1.0)]);
    let (left, right) = run(&mut k, &input(48_000));
    let values = distinct(&[left, right].concat());
    assert_eq!(values.len(), 2, "{:?}", values.iter().map(|b| f32::from_bits(*b)).collect::<Vec<_>>());
    let v: Vec<f32> = values.iter().map(|b| f32::from_bits(*b)).collect();
    assert_eq!(v[0], -v[1], "the two levels are symmetric: {v:?}");
}

#[test]
fn n_bits_gives_two_to_the_n_levels() {
    for bits in [2.0, 3.0, 4.0, 6.0] {
        let mut k = decimator(&[("bits", bits)]);
        let (left, _) = run(&mut k, &input(48_000));
        assert_eq!(distinct(&left).len(), 1 << bits as u32, "{bits} bits");
    }
}

#[test]
fn fewer_bits_means_more_error() {
    let audio = input(48_000);
    let error = |bits: f32| {
        let mut k = decimator(&[("bits", bits)]);
        let (left, _) = run(&mut k, &audio);
        left.iter().zip(&audio.0).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max)
    };
    let errors: Vec<f32> = [16.0, 8.0, 4.0, 2.0].iter().map(|&b| error(b)).collect();
    assert!(errors.windows(2).all(|w| w[0] < w[1]), "{errors:?}");
    // No worse than half a step.
    assert!(errors[1] <= 0.5 / 128.0 + 1e-6, "8 bits: {}", errors[1]);
}

#[test]
fn a_lower_rate_holds_each_sample_unfiltered() {
    for (rate, hold) in [(50.0, 2), (25.0, 4), (12.5, 8)] {
        let mut k = decimator(&[("rate", rate)]);
        let audio = input(4_800);
        let (left, right) = run(&mut k, &audio);
        for i in 0..left.len() {
            let held = i - i % hold;
            // The held value is the input sample itself: nothing smooths the
            // steps, so the aliasing stays in.
            assert_eq!(left[i], audio.0[held], "{rate} %, frame {i}");
            assert_eq!(right[i], audio.1[held], "{rate} %, frame {i}");
        }
    }
}

#[test]
fn a_rate_that_does_not_divide_evenly_still_averages_out() {
    let mut k = decimator(&[("rate", 30.0)]);
    let audio = input(30_000);
    let (left, _) = run(&mut k, &audio);
    let changes = left.windows(2).filter(|w| w[0] != w[1]).count() + 1;
    assert!((changes as i64 - 9_000).abs() <= 2, "{changes} new samples in 30 000 frames");
}

#[test]
fn mix_blends_the_crushed_signal_with_the_dry_one() {
    let audio = input(4_800);
    let crushed = run(&mut decimator(&[("rate", 10.0), ("bits", 3.0)]), &audio);
    let dry = run(&mut decimator(&[("rate", 10.0), ("bits", 3.0), ("mix", 0.0)]), &audio);
    assert_eq!(dry, audio);
    let half = run(&mut decimator(&[("rate", 10.0), ("bits", 3.0), ("mix", 0.5)]), &audio);
    for i in 0..audio.0.len() {
        let expected = 0.5 * (audio.0[i] + crushed.0[i]);
        assert!((half.0[i] - expected).abs() < 1e-6, "frame {i}");
    }
}
