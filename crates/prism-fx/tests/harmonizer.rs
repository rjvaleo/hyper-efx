//! Harmonizer: the port of MultiGrainLive's `ShapeKind::Harmonizer` renders what
//! the original renders, and carries the original's control table.

#[path = "support/mgl_dattorro.rs"]
mod parity;

const ID: &str = "harmonizer";

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::matches_multigrainlive(ID);
}

#[test]
fn it_carries_multigrainlives_control_table() {
    parity::carries_the_table(ID, "Harmonizer", &[
        ("semitones", "Pitch", -24.0, 24.0, 7.0, false, "st"),
        ("windowMs", "Window", 10.0, 250.0, 60.0, true, "ms"),
        ("feedback", "Feedback", 0.0, 0.95, 0.0, false, ""),
        ("wet", "Wet", 0.0, 1.0, 0.5, false, ""),
        ("dry", "Dry", 0.0, 1.0, 1.0, false, ""),
    ]);
}
