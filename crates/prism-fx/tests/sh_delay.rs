//! ++delay: SoundHack Delay Trio's delay, as its code does it.

const SR: f32 = 48_000.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn delay(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("sh-delay", SR).expect("sh-delay is in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.reset();
    k
}

fn run(k: &mut Boxed, x: &[f32]) -> Vec<f32> {
    let (mut l, mut r) = (x.to_vec(), x.to_vec());
    k.process(&mut l, &mut r);
    l
}

fn click(n: usize) -> Vec<f32> {
    let mut x = vec![0.0; n];
    x[0] = 1.0;
    x
}

fn loudest(x: &[f32]) -> usize {
    (0..x.len()).max_by(|&a, &b| x[a].abs().total_cmp(&x[b].abs())).unwrap()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
}

/// Wet only, no repeats.
const WET: &[(&str, f32)] = &[("mix", 1.0), ("feedback", 0.0)];

#[test]
fn a_click_comes_back_one_sample_after_the_time() {
    // The read lands a sample behind the newest write, so the echo is at the
    // set time plus one, through the 20 Hz high-pass and the wet arctangent.
    for ms in [10.0, 80.0, 250.0] {
        let y = run(&mut delay(&[WET, &[("delayMs", ms)]].concat()), &click(20_000));
        let at = (ms * SR / 1000.0).round() as usize + 1;
        assert_eq!(loudest(&y), at, "{ms} ms");
        assert!((y[at] - (1.0f32.atan() + 1e-6)).abs() < 2e-3, "{}", y[at]);
    }
}

#[test]
fn feedback_plus_repeats_and_feedback_minus_flips_each_repeat() {
    let repeats = |invert: f32| {
        let y = run(&mut delay(&[("mix", 1.0), ("delayMs", 10.0), ("feedback", 0.5), ("invert", invert)]), &click(4_000));
        (y[481], y[962])
    };
    let (first, second) = repeats(0.0);
    assert!(first > 0.0 && second > 0.0, "feedback + : {first}, {second}");
    let (first, second) = repeats(1.0);
    assert!(first > 0.0 && second < 0.0, "feedback − : {first}, {second}");
}

#[test]
fn two_hundred_percent_feedback_is_held_in_check() {
    let x: Vec<f32> = (0..(20.0 * SR) as usize).map(|i| if i < 48_000 { ((i * 7919) % 97) as f32 / 97.0 - 0.5 } else { 0.0 }).collect();
    let y = run(&mut delay(&[("mix", 1.0), ("delayMs", 50.0), ("feedback", 2.0)]), &x);
    assert!(y.iter().all(|s| s.abs() <= 1.5));
    let early = rms(&y[48_000..96_000]);
    let late = rms(&y[y.len() - 48_000..]);
    assert!(late < early * 1.5, "it did not run away: {early} then {late}");
}

#[test]
fn freeze_holds_the_loop() {
    let mut k = delay(&[("mix", 1.0), ("delayMs", 200.0), ("feedback", 0.0)]);
    let tone: Vec<f32> = (0..48_000).map(|i| 0.5 * (std::f32::consts::TAU * 330.0 * i as f32 / SR).sin()).collect();
    let before = run(&mut k, &tone);
    let i = k.descriptor().index_of("freeze").unwrap();
    k.set_param(i, 1.0);
    let after = run(&mut k, &vec![0.0; (5.0 * SR) as usize]);
    let (a, b) = (rms(&before[24_000..]), rms(&after[after.len() - 48_000..]));
    assert!((20.0 * (b / a).log10()).abs() < 1.5, "held at {b} against {a}");
}

#[test]
fn two_heads_on_a_ramp_shift_the_pitch() {
    let x: Vec<f32> = (0..(2.0 * SR) as usize).map(|i| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / SR).sin()).collect();
    let y = run(&mut delay(&[WET, &[("delayMs", 100.0), ("twoHead", 1.0), ("shape", 3.0), ("depth", 1.0), ("rateHz", 2.0)]].concat()), &x);
    let crossings = |s: &[f32]| s.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count() as f32;
    let shifted = crossings(&y[48_000..]) / 2.0;
    assert!((shifted - 440.0).abs() > 30.0, "{shifted} Hz");
}

#[test]
fn the_low_pass_darkens_the_repeats() {
    let x: Vec<f32> = (0..4_800).map(|i| if i < 480 { ((i * 7919) % 97) as f32 / 97.0 - 0.5 } else { 0.0 }).collect();
    let bright = run(&mut delay(&[("mix", 1.0), ("delayMs", 20.0), ("feedback", 0.8)]), &x);
    let dark = run(&mut delay(&[("mix", 1.0), ("delayMs", 20.0), ("feedback", 0.8), ("lowpassHz", 500.0)]), &x);
    let edges = |s: &[f32]| s.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / rms(s).max(1e-9);
    assert!(edges(&dark[1_920..]) < edges(&bright[1_920..]) * 0.5);
}

#[test]
fn loud_input_shortens_the_delay_with_auto_time() {
    // The envelope is the mean of |L + R| over 512 samples; with auto time up,
    // a loud sound pulls the echo in.
    let mut x = vec![0.5; 24_000];
    x[12_000] = 1.0;
    let plain = run(&mut delay(&[WET, &[("delayMs", 200.0)]].concat()), &x);
    let auto = run(&mut delay(&[WET, &[("delayMs", 200.0), ("envMod", 0.1)]].concat()), &x);
    let peak_after = |y: &[f32]| 12_000 + loudest(&y[12_000..]);
    assert!(peak_after(&auto) < peak_after(&plain), "{} against {}", peak_after(&auto), peak_after(&plain));
}

#[test]
fn two_heads_on_the_random_shape_stay_smooth() {
    // The original stepped the second head's random value every sample, a
    // buzz near the Nyquist frequency; here each head slides to its own targets.
    let x: Vec<f32> = (0..(2.0 * SR) as usize).map(|i| 0.5 * (std::f32::consts::TAU * 220.0 * i as f32 / SR).sin()).collect();
    let random = |two: f32| run(&mut delay(&[WET, &[("delayMs", 100.0), ("twoHead", two), ("shape", 5.0), ("depth", 0.5), ("rateHz", 2.0)]].concat()), &x);
    let edges = |s: &[f32]| s.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    let (one, two) = (random(0.0), random(1.0));
    assert!(edges(&two[48_000..]) < edges(&one[48_000..]) * 2.0, "{} against {}", edges(&two[48_000..]), edges(&one[48_000..]));
}

#[test]
fn freeze_keeps_both_sides_at_the_out_level() {
    // The original's right side skipped the out gain while frozen.
    let mut k = delay(&[("mix", 1.0), ("delayMs", 100.0), ("outDb", -12.0)]);
    let tone: Vec<f32> = (0..24_000).map(|i| 0.5 * (std::f32::consts::TAU * 330.0 * i as f32 / SR).sin()).collect();
    let (mut l, mut r) = (tone.clone(), tone.clone());
    k.process(&mut l, &mut r);
    let i = k.descriptor().index_of("freeze").unwrap();
    k.set_param(i, 1.0);
    let (mut l, mut r) = (vec![0.0; 48_000], vec![0.0; 48_000]);
    k.process(&mut l, &mut r);
    assert!((rms(&l) - rms(&r)).abs() < rms(&l) * 0.01, "left {} right {}", rms(&l), rms(&r));
    assert!(rms(&l) < 0.5 * 0.25 * 0.71 * 1.1);
}
