//! Hyperprism Sonic Decimator (ESonicDecimator, process 38), as its code does
//! it: a sample-and-hold on a fractional counter, and an arithmetic-shift
//! floor in 16-bit integers, crossfaded by the fractional bit.

const SR: f32 = 44_100.0;
/// Full scale: a float ±1 is ±32768 in the original's 16-bit units.
const FULL: f32 = 32_768.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn decimator(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-sonic-decimator", SR).expect("hp-sonic-decimator is in the catalogue");
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
            (x as f32 / u32::MAX as f32) * 1.8 - 0.9
        })
        .collect()
}

/// Floor to `k` bits of a 16-bit word.
fn floor_bits(x: f32, k: i32) -> f32 {
    let v = (x * FULL) as i32;
    let sh = 16 - k;
    (((v >> sh) << sh) as f32) / FULL
}

#[test]
fn at_full_rate_and_sixteen_bits_it_truncates_to_the_word() {
    let x = noise(4_096, 1);
    let (l, _) = run(&mut decimator(&[("rate", 100.0), ("bits", 16.0)]), &x, &x);
    assert_eq!(l[0], 0.0, "the first sample is the initial hold");
    for i in 1..x.len() {
        assert_eq!(l[i], (x[i] * FULL).trunc() / FULL, "frame {i}");
    }
}

#[test]
fn bits_floor_with_zero_as_a_level() {
    let x = [-0.01f32, 0.1, 0.13, -0.5, 0.99, 0.0];
    let mut held = vec![0.0f32];
    held.extend_from_slice(&x);
    let (l, _) = run(&mut decimator(&[("rate", 100.0), ("bits", 4.0)]), &held, &held);
    assert_eq!(&l[1..], &[-0.125, 0.0, 0.125, -0.5, 0.875, 0.0]);
    let (l, _) = run(&mut decimator(&[("rate", 100.0), ("bits", 1.0)]), &held, &held);
    assert_eq!(&l[1..], &[-1.0, 0.0, 0.0, -1.0, 0.0, 0.0]);
}

#[test]
fn a_fractional_bit_crossfades_the_two_floors() {
    let x = noise(2_048, 2);
    for bits in [3.25f32, 7.5, 12.8, 15.9] {
        let (l, _) = run(&mut decimator(&[("rate", 100.0), ("bits", bits)]), &x, &x);
        let k = bits as i32;
        for i in 1..x.len() {
            let (a, b) = (floor_bits(x[i], k), floor_bits(x[i], k + 1));
            let expect = a + (b - a) * (bits - k as f32);
            assert!((l[i] - expect).abs() < 1e-6, "{bits} bits, frame {i}: {} vs {expect}", l[i]);
        }
    }
}

#[test]
fn the_rate_holds_each_sample_for_one_over_the_rate() {
    let x: Vec<f32> = (0..64).map(|i| i as f32 / 64.0).collect();
    let (l, _) = run(&mut decimator(&[("rate", 50.0), ("bits", 16.0)]), &x, &x);
    assert_eq!((l[0], l[1]), (0.0, 0.0));
    for k in 1..32 {
        assert_eq!(l[2 * k], x[2 * k]);
        assert_eq!(l[2 * k + 1], x[2 * k]);
    }
    // 30 %: holds of 3 and 4 alternate, 10 a ten-sample stretch.
    let x: Vec<f32> = (0..400).map(|i| (i as f32 + 1.0) / 512.0).collect();
    let (l, _) = run(&mut decimator(&[("rate", 30.0), ("bits", 16.0)]), &x, &x);
    let changes = (1..400).filter(|&i| l[i] != l[i - 1]).count();
    assert!((119..=121).contains(&changes), "{changes}");
    let runs: Vec<usize> = (1..400).filter(|&i| l[i] != l[i - 1]).collect::<Vec<_>>().windows(2).map(|w| w[1] - w[0]).collect();
    assert!(runs.iter().all(|r| *r == 3 || *r == 4), "{runs:?}");
}

#[test]
fn full_scale_saturates_instead_of_wrapping() {
    // The original cast to a short, so +1.0 (32768) wrapped to −32768.
    let x = [0.0f32, 1.0, 1.7, -1.0, -3.0];
    let (l, _) = run(&mut decimator(&[("rate", 100.0), ("bits", 16.0)]), &x, &x);
    assert_eq!(&l[1..], &[32_767.0 / FULL, 32_767.0 / FULL, -1.0, -1.0]);
}

#[test]
fn bit_moves_glide_in_forty_five_sample_steps() {
    let mut k = decimator(&[("rate", 100.0), ("bits", 16.0), ("smoothing", 0.0)]);
    let i = k.descriptor().index_of("bits").unwrap();
    k.set_param(i, 1.0);
    let x = vec![0.3f32; 50];
    let (l, _) = run(&mut k, &x, &x);
    assert!(l[1..44].iter().all(|s| *s == (0.3 * FULL).trunc() / FULL));
    assert_eq!(l[44], 0.0);
}
