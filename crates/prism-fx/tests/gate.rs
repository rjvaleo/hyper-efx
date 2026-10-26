//! Gate: MultiGrainLive's noise gate (fx::shape::Gate).

#[path = "support/shape_golden.rs"]
mod golden;
#[path = "support/shape_parity.rs"]
mod parity;

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::check("gate");
}

#[test]
fn it_is_multigrainlives_gate() {
    parity::check_descriptor(
        "gate",
        73,
        "Gate",
        &[
            ("thresholdDb", "Threshold", -80.0, 0.0, -40.0, false, "dB"),
            ("attackMs", "Attack", 0.1, 200.0, 3.0, true, "ms"),
            ("releaseMs", "Release", 5.0, 2000.0, 120.0, true, "ms"),
            ("depth", "Depth", 0.0, 1.0, 1.0, false, ""),
        ],
    );
    let pad = prism_fx::find("gate").unwrap().descriptor.panel.pad.expect("a pad");
    assert_eq!((pad.x, pad.x_range, pad.y, pad.y_range), ("attackMs", (0.1, 200.0), "thresholdDb", (-80.0, 0.0)));
}

#[test]
fn it_shuts_under_the_threshold() {
    let mut k = parity::make("gate");
    let mut f = [0.0; 2];
    for _ in 0..48_000 {
        f = [0.001, 0.001];
        k.tick(&mut f);
    }
    assert!(f[0].abs() < 1e-6, "{f:?}");
}
