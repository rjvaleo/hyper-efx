//! Leslie speaker: the port of MultiGrainLive's `ShapeKind::Leslie` renders what
//! the original renders, and carries the original's control table.

#[path = "support/mgl_dattorro.rs"]
mod parity;

const ID: &str = "leslie";

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::matches_multigrainlive(ID);
}

#[test]
fn it_carries_multigrainlives_control_table() {
    parity::carries_the_table(ID, "Leslie speaker", &[
        ("rateHz", "Rate", 0.1, 12.0, 1.2, true, "Hz"),
        ("depthMs", "Doppler", 0.0, 8.0, 2.0, false, "ms"),
        ("amDepth", "Amplitude", 0.0, 1.0, 0.35, false, ""),
        ("width", "Stereo width", 0.0, 1.0, 1.0, false, ""),
        ("wet", "Wet", 0.0, 1.0, 1.0, false, ""),
        ("dry", "Dry", 0.0, 1.0, 0.0, false, ""),
        ("baseMs", "Distance", 0.5, 40.0, 3.0, false, "ms"),
    ]);
}
