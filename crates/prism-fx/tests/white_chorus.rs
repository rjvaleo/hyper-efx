//! White chorus: the port of MultiGrainLive's `ShapeKind::WhiteChorus` renders what
//! the original renders, and carries the original's control table.

#[path = "support/mgl_dattorro.rs"]
mod parity;

const ID: &str = "white-chorus";

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::matches_multigrainlive(ID);
}

#[test]
fn it_carries_multigrainlives_control_table() {
    parity::carries_the_table(ID, "White chorus", &[
        ("delayMs", "Delay", 0.1, 100.0, 14.0, true, "ms"),
        ("depthMs", "Depth", 0.0, 50.0, 7.0, false, "ms"),
        ("rateHz", "Rate", 0.01, 20.0, 0.55, true, "Hz"),
        ("feedback", "Feedback", -0.98, 0.98, 0.12, false, ""),
        ("wet", "Wet", 0.0, 1.0, 0.8, false, ""),
        ("dry", "Dry", 0.0, 1.0, 1.0, false, ""),
        ("stereo", "Stereo phase", 0.0, 1.0, 0.5, false, ""),
        ("allpass", "All-pass interp", 0.0, 1.0, 0.0, false, ""),
    ]);
}
