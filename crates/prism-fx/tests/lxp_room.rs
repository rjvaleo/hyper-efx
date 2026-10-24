//! LXP Room: Lexicon's LXP Native Room, from its readable files and an
//! engine of this crate's own — the original's code is encrypted.

#[path = "support/lxp.rs"]
mod lxp;

use lxp::*;

const ID: &str = "lxp-room";

#[test]
fn the_reverb_time_is_the_time_set() {
    reverb_time_follows_the_control(ID, &[0.3, 0.8, 2.0]);
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

const GAINS: &[&str] = &["llRflDb", "lrRflDb", "rrRflDb", "rlRflDb"];

#[test]
fn each_reflection_crosses_as_its_name_says() {
    a_reflection_lands_on_its_time(ID, "llRflMs", "llRflDb", true, true, GAINS);
    a_reflection_lands_on_its_time(ID, "lrRflMs", "lrRflDb", true, false, GAINS);
    a_reflection_lands_on_its_time(ID, "rrRflMs", "rrRflDb", false, false, GAINS);
    a_reflection_lands_on_its_time(ID, "rlRflMs", "rlRflDb", false, true, GAINS);
}

#[test]
fn a_large_room_lets_its_reflections_through() {
    // The manual: in the Room, Size sets how grainy the reverb is. Grainy is
    // few loud moments in the first tenth of a second.
    let loud = |size: f32| {
        let y = response(ID, &tail(ID, &[("sizeM", size), ("spread", 0.0), ("diffusion", 0.0), ("reverbTimeS", 1.0)]), 0.6);
        let start = onset(&y, 1e-5);
        let w = &y[start..start + secs(0.1)];
        let peak = w.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        w.iter().filter(|s| s.abs() > 0.1 * peak).count()
    };
    assert!(loud(8.0) as f32 > loud(60.0) as f32 * 1.5, "{} against {}", loud(8.0), loud(60.0));
}
