//! Hyperprism Echo (1.2.1's EDelay2, `Delay2`): a delay with feedback whose
//! time changes by equal-power crossfades between whole-sample taps.

use std::f64::consts::FRAC_PI_2;

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn echo_at(sr: f32, settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-echo", sr).expect("hp-echo is in the catalogue");
    for &(key, value) in settings {
        set(&mut k, key, value);
    }
    k
}

fn echo(settings: &[(&str, f32)]) -> Boxed {
    echo_at(SR, settings)
}

fn set(k: &mut Boxed, key: &str, value: f32) {
    let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
    k.set_param(i, value);
}

fn run(k: &mut Boxed, x: &[f32]) -> Vec<f32> {
    let (mut l, mut r) = (x.to_vec(), x.to_vec());
    k.process(&mut l, &mut r);
    assert_eq!(l, r, "both sides take the same taps");
    l
}

fn click(n: usize) -> Vec<f32> {
    let mut x = vec![0.0; n];
    x[0] = 1.0;
    x
}

fn noise(n: usize, seed: u32) -> Vec<f32> {
    let mut s = seed;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            s as f32 / u32::MAX as f32 - 0.5
        })
        .collect()
}

/// The original's delay in samples: `0.001 × (float)(ms × SR)`, as a float.
fn samples(ms: f32, sr: f32) -> f32 {
    (0.001 * ((ms as f64 * sr as f64) as f32) as f64) as f32
}

/// Lengths of the stretches where the output rises above 1: on a steady input,
/// each crossfade step sums the two taps at cos θ + sin θ, above 1 everywhere
/// inside the step and exactly 1 at its two ends.
fn swells(y: &[f32]) -> Vec<usize> {
    let mut runs = Vec::new();
    let mut n = 0;
    for &v in y {
        if v > 1.0 + 1e-6 {
            n += 1;
        } else if n > 0 {
            runs.push(n);
            n = 0;
        }
    }
    runs
}

/// Wet only, no repeats.
const WET: &[(&str, f32)] = &[("mix", 100.0), ("feedback", 0.0)];

#[test]
fn a_click_returns_at_the_whole_sample_delay() {
    // Delay2 reads whole-sample taps: 10.01 ms is 441.441 samples, read at 441.
    for ms in [10.0, 10.01, 250.0, 1000.0] {
        let mut k = echo(&[WET, &[("delayMs", ms)]].concat());
        let y = run(&mut k, &click(50_000));
        let at = samples(ms, SR) as usize;
        assert_eq!(y[at], 1.0, "{ms} ms at {at}");
        let elsewhere = y.iter().enumerate().filter(|&(i, v)| i != at && *v != 0.0).count();
        assert_eq!(elsewhere, 0, "{ms} ms: nothing but the one tap");
    }
}

#[test]
fn feedback_feeds_the_tap_back_at_its_percentage() {
    let mut k = echo(&[("mix", 100.0), ("feedback", 50.0), ("delayMs", 10.0)]);
    let y = run(&mut k, &click(3_000));
    for repeat in 1..=5 {
        assert_eq!(y[441 * repeat], 0.5f32.powi(repeat as i32 - 1), "repeat {repeat}");
    }
}

#[test]
fn mix_sets_wet_and_the_rest_is_dry() {
    let mut k = echo(&[("mix", 25.0), ("feedback", 0.0), ("delayMs", 10.0)]);
    let y = run(&mut k, &click(1_000));
    assert_eq!(y[0], 0.75);
    assert_eq!(y[441], 0.25);
}

#[test]
fn a_time_change_is_an_equal_power_crossfade_from_the_old_tap_to_the_next() {
    // Idle, the step counter fires every 2049 samples from the first; a new
    // time set on one of those samples starts its first step there. 1000 ms to
    // 500 ms at the default speed (50 %): a first step to 750 ms over 22 050
    // samples, then to 625 ms over 11 025.
    let x = noise(200_000, 7);
    let mut k = echo(&[WET, &[("delayMs", 1000.0)]].concat());
    let n0 = 2049 * 22;
    let mut y = run(&mut k, &x[..n0]);
    set(&mut k, "delayMs", 500.0);
    y.extend(run(&mut k, &x[n0..]));
    for i in 0..=22_050usize {
        let n = n0 + i;
        let theta = FRAC_PI_2 / 22_050.0 * i as f64;
        let expected = theta.cos() * x[n - 44_100] as f64 + theta.sin() * x[n - 33_075] as f64;
        assert!((y[n] as f64 - expected).abs() < 1e-6, "step 1, sample {i}: {} against {expected}", y[n]);
    }
    let n1 = n0 + 22_051;
    for i in 0..=11_025usize {
        let n = n1 + i;
        let theta = FRAC_PI_2 / 11_025.0 * i as f64;
        let expected = theta.cos() * x[n - 33_075] as f64 + theta.sin() * x[n - 27_562] as f64;
        assert!((y[n] as f64 - expected).abs() < 1e-6, "step 2, sample {i}: {} against {expected}", y[n]);
    }
}

