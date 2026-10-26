//! Dattorro filter bank: the port of MultiGrainLive's `ShapeKind::DattorroFilterBank` renders what
//! the original renders, and carries the original's control table.

#[path = "support/mgl_dattorro.rs"]
mod parity;

const ID: &str = "dattorro-filter-bank";

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::matches_multigrainlive(ID);
}

#[test]
fn it_carries_multigrainlives_control_table() {
    parity::carries_the_table(ID, "Dattorro filter bank", &[
        ("notchOn", "Notch", 0.0, 1.0, 1.0, false, ""),
        ("notchHz", "Notch frequency", 20.0, 20000.0, 500.0, true, "Hz"),
        ("notchQ", "Notch Q", 0.2, 18.0, 4.0, false, ""),
        ("notchAmp", "Notch amplitude", 0.0, 1.0, 1.0, false, ""),
        ("resonatorOn", "Resonator", 0.0, 1.0, 1.0, false, ""),
        ("resonatorHz", "Resonator frequency", 20.0, 20000.0, 1200.0, true, "Hz"),
        ("resonatorQ", "Resonator Q", 0.2, 18.0, 5.0, false, ""),
        ("resonatorAmp", "Resonator amplitude", 0.0, 1.0, 0.7, false, ""),
        ("regaliaOn", "Regalia-Mitra", 0.0, 1.0, 1.0, false, ""),
        ("regaliaHz", "Regalia frequency", 20.0, 20000.0, 3000.0, true, "Hz"),
        ("regaliaQ", "Regalia Q", 0.2, 18.0, 2.0, false, ""),
        ("regaliaAmp", "Regalia amplitude", 0.0, 1.0, 0.7, false, ""),
        ("dampingOn", "Damping", 0.0, 1.0, 1.0, false, ""),
        ("dampingHz", "Damping cutoff", 20.0, 20000.0, 6000.0, true, "Hz"),
        ("dampingQ", "Damping Q", 0.2, 18.0, 0.7, false, ""),
        ("dampingAmp", "Damping amplitude", 0.0, 1.0, 1.0, false, ""),
    ]);
}
