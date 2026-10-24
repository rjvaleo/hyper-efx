//! Hyperprism Medium Room Reverb: EReverb's MediumRoomMono, as the 1.2.1 code
//! does it.

#[path = "support/hp_fr_measure.rs"]
mod m;
use m::*;

const ID: &str = "hp-medium-room";

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
fn the_taps_read_two_halves_each_fed_the_input() {
    // The input is written at age 0 and again at age 6646, so a tap past 6646
    // hears it (age − 6646) samples late.
    let x = impulse(14_000);
    let (l, r) = run(&mut make(ID, SR, BARE), &x, &x);
    close(&nonzero(&l), &[(1_600, -0.5), (1_800, -0.5), (2_672, 0.4), (6_007, 0.5)]);
    close(&nonzero(&r), &[(1_599, 0.5), (1_807, 0.5), (2_699, 0.4), (6_000, 0.5)]);
}

#[test]
fn feedback_carries_the_first_half_into_the_second_and_closes_the_loop() {
    let x = impulse(9_000);
    let (l, r) = run(&mut make(ID, SR, &with(&[("feedback", 50.0)])), &x, &x);
    // Mid injection: age 6646 += fb·age 6645, heard at 8446 one sample early.
    assert!((l[8_445] - (-0.25)).abs() < 1e-6, "{}", l[8_445]);
    assert!((r[8_452] - 0.25).abs() < 1e-6, "{}", r[8_452]);
    // The loop: fb·age 13106 back in at age 1, which the second half's direct
    // copy reaches after 6460 samples: an echo entering at 6459.
    assert!((l[6_459 + 1_600] - (-0.25)).abs() < 1e-6, "{}", l[6_459 + 1_600]);
}

#[test]
fn the_mid_injection_takes_the_feedback_unsmoothed() {
    // At 100 % smoothing the loop's feedback has barely moved after 8000
    // samples; the mid injection uses the new value at once.
    let mut k = make(ID, SR, &with(&[("smoothing", 100.0)]));
    set(&mut k, &[("feedback", 50.0)]);
    let x = impulse(9_000);
    let (l, _) = run(&mut k, &x, &x);
    assert!((l[8_445] - (-0.25)).abs() < 1e-6, "{}", l[8_445]);
    assert!(l[6_459 + 1_600].abs() < 0.01, "{}", l[6_459 + 1_600]);
}

#[test]
fn diffusion_scales_the_outer_allpass_first_seen_at_the_1600_tap() {
    // Allpass 1 runs 3 → 1541 with g 0.3·d: its direct path puts −g at 1541
    // at once, heard at the 1599 and 1600 taps.
    let x = impulse(90);
    let (l, r) = run(&mut make(ID, SR, &with(&[("diffusion", 100.0)])), &x, &x);
    close(&nonzero(&l), &[(62, 0.15)]);
    close(&nonzero(&r), &[(61, -0.15)]);
}

#[test]
fn full_diffusion_stays_stable() {
    // At 150 % the inner allpass's gain would be 1.05; it is held below one.
    let x = sine(300.0, 4 * 44_100, SR, 0.5);
    let (l, r) = run(&mut make(ID, SR, &[("diffusion", 150.0), ("feedback", 50.0), ("mix", 100.0)]), &x, &x);
    let peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak.is_finite() && peak < 100.0, "{peak}");
}

#[test]
fn the_ages_scale_with_the_rate() {
    let sr = 48_000.0;
    let x = impulse(1_800);
    let (l, r) = run(&mut make(ID, sr, BARE), &x, &x);
    assert_eq!(nonzero(&l)[0].0, 1_741);
    assert_eq!(nonzero(&r)[0].0, 1_740);
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
    assert_eq!(d.label, "Hyperprism Medium Room Reverb");
    assert_eq!(d.source, "Hyperprism-PPC 1.2.1: Medium Room Reverb");
}