#[test]
fn each_step_lasts_the_larger_of_4096_samples_and_its_distance() {
    // Steps of half the remaining distance: 22 050, 11 025, 5 512, then the
    // 4 096-sample floor while the whole-sample taps still differ.
    let mut k = echo(&[WET, &[("delayMs", 1000.0)]].concat());
    let n0 = 2049 * 22;
    run(&mut k, &vec![1.0; n0]);
    set(&mut k, "delayMs", 500.0);
    let y = run(&mut k, &vec![1.0; 10 * 44_100]);
    let runs = swells(&y);
    assert_eq!(&runs[..4], &[22_049, 11_024, 5_511, 4_095], "{runs:?}");
    assert!(runs[4..].iter().all(|&r| r == 4_095), "{runs:?}");
}

#[test]
fn the_pitch_holds_while_the_time_moves() {
    // No glide: a steady tone keeps its frequency through the whole walk.
    let tone: Vec<f32> = (0..(4.0 * SR) as usize).map(|i| 0.5 * (std::f32::consts::TAU * 1000.0 * i as f32 / SR).sin()).collect();
    let mut k = echo(&[WET, &[("delayMs", 1000.0)]].concat());
    let n0 = 2049 * 22;
    let mut y = run(&mut k, &tone[..n0]);
    set(&mut k, "delayMs", 500.0);
    y.extend(run(&mut k, &tone[n0..]));
    for w in y[n0..n0 + 44_100].chunks(4_410) {
        let crossings = w.windows(2).filter(|p| (p[0] < 0.0) != (p[1] < 0.0)).count() as f32;
        let hz = crossings / 2.0 / (w.len() as f32 / SR);
        assert!((hz - 1000.0).abs() < 10.0, "{hz} Hz");
    }
}

#[test]
fn at_full_speed_the_time_jumps() {
    let mut k = echo(&[WET, &[("delayMs", 1000.0), ("speed", 100.0)]].concat());
    run(&mut k, &vec![0.0; 1_000]);
    set(&mut k, "delayMs", 10.0);
    let y = run(&mut k, &click(2_000));
    assert_eq!(y[441], 1.0);
}

#[test]
fn at_48_khz_the_shortest_step_is_4096_samples_scaled() {
    // round(4096 × 48 000 / 44 100) = 4458; idle, the counter fires every
    // round(2048 × 48 000 / 44 100) + 1 = 2230 samples.
    let sr = 48_000.0;
    let mut k = echo_at(sr, &[WET, &[("delayMs", 1000.0)]].concat());
    let n0 = 2230 * 22;
    run(&mut k, &vec![1.0; n0]);
    set(&mut k, "delayMs", 990.0);
    let y = run(&mut k, &vec![1.0; 48_000]);
    assert_eq!(swells(&y)[0], 4_457);
}

// ---- the original's bugs, fixed ------------------------------------------------

#[test]
fn setting_the_delay_leaves_feedback_alone() {
    // EDelay2::SetParameter had no break after the delay: it also set the
    // feedback to 0.01 × the delay in ms (10 % at 10 ms).
    let mut k = echo(&[("mix", 100.0), ("feedback", 0.0)]);
    set(&mut k, "delayMs", 10.0);
    let y = run(&mut k, &click(1_000));
    assert_eq!(y[441], 1.0);
    assert_eq!(y[882], 0.0, "no repeat");
}

#[test]
fn zero_ms_is_reached() {
    // A target of 0 meant idle in the original, so the time never moved.
    let mut k = echo(WET);
    run(&mut k, &vec![0.0; 1_000]);
    set(&mut k, "delayMs", 0.0);
    run(&mut k, &vec![0.0; 6 * 44_100]);
    let y = run(&mut k, &click(1_000));
    assert_eq!(y[1], 1.0, "the shortest delay is one sample");
}

