//! Hyperprism Multi Delay (1.2.1's EDelay, `DelayMultiTap`): three
//! whole-sample taps at d1, d2 and d1 + d2, each with its own attenuation,
//! and no feedback in the original; Feedback is added, default 0.

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn multi(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-multi-delay", SR).expect("hp-multi-delay is in the catalogue");
    for &(key, value) in settings {
        set(&mut k, key, value);
    }
    k
}

fn set(k: &mut Boxed, key: &str, value: f32) {
    let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
    k.set_param(i, value);
}

fn run(k: &mut Boxed, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (l.to_vec(), r.to_vec());
    k.process(&mut l, &mut r);
    (l, r)
}

fn click(n: usize) -> Vec<f32> {
    let mut x = vec![0.0; n];
    x[0] = 1.0;
    x
}

fn db(att: f64) -> f32 {
    10f64.powf(-att / 20.0) as f32
}

#[test]
fn three_taps_at_d1_d2_and_their_sum_each_attenuated() {
    let mut k = multi(&[("mix", 100.0), ("delay1Ms", 150.0), ("delay2Ms", 250.0), ("att1Db", 6.0), ("att2Db", 12.0), ("att3Db", 18.0)]);
    let (y, _) = run(&mut k, &click(30_000), &click(30_000));
    assert_eq!(y[6_615], db(6.0));
    assert_eq!(y[11_025], db(12.0));
    assert_eq!(y[17_640], db(18.0));
    assert_eq!(y.iter().filter(|v| **v != 0.0).count(), 3, "three taps, no feedback");
}

#[test]
fn mix_sets_wet_and_the_rest_is_dry() {
    let mut k = multi(&[("mix", 25.0), ("delay1Ms", 150.0), ("delay2Ms", 250.0)]);
    let (y, _) = run(&mut k, &click(30_000), &click(30_000));
    assert_eq!(y[0], 0.75);
    assert_eq!(y[6_615], 0.25);
}

#[test]
fn each_side_runs_its_own_line() {
    let mut k = multi(&[("mix", 100.0), ("delay1Ms", 150.0), ("delay2Ms", 250.0)]);
    let (l, r) = run(&mut k, &click(30_000), &vec![0.0; 30_000]);
    assert_eq!(l[6_615], 1.0);
    assert!(r.iter().all(|v| *v == 0.0));
}

#[test]
fn the_taps_glide_once_a_sample_toward_whole_sample_targets() {
    // s ← a·int(t) + b·s, once per sample (EchoTranz runs it twice); at
    // speed 75, a = 1e-5. Clicks every 0.25 s show where the first tap is.
    let mut k = multi(&[("mix", 100.0), ("delay1Ms", 100.0), ("delay2Ms", 1000.0), ("att2Db", 36.0), ("att3Db", 36.0), ("speed", 75.0)]);
    let a = 1e-5f64;
    let b = 1.0 - a;
    let mut s = 4_410.0f64;
    run(&mut k, &vec![0.0; 1_000], &vec![0.0; 1_000]);
    for _ in 0..1_000 {
        s = a.mul_add(4_410.0, b * s);
    }
    set(&mut k, "delay1Ms", 200.0);
    let mut first = Vec::new();
    for round in 0..8 {
        // Each sample updates the tap, then reads int(s) samples back; the
        // first tap meets a click at the block's start where int(s) = i.
        let (y, _) = run(&mut k, &click(22_050), &vec![0.0; 22_050]);
        let taps: Vec<usize> = (0..22_050)
            .map(|_| {
                s = a.mul_add(8_820.0, b * s);
                s as usize
            })
            .collect();
        let at = y.iter().position(|v| *v > 0.5).expect("the first tap reads the click");
        assert_eq!(at, taps[at], "round {round}");
        first.push(at);
    }
    assert!(first.windows(2).all(|w| w[1] > w[0]) && first[7] < 8_820, "{first:?}");
}

