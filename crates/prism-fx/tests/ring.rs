//! Ring mod: MultiGrainLive's ring modulator (fx::shape::RingMod).

#[path = "support/shape_golden.rs"]
mod golden;
#[path = "support/shape_parity.rs"]
mod parity;

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::check("ring");
}

#[test]
fn it_is_multigrainlives_ring_mod() {
    parity::check_descriptor(
        "ring",
        69,
        "Ring mod",
        &[
            ("hz", "Frequency", 0.1, 8000.0, 220.0, true, "Hz"),
            ("mix", "Mix", 0.0, 1.0, 1.0, false, ""),
            ("sweep", "Sweep", -2000.0, 2000.0, 0.0, false, "Hz/s"),
        ],
    );
    let pad = prism_fx::find("ring").unwrap().descriptor.panel.pad.expect("a pad");
    assert_eq!((pad.x, pad.x_range, pad.y, pad.y_range), ("hz", (0.1, 4_000.0), "mix", (0.0, 1.0)));
}

#[test]
fn no_mix_passes_the_input() {
    let mut k = parity::make("ring");
    k.set_param(1, 0.0);
    for i in 0..1000 {
        let x = (i as f32 * 0.01).sin();
        let mut f = [x, -x];
        k.tick(&mut f);
        assert_eq!(f, [x, -x]);
    }
}
