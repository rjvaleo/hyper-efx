//! Multibit PN noise: the port of MultiGrainLive's `ShapeKind::PnNoise` renders what
//! the original renders, and carries the original's control table.

#[path = "support/mgl_dattorro.rs"]
mod parity;

const ID: &str = "pn-noise";

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::matches_multigrainlive(ID);
}

#[test]
fn it_carries_multigrainlives_control_table() {
    parity::carries_the_table(ID, "Multibit PN noise", &[
        ("clockHz", "Clock", 1.0, 48000.0, 48000.0, true, "Hz"),
        ("level", "Level", 0.0, 1.0, 0.1, false, ""),
        ("mix", "Mix", 0.0, 1.0, 0.25, false, ""),
        ("seed", "Seed", 1.0, 16777215.0, 1.0, false, ""),
    ]);
}
