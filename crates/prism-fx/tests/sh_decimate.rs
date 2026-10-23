//! ++decimate: SoundHack Freesound's decimator, as its code does it
//! (docs/effects/soundhack.md in MultiGrainLive; the algorithm note from the
//! decompile).

const SR: f32 = 48_000.0;
const FRAME: usize = 256;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn decimate(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("sh-decimate", SR).expect("sh-decimate is in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.reset();
    k
}

fn noise(n: usize, scale: f32) -> Vec<f32> {
    let mut x = 0x2545_f491u32;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            ((x as f32 / u32::MAX as f32) * 2.0 - 1.0) * scale
        })
        .collect()
}

fn run(k: &mut Boxed, x: &[f32]) -> Vec<f32> {
    let (mut l, mut r) = (x.to_vec(), x.to_vec());
    k.process(&mut l, &mut r);
    assert_eq!(l, r, "both sides alike");
    l
}

#[test]
fn it_works_in_frames_of_256_and_says_so() {
    let mut k = decimate(&[]);
    assert_eq!(k.latency_frames() as usize, FRAME);
    let x = noise(4_096, 0.5);
    let y = run(&mut k, &x);
    for i in FRAME..x.len() {
        assert!((y[i] - x[i - FRAME]).abs() < 1e-6, "32 bits and no averaging is the input, a frame late: {i}");
    }
}

#[test]
fn averaging_holds_the_mean_of_each_group() {
    let mut k = decimate(&[("averaging", 4.0)]);
    let x = noise(4_096, 0.5);
    let y = run(&mut k, &x);
    for (i, out) in y.iter().enumerate().skip(FRAME) {
        let start = (i - FRAME) / 4 * 4;
        let mean = x[start..start + 4].iter().sum::<f32>() / 4.0;
        assert!((out - mean).abs() < 1e-5, "frame {i}: {out} against {mean}");
    }
}

#[test]
fn averaging_snaps_to_powers_of_two() {
    // Five is nearer four than eight on SoundHack's table: it averages fours.
    let x = noise(2_048, 0.5);
    let four = run(&mut decimate(&[("averaging", 4.0)]), &x);
    let five = run(&mut decimate(&[("averaging", 5.0)]), &x);
    assert_eq!(four, five);
    let six = run(&mut decimate(&[("averaging", 6.0)]), &x);
    assert_ne!(four, six, "six is past the midpoint: eights");
}

#[test]
fn one_bit_is_the_sign_rounded_down() {
    // Masking keeps the top bit: −1 for anything negative, 0 otherwise.
    let x = noise(4_096, 0.9);
    let y = run(&mut decimate(&[("depth", 1.0)]), &x);
    for i in FRAME..x.len() {
        let want = if x[i - FRAME] < 0.0 { -1.0 } else { 0.0 };
        assert_eq!(y[i], want, "frame {i}");
    }
}

#[test]
fn bits_round_toward_minus_infinity() {
    // Four bits: steps of 1/8, and every value goes down to the step below.
    let x = noise(4_096, 0.9);
    let y = run(&mut decimate(&[("depth", 4.0)]), &x);
    for i in FRAME..x.len() {
        let want = (x[i - FRAME] * 8.0).floor() / 8.0;
        assert!((y[i] - want).abs() < 1e-6, "frame {i}: {} against {want}", y[i]);
    }
}

#[test]
fn a_fractional_depth_blends_toward_the_next_bit() {
    // Halfway between four bits and five: halfway between the two floors,
    // then cut toward zero at the integer step.
    let x = noise(4_096, 0.9);
    let y = run(&mut decimate(&[("depth", 4.5)]), &x);
    for i in FRAME..x.len() {
        let v = x[i - FRAME];
        let lo = (v * 8.0).floor() / 8.0;
        let hi = (v * 16.0).floor() / 16.0;
        let want = lo + 0.5 * (hi - lo);
        assert!((y[i] - want).abs() < 1e-6, "frame {i}: {} against {want}", y[i]);
    }
}

#[test]
fn the_in_gain_drives_the_quantiser_and_the_out_gain_scales_after() {
    let x = noise(4_096, 0.25);
    let driven = run(&mut decimate(&[("inDb", 6.0206)]), &x);
    let quiet = run(&mut decimate(&[("outDb", -6.0206)]), &x);
    for i in FRAME..x.len() {
        assert!((driven[i] - 2.0 * x[i - FRAME]).abs() < 1e-5);
        assert!((quiet[i] - 0.5 * x[i - FRAME]).abs() < 1e-5);
    }
}

#[test]
fn full_scale_does_not_wrap_and_both_sides_are_clipped() {
    // The original wraps a mean of +1 to −1 and clips only the right side.
    // Here a full-scale input stays at full scale, and both sides stop at 1.5.
    let y = run(&mut decimate(&[]), &vec![1.0; 2_048]);
    assert!(y[FRAME..].iter().all(|s| (*s - 1.0).abs() < 1e-6), "{:?}", &y[FRAME..FRAME + 4]);
    let y = run(&mut decimate(&[("inDb", 20.0)]), &vec![0.5; 2_048]);
    assert!(y[FRAME..].iter().all(|s| *s <= 1.5 && *s > 0.9));
}
