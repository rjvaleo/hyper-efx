//! Utility: MultiGrainLive's gain, polarity, swap, width and amp fit in one (fx::shape::Utility).

#[path = "support/shape_golden.rs"]
mod golden;
#[path = "support/shape_parity.rs"]
mod parity;

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::check("utility");
}

#[test]
fn it_is_multigrainlives_utility() {
    parity::check_descriptor(
        "utility",
        74,
        "Utility",
        &[
            ("gainDb", "Gain", -24.0, 24.0, 0.0, false, "dB"),
            ("invert", "Invert", 0.0, 1.0, 0.0, false, ""),
            ("swap", "Swap", 0.0, 1.0, 0.0, false, ""),
            ("width", "Width", 0.0, 2.0, 1.0, false, "x"),
            ("ampFit", "Amp fit", 0.0, 1.0, 0.0, false, ""),
            ("grainMs", "Fit grain", 5.0, 500.0, 30.0, true, "ms"),
            ("amount", "Fit amount", 0.0, 1.0, 0.7, false, ""),
            ("floorDb", "Fit floor", -80.0, -20.0, -50.0, false, "dB"),
        ],
    );
}

#[test]
fn at_rest_it_passes_the_input() {
    let mut k = parity::make("utility");
    let mut f = [0.3, -0.7];
    k.tick(&mut f);
    assert_eq!(f, [0.3, -0.7]);
}
