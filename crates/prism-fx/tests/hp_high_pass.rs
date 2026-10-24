//! Hyperprism High Pass: EFilter's single biquad, as the 1.2.1 code does it.

#[path = "support/hp_fr_measure.rs"]
mod m;
use m::*;

const ID: &str = "hp-high-pass";

/// The decoded law: β, γ as the low-pass, α = √((½−β)² + γ²), section
/// 2·(α, −2α, α; γ, −β).
fn formula_db(f: f64, q: f64, hz: f64) -> f64 {
    let sr = SR as f64;
    let f = f.max(0.0003 * sr);
    let q = q.max(0.001);
    let (_, beta, gamma) = beta_gamma(f, q, sr);
    let alpha = ((0.5 - beta).powi(2) + gamma * gamma).sqrt();
    section_db(2.0, [alpha, -2.0 * alpha, alpha], [gamma, -beta], w_of(hz, SR))
}

#[test]
fn the_measured_response_is_the_decoded_formula() {
    for (f, q) in [(20.0, 1.0), (1_000.0, 1.0), (1_000.0, 5.0), (1_000.0, 10.0), (5_000.0, 0.3), (1_000.0, 0.1)] {
        for hz in [50.0, 300.0, 700.0, 1_000.0, 3_000.0, 5_000.0, 10_000.0, 20_000.0] {
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
fn the_passband_is_six_db_up_and_the_start_is_twenty_hertz() {
    let k = prism_fx::make(ID, SR).unwrap();
    let d = k.descriptor();
    assert_eq!(k.param(d.index_of("freqHz").unwrap()), 20.0);
    assert_eq!(k.param(d.index_of("q").unwrap()), 1.0);
    assert_eq!(d.label, "Hyperprism High Pass");
    let got = gain_db(ID, SR, &[], 10_000.0);
    assert!((got - 6.02).abs() < 0.1, "{got}");
}

#[test]
fn resonance_grows_with_q() {
    for (q, peak) in [(5.0, 19.93), (10.0, 25.95)] {
        let got = gain_db(ID, SR, &[("freqHz", 1_000.0), ("q", q)], 1_000.0);
        assert!((got - peak).abs() < 0.1, "q {q}: {got}");
    }
}

#[test]
fn q_at_the_bottom_of_the_pad_rises_toward_nyquist_as_the_original_does() {
    // q is held at 0.001: β → −½ and the section nearly cancels its own
    // denominator at Nyquist, +43 dB at 22 kHz. Kept: it is finite and it is
    // what the pad's bottom edge did.
    let got = gain_db(ID, SR, &[("freqHz", 1_000.0), ("q", 0.0)], 22_000.0);
    let want = formula_db(1_000.0, 0.0, 22_000.0);
    assert!((want - 42.85).abs() < 0.05, "{want}");
    assert!((got - want).abs() < 0.2, "{got} against {want}");
}

#[test]
fn a_move_lands_in_forty_five_sample_steps() {
    let x = sine(440.0, 400, SR, 0.5);
    let still = run(&mut make(ID, SR, &[("freqHz", 200.0)]), &x, &x).0;
    let mut moved = make(ID, SR, &[("freqHz", 200.0)]);
    set(&mut moved, &[("q", 6.0)]);
    let moved = run(&mut moved, &x, &x).0;
    assert_eq!(first_difference(&still, &moved), Some(44));
}
