//! Hyperprism Stereo Dynamics (EDynamics process 20), as its code does it:
//! X is the right channel's gain and Y the left's, linear, whatever the
//! window's captions say.

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn dynamics(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-stereo-dynamics", SR).expect("hp-stereo-dynamics is in the catalogue");
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

fn gain(percent: f32) -> f32 {
    (percent as f64 * 0.01f32 as f64) as f32
}

#[test]
fn x_is_the_right_gain_and_y_the_left_with_no_mid_or_side() {
    let x: Vec<f32> = (0..512).map(|i| (i as f32 * 0.37).sin()).collect();
    let y: Vec<f32> = (0..512).map(|i| (i as f32 * 0.11).cos()).collect();
    let k = dynamics(&[]);
    let pad = k.descriptor().panel.pad.unwrap();
    assert_eq!((pad.x, pad.y), ("rightGain", "leftGain"));
    let (l, r) = run(&mut dynamics(&[("rightGain", 30.0), ("leftGain", 80.0)]), &x, &y);
    for i in 0..x.len() {
        assert!((l[i] - x[i] * gain(80.0)).abs() < 1e-7, "frame {i}");
        assert!((r[i] - y[i] * gain(30.0)).abs() < 1e-7, "frame {i}");
    }
    let (_, r) = run(&mut dynamics(&[("rightGain", 100.0)]), &[1.0; 32], &[0.0; 32]);
    assert!(r.iter().all(|s| *s == 0.0), "the left never reaches the right");
}

#[test]
fn both_start_at_half_gain() {
    let (l, r) = run(&mut dynamics(&[]), &[1.0; 8], &[1.0; 8]);
    assert!(l.iter().chain(&r).all(|s| (*s - gain(50.0)).abs() < 1e-7));
}

#[test]
fn moves_glide_in_steps_of_forty_five_samples() {
    let mut k = dynamics(&[("smoothing", 50.0)]);
    let i = k.descriptor().index_of("leftGain").unwrap();
    k.set_param(i, 100.0);
    let (l, _) = run(&mut k, &[1.0; 100], &[1.0; 100]);
    let half = gain(50.0);
    assert!(l[..44].iter().all(|s| *s == half));
    let step = 0.01f32 * gain(100.0) + (1.0 - 0.01f64) as f32 * half;
    assert!((l[44] - step).abs() < 1e-7, "{}", l[44]);
    assert!(l[44..89].iter().all(|s| *s == l[44]) && l[89] > l[88]);
}
