//! Hyperprism Band Pass: EFilter's two parallel biquads, as the 1.2.1 code
//! does it.

#[path = "support/hp_fr_measure.rs"]
mod m;
use m::*;

const ID: &str = "hp-band-pass";

/// The decoded topology: a low-pass and a high-pass numerator on the same
/// input, each 2·(α, ±2α, α; γ, −β) with α = ½(½−β), summed and halved.
/// Net `2α(1 + z⁻²)/(1 − 2γz⁻¹ + 2βz⁻²)`.
fn formula_db(f: f64, q: f64, hz: f64) -> f64 {
    let sr = SR as f64;
    let f = f.max(0.0003 * sr);
    let q = q.max(1.0);
    let (_, beta, gamma) = beta_gamma(f, q, sr);
    let alpha = 0.5 * (0.5 - beta);
    section_db(2.0, [alpha, 0.0, alpha], [gamma, -beta], w_of(hz, SR))
}

#[test]
fn the_measured_response_is_the_decoded_formula() {
    for (f, q) in [(1_000.0, 1.0), (1_000.0, 10.0), (100.0, 5.0), (2_000.0, 3.0), (500.0, 1.0)] {
        for hz in [50.0, 100.0, 500.0, 1_000.0, 2_000.0, 5_000.0, 10_000.0, 15_000.0] {
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
fn the_gain_at_the_frequency_is_cot_w_whatever_the_q() {
    let cot = 20.0 * (1.0 / w_of(1_000.0, SR).tan()).log10();
    assert!((cot - 16.87).abs() < 0.01);
    for q in [1.0, 4.0, 10.0] {
        let got = gain_db(ID, SR, &[("freqHz", 1_000.0), ("q", q)], 1_000.0);
        assert!((got - cot).abs() < 0.1, "q {q}: {got}");
    }
}

#[test]
fn the_numerator_has_its_zero_at_a_quarter_of_the_rate() {
    let got = gain_db(ID, SR, &[("freqHz", 1_000.0), ("q", 1.0)], 11_025.0);
    assert!(got < -60.0, "{got}");
    // And none at DC: at Q 1 it is a loud low-pass.
    let low = gain_db(ID, SR, &[("freqHz", 1_000.0), ("q", 1.0)], 50.0);
    assert!((low - 16.95).abs() < 0.1, "{low}");
}

#[test]
fn the_start_point_and_pad_are_the_originals() {
    let k = prism_fx::make(ID, SR).unwrap();
    let d = k.descriptor();
    assert_eq!(k.param(d.index_of("freqHz").unwrap()), 1_000.0);
    assert_eq!(k.param(d.index_of("q").unwrap()), 1.0);
    let pad = d.panel.pad.unwrap();
    assert_eq!((pad.x_range, pad.y_range), ((0.0, 2_000.0), (1.0, 10.0)));
    assert_eq!(d.label, "Hyperprism Band Pass");
}

#[test]
fn a_move_lands_in_forty_five_sample_steps() {
    let x = sine(440.0, 400, SR, 0.5);
    let still = run(&mut make(ID, SR, &[]), &x, &x).0;
    let mut moved = make(ID, SR, &[]);
    set(&mut moved, &[("freqHz", 300.0)]);
    let moved = run(&mut moved, &x, &x).0;
    assert_eq!(first_difference(&still, &moved), Some(44));
}
