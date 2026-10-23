//! Stereo Matrix: Hyperprism's four stereo processes — Pan, Stereo Dynamics,
//! More Stereo and MS Matrix — as gains and a rotation on mid and side, as
//! HYPERPRISM.md describes it.

const SR: f32 = 48_000.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

/// Left and right.
type Stereo = (Vec<f32>, Vec<f32>);

fn matrix(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("stereo-matrix", SR).expect("stereo-matrix is in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.reset();
    k
}

fn tone(hz: f32, n: usize) -> Vec<f32> {
    (0..n).map(|i| (std::f32::consts::TAU * hz * i as f32 / SR).sin() * 0.5).collect()
}

/// A stereo signal from a mid and a side.
fn from_mid_side(mid: &[f32], side: &[f32]) -> Stereo {
    (mid.iter().zip(side).map(|(m, s)| m + s).collect(), mid.iter().zip(side).map(|(m, s)| m - s).collect())
}

fn run(k: &mut Boxed, input: &Stereo) -> Stereo {
    let (mut l, mut r) = input.clone();
    k.process(&mut l, &mut r);
    (l, r)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

fn mid_of(s: &Stereo) -> Vec<f32> {
    s.0.iter().zip(&s.1).map(|(l, r)| 0.5 * (l + r)).collect()
}

fn side_of(s: &Stereo) -> Vec<f32> {
    s.0.iter().zip(&s.1).map(|(l, r)| 0.5 * (l - r)).collect()
}

fn stereo() -> Stereo {
    from_mid_side(&tone(220.0, 9_600), &tone(330.0, 9_600).iter().map(|v| v * 0.6).collect::<Vec<_>>())
}

fn close(a: &[f32], b: &[f32], tolerance: f32) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tolerance)
}

#[test]
fn at_its_defaults_the_audio_is_unchanged() {
    let input = stereo();
    let out = run(&mut matrix(&[]), &input);
    assert!(close(&out.0, &input.0, 1e-6) && close(&out.1, &input.1, 1e-6));
}

#[test]
fn rotating_by_ninety_degrees_swaps_mid_and_side() {
    let mid = tone(220.0, 4_800);
    let silence = vec![0.0; 4_800];
    // All mid in: all side out.
    let out = run(&mut matrix(&[("rotate", 90.0)]), &from_mid_side(&mid, &silence));
    assert!(rms(&mid_of(&out)) < 1e-5, "mid left over: {}", rms(&mid_of(&out)));
    assert!(close(&side_of(&out).iter().map(|v| v.abs()).collect::<Vec<_>>(), &mid.iter().map(|v| v.abs()).collect::<Vec<_>>(), 1e-5));
    // All side in: all mid out.
    let out = run(&mut matrix(&[("rotate", 90.0)]), &from_mid_side(&silence, &mid));
    assert!(rms(&side_of(&out)) < 1e-5);
    assert!((rms(&mid_of(&out)) - rms(&mid)).abs() < 1e-5);
}

#[test]
fn no_side_is_mono_and_no_mid_is_out_of_phase() {
    let input = stereo();
    let mono = run(&mut matrix(&[("sideGain", 0.0)]), &input);
    assert!(close(&mono.0, &mono.1, 1e-6), "no side, so both sides are the same");
    assert!(close(&mid_of(&mono), &mid_of(&input), 1e-6), "and the mid is untouched");
    let hollow = run(&mut matrix(&[("midGain", 0.0)]), &input);
    assert!(close(&hollow.0, &hollow.1.iter().map(|v| -v).collect::<Vec<_>>(), 1e-6));
}

#[test]
fn more_side_is_wider_and_leaves_the_mid_alone() {
    let input = stereo();
    let wide = run(&mut matrix(&[("sideGain", 2.0)]), &input);
    assert!((rms(&side_of(&wide)) - 2.0 * rms(&side_of(&input))).abs() < 1e-4);
    assert!(close(&mid_of(&wide), &mid_of(&input), 1e-6));
}

#[test]
fn pan_turns_one_side_down_and_leaves_the_other() {
    let input = stereo();
    let left = run(&mut matrix(&[("pan", -1.0)]), &input);
    assert!(rms(&left.1) < 1e-6, "panned hard left, the right is silent");
    assert!(close(&left.0, &input.0, 1e-6), "and the left is as it was");
    let half = run(&mut matrix(&[("pan", 0.5)]), &input);
    assert!(close(&half.1, &input.1, 1e-6));
    assert!(close(&half.0, &input.0.iter().map(|v| v * 0.5).collect::<Vec<_>>(), 1e-6));
}

#[test]
fn gain_is_in_decibels_down_to_almost_nothing() {
    let input = stereo();
    let half = run(&mut matrix(&[("gainDb", -6.0206)]), &input);
    assert!((rms(&half.0) / rms(&input.0) - 0.5).abs() < 1e-3);
    let gone = run(&mut matrix(&[("gainDb", -96.0)]), &input);
    assert!(rms(&gone.0) < rms(&input.0) * 2e-5);
}

#[test]
fn mono_below_takes_the_low_side_out_and_keeps_the_rest() {
    let n = 24_000;
    let low_side = from_mid_side(&vec![0.0; n], &tone(80.0, n));
    let high_side = from_mid_side(&vec![0.0; n], &tone(5_000.0, n));
    let low_mid = from_mid_side(&tone(80.0, n), &vec![0.0; n]);
    let k = || matrix(&[("monoBelowHz", 1_000.0)]);
    // Skip the filter's first moments, then measure.
    let settled = |s: &Stereo, f: fn(&Stereo) -> Vec<f32>| rms(&f(s)[4_800..]);
    let low = run(&mut k(), &low_side);
    assert!(settled(&low, side_of) < settled(&low_side, side_of) * 0.02, "the bass is still wide");
    let high = run(&mut k(), &high_side);
    assert!(settled(&high, side_of) > settled(&high_side, side_of) * 0.95, "the top was narrowed too");
    let mid = run(&mut k(), &low_mid);
    assert!(close(&mid_of(&mid), &mid_of(&low_mid), 1e-6), "the mid is left alone");
}
