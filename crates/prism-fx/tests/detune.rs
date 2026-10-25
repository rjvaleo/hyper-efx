//! Detune: the port of MultiGrainLive's `ShapeKind::Detune` renders what
//! the original renders, and carries the original's control table.

#[path = "support/mgl_dattorro.rs"]
mod parity;

const ID: &str = "detune";

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::matches_multigrainlive(ID);
}

#[test]
fn it_carries_multigrainlives_control_table() {
    parity::carries_the_table(ID, "Detune", &[
        ("cents", "Detune", 1.0, 100.0, 12.0, true, "ct"),
        ("windowMs", "Window", 10.0, 250.0, 80.0, true, "ms"),
        ("wet", "Wet", 0.0, 1.0, 0.65, false, ""),
        ("dry", "Dry", 0.0, 1.0, 1.0, false, ""),
    ]);
}
