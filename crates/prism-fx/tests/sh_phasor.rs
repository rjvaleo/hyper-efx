//! ++phasor: SoundHack Freesound's phaser, as its code does it.

const SR: f32 = 48_000.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn phasor(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("sh-phasor", SR).expect("sh-phasor is in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.reset();
    k
}

fn sine(hz: f32, seconds: f32, amp: f32) -> Vec<f32> {
    (0..(seconds * SR) as usize).map(|i| amp * (std::f32::consts::TAU * hz * i as f32 / SR).sin()).collect()
}

fn run(k: &mut Boxed, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut a, mut b) = (l.to_vec(), r.to_vec());
    k.process(&mut a, &mut b);
    (a, b)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
}

/// No sweep, no feedback: a fixed phaser at `freqHz`.
const STILL: &[(&str, f32)] = &[("depth", 0.0), ("feedback", 0.0)];

#[test]
fn with_every_tap_off_it_is_the_dry_sound_through_an_arctangent() {
    let x = sine(440.0, 0.5, 0.8);
    let (y, _) = run(&mut phasor(&[STILL, &[("out360", 0.0)]].concat()), &x, &x);
    for i in 0..x.len() {
        assert!((y[i] - x[i].atan()).abs() < 1e-5, "sample {i}");
    }
}

#[test]
fn one_section_notches_just_above_its_frequency() {
    // One allpass section is half a turn behind a little above its pole
    // frequency — about 560 Hz for a section at 500 — and with the dry sound
    // added, that frequency cancels. Quiet, so the arctangent at the chain's
    // input is close to straight and the cancellation is clean.
    let k = || phasor(&[STILL, &[("freqHz", 500.0)]].concat());
    let level = |hz: f32| {
        let x = sine(hz, 0.5, 0.05);
        rms(&run(&mut k(), &x, &x).0[12_000..])
    };
    let (deepest, at) = (500..=620).step_by(2).map(|hz| (hz, level(hz as f32))).fold((0, f32::MAX), |m, (h, v)| if v < m.1 { (h, v) } else { m });
    assert!((540..=580).contains(&deepest), "the notch is at {deepest} Hz");
    assert!(at < level(2_000.0) * 0.05, "and deep: {at} against {}", level(2_000.0));
}

#[test]
fn more_taps_make_more_notches() {
    // Taps after 1, 2, 4 and 8 sections each add their own phase pattern; with
    // all four the response is no longer one clean notch.
    let response = |settings: &[(&str, f32)]| {
        (300..3_000).step_by(100).map(|hz| {
            let x = sine(hz as f32, 0.25, 0.3);
            rms(&run(&mut phasor(settings), &x, &x).0[6_000..])
        }).collect::<Vec<f32>>()
    };
    let one = response(&[STILL, &[("freqHz", 800.0)]].concat());
    let all = response(&[STILL, &[("freqHz", 800.0), ("out720", 1.0), ("out1080", 1.0), ("out1440", 1.0)]].concat());
    let dips = |r: &[f32]| r.windows(3).filter(|w| w[1] < w[0] && w[1] < w[2]).count();
    assert!(dips(&all) > dips(&one), "{} dips against {}", dips(&all), dips(&one));
}

/// Where the single-section notch is, for a section at `freq_hz`.
fn notch_for(freq_hz: f32) -> f32 {
    let level = |hz: f32| {
        let x = sine(hz, 0.5, 0.05);
        rms(&run(&mut phasor(&[STILL, &[("freqHz", freq_hz)]].concat()), &x, &x).0[12_000..])
    };
    (0..80).map(|i| freq_hz * (0.9 + i as f32 * 0.005)).fold((0.0, f32::MAX), |m, hz| {
        let v = level(hz);
        if v < m.1 { (hz, v) } else { m }
    }).0
}

#[test]
fn the_lfo_sweeps_two_octaves_each_way_at_full_depth() {
    // At 0.5 Hz the sine is at its top after half a second: the section is
    // then at four times the frequency, and a tone at that section's notch
    // cancels there, but not a quarter of a cycle later.
    let x = sine(notch_for(2_000.0), 1.25, 0.05);
    let (y, _) = run(&mut phasor(&[("freqHz", 500.0), ("rateHz", 0.5), ("depth", 1.0), ("feedback", 0.0)]), &x, &x);
    let around = |t: f32| rms(&y[(t * SR) as usize - 480..(t * SR) as usize + 480]);
    assert!(around(0.5) < around(1.0) * 0.2, "top of the sweep {} against the middle {}", around(0.5), around(1.0));
}

#[test]
fn the_right_side_is_fed_back_from_the_left() {
    // The original feeds both channels' loops from the left's output: a sound
    // on the left alone comes out on the right too, with feedback.
    let l = sine(440.0, 0.5, 0.5);
    let silent = vec![0.0; l.len()];
    let (_, r) = run(&mut phasor(&[STILL, &[("feedback", 1.0)]].concat()), &l, &silent);
    assert!(rms(&r) > 0.01, "{}", rms(&r));
    let (_, r) = run(&mut phasor(STILL), &l, &silent);
    assert!(rms(&r) < 1e-4, "and not without it: {}", rms(&r));
}

#[test]
fn both_sides_sweep_together() {
    let x = sine(700.0, 1.0, 0.5);
    let (l, r) = run(&mut phasor(&[("depth", 1.0), ("feedback", 0.0)]), &x, &x);
    assert_eq!(l, r);
}

#[test]
fn the_random_shape_slides_and_repeats_from_a_reset() {
    let x = sine(1_000.0, 2.0, 0.5);
    let random = [("shape", 2.0), ("rateHz", 4.0), ("depth", 1.0), ("feedback", 0.0)];
    let a = run(&mut phasor(&random), &x, &x).0;
    let b = run(&mut phasor(&random), &x, &x).0;
    assert_eq!(a, b, "the same from a reset");
    let sine_lfo = run(&mut phasor(&[("shape", 0.0), ("rateHz", 4.0), ("depth", 1.0), ("feedback", 0.0)]), &x, &x).0;
    assert_ne!(a, sine_lfo, "and not the sine");
}

#[test]
fn nothing_passes_one_and_a_half() {
    let x = sine(300.0, 0.5, 1.0);
    let (l, r) = run(&mut phasor(&[("feedback", 1.0), ("inDb", 20.0), ("outDb", 20.0), ("out1440", 1.0)]), &x, &x);
    assert!(l.iter().chain(&r).all(|s| s.abs() <= 1.5));
}
