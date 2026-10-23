//! Echo: what it does, as MultiGrainLive's own Echo tests say it, plus what the
//! move adds — glides on the level controls, and a tail a host can read.

const SR: f32 = 48_000.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn echo(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("echo", SR).expect("echo is in the catalogue");
    for &(key, value) in settings {
        set(&mut k, key, value);
    }
    k
}

fn set(k: &mut Boxed, key: &str, value: f32) {
    let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
    k.set_param(i, value);
}

/// Mono through both sides; returns the left.
fn run(k: &mut Boxed, input: &[f32]) -> Vec<f32> {
    let (mut left, mut right) = (input.to_vec(), input.to_vec());
    k.process(&mut left, &mut right);
    assert_eq!(left, right, "both sides get the same echo");
    left
}

fn click(n: usize) -> Vec<f32> {
    let mut x = vec![0.0; n];
    x[0] = 1.0;
    x
}

/// Only the delay line: all wet, nothing dry, no darkening.
const LINE_ONLY: &[(&str, f32)] = &[("damping", 0.0), ("dry", 0.0), ("wet", 1.0)];

#[test]
fn a_click_lands_at_the_delay_time_and_repeats() {
    let mut k = echo(&[LINE_ONLY, &[("delayMs", 100.0), ("feedback", 0.5)]].concat());
    let out = run(&mut k, &click(24_000));
    assert!(out[4_799].abs() < 1e-6 && out[4_800] > 0.99);
    assert!((out[9_600] - 0.5).abs() < 0.02);
    assert!((out[14_400] - 0.25).abs() < 0.02);
}

#[test]
fn negative_feedback_flips_each_repeat() {
    let mut k = echo(&[LINE_ONLY, &[("delayMs", 50.0), ("feedback", -0.5)]].concat());
    let out = run(&mut k, &click(12_000));
    assert!(out[2_400] > 0.99);
    assert!((out[4_800] + 0.5).abs() < 0.02);
    assert!((out[7_200] - 0.25).abs() < 0.02);
}

#[test]
fn damping_darkens_each_repeat() {
    // The repeats of a click spread out as damping rises: their peak falls
    // while what they carry stays.
    let peak_of_second = |damping: f32| {
        let mut k = echo(&[("delayMs", 50.0), ("feedback", 0.9), ("damping", damping), ("dry", 0.0), ("wet", 1.0)]);
        let out = run(&mut k, &click(12_000));
        out[4_700..5_400].iter().cloned().fold(0.0, f32::max)
    };
    let (bright, dark) = (peak_of_second(0.0), peak_of_second(0.8));
    assert!(bright > 0.85 && dark < bright * 0.5, "bright {bright}, dark {dark}");
}

#[test]
fn dry_and_wet_set_the_two_levels() {
    let mut k = echo(&[("delayMs", 10.0), ("feedback", 0.0), ("damping", 0.0), ("dry", 0.25), ("wet", 0.75)]);
    let out = run(&mut k, &click(1_000));
    assert_eq!(out[0], 0.25);
    assert!((out[480] - 0.75).abs() < 1e-6);
}

#[test]
fn a_new_echo_starts_at_its_delay_time_without_a_sweep() {
    // Set before the first frame, the time is where the pointer starts: no
    // glide up from zero, and the level controls are at their values at once.
    let mut k = echo(&[LINE_ONLY, &[("delayMs", 300.0), ("feedback", 0.0)]].concat());
    let out = run(&mut k, &click(20_000));
    assert!(out[14_400] > 0.99, "{}", out[14_400]);
    assert!(out[..14_399].iter().all(|s| s.abs() < 1e-6));
}

#[test]
fn moving_the_delay_time_sweeps_and_then_settles() {
    let mut k = echo(&[LINE_ONLY, &[("delayMs", 200.0), ("feedback", 0.0), ("glideMs", 200.0)]].concat());
    let tone: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    run(&mut k, &tone[..24_000]);
    set(&mut k, "delayMs", 400.0);
    let moving = run(&mut k, &tone[24_000..]);
    let crossings = |x: &[f32]| x.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count() as i32;
    let source = crossings(&tone[24_000..33_600]);
    assert!((source - crossings(&moving[..9_600])).abs() > 20, "the delay did not sweep");
    let settled = run(&mut k, &tone[..9_600]);
    assert!((crossings(&tone[..9_600]) - crossings(&settled)).abs() <= 8, "the delay never settled");
}

#[test]
fn with_no_glide_a_new_time_is_there_at_once() {
    let mut k = echo(&[LINE_ONLY, &[("delayMs", 100.0), ("feedback", 0.0), ("glideMs", 0.0)]].concat());
    run(&mut k, &vec![0.0; 512]);
    set(&mut k, "delayMs", 300.0);
    // A click now comes back exactly 300 ms later.
    let out = run(&mut k, &click(20_000));
    assert!(out[14_400] > 0.99, "{}", out[14_400]);
}

#[test]
fn a_moved_level_glides_rather_than_stepping() {
    // MultiGrainLive stepped these at the block edge. Here they glide.
    let mut k = echo(&[("delayMs", 1.0), ("feedback", 0.0), ("dry", 1.0), ("wet", 0.0)]);
    run(&mut k, &vec![1.0; 512]);
    set(&mut k, "dry", 0.0);
    let out = run(&mut k, &vec![1.0; 4_800]);
    assert!(out[0] > 0.9, "dry stepped to {}", out[0]);
    assert!(out.windows(2).all(|w| w[1] <= w[0]));
    // The glide is a 5 ms time constant: 20 of them in 100 ms.
    assert!(out[4_799] < 1e-6, "{}", out[4_799]);
}

#[test]
fn reset_silences_the_tail() {
    let mut k = echo(&[("delayMs", 100.0), ("feedback", 0.9)]);
    run(&mut k, &click(2_000));
    k.reset();
    assert!(run(&mut k, &vec![0.0; 48_000]).iter().all(|s| *s == 0.0));
}

#[test]
fn the_tail_is_how_long_the_repeats_take_to_die_away() {
    // Down 60 dB: n repeats of |feedback| reach 0.001.
    let tail = |delay: f32, feedback: f32| echo(&[("delayMs", delay), ("feedback", feedback)]).tail_seconds();
    let expected = 0.5 * (0.001f32.ln() / 0.5f32.ln()).ceil();
    assert!((tail(500.0, 0.5) - expected).abs() < 1e-3, "{}", tail(500.0, 0.5));
    assert!((tail(500.0, -0.5) - expected).abs() < 1e-3, "the sign does not matter");
    assert!(tail(500.0, 0.9) > tail(500.0, 0.5));
    assert_eq!(tail(500.0, 0.0), 0.5, "no feedback is one repeat");
    assert!(tail(4_000.0, 0.98).is_finite());
}
