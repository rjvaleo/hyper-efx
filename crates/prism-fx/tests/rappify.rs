//! Rappify: MultiGrainLive's envelope-swept band pass (fx::shape::Rappify).

#[path = "support/shape_golden.rs"]
mod golden;
#[path = "support/shape_parity.rs"]
mod parity;

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::check("rappify");
}

#[test]
fn it_is_multigrainlives_rappify() {
    parity::check_descriptor(
        "rappify",
        70,
        "Rappify",
        &[
            ("amount", "Amount", 0.0, 1.0, 0.6, false, ""),
            ("hz", "Centre", 60.0, 6000.0, 400.0, true, "Hz"),
            ("speed", "Speed", 1.0, 200.0, 40.0, true, "Hz"),
        ],
    );
}

#[test]
fn no_amount_passes_the_input() {
    let mut k = parity::make("rappify");
    k.set_param(0, 0.0);
    let mut f = [0.3, -0.7];
    k.tick(&mut f);
    assert_eq!(f, [0.3, -0.7]);
}
