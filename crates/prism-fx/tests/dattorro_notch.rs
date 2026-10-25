//! Dattorro notch: the port of MultiGrainLive's `ShapeKind::DattorroNotch` renders what
//! the original renders, and carries the original's control table.

#[path = "support/mgl_dattorro.rs"]
mod parity;

const ID: &str = "dattorro-notch";

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::matches_multigrainlive(ID);
}

#[test]
fn it_carries_multigrainlives_control_table() {
    parity::carries_the_table(ID, "Dattorro notch", &[
        ("frequency", "Frequency", 20.0, 20_000.0, 1000.0, true, "Hz"),
        ("bandwidth", "Bandwidth", 1.0, 10_000.0, 200.0, true, "Hz"),
        ("gainDb", "Gain", -24.0, 24.0, 6.0, false, "dB"),
        ("mix", "Dry / wet", 0.0, 1.0, 1.0, false, ""),
    ]);
}
