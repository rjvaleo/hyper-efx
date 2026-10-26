//! Gain: MultiGrainLive's rack gain slot (fx::Gain).

#[path = "support/shape_golden.rs"]
mod golden;
#[path = "support/shape_parity.rs"]
mod parity;

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::check("gain");
}

#[test]
fn it_is_multigrainlives_gain_slot() {
    // The slot has no ParamSpec of its own; this is its range
    // (GAIN_DB_MIN..GAIN_DB_MAX), its document default and its automation
    // label.
    parity::check_descriptor("gain", 76, "Gain", &[("db", "Level", -24.0, 24.0, 0.0, false, "dB")]);
}

#[test]
fn six_db_doubles() {
    let mut k = parity::make("gain");
    k.set_param(0, 6.0206);
    let mut f = [0.25, -0.25];
    k.tick(&mut f);
    assert!((f[0] - 0.5).abs() < 1e-4 && (f[1] + 0.5).abs() < 1e-4, "{f:?}");
}
