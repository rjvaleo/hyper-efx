//! Dattorro vibrato: the port of MultiGrainLive's `ShapeKind::Vibrato` renders what
//! the original renders, and carries the original's control table.

#[path = "support/mgl_dattorro.rs"]
mod parity;

const ID: &str = "vibrato";

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::matches_multigrainlive(ID);
}

#[test]
fn it_carries_multigrainlives_control_table() {
    parity::carries_the_table(ID, "Dattorro vibrato", &[
        ("delayMs", "Delay", 0.1, 100.0, 8.0, true, "ms"),
        ("depthMs", "Depth", 0.0, 50.0, 3.0, false, "ms"),
        ("rateHz", "Rate", 0.01, 20.0, 0.3, true, "Hz"),
        ("feedback", "Feedback", -0.98, 0.98, 0.0, false, ""),
        ("wet", "Wet", 0.0, 1.0, 1.0, false, ""),
        ("dry", "Dry", 0.0, 1.0, 0.0, false, ""),
        ("stereo", "Stereo phase", 0.0, 1.0, 0.5, false, ""),
        ("allpass", "All-pass interp", 0.0, 1.0, 1.0, false, ""),
    ]);
}
