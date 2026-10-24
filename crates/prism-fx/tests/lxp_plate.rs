//! LXP Plate: Lexicon's LXP Native Plate, from its readable files and an
//! engine of this crate's own — the original's code is encrypted.

#[path = "support/lxp.rs"]
mod lxp;

use lxp::*;

const ID: &str = "lxp-plate";

#[test]
fn the_reverb_time_is_the_time_set() {
    reverb_time_follows_the_control(ID, &[0.5, 1.5, 3.0]);
}

#[test]
fn size_changes_the_space_but_not_the_reverb_time() {
    lxp::size_changes_the_space_but_not_the_reverb_time(ID);
}

#[test]
fn bass_rt_lengthens_the_lows() {
    lxp::bass_rt_lengthens_the_lows(ID);
}

#[test]
fn rt_hi_cut_shortens_the_highs() {
    lxp::rt_hi_cut_shortens_the_highs(ID);
}

#[test]
fn predelay_holds_back_the_tail_and_not_the_early_voices() {
    lxp::predelay_holds_back_the_tail_and_not_the_early_voices(ID);
}

#[test]
fn the_bottom_of_each_level_is_off() {
    lxp::the_bottom_of_each_level_is_off(ID);
}

#[test]
fn mix_at_zero_is_the_dry_sound() {
    lxp::mix_at_zero_is_the_dry_sound(ID);
}

#[test]
fn shape_moves_the_injection_later() {
    lxp::shape_moves_the_injection_later(ID);
}

#[test]
fn spread_widens_the_injection() {
    lxp::spread_widens_the_injection(ID);
}

#[test]
fn tap_slope_tilts_the_output() {
    lxp::tap_slope_tilts_the_output(ID);
}

#[test]
fn spin_at_zero_holds_the_wander_still() {
    lxp::spin_at_zero_holds_the_wander_still(ID);
}

#[test]
fn wander_smooths_a_ringing_tail() {
    lxp::wander_smooths_a_ringing_tail(ID);
}

#[test]
fn rolloff_darkens_the_wet_sound() {
    lxp::rolloff_darkens_the_wet_sound(ID);
}

#[test]
fn diffusion_thickens_the_attack() {
    lxp::diffusion_thickens_the_attack(ID);
}

#[test]
fn one_side_in_reaches_both_sides_out() {
    lxp::one_side_in_reaches_both_sides_out(ID);
}

#[test]
fn the_tail_sits_near_the_input_level() {
    lxp::the_tail_sits_near_the_input_level(ID);
}

#[test]
fn the_tail_dies_away() {
    lxp::the_tail_dies_away(ID);
}

const GAINS: &[&str] = &["lRflDb", "rRflDb"];

#[test]
fn each_reflection_stays_on_its_side() {
    a_reflection_lands_on_its_time(ID, "lRflMs", "lRflDb", true, true, GAINS);
    a_reflection_lands_on_its_time(ID, "rRflMs", "rRflDb", false, false, GAINS);
}

/// The early voices alone, the reflections silent, one click in on the left.
fn echoes(settings: &[(&str, f32)]) -> (Vec<f32>, Vec<f32>) {
    let base: &[(&str, f32)] = &[
        ("lRflDb", -80.0),
        ("rRflDb", -80.0),
        ("ekoTimeMaster", 1.0),
        ("ekoFbMaster", 1.0),
        ("lEkoMs", 100.0),
        ("rEkoMs", 170.0),
        ("lEkoFbDb", -6.0),
        ("rEkoFbDb", -6.0),
        ("lEkoPolarity", 0.0),
        ("rEkoPolarity", 0.0),
    ];
    let x = click(secs(1.0));
    let silent = vec![0.0; x.len()];
    run(&mut make(ID, &[EARLY, base, settings].concat()), &x, &silent)
}

/// What a diffused echo puts out around `ms`: its gain, with its sign.
fn echo_at(y: &[f32], ms: f32) -> f32 {
    let at = (ms * SR / 1000.0).round() as usize;
    y[at..at + secs(0.03)].iter().sum()
}

#[test]
fn an_echo_repeats_at_its_time_and_feeds_back() {
    let (l, r) = echoes(&[]);
    let g = 10f32.powf(-6.0 / 20.0);
    assert_eq!(onset(&l, 1e-6), secs(0.1));
    assert!((echo_at(&l, 100.0) - 1.0).abs() < 0.02, "{}", echo_at(&l, 100.0));
    assert!((echo_at(&l, 200.0) - g).abs() < 0.02, "{}", echo_at(&l, 200.0));
    assert!((echo_at(&l, 300.0) - g * g).abs() < 0.02, "{}", echo_at(&l, 300.0));
    // The left echo stays on the left.
    assert!(r[..secs(0.5)].iter().all(|&s| s == 0.0));
}

#[test]
fn polarity_turns_each_repeat_over() {
    let (l, _) = echoes(&[("lEkoPolarity", 1.0)]);
    assert!(echo_at(&l, 100.0) > 0.9);
    assert!(echo_at(&l, 200.0) < -0.4, "{}", echo_at(&l, 200.0));
    assert!(echo_at(&l, 300.0) > 0.2, "{}", echo_at(&l, 300.0));
}

#[test]
fn the_masters_scale_every_echo() {
    let (l, _) = echoes(&[("ekoTimeMaster", 0.5)]);
    assert_eq!(onset(&l, 1e-6), secs(0.05));
    let (l, _) = echoes(&[("ekoFbMaster", 0.0)]);
    assert!((echo_at(&l, 100.0) - 1.0).abs() < 0.02);
    // Only the echo's own all-pass, dying away.
    assert!(l[secs(0.15)..].iter().all(|&s| s.abs() < 1e-6), "a repeat with the feedback master at zero");
}

#[test]
fn echoes_are_not_held_back_by_the_predelay() {
    let (l, _) = echoes(&[("predelayMs", 300.0)]);
    assert_eq!(onset(&l, 1e-6), secs(0.1));
}

#[test]
fn full_feedback_rings_on_without_running_away() {
    let (l, _) = echoes(&[("lEkoFbDb", 0.0), ("lEkoMs", 10.0)]);
    let late = l[secs(0.9)..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(late > 0.01 && late < 1.0, "{late}");
}

/// How many samples of the first 60 ms of the tank are loud, a measure of how
/// quickly the echoes thicken.
fn density(id: &str) -> usize {
    let y = response(id, &tail(id, &[("sizeM", 30.0), ("spread", 0.0), ("diffusion", 0.0), ("reverbTimeS", 2.0)]), 0.5);
    let start = onset(&y, 1e-5);
    let w = &y[start..start + secs(0.06)];
    let peak = w.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    w.iter().filter(|s| s.abs() > 0.1 * peak).count()
}

#[test]
fn it_thickens_faster_than_the_hall() {
    let (this, hall) = (density(ID), density("lxp-hall"));
    assert!(this as f32 > hall as f32 * 1.5, "{this} against the hall's {hall}");
}
