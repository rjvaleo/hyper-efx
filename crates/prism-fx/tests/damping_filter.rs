//! Damping filter: the port of MultiGrainLive's `ShapeKind::DampingFilter` renders what
//! the original renders, and carries the original's control table.

#[path = "support/mgl_dattorro.rs"]
mod parity;

const ID: &str = "damping-filter";

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::matches_multigrainlive(ID);
}

#[test]
fn it_carries_multigrainlives_control_table() {
    parity::carries_the_table(ID, "Damping filter", &[
        ("cutoffHz", "Cutoff", 20.0, 20000.0, 6000.0, true, "Hz"),
        ("wet", "Wet", 0.0, 1.0, 1.0, false, ""),
        ("dry", "Dry", 0.0, 1.0, 0.0, false, ""),
    ]);
}
