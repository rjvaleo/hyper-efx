//! Width: MultiGrainLive's mid/side width (fx::shape::Width).

#[path = "support/shape_golden.rs"]
mod golden;
#[path = "support/shape_parity.rs"]
mod parity;

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::check("width");
}

#[test]
fn it_is_multigrainlives_width() {
    parity::check_descriptor("width", 67, "Width", &[("width", "Width", 0.0, 2.0, 1.0, false, "x")]);
}

#[test]
fn zero_collapses_to_the_middle() {
    let mut k = parity::make("width");
    k.set_param(0, 0.0);
    let mut f = [1.0, 0.0];
    k.tick(&mut f);
    assert_eq!(f, [0.5, 0.5]);
}
