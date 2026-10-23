//! Multi Delay: two taps with their own times, one feedback path, and the taps
//! spread apart, as HYPERPRISM.md describes it.

const SR: f32 = 48_000.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn multi(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("multi-delay", SR).expect("multi-delay is in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.reset();
    k
}

/// A click on both sides.
fn click(k: &mut Boxed, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    left[0] = 1.0;
    right[0] = 1.0;
    k.process(&mut left, &mut right);
    (left, right)
}

fn at(ms: f32) -> usize {
    (ms * SR / 1000.0).round() as usize
}

/// Wet only, no feedback: the line and nothing else.
const TAPS_ONLY: &[(&str, f32)] = &[("dry", 0.0), ("wet", 1.0), ("feedback", 0.0)];

#[test]
fn the_two_taps_arrive_at_their_two_times() {
    let mut k = multi(&[TAPS_ONLY, &[("delay1Ms", 100.0), ("delay2Ms", 260.0), ("spread", 0.0)]].concat());
    let (left, right) = click(&mut k, at(400.0));
    let loud: Vec<usize> = (0..left.len()).filter(|&i| left[i].abs() > 1e-3).collect();
    assert_eq!(loud, [at(100.0), at(260.0)]);
    assert_eq!(left, right, "unspread, both sides hear both taps alike");
    assert!((left[at(100.0)] - 0.5).abs() < 1e-3, "each tap is half of each side: {}", left[at(100.0)]);
}

#[test]
fn full_spread_puts_one_tap_on_each_side() {
    let mut k = multi(&[TAPS_ONLY, &[("delay1Ms", 100.0), ("delay2Ms", 260.0), ("spread", 1.0)]].concat());
    let (left, right) = click(&mut k, at(400.0));
    assert!(left[at(100.0)] > 0.99 && right[at(100.0)].abs() < 1e-6, "tap 1 is on the left");
    assert!(right[at(260.0)] > 0.99 && left[at(260.0)].abs() < 1e-6, "tap 2 is on the right");
}

#[test]
fn half_spread_leans_each_tap_its_own_way() {
    let mut k = multi(&[TAPS_ONLY, &[("delay1Ms", 100.0), ("delay2Ms", 260.0), ("spread", 0.5)]].concat());
    let (left, right) = click(&mut k, at(400.0));
    let (l, r) = (left[at(100.0)], right[at(100.0)]);
    assert!(l > r && r > 0.0, "tap 1 leans left: {l} / {r}");
    assert!((l + r - 1.0).abs() < 1e-4, "and keeps its level: {l} + {r}");
}

#[test]
fn feedback_returns_both_taps_through_the_line() {
    // Each pass feeds back the average of the two taps, so the click comes
    // back at every sum of the two times, scaled by the feedback.
    let mut k =
        multi(&[TAPS_ONLY, &[("delay1Ms", 100.0), ("delay2Ms", 260.0), ("spread", 0.0), ("feedback", 0.8)]].concat());
    let (left, _) = click(&mut k, at(700.0));
    // 100 + 100: tap 1 hearing its own first repeat, which went back in at
    // 0.8 × half.
    assert!((left[at(200.0)] - 0.5 * 0.8 * 0.5).abs() < 2e-3, "{}", left[at(200.0)]);
    assert!(left[at(360.0)] > 0.1, "100 + 260 comes back");
    assert!(left[at(520.0)] > 0.05, "260 + 260 comes back");
    assert!(left[at(150.0)].abs() < 1e-6, "nothing between the repeats");
}

#[test]
fn dry_and_wet_set_the_two_levels() {
    let mut k =
        multi(&[("delay1Ms", 50.0), ("delay2Ms", 80.0), ("feedback", 0.0), ("spread", 0.0), ("dry", 0.25), ("wet", 0.5)]);
    let (left, _) = click(&mut k, at(100.0));
    assert_eq!(left[0], 0.25);
    assert!((left[at(50.0)] - 0.25).abs() < 1e-4, "half a tap at half wet: {}", left[at(50.0)]);
}

#[test]
fn a_moved_time_glides_rather_than_clicking() {
    // A steady tone through the taps: moving their times must bend the pitch,
    // like tape, not jump the output.
    let mut k = multi(&[TAPS_ONLY, &[("delay1Ms", 200.0), ("delay2Ms", 200.0), ("spread", 0.0)]].concat());
    let tone: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();
    let (mut l, mut r) = (tone[..24_000].to_vec(), tone[..24_000].to_vec());
    k.process(&mut l, &mut r);
    for key in ["delay1Ms", "delay2Ms"] {
        let i = k.descriptor().index_of(key).unwrap();
        k.set_param(i, 900.0);
    }
    let (mut l2, mut r2) = (tone[24_000..].to_vec(), tone[24_000..].to_vec());
    k.process(&mut l2, &mut r2);
    let mut last = l[23_999];
    let worst = l2.iter().fold(0.0f32, |m, &s| {
        let step = (s - last).abs();
        last = s;
        m.max(step)
    });
    assert!(worst < 0.05, "a step of {worst}");
}

#[test]
fn the_tail_covers_the_repeats() {
    let k = multi(&[("delay1Ms", 500.0), ("delay2Ms", 1_000.0), ("feedback", 0.5)]);
    let expected = 1.0 * (0.001f32.ln() / 0.5f32.ln()).ceil();
    assert!((k.tail_seconds() - expected).abs() < 1e-3, "{}", k.tail_seconds());
    assert_eq!(multi(&[("delay1Ms", 500.0), ("delay2Ms", 1_000.0), ("feedback", 0.0)]).tail_seconds(), 1.0);
}
