//! Hyperprism Hall Reverb: EReverb's LargeHallMono, as the 1.2.1 code does it.

#[path = "support/hp_fr_measure.rs"]
mod m;
use m::*;

const ID: &str = "hp-hall";

/// Wet only, no diffusion, no damping, no feedback: the memory's taps bare.
const BARE: &[(&str, f32)] = &[("mix", 100.0), ("feedback", 0.0), ("diffusion", 0.0), ("brightness", 100.0)];

fn with(extra: &[(&'static str, f32)]) -> Vec<(&'static str, f32)> {
    let mut s: Vec<(&'static str, f32)> = BARE.to_vec();
    for &(k, v) in extra {
        s.retain(|(key, _)| *key != k);
        s.push((k, v));
    }
    s
}

fn nonzero(x: &[f32]) -> Vec<(usize, f32)> {
    x.iter().enumerate().filter(|(_, v)| v.abs() > 1e-9).map(|(i, v)| (i, *v)).collect()
}

fn close(got: &[(usize, f32)], want: &[(usize, f32)]) {
    assert_eq!(got.len(), want.len(), "{got:?} against {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert_eq!(g.0, w.0, "{got:?} against {want:?}");
        assert!((g.1 - w.1).abs() < 1e-6, "{got:?} against {want:?}");
    }
}

#[test]
fn the_output_taps_sit_at_their_ages() {
    let x = impulse(12_420);
    let (l, r) = run(&mut make(ID, SR, BARE), &x, &x);
    close(&nonzero(&l), &[(851, 0.6), (7_001, -0.2), (12_415, 0.1)]);
    close(&nonzero(&r[..12_420]), &[(657, 0.2), (1_002, -0.6)]);
    let x = impulse(12_500);
    let (_, r) = run(&mut make(ID, SR, BARE), &x, &x);
    close(&nonzero(&r), &[(657, 0.2), (1_002, -0.6), (12_421, -0.1)]);
}

#[test]
fn a_stereo_input_is_summed_to_mono() {
    let x = impulse(2_000);
    let silent = vec![0.0; x.len()];
    let (l, r) = run(&mut make(ID, SR, BARE), &x, &silent);
    close(&nonzero(&l), &[(851, 0.3)]);
    close(&nonzero(&r), &[(657, 0.1), (1_002, -0.3)]);
}

#[test]
fn the_loop_is_12419_samples_and_feedback_is_its_gain() {
    let x = impulse(30_000);
    let (l, _) = run(&mut make(ID, SR, &with(&[("feedback", 50.0)])), &x, &x);
    let loop_len = 12_419;
    assert!((l[851 + loop_len] - 0.3).abs() < 1e-6, "{}", l[851 + loop_len]);
    assert!((l[851 + 2 * loop_len] - 0.15).abs() < 1e-6, "{}", l[851 + 2 * loop_len]);
    assert!((l[7_001 + loop_len] - (-0.1)).abs() < 1e-6);
}

#[test]
fn brightness_is_a_one_pole_on_the_input_and_in_the_loop() {
    let x = impulse(14_000);
    let (l, _) = run(&mut make(ID, SR, &with(&[("brightness", 50.0)])), &x, &x);
    for (i, want) in [(851, 0.3), (852, 0.15), (853, 0.075)] {
        assert!((l[i] - want).abs() < 1e-7, "{i}: {}", l[i]);
    }
    // In the loop: lpf = fb·c·tap + (1 − c)·lpf, with the input already
    // smoothed once.
    let (l, _) = run(&mut make(ID, SR, &with(&[("brightness", 50.0), ("feedback", 50.0)])), &x, &x);
    assert!((l[851 + 12_419] - 0.6 * 0.125).abs() < 1e-6, "{}", l[851 + 12_419]);
    assert!((l[852 + 12_419] - 0.6 * 0.125).abs() < 1e-6, "{}", l[852 + 12_419]);
}

#[test]
fn diffusion_scales_allpasses_placed_where_the_code_places_them() {
    // Allpass 1 (3 → 334, g 0.3·d) then allpass 2 (335 → 846, g 0.3·d), seen
    // five samples on at the 851 tap: both direct paths at 9, one recirculation
    // of either at 9 + 331 and 9 + 511, one of each at 851.
    let x = impulse(900);
    let (l, _) = run(&mut make(ID, SR, &with(&[("diffusion", 100.0)])), &x, &x);
    let g: f32 = 0.3;
    for (i, want) in [
        (9, 0.6 * g * g),
        (340, 0.6 * (1.0 - g * g) * -g),
        (520, 0.6 * -g * (1.0 - g * g)),
        (851, 0.6 * (1.0 - g * g) * (1.0 - g * g)),
    ] {
        assert!((l[i] - want).abs() < 1e-6, "{i}: {} against {want}", l[i]);
    }
    assert!(l[..9].iter().all(|&v| v == 0.0));
}

#[test]
fn mix_crossfades_dry_and_wet() {
    let x = impulse(1_000);
    let (l, r) = run(&mut make(ID, SR, &with(&[("mix", 30.0)])), &x, &x);
    assert!((l[0] - 0.7).abs() < 1e-6 && (r[0] - 0.7).abs() < 1e-6);
    assert!((l[851] - 0.3 * 0.6).abs() < 1e-6);
}

#[test]
fn mix_glides_in_forty_five_sample_steps() {
    // A steady input, and the wet sound not yet arrived: out = 1 − mix.
    let mut k = make(ID, SR, BARE);
    set(&mut k, &[("mix", 0.0)]);
    let x = vec![1.0; 200];
    let (l, _) = run(&mut k, &x, &x);
    assert!(l[..44].iter().all(|&v| v.abs() < 1e-7));
    assert!((l[44] - 0.1).abs() < 1e-6, "{}", l[44]);
    assert!((l[88] - 0.1).abs() < 1e-6);
    assert!((l[89] - 0.19).abs() < 1e-6, "{}", l[89]);
}

#[test]
fn the_ages_scale_with_the_rate() {
    let sr = 48_000.0;
    let x = impulse(2_000);
    let (l, _) = run(&mut make(ID, sr, BARE), &x, &x);
    let at = (851.0f32 * sr / 44_100.0).round() as usize;
    close(&nonzero(&l), &[(at, 0.6)]);
}

#[test]
fn defaults_and_pad_are_the_originals() {
    let k = prism_fx::make(ID, SR).unwrap();
    let d = k.descriptor();
    let value = |key: &str| k.param(d.index_of(key).unwrap());
    assert_eq!(
        [value("mix"), value("feedback"), value("diffusion"), value("brightness"), value("smoothing")],
        [50.0, 20.0, 85.0, 50.0, 25.0]
    );
    let pad = d.panel.pad.unwrap();
    assert_eq!((pad.x, pad.x_range, pad.y, pad.y_range), ("mix", (0.0, 100.0), "feedback", (0.0, 100.0)));
    assert_eq!(d.spec("diffusion").unwrap().max, 150.0);
    assert_eq!(d.label, "Hyperprism Hall Reverb");
    assert_eq!(d.source, "Hyperprism-PPC 1.2.1: Hall Reverb");
    assert_eq!(d.category, prism_fx::Category::Reverb);
}