#[test]
fn a_tap_is_never_crossfaded_with_itself() {
    // Once the walk's steps are smaller than a sample both taps read the same
    // sample, and the original summed it with itself at cos θ + sin θ, a
    // 3 dB swell every 4096 samples. Waiting for the first step it did the same.
    let mut k = echo(&[WET, &[("delayMs", 1000.0)]].concat());
    run(&mut k, &vec![1.0; 2049 * 22]);
    set(&mut k, "delayMs", 500.0);
    let y = run(&mut k, &vec![1.0; 20 * 44_100]);
    assert!(y[12 * 44_100..].iter().all(|&v| v == 1.0), "steady once the taps meet");

    let mut k = echo(&[WET, &[("delayMs", 1000.0)]].concat());
    run(&mut k, &vec![1.0; 2049 * 22 + 500]);
    set(&mut k, "delayMs", 500.0);
    let y = run(&mut k, &vec![1.0; 1_549]);
    assert!(y.iter().all(|&v| v == 1.0), "no swell before the first step");
}

#[test]
fn a_walk_ends_on_its_target() {
    // At speeds under 50 % the last steps are too small to move the float:
    // the original stalled one float step short of 1000 ms, reading a sample
    // short (44 099) for good, and kept stepping.
    let mut k = echo(&[WET, &[("delayMs", 500.0), ("speed", 25.0)]].concat());
    run(&mut k, &vec![0.0; 1_000]);
    set(&mut k, "delayMs", 1000.0);
    run(&mut k, &vec![0.0; 12 * 44_100]);
    let y = run(&mut k, &click(50_000));
    assert_eq!(y[44_100], 1.0);
    assert_eq!(y.iter().filter(|v| **v != 0.0).count(), 1);
}

#[test]
fn a_jump_at_full_speed_leaves_no_stale_tap() {
    // The jump wrote the current tap but not the next one, so a later time set
    // at a lower speed crossfaded back to the old time first.
    let mut k = echo(&[WET, &[("delayMs", 1000.0), ("speed", 100.0)]].concat());
    run(&mut k, &vec![0.0; 1_000]);
    set(&mut k, "delayMs", 500.0);
    set(&mut k, "speed", 50.0);
    set(&mut k, "delayMs", 500.0);
    run(&mut k, &vec![0.0; 3_000]);
    let y = run(&mut k, &click(50_000));
    assert_eq!(y[22_050], 1.0);
    assert_eq!(y.iter().filter(|v| **v != 0.0).count(), 1);
}

#[test]
fn full_feedback_cannot_run_away() {
    // The original's feedback had no ceiling, and even at 100 % a crossfade
    // between neighbouring taps lifts the loop gain above one.
    let mut k = echo(&[("mix", 100.0), ("feedback", 100.0), ("delayMs", 0.05)]);
    let x = noise(5 * 44_100, 3);
    let mut y = Vec::new();
    for (i, block) in x.chunks(4_410).enumerate() {
        set(&mut k, "delayMs", if i % 2 == 0 { 0.1 } else { 0.05 });
        let input: Vec<f32> = if i < 2 { block.to_vec() } else { vec![0.0; block.len()] };
        y.extend(run(&mut k, &input));
    }
    assert!(y.iter().all(|v| v.is_finite() && v.abs() <= 6.0), "peak {}", y.iter().fold(0.0f32, |m, v| m.max(v.abs())));
}

#[test]
fn controls_are_the_originals() {
    let d = prism_fx::find("hp-echo").unwrap().descriptor;
    assert_eq!((d.code, d.label, d.source), (41, "Hyperprism Echo", "Hyperprism-PPC 1.2.1: Echo"));
    assert_eq!(d.category, prism_fx::Category::Delay);
    let p = |key: &str| d.spec(key).unwrap_or_else(|| panic!("no {key}"));
    assert_eq!((p("delayMs").min, p("delayMs").max, p("delayMs").default), (0.0, 2000.0, 1000.0));
    assert_eq!((p("feedback").min, p("feedback").max, p("feedback").default), (0.0, 100.0, 0.0));
    assert_eq!((p("mix").min, p("mix").max, p("mix").default), (0.0, 100.0, 50.0));
    assert_eq!((p("speed").min, p("speed").max, p("speed").default), (1.0, 100.0, 50.0));
    assert!(d.params.iter().all(|s| !s.log), "the pad and fields are linear");
    let pad = d.panel.pad.unwrap();
    assert_eq!((pad.x, pad.x_range, pad.y, pad.y_range), ("delayMs", (0.0, 2000.0), "feedback", (0.0, 100.0)));
}
