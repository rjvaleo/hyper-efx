//! Invert: MultiGrainLive's polarity flip (fx::shape::Invert).

#[path = "support/shape_golden.rs"]
mod golden;
#[path = "support/shape_parity.rs"]
mod parity;

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::check("invert");
}

#[test]
fn it_is_multigrainlives_invert() {
    parity::check_descriptor("invert", 65, "Invert", &[] as &[parity::Spec]);
}

#[test]
fn it_flips_both_sides() {
    let mut k = parity::make("invert");
    let mut f = [0.25, -0.5];
    k.tick(&mut f);
    assert_eq!(f, [-0.25, 0.5]);
}
