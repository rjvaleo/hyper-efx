//! Boomerang: MultiGrainLive's live reverse boomerang (fx::shape::ReverseMix).

#[path = "support/shape_golden.rs"]
mod golden;
#[path = "support/shape_parity.rs"]
mod parity;

#[test]
fn it_renders_what_multigrainlive_renders() {
    parity::check("boomerang");
}

#[test]
fn it_is_multigrainlives_boomerang() {
    parity::check_descriptor(
        "boomerang",
        71,
        "Boomerang",
        &[("throwMs", "Throw", 20.0, 2000.0, 400.0, true, "ms"), ("mix", "Mix", 0.0, 1.0, 0.5, false, "")],
    );
}

#[test]
fn the_recent_past_comes_back_reversed() {
    // All wet, a 100 ms throw: a ramp in comes out falling.
    let mut k = parity::make("boomerang");
    k.set_param(0, 100.0);
    k.set_param(1, 1.0);
    let out: Vec<f32> = (0..4_800)
        .map(|i| {
            let mut f = [i as f32 / 4_800.0; 2];
            k.tick(&mut f);
            f[0]
        })
        .collect();
    // Mid-way through the second pass (the first reaches back before the
    // input began), well away from the faded crossing, each frame reads one
    // further back than the last.
    assert!(out[4_400..4_500].windows(2).all(|w| w[1] < w[0]), "{:?}", &out[4_400..4_410]);
}
