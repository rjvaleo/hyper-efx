//! Doppler: the port of MultiGrainLive's `ShapeKind::Doppler` renders what
//! the original renders, and carries the original's control table.

#[path = "support/mgl_dattorro.rs"]
mod parity;

const ID: &str = "doppler";

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::matches_multigrainlive(ID);
}

#[test]
fn it_carries_multigrainlives_control_table() {
    parity::carries_the_table(ID, "Doppler", &[
        ("distanceMs", "Distance", 0.1, 1500.0, 40.0, true, "ms"),
        ("velocityMps", "Velocity", -150.0, 150.0, 0.0, false, "m/s"),
        ("wet", "Wet", 0.0, 1.0, 1.0, false, ""),
        ("dry", "Dry", 0.0, 1.0, 0.0, false, ""),
        ("speedMps", "Speed of sound", 20.0, 2000.0, 343.0, true, "m/s"),
    ]);
}
