//! LXP Hall: Lexicon's LXP Native Hall, from its readable files and an
//! engine of this crate's own — the original's code is encrypted.

#[path = "support/lxp.rs"]
mod lxp;

use lxp::*;

const ID: &str = "lxp-hall";

#[test]
fn the_reverb_time_is_the_time_set() {
    reverb_time_follows_the_control(ID, &[0.5, 1.5, 4.0]);
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