#[test]
fn at_the_default_speed_some_taps_settle_one_sample_short() {
    // The original's smoother, a·t + b·s with a + b a hair under one, settles
    // just below some whole-sample targets, and the tap truncates: 100 ms
    // reads 4409 samples back at speed 50. This keeps that arithmetic.
    let mut k = multi(&[("mix", 100.0), ("delay1Ms", 100.0), ("delay2Ms", 1000.0), ("att2Db", 36.0), ("att3Db", 36.0)]);
    run(&mut k, &vec![0.0; 200_000], &vec![0.0; 200_000]);
    let (y, _) = run(&mut k, &click(5_000), &vec![0.0; 5_000]);
    assert_eq!(y[4_409], 1.0);
}

#[test]
fn the_shortest_tap_is_one_sample() {
    let mut k = multi(&[("mix", 100.0), ("delay1Ms", 0.0), ("delay2Ms", 0.0)]);
    let (y, _) = run(&mut k, &click(10), &click(10));
    assert_eq!(y[1], 2.0, "taps 1 and 2 both one sample back");
    assert_eq!(y[2], 1.0, "tap 3 two samples back");
}

#[test]
fn controls_are_the_originals_plus_feedback() {
    let d = prism_fx::find("hp-multi-delay").unwrap().descriptor;
    assert_eq!((d.code, d.label, d.source), (43, "Hyperprism Multi Delay", "Hyperprism-PPC 1.2.1: Multi Delay"));
    assert_eq!(d.category, prism_fx::Category::Delay);
    let keys: Vec<&str> = d.params.iter().map(|p| p.key).collect();
    assert_eq!(keys, ["delay1Ms", "delay2Ms", "att1Db", "att2Db", "att3Db", "mix", "speed", "feedback"]);
    let p = |key: &str| d.spec(key).unwrap();
    for key in ["delay1Ms", "delay2Ms"] {
        assert_eq!((p(key).min, p(key).max, p(key).default), (0.0, 1000.0, 500.0));
    }
    assert_eq!((p("feedback").min, p("feedback").max, p("feedback").default), (0.0, 100.0, 0.0));
    for key in ["att1Db", "att2Db", "att3Db"] {
        assert_eq!((p(key).min, p(key).max, p(key).default), (0.0, 36.0, 0.0));
    }
    assert_eq!((p("mix").min, p("mix").max, p("mix").default), (0.0, 100.0, 50.0));
    assert_eq!((p("speed").min, p("speed").max, p("speed").default), (1.0, 100.0, 50.0));
    assert!(d.params.iter().all(|s| !s.log));
    let pad = d.panel.pad.unwrap();
    assert_eq!((pad.x, pad.x_range, pad.y, pad.y_range), ("delay1Ms", (0.0, 1000.0), "delay2Ms", (0.0, 1000.0)));
}

#[test]
fn feedback_sends_the_taps_round_again_over_their_total_gain() {
    // 150 ms and 250 ms, all taps at 0 dB, 60 % feedback: the line takes
    // in + 0.6·sum/3, so the click's first return (0.2) repeats at 6 615.
    let mut k = multi(&[("mix", 100.0), ("delay1Ms", 150.0), ("delay2Ms", 250.0), ("feedback", 60.0)]);
    let (y, _) = run(&mut k, &click(40_000), &click(40_000));
    assert_eq!(y[6_615], 1.0);
    let regen = (0.01f64 * 60.0) as f32 / 3.0;
    assert_eq!(y[13_230], regen, "tap 1 of the first return through tap 1");
    assert!(y.iter().filter(|v| **v != 0.0).count() > 3, "it repeats");
    assert!(y[20_000..].iter().all(|v| v.abs() < 1.0), "never gains");
}

#[test]
fn feedback_at_zero_is_the_original() {
    let settings = [("mix", 100.0), ("delay1Ms", 150.0), ("delay2Ms", 250.0), ("att1Db", 6.0)];
    let mut plain = multi(&settings);
    let mut zero = multi(&settings);
    set(&mut zero, "feedback", 0.0);
    assert_eq!(run(&mut plain, &click(30_000), &click(30_000)), run(&mut zero, &click(30_000), &click(30_000)));
}

#[test]
fn full_feedback_stays_bounded() {
    let mut k = multi(&[("mix", 100.0), ("delay1Ms", 10.0), ("delay2Ms", 15.0), ("feedback", 100.0)]);
    let (y, _) = run(&mut k, &vec![1.0; 200_000], &vec![1.0; 200_000]);
    assert!(y.iter().all(|v| v.is_finite() && v.abs() <= 12.0));
}
