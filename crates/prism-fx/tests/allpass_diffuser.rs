//! All-pass diffuser: the port of MultiGrainLive's `ShapeKind::AllpassDiffuser` renders what
//! the original renders, and carries the original's control table.

#[path = "support/mgl_dattorro.rs"]
mod parity;

const ID: &str = "allpass-diffuser";

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::matches_multigrainlive(ID);
}

#[test]
fn it_carries_multigrainlives_control_table() {
    parity::carries_the_table(ID, "All-pass diffuser", &[
        ("delayMs", "Delay", 0.1, 100.0, 12.0, true, "ms"),
        ("coefficient", "Coefficient", -0.999, 0.999, 0.625, false, ""),
        ("wet", "Wet", 0.0, 1.0, 1.0, false, ""),
        ("dry", "Dry", 0.0, 1.0, 0.0, false, ""),
    ]);
}
