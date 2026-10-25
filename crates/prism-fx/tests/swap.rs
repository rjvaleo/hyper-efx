//! Swap: MultiGrainLive's channel swap (fx::shape::SwapChannels).

#[path = "support/shape_golden.rs"]
mod golden;
#[path = "support/shape_parity.rs"]
mod parity;

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::check("swap");
}

#[test]
fn it_is_multigrainlives_swap() {
    parity::check_descriptor("swap", 66, "Swap", &[] as &[parity::Spec]);
}

#[test]
fn left_becomes_right() {
    let mut k = parity::make("swap");
    let mut f = [0.25, -0.5];
    k.tick(&mut f);
    assert_eq!(f, [-0.5, 0.25]);
}
