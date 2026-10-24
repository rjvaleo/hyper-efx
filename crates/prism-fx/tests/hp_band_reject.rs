//! Hyperprism Band Reject: EFilter2's Chamberlin notch, as the 1.2.1 code
//! does it.

#[path = "support/hp_fr_measure.rs"]
mod m;
use m::*;

const ID: &str = "hp-band-reject";

/// One Chamberlin iteration a sample, out = low + high:
/// `H = (u² + F²z⁻¹)/(u² + F²z⁻¹ + qF z⁻¹u)`, u = 1 − z⁻¹, F = 2 sin(πf/sr),
/// q = 1/Q.
fn formula_db(f: f64, q_big: f64, hz: f64) -> f64 {
    let sr = SR as f64;
    let f = f.max(1.0);
    let q_big = q_big.max((1.35 * f / (0.5 * sr - f)).powi(2));
    let (q, big_f) = (1.0 / q_big, 2.0 * (std::f64::consts::PI * f / sr).sin());
    let w = w_of(hz, SR);
    let z = (w.cos(), -w.sin());
    let mul = |a: (f64, f64), b: (f64, f64)| (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0);
    let u = (1.0 - z.0, -z.1);
    let u2 = mul(u, u);
    let num = (u2.0 + big_f * big_f * z.0, u2.1 + big_f * big_f * z.1);
    let zu = mul(z, u);
    let den = (num.0 + q * big_f * zu.0, num.1 + q * big_f * zu.1);
    20.0 * ((num.0 * num.0 + num.1 * num.1) / (den.0 * den.0 + den.1 * den.1)).sqrt().log10()
}

#[test]
fn the_measured_response_is_the_decoded_formula() {
    for (f, q) in [(1_000.0, 1.0), (1_000.0, 10.0), (200.0, 2.0), (2_000.0, 5.0)] {
        for hz in [50.0, 150.0, 500.0, 900.0, 1_100.0, 2_500.0, 10_000.0] {
            let want = formula_db(f, q, hz);
            if want < -60.0 {
                continue;
            }
            let got = gain_db(ID, SR, &[("freqHz", f as f32), ("q", q as f32)], hz);
            assert!((got - want).abs() < 0.1, "f {f} q {q} at {hz} Hz: {got:.3} dB, formula {want:.3}");
        }
    }
}

#[test]
fn the_notch_is_deep_at_its_frequency_and_its_width_is_f_over_q() {
    let at = gain_db(ID, SR, &[("freqHz", 1_000.0), ("q", 1.0)], 1_000.0);
    assert!(at < -50.0, "{at}");
    let near = gain_db(ID, SR, &[("freqHz", 1_000.0), ("q", 1.0)], 900.0);
    assert!((near - (-13.82)).abs() < 0.1, "{near}");
    let narrow = gain_db(ID, SR, &[("freqHz", 1_000.0), ("q", 10.0)], 900.0);
    assert!((narrow - (-1.09)).abs() < 0.1, "{narrow}");
}

#[test]
fn the_start_point_and_pad_are_the_originals() {
    let k = prism_fx::make(ID, SR).unwrap();
    let d = k.descriptor();
    assert_eq!(k.param(d.index_of("freqHz").unwrap()), 1_000.0);
    assert_eq!(k.param(d.index_of("q").unwrap()), 1.0);
    let pad = d.panel.pad.unwrap();
    assert_eq!((pad.x_range, pad.y_range), ((0.0, 2_000.0), (1.0, 10.0)));
    assert_eq!(d.label, "Hyperprism Band Reject");
    assert_eq!(d.source, "Hyperprism-PPC 1.2.1: Band Reject");
}

#[test]
fn out_is_the_input_less_q_times_the_last_band() {
    // low + high = x − q·band[n−1]: the first sample passes untouched.
    let x = impulse(64);
    let (y, _) = run(&mut make(ID, SR, &[("freqHz", 1_000.0), ("q", 2.0)]), &x, &x);
    assert_eq!(y[0], 1.0);
    // band[0] = F·high[0] = F·1, so y[1] = −q·F.
    let big_f = 2.0 * (std::f64::consts::PI * 1_000.0 / 44_100.0).sin();
    assert!((y[1] as f64 - (-0.5 * big_f)).abs() < 1e-6, "{}", y[1]);
}

#[test]
fn a_move_lands_in_forty_five_sample_steps() {
    // F and q step on the 45th sample. q acts on that sample's output; F
    // first moves `band`, which the output reads a sample later.
    let x = sine(440.0, 400, SR, 0.5);
    let still = run(&mut make(ID, SR, &[]), &x, &x).0;
    for (key, value, first) in [("q", 4.0, 44), ("freqHz", 300.0, 45)] {
        let mut moved = make(ID, SR, &[]);
        set(&mut moved, &[(key, value)]);
        let moved = run(&mut moved, &x, &x).0;
        assert_eq!(first_difference(&still, &moved), Some(first), "{key}");
    }
}
