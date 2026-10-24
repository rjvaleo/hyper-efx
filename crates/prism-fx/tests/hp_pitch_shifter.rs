//! Hyperprism Pitch Shifter: a two-tap rotating delay line, grains of
//! 163.84 × grain % samples, an equal-power crossfade.

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn shifter(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-pitch-shifter", SR).expect("hp-pitch-shifter is in the catalogue");
    for &(key, value) in settings {
        set(&mut k, key, value);
    }
    k.reset();
    k
}

fn set(k: &mut Boxed, key: &str, value: f32) {
    let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
    k.set_param(i, value);
}

fn run(k: &mut Boxed, x: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut a, mut b) = (x.to_vec(), x.to_vec());
    k.process(&mut a, &mut b);
    (a, b)
}

fn ramp(n: usize) -> Vec<f32> {
    (0..n).map(|i| i as f32).collect()
}

#[test]
fn a_ramp_comes_out_at_the_pitch_ratio_between_grain_jumps() {
    // No crossfade and all wet: the output is the line read at a delay that
    // shrinks (or grows) by r = pitch/100 − 1 a sample, so a ramp's slope is
    // the ratio. The read jumps back every (2G − X)/|r| samples.
    for (pitch, period) in [(150.0f32, 4912usize), (50.0, 4912), (125.0, 4912)] {
        let ratio = pitch / 100.0;
        let mut k = shifter(&[("pitch", pitch), ("mix", 100.0), ("xfade", 0.0), ("feedback", 0.0)]);
        let (y, r) = run(&mut k, &ramp(20_000));
        assert_eq!(y, r);
        let slopes: Vec<f32> = y.windows(2).map(|w| w[1] - w[0]).collect();
        let jumps: Vec<usize> = (6_000..slopes.len()).filter(|&i| (slopes[i] - ratio).abs() > 0.01).collect();
        assert!(!jumps.is_empty(), "{pitch} %: no grain jumps");
        for pair in jumps.windows(2) {
            assert_eq!(pair[1] - pair[0], period, "{pitch} %: jumps at {jumps:?}");
        }
    }
}

#[test]
fn a_sine_comes_out_at_the_pitch_ratio() {
    let n = 88_200;
    let x: Vec<f32> = (0..n).map(|i| (std::f32::consts::TAU * 500.0 * i as f32 / SR).sin()).collect();
    for pitch in [150.0f32, 75.0] {
        let mut k = shifter(&[("pitch", pitch), ("mix", 100.0)]);
        let (y, _) = run(&mut k, &x);
        let rising = y[44_100..].windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as f32;
        let want = 500.0 * pitch / 100.0;
        assert!((rising - want).abs() / want < 0.03, "{pitch} %: {rising} Hz, want {want}");
    }
}

#[test]
fn a_grain_is_163_84_samples_per_grain_percent_at_any_ratio() {
    // With a full crossfade a steady input comes out as sin + cos of the
    // grain's position: 1 at each grain's start, √2 in its middle. The grain
    // is G/|r| = int(16384·0.01·|r|·grain)/|r| samples: 2456 at 15 % for
    // r = ±0.5 and ±0.25.
    for pitch in [150.0f32, 50.0, 125.0, 75.0] {
        let mut k = shifter(&[("pitch", pitch), ("mix", 100.0), ("grain", 15.0), ("xfade", 100.0)]);
        let (y, _) = run(&mut k, &vec![1.0; 30_000]);
        let starts: Vec<usize> = (5_000..y.len() - 1).filter(|&i| y[i] < y[i - 1] && y[i] <= y[i + 1]).collect();
        assert!(starts.len() >= 5, "{pitch} %: {starts:?}");
        for pair in starts.windows(2) {
            assert_eq!(pair[1] - pair[0], 2456, "{pitch} %: grain starts at {starts:?}");
        }
        for &s in &starts {
            assert!((y[s] - 1.0).abs() < 2e-3, "{pitch} %: {} at a grain start", y[s]);
        }
        let top = y[5_000..].iter().cloned().fold(0.0f32, f32::max);
        assert!((top - std::f32::consts::SQRT_2).abs() < 1e-3, "{pitch} %: equal power peaks at {top}");
    }
}

#[test]
fn full_mix_is_all_wet() {
    // The Pitch Time Changer ran half dry, because it took its mix before
    // setting 100 %; here the mix is a control, and 100 % leaves no dry sound
    // (the ramp above has the pitch ratio's slope, not a blend of it and 1).
    let mut k = shifter(&[("pitch", 150.0), ("mix", 100.0), ("xfade", 0.0)]);
    let (y, _) = run(&mut k, &ramp(8_000));
    assert!((y[7_000] - y[6_999] - 1.5).abs() < 1e-3);
}

#[test]
fn the_mix_steps_every_513_samples() {
    // At 100 % the read sits still on the second tap, X = 16 frames back (the
    // smallest grain), so a ramp comes out as n − 16·mix. Smoothing fires on
    // the first sample and every 513th after: m ← a·target + (1 − a)·m with
    // a = 0.01 at 50 %.
    let mut k = shifter(&[("pitch", 100.0), ("mix", 50.0)]);
    set(&mut k, "mix", 100.0);
    let (y, _) = run(&mut k, &ramp(1_600));
    let mix = |i: usize| (i as f32 - y[i]) / 16.0;
    let m0 = 0.01 * 1.0 + 0.99 * 0.5;
    let m1 = 0.01 * 1.0 + 0.99 * m0;
    let m2 = 0.01 * 1.0 + 0.99 * m1;
    for (range, m) in [(16..513, m0), (513..1026, m1), (1026..1539, m2)] {
        for i in range {
            assert!((mix(i) - m).abs() < 1e-4, "sample {i}: {} vs {m}", mix(i));
        }
    }
}

#[test]
fn feedback_returns_the_whole_output_to_the_line() {
    // The line takes x + fb·y, dry included: at pitch 100 % and 100 % mix a
    // click comes back every 16 frames, each time at fb times the last.
    let mut k = shifter(&[("pitch", 100.0), ("mix", 100.0), ("feedback", 50.0)]);
    let mut x = vec![0.0; 100];
    x[0] = 1.0;
    let (y, _) = run(&mut k, &x);
    assert!((y[16] - 1.0).abs() < 1e-6 && (y[32] - 0.5).abs() < 1e-6 && (y[48] - 0.25).abs() < 1e-6, "{:?}", &y[..50]);
}

#[test]
fn the_ranges_are_the_originals() {
    let d = prism_fx::find("hp-pitch-shifter").unwrap().descriptor;
    let check = |key: &str, min: f32, max: f32, default: f32| {
        let s = d.spec(key).unwrap_or_else(|| panic!("{key}"));
        assert!(!s.log, "{key}");
        assert_eq!((s.min, s.max, s.default), (min, max, default), "{key}");
    };
    check("pitch", 0.0, 200.0, 100.0);
    check("mix", 0.0, 100.0, 50.0);
    check("feedback", 0.0, 100.0, 0.0);
    check("grain", 0.0, 100.0, 15.0);
    check("xfade", 0.0, 100.0, 100.0);
    check("smoothing", 0.0, 100.0, 50.0);
    assert_eq!(d.label, "Hyperprism Pitch Shifter");
    assert_eq!(d.source, "Hyperprism-PPC 1.2.1: Pitch Shifter");
}
