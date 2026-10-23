//! ++bubbler: SoundHack Freesound's granular delay, as its code does it.

const SR: f32 = 48_000.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn bubbler(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("sh-bubbler", SR).expect("sh-bubbler is in the catalogue");
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

fn level_at(x: &[f32], hz: f32) -> f32 {
    let w = std::f64::consts::TAU * hz as f64 / SR as f64;
    let (re, im) = x.iter().enumerate().fold((0.0f64, 0.0f64), |(re, im), (n, &s)| {
        (re + s as f64 * (w * n as f64).cos(), im + s as f64 * (w * n as f64).sin())
    });
    ((re * re + im * im).sqrt() / x.len() as f64) as f32
}

/// Wet only, every grain alike: no variation, no reversal.
const PLAIN: &[(&str, f32)] =
    &[("mix", 1.0), ("timeVar", 0.0), ("startVar", 0.0), ("panVar", 0.0), ("octaveVar", 0.0), ("reverse", 0.0)];

#[test]
fn a_new_one_starts_where_the_original_does() {
    let k = prism_fx::make("sh-bubbler", SR).unwrap();
    let get = |key: &str| k.param(k.descriptor().index_of(key).unwrap());
    assert!((get("delayMs") - 190.5).abs() < 0.01);
    assert_eq!((get("timeVar"), get("grainSize"), get("density"), get("reverse"), get("mix")), (0.5, 0.1, 0.8, 0.5, 0.5));
}

#[test]
fn in_and_out_gain_act() {
    // The original never read them.
    let x = sine(220.0, 0.25);
    let y = run(&mut bubbler(&[("mix", 0.0), ("inDb", -6.0206), ("outDb", -6.0206)]), &x);
    for i in 0..x.len() {
        assert!((y[i] - 0.25 * x[i]).abs() < 1e-5);
    }
}

#[test]
fn every_grain_reads_the_set_time_back() {
    // D = time + 2 samples, read on the sample itself. The original mirrored
    // the read's fraction and landed a sample early.
    let mut x = vec![0.0; 20_000];
    x[0] = 1.0;
    let y = run(&mut bubbler(&[PLAIN, &[("delayMs", 100.0)]].concat()), &x);
    let loudest = (0..y.len()).max_by(|&a, &b| y[a].abs().total_cmp(&y[b].abs())).unwrap();
    assert_eq!(loudest, 4_802);
    assert!(y[..4_802].iter().all(|s| s.abs() < 1e-5));
}

/// The energy within `width` Hz of `hz`. Overlapping grains read a shifted
/// sound at different phases, so a single frequency can cancel; a band does not.
fn band(x: &[f32], hz: f32, width: f32) -> f32 {
    (0..=(2.0 * width) as usize).map(|i| level_at(x, hz - width + i as f32).powi(2)).sum()
}

#[test]
fn octaves_and_keys_set_the_grain_pitch() {
    // At a quarter density the grains follow one another without overlapping.
    // Overlapping, each new grain reads its shifted sound from a later start,
    // a fraction of a cycle behind the last, and their sum is pulled off pitch
    // (an octave up at the defaults lands near 390 Hz) — the original's
    // schedule, kept.
    let tail = |settings: &[(&str, f32)]| {
        run(&mut bubbler(&[PLAIN, &[("density", 0.25)], settings].concat()), &sine(220.0, 2.0))[48_000..].to_vec()
    };
    let up = tail(&[("octave", 1.0)]);
    assert!(band(&up, 440.0, 60.0) > 5.0 * band(&up, 220.0, 60.0), "{} against {}", band(&up, 440.0, 60.0), band(&up, 220.0, 60.0));
    let fifth = tail(&[("key7", 1.0), ("just", 1.0)]);
    assert!(band(&fifth, 330.0, 60.0) > 5.0 * band(&fifth, 220.0, 60.0), "{} against {}", band(&fifth, 330.0, 60.0), band(&fifth, 220.0, 60.0));
}

#[test]
fn reversal_is_a_chance_per_grain() {
    // The original compared a 0–1 random number with the 0–100 dial, so 30
    // and 100 both reversed every grain.
    let x = sine(220.0, 1.0);
    let some = run(&mut bubbler(&[("reverse", 0.3)]), &x);
    let all = run(&mut bubbler(&[("reverse", 1.0)]), &x);
    let none = run(&mut bubbler(&[("reverse", 0.0)]), &x);
    assert_ne!(some, all);
    assert_ne!(some, none);
}

#[test]
fn two_hundred_percent_feedback_is_held_in_check() {
    let x: Vec<f32> = (0..(10.0 * SR) as usize).map(|i| if i < 48_000 { ((i * 7919) % 97) as f32 / 97.0 - 0.5 } else { 0.0 }).collect();
    let y = run(&mut bubbler(&[("feedback", 2.0), ("mix", 1.0), ("delayMs", 50.0)]), &x);
    assert!(y.iter().all(|s| s.abs() <= 1.5));
    let rms = |s: &[f32]| (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt();
    assert!(rms(&y[y.len() - 48_000..]) < rms(&y[48_000..96_000]) * 2.0);
}
