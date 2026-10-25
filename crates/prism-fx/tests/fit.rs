//! Amp fit: MultiGrainLive's live amplitude fit (fx::shape::AmplitudeFit).

#[path = "support/shape_golden.rs"]
mod golden;
#[path = "support/shape_parity.rs"]
mod parity;

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::check("fit");
}

#[test]
fn it_is_multigrainlives_amp_fit() {
    parity::check_descriptor(
        "fit",
        72,
        "Amp fit",
        &[
            ("grainMs", "Grain", 5.0, 500.0, 30.0, true, "ms"),
            ("amount", "Amount", 0.0, 1.0, 0.7, false, ""),
            ("floorDb", "Floor", -80.0, -20.0, -50.0, false, "dB"),
        ],
    );
}

#[test]
fn a_quiet_tone_comes_up_toward_point_nine() {
    let mut k = parity::make("fit");
    k.set_param(1, 1.0);
    let mut peak = 0f32;
    for i in 0..48_000 {
        let x = 0.05 * (std::f32::consts::TAU * 220.0 * i as f32 / 48_000.0).sin();
        let mut f = [x, x];
        k.tick(&mut f);
        if i > 24_000 {
            peak = peak.max(f[0].abs());
        }
    }
    assert!(peak > 0.7 && peak < 1.0, "{peak}");
}
