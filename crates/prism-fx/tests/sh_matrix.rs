//! ++matrix: SoundHack Freesound's mid/side encoder and decoder, as its code
//! does it.

const SR: f32 = 48_000.0;

type Boxed = Box<dyn prism_fx::DynKernel>;
type Stereo = (Vec<f32>, Vec<f32>);

fn matrix(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("sh-matrix", SR).expect("sh-matrix is in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.reset();
    k
}

fn run(k: &mut Boxed, x: &Stereo) -> Stereo {
    let (mut l, mut r) = x.clone();
    k.process(&mut l, &mut r);
    (l, r)
}

fn signal(scale: f32) -> Stereo {
    let mut x = 0x9e37_79b9u32;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        ((x as f32 / u32::MAX as f32) * 2.0 - 1.0) * scale
    };
    let l: Vec<f32> = (0..4_096).map(|_| next()).collect();
    let r: Vec<f32> = (0..4_096).map(|_| next()).collect();
    (l, r)
}

const ENCODE: f32 = 0.0;
const DECODE: f32 = 1.0;

#[test]
fn encoding_makes_mid_and_side() {
    let x = signal(0.8);
    let y = run(&mut matrix(&[("direction", ENCODE)]), &x);
    for i in 0..x.0.len() {
        assert!((y.0[i] - 0.5 * (x.0[i] + x.1[i])).abs() < 1e-6);
        assert!((y.1[i] - 0.5 * (x.0[i] - x.1[i])).abs() < 1e-6);
    }
}

#[test]
fn decoding_makes_left_and_right() {
    let x = signal(0.6);
    let y = run(&mut matrix(&[("direction", DECODE)]), &x);
    for i in 0..x.0.len() {
        assert!((y.0[i] - (x.0[i] + x.1[i])).abs() < 1e-6);
        assert!((y.1[i] - (x.0[i] - x.1[i])).abs() < 1e-6);
    }
}

#[test]
fn encoding_then_decoding_gives_the_sound_back() {
    let x = signal(0.6);
    let m = run(&mut matrix(&[("direction", ENCODE)]), &x);
    let y = run(&mut matrix(&[("direction", DECODE)]), &m);
    for i in 0..x.0.len() {
        assert!((y.0[i] - x.0[i]).abs() < 1e-6 && (y.1[i] - x.1[i]).abs() < 1e-6);
    }
}

#[test]
fn mid_and_side_gains_scale_their_own_part() {
    let x = signal(0.5);
    let y = run(&mut matrix(&[("direction", ENCODE), ("midDb", -6.0206), ("sideDb", 6.0206)]), &x);
    for i in 0..x.0.len() {
        assert!((y.0[i] - 0.25 * (x.0[i] + x.1[i])).abs() < 1e-5);
        assert!((y.1[i] - (x.0[i] - x.1[i])).abs() < 1e-5);
    }
}

#[test]
fn decoding_clips_at_one_and_a_half_and_encoding_does_not() {
    let loud = (vec![2.0; 1_024], vec![0.0; 1_024]);
    let enc = run(&mut matrix(&[("direction", ENCODE)]), &loud);
    assert!(enc.0.iter().all(|s| (*s - 1.0).abs() < 1e-6));
    let dec = run(&mut matrix(&[("direction", DECODE)]), &loud);
    assert!(dec.0.iter().all(|s| *s == 1.5) && dec.1.iter().all(|s| *s == 1.5));
    let enc = run(&mut matrix(&[("direction", ENCODE), ("inDb", 12.0)]), &loud);
    assert!(enc.0.iter().all(|s| *s > 3.9), "encode is never clipped");
}
