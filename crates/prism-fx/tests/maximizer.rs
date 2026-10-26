//! Maximizer: MultiGrainLive's one-knob channel compressor (fx::master::Maximizer).

#[path = "support/shape_golden.rs"]
mod golden;
#[path = "support/shape_parity.rs"]
mod parity;

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::check("maximizer");
}

#[test]
fn it_is_multigrainlives_maximizer() {
    parity::check_descriptor(
        "maximizer",
        75,
        "Maximizer",
        &[
            ("amount", "Amount", 0.0, 1.0, 0.5, false, ""),
            ("ceilingDb", "Ceiling", -24.0, 0.0, -0.3, false, "dB"),
            ("autoLevel", "Auto level", 0.0, 1.0, 1.0, false, ""),
            ("autoComp", "Auto compression", 0.0, 1.0, 1.0, false, ""),
        ],
    );
}

#[test]
fn nothing_passes_the_ceiling() {
    let mut k = parity::make("maximizer");
    k.set_param(1, -6.0);
    let ceiling = 10f32.powf(-6.0 / 20.0);
    for i in 0..48_000 {
        let x = 2.0 * (std::f32::consts::TAU * 220.0 * i as f32 / 48_000.0).sin();
        let mut f = [x, -x];
        k.tick(&mut f);
        assert!(f[0].abs() <= ceiling && f[1].abs() <= ceiling, "{f:?}");
    }
}
