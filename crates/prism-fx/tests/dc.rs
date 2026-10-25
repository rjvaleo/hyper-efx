//! DC: MultiGrainLive's DC offset filter (fx::shape::DcOffset).

#[path = "support/shape_golden.rs"]
mod golden;
#[path = "support/shape_parity.rs"]
mod parity;

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::check("dc");
}

#[test]
fn it_is_multigrainlives_dc() {
    parity::check_descriptor("dc", 68, "DC", &[("hz", "Below", 1.0, 60.0, 5.0, true, "Hz")]);
}

#[test]
fn an_offset_is_taken_out() {
    let mut k = parity::make("dc");
    let mut f = [0.0; 2];
    for _ in 0..48_000 {
        f = [0.3, -0.2];
        k.tick(&mut f);
    }
    assert!(f[0].abs() < 1e-4 && f[1].abs() < 1e-4, "{f:?}");
}
