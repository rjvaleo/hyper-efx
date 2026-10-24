//! Hyperprism Hyper Stereo (EHyperStereo process 23, which 1.2.1's menu cannot
//! reach), as its code does it: mid and side gains on an L/R or M/S input.

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn hyper(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-hyper-stereo", SR).expect("hp-hyper-stereo is in the catalogue");
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

fn pc(v: f32) -> f32 {
    (0.01 * v as f64) as f32
}

#[test]
fn both_gains_start_at_zero_so_it_starts_silent() {
    let (l, r) = run(&mut hyper(&[]), &noise(256, 1), &noise(256, 2));
    assert!(l.iter().chain(&r).all(|s| *s == 0.0));
}

#[test]
fn from_left_and_right_it_scales_mid_and_side() {
    let (x, y) = (noise(1_024, 3), noise(1_024, 4));
    let (l, r) = run(&mut hyper(&[("midGain", 100.0), ("sideGain", 0.0)]), &x, &y);
    for i in 0..x.len() {
        let m = (0.5 * (x[i] + y[i]) as f64) as f32;
        assert!(l[i] == m && r[i] == m, "mid alone is the mono sum, frame {i}");
    }
    let (l, r) = run(&mut hyper(&[("midGain", 40.0), ("sideGain", -70.0)]), &x, &y);
    for i in 0..x.len() {
        let (m, s) = ((0.5 * (x[i] + y[i]) as f64) as f32, (0.5 * (x[i] - y[i]) as f64) as f32);
        assert!((l[i] - (pc(40.0) * m + pc(-70.0) * s)).abs() < 1e-6, "frame {i}");
        assert!((r[i] - (pc(40.0) * m - pc(-70.0) * s)).abs() < 1e-6, "frame {i}");
    }
}

#[test]
fn the_side_gain_is_held_to_the_class_limits() {
    let mut k = hyper(&[]);
    let i = k.descriptor().index_of("sideGain").unwrap();
    k.set_param(i, 100.0);
    assert_eq!(k.param(i), 90.0);
}

#[test]
fn from_mid_and_side_it_decodes() {
    let (m, s) = (noise(512, 5), noise(512, 6));
    let (l, r) = run(&mut hyper(&[("midGain", 100.0), ("sideGain", 50.0), ("input", 1.0)]), &m, &s);
    // M/S in: L = M + ½S, R = M − ½S.
    for i in 0..512 {
        assert!((l[i] - (m[i] + 0.5 * s[i])).abs() < 1e-6 && (r[i] - (m[i] - 0.5 * s[i])).abs() < 1e-6);
    }
}

#[test]
fn moves_glide_in_steps_of_forty_five_samples() {
    let mut k = hyper(&[("smoothing", 0.0)]);
    let i = k.descriptor().index_of("midGain").unwrap();
    k.set_param(i, 100.0);
    let (l, _) = run(&mut k, &[1.0; 50], &[1.0; 50]);
    assert!(l[..44].iter().all(|s| *s == 0.0));
    assert!((l[44] - 1.0).abs() < 1e-7);
}

#[test]
fn a_mono_input_passes_unchanged() {
    let x = noise(256, 7);
    let (l, r) = run(&mut hyper(&[("mono", 1.0), ("midGain", 30.0)]), &x, &[0.5; 256]);
    assert_eq!(l, x);
    assert_eq!(r, x);
}
