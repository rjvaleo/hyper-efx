//! Doubler: the port of MultiGrainLive's `ShapeKind::Doubler` renders what
//! the original renders, and carries the original's control table.

#[path = "support/mgl_dattorro.rs"]
mod parity;

const ID: &str = "doubler";

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::matches_multigrainlive(ID);
}

#[test]
fn it_carries_multigrainlives_control_table() {
    parity::carries_the_table(ID, "Doubler", &[
        ("delayMs", "Delay", 5.0, 100.0, 32.0, true, "ms"),
        ("variationMs", "Variation", 0.0, 20.0, 3.0, false, "ms"),
        ("rateHz", "Rate", 0.05, 5.0, 0.35, true, "Hz"),
        ("wet", "Wet", 0.0, 1.0, 0.65, false, ""),
        ("dry", "Dry", 0.0, 1.0, 1.0, false, ""),
    ]);
}
