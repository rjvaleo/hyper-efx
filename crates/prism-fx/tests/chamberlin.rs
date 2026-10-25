//! Chamberlin filter: the port of MultiGrainLive's `ShapeKind::Chamberlin` renders what
//! the original renders, and carries the original's control table.

#[path = "support/mgl_dattorro.rs"]
mod parity;

const ID: &str = "chamberlin";

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::matches_multigrainlive(ID);
}

#[test]
fn it_carries_multigrainlives_control_table() {
    parity::carries_the_table(ID, "Chamberlin filter", &[
        ("lowOn", "Low on", 0.0, 1.0, 1.0, false, ""),
        ("lowFreq", "Low frequency", 20.0, 18_000.0, 1000.0, true, "Hz"),
        ("lowQ", "Low Q", 0.2, 10.0, 0.7, false, ""),
        ("lowAmp", "Low amplitude", 0.0, 1.0, 1.0, false, ""),
        ("bandOn", "Band on", 0.0, 1.0, 0.0, false, ""),
        ("bandFreq", "Band frequency", 20.0, 18_000.0, 1000.0, true, "Hz"),
        ("bandQ", "Band Q", 0.2, 10.0, 1.0, false, ""),
        ("bandAmp", "Band amplitude", 0.0, 1.0, 0.0, false, ""),
        ("highOn", "High on", 0.0, 1.0, 0.0, false, ""),
        ("highFreq", "High frequency", 20.0, 18_000.0, 1000.0, true, "Hz"),
        ("highQ", "High Q", 0.2, 10.0, 0.7, false, ""),
        ("highAmp", "High amplitude", 0.0, 1.0, 0.0, false, ""),
        ("notchOn", "Notch on", 0.0, 1.0, 0.0, false, ""),
        ("notchFreq", "Notch frequency", 20.0, 18_000.0, 1000.0, true, "Hz"),
        ("notchQ", "Notch Q", 0.2, 10.0, 1.0, false, ""),
        ("notchAmp", "Notch amplitude", 0.0, 1.0, 0.0, false, ""),
        ("drive", "Drive", 0.25, 8.0, 1.0, true, "x"),
    ]);
}
