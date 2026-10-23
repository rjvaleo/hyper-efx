//! ++pitchdelay: SoundHack Delay Trio's pitch-shifting delay, as its code
//! does it.

const SR: f32 = 48_000.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn pitchdelay(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("sh-pitchdelay", SR).expect("sh-pitchdelay is in the catalogue");
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

fn sine(hz: f32, seconds: f32) -> Vec<f32> {
    (0..(seconds * SR) as usize).map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / SR).sin()).collect()
}

/// The level of `hz` in `x`, by a single-bin Fourier sum.
fn level_at(x: &[f32], hz: f32) -> f32 {
    let w = std::f64::consts::TAU * hz as f64 / SR as f64;
    let (re, im) = x.iter().enumerate().fold((0.0f64, 0.0f64), |(re, im), (n, &s)| {
        (re + s as f64 * (w * n as f64).cos(), im + s as f64 * (w * n as f64).sin())
    });
    ((re * re + im * im).sqrt() / x.len() as f64) as f32
}

/// The loudest frequency between `lo` and `hi`, to a tenth of a hertz.
fn peak(x: &[f32], lo: f32, hi: f32) -> f32 {
    (0..=((hi - lo) * 10.0) as usize).map(|i| lo + i as f32 * 0.1).fold((lo, 0.0), |m, hz| {
        let v = level_at(x, hz);
        if v > m.1 { (hz, v) } else { m }
    }).0
}

#[test]
fn a_new_one_starts_where_the_original_does() {
    // The original's constructor moves its own controls: just tuning, full
    // depth, all wet, and a time of about 210 ms.
    let k = prism_fx::make("sh-pitchdelay", SR).unwrap();
    let get = |key: &str| k.param(k.descriptor().index_of(key).unwrap());
    assert_eq!(get("just"), 1.0);
    assert_eq!(get("depth"), 1.0);
    assert_eq!(get("mix"), 1.0);
    assert!((get("delayMs") - 209.83).abs() < 0.01, "{}", get("delayMs"));
}

#[test]
fn unshifted_a_click_comes_back_one_sample_after_the_time() {
    let mut x = vec![0.0; 12_000];
    x[0] = 1.0;
    let y = run(&mut pitchdelay(&[("delayMs", 100.0)]), &x);
    let loudest = (0..y.len()).max_by(|&a, &b| y[a].abs().total_cmp(&y[b].abs())).unwrap();
    assert_eq!(loudest, 4_801);
    // The wet path is 0.9·atan(2y), on a read clamped to ±1.
    assert!((y[4_801] - 0.9 * 2.0f32.atan()).abs() < 1e-3, "{}", y[4_801]);
}

#[test]
fn a_just_fifth_up_is_three_halves() {
    let y = run(&mut pitchdelay(&[("delayMs", 100.0), ("semitones", 7.0)]), &sine(220.0, 2.0));
    let tail = &y[24_000..];
    assert!(level_at(tail, 330.0) > 10.0 * level_at(tail, 220.0), "{} against {}", level_at(tail, 330.0), level_at(tail, 220.0));
}

#[test]
fn just_and_equal_tuning_differ_by_their_ratios() {
    // A major third: 5/4 just, 2^(4/12) equal.
    let third = |just: f32| {
        let y = run(&mut pitchdelay(&[("delayMs", 100.0), ("semitones", 4.0), ("just", just)]), &sine(220.0, 3.0));
        peak(&y[24_000..], 270.0, 282.0)
    };
    let (just, equal) = (third(1.0), third(0.0));
    assert!((just - 275.0).abs() < 0.6, "just {just}");
    assert!((equal - 277.18).abs() < 0.6, "equal {equal}");
}

#[test]
fn octaves_and_cents_move_the_ratio() {
    let y = run(&mut pitchdelay(&[("delayMs", 100.0), ("octave", -1.0)]), &sine(440.0, 2.0));
    assert!((peak(&y[24_000..], 200.0, 240.0) - 220.0).abs() < 0.6);
    let y = run(&mut pitchdelay(&[("delayMs", 100.0), ("cents", 100.0), ("just", 0.0)]), &sine(440.0, 3.0));
    assert!((peak(&y[24_000..], 450.0, 480.0) - 466.16).abs() < 0.6);
}

#[test]
fn a_new_time_glides_instead_of_jumping() {
    // The original computed a 100 ms glide and then threw it away; here it
    // runs, so the read moves smoothly and the sound does not click.
    let mut k = pitchdelay(&[("delayMs", 100.0)]);
    let before = run(&mut k, &sine(440.0, 0.5));
    let i = k.descriptor().index_of("delayMs").unwrap();
    k.set_param(i, 300.0);
    let after = run(&mut k, &sine(440.0, 0.2));
    let jump = |s: &[f32]| s.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    assert!(jump(&after) < 1.5 * jump(&before[12_000..]), "{} against {}", jump(&after), jump(&before[12_000..]));
}

#[test]
fn two_hundred_percent_feedback_is_held_in_check() {
    let x: Vec<f32> = (0..(10.0 * SR) as usize).map(|i| if i < 48_000 { ((i * 7919) % 97) as f32 / 97.0 - 0.5 } else { 0.0 }).collect();
    let y = run(&mut pitchdelay(&[("delayMs", 50.0), ("feedback", 2.0), ("semitones", 3.0)]), &x);
    assert!(y.iter().all(|s| s.abs() <= 1.0));
    let rms = |s: &[f32]| (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt();
    assert!(rms(&y[y.len() - 48_000..]) < rms(&y[48_000..96_000]) * 1.5);
}
