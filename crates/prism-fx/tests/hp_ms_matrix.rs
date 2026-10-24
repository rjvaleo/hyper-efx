//! Hyperprism MS Matrix (EHyperStereo process 25), as its code does it: the
//! two input channels taken as mid and side, summed and differenced, with a
//! constant-power balance of the outputs at an overall half gain.

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn matrix(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-ms-matrix", SR).expect("hp-ms-matrix is in the catalogue");
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
            (x as f32 / u32::MAX as f32) * 2.0 - 1.0
        })
        .collect()
}

/// The output gains: half of cos and sin of π/4 (1 + position / 90).
fn gains(position: f32) -> (f32, f32) {
    let q = std::f32::consts::FRAC_PI_4;
    let theta = (q as f64 + (position as f64 * q as f64) as f32 as f64 / 90.0) as f32 as f64;
    ((theta.cos() as f32 as f64 * 0.5) as f32, theta.sin() as f32 * 0.5)
}

#[test]
fn at_its_start_values_an_lr_file_gives_the_right_channel_nine_db_down() {
    let (x, y) = (noise(1_024, 1), noise(1_024, 2));
    let (l, r) = run(&mut matrix(&[]), &x, &y);
    let (ga, gb) = gains(0.0);
    for i in 0..x.len() {
        assert!((l[i] - y[i] * ga).abs() < 1e-6 && (r[i] - y[i] * gb).abs() < 1e-6, "frame {i}");
    }
    assert!((20.0 * ga.log10() - -9.03).abs() < 0.01, "{ga}");
}

#[test]
fn the_matrix_follows_both_buttons_sample_by_sample() {
    let (x, y) = (noise(1_024, 3), noise(1_024, 4));
    let sv = (120.0f64 * 0.01) as f32;
    let (ga, gb) = gains(30.0);
    for input in [0.0, 1.0] {
        for output in [0.0, 1.0] {
            let k = &mut matrix(&[("position", 30.0), ("sideVolume", 120.0), ("input", input), ("output", output)]);
            let (l, r) = run(k, &x, &y);
            for i in 0..x.len() {
                let (s, m) = if input == 0.0 { (x[i] * sv, y[i]) } else { (y[i] * sv, x[i]) };
                let (el, er) = if output == 0.0 { ((s + m) * ga, (m - s) * gb) } else { ((m - s) * ga, (s + m) * gb) };
                assert!((l[i] - el).abs() < 1e-6 && (r[i] - er).abs() < 1e-6, "in {input} out {output} frame {i}");
            }
        }
    }
}

#[test]
fn it_decodes_mid_and_side_back_to_left_and_right() {
    // Side on the right, sum to the left, side at 100 %: (M ± S) × 0.354.
    let (a, b) = (noise(512, 5), noise(512, 6));
    let mid: Vec<f32> = (0..512).map(|i| a[i] + b[i]).collect();
    let side: Vec<f32> = (0..512).map(|i| a[i] - b[i]).collect();
    let (l, r) = run(&mut matrix(&[("sideVolume", 100.0), ("input", 1.0)]), &mid, &side);
    let (ga, _) = gains(0.0);
    for i in 0..512 {
        assert!((l[i] - 2.0 * a[i] * ga).abs() < 1e-5 && (r[i] - 2.0 * b[i] * ga).abs() < 1e-5);
    }
}

#[test]
fn moves_are_not_smoothed() {
    let mut k = matrix(&[]);
    let i = k.descriptor().index_of("position").unwrap();
    k.set_param(i, 90.0);
    let (l, r) = run(&mut k, &[1.0; 4], &[1.0; 4]);
    let (ga, gb) = gains(90.0);
    assert!((l[0] - ga).abs() < 1e-7 && (r[0] - gb).abs() < 1e-7, "{} {}", l[0], r[0]);
    assert!(l[0].abs() < 1e-7, "at 90° the left is silent");
}

#[test]
fn a_mono_input_passes_unchanged() {
    let x = noise(256, 7);
    let (l, r) = run(&mut matrix(&[("mono", 1.0), ("sideVolume", 150.0), ("position", -40.0)]), &x, &[0.5; 256]);
    assert_eq!(l, x);
    assert_eq!(r, x);
}
