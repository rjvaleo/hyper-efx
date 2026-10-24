//! Hyperprism Low Pass: EFilter's single biquad, as the 1.2.1 code does it.

#[path = "support/hp_fr_measure.rs"]
mod m;
use m::*;

const ID: &str = "hp-low-pass";

/// The decoded law: α = min((½−β)·sin w/(1+cos w), ½(½+β−γ)), section
/// 2·(α, 2α, α; γ, −β).
fn formula_db(f: f64, q: f64, hz: f64) -> f64 {
    let sr = SR as f64;
    let f = f.max(0.0003 * sr);
    let q = q.max(0.001);
    let (w, beta, gamma) = beta_gamma(f, q, sr);
    let a = (0.5 - beta) * w.sin() / (1.0 + w.cos());
    let b = 0.5 * (0.5 + beta - gamma);
    let alpha = a.min(b);
    section_db(2.0, [alpha, 2.0 * alpha, alpha], [gamma, -beta], w_of(hz, SR))
}

#[test]
fn the_measured_response_is_the_decoded_formula() {
    for (f, q) in [(1_000.0, 1.0), (1_000.0, 5.0), (1_000.0, 10.0), (5_000.0, 0.3), (20_000.0, 1.0), (300.0, 0.05)] {
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
fn the_passband_is_six_db_up() {
    let got = gain_db(ID, SR, &[("freqHz", 1_000.0), ("q", 1.0)], 50.0);
    assert!((got - 6.02).abs() < 0.1, "{got}");
}

#[test]
fn high_q_pins_the_peak_at_six_db_and_sinks_the_passband() {
    for (q, passband) in [(5.0, -8.0), (10.0, -14.0)] {
        let peak = gain_db(ID, SR, &[("freqHz", 1_000.0), ("q", q)], 1_000.0);
        assert!((peak - 6.02).abs() < 0.1, "q {q}: peak {peak}");
        let low = gain_db(ID, SR, &[("freqHz", 1_000.0), ("q", q)], 50.0);
        assert!((low - passband).abs() < 0.6, "q {q}: passband {low}");
    }
}

#[test]
fn the_start_point_is_twenty_kilohertz_and_q_one() {
    let k = prism_fx::make(ID, SR).unwrap();
    let d = k.descriptor();
    assert_eq!(k.param(d.index_of("freqHz").unwrap()), 20_000.0);
    assert_eq!(k.param(d.index_of("q").unwrap()), 1.0);
    assert_eq!(k.param(d.index_of("smoothing").unwrap()), 25.0);
    let pad = d.panel.pad.unwrap();
    assert_eq!((pad.x, pad.x_range, pad.y, pad.y_range), ("freqHz", (0.0, 20_000.0), "q", (0.0, 10.0)));
    assert!(!d.params.iter().any(|p| p.log), "the pad axes are linear");
    assert_eq!(d.label, "Hyperprism Low Pass");
    assert_eq!(d.source, "Hyperprism-PPC 1.2.1: Low Pass");
    let top = gain_db(ID, SR, &[], 20_000.0);
    assert!((top - (-13.9)).abs() < 0.2, "{top}");
    let flat = gain_db(ID, SR, &[], 1_000.0);
    assert!((flat - 6.0).abs() < 0.1, "{flat}");
}

#[test]
fn a_move_lands_in_forty_five_sample_steps() {
    // The coefficients move only when the countdown reads zero: first at the
    // 45th sample, then every 45th.
    let x = sine(440.0, 400, SR, 0.5);
    let still = run(&mut make(ID, SR, &[("freqHz", 2_000.0)]), &x, &x).0;
    let mut moved = make(ID, SR, &[("freqHz", 2_000.0)]);
    set(&mut moved, &[("freqHz", 500.0)]);
    let moved = run(&mut moved, &x, &x).0;
    assert_eq!(first_difference(&still, &moved), Some(44));
}

#[test]
fn the_step_is_scaled_with_the_rate() {
    // 45 samples at 44.1 kHz is 49 at 48 kHz.
    let sr = 48_000.0;
    let x = sine(440.0, 400, sr, 0.5);
    let still = run(&mut make(ID, sr, &[("freqHz", 2_000.0)]), &x, &x).0;
    let mut moved = make(ID, sr, &[("freqHz", 2_000.0)]);
    set(&mut moved, &[("freqHz", 500.0)]);
    let moved = run(&mut moved, &x, &x).0;
    assert_eq!(first_difference(&still, &moved), Some(48));
}

#[test]
fn smoothing_off_lands_on_the_target_at_the_first_step() {
    // a = 1 at 0 %: from the first step on the coefficients are the target's,
    // so a filter set from the start and one moved there settle together.
    let x = sine(440.0, 44_100, SR, 0.5);
    let target = run(&mut make(ID, SR, &[("freqHz", 500.0), ("smoothing", 0.0)]), &x, &x).0;
    let mut moved = make(ID, SR, &[("freqHz", 2_000.0), ("smoothing", 0.0)]);
    set(&mut moved, &[("freqHz", 500.0)]);
    let moved = run(&mut moved, &x, &x).0;
    assert!((target[40_000] - moved[40_000]).abs() < 1e-5);
    // At 25 % (a = 0.1) the coefficients are still far from the target 45
    // samples later: the two differ by more there.
    let mut slow = make(ID, SR, &[("freqHz", 2_000.0)]);
    set(&mut slow, &[("freqHz", 500.0)]);
    let slow = run(&mut slow, &x, &x).0;
    let fast_err: f32 = (100..300).map(|i| (target[i] - moved[i]).abs()).sum();
    let slow_err: f32 = (100..300).map(|i| (target[i] - slow[i]).abs()).sum();
    assert!(slow_err > 2.0 * fast_err, "{slow_err} vs {fast_err}");
}

#[test]
fn the_sides_are_filtered_apart_with_the_same_coefficients() {
    let x = sine(3_000.0, 4_000, SR, 0.5);
    let silent = vec![0.0; x.len()];
    let mut k = make(ID, SR, &[("freqHz", 1_000.0), ("q", 3.0)]);
    let (l, r) = run(&mut k, &x, &silent);
    assert!(r.iter().all(|&s| s == 0.0));
    let mut k = make(ID, SR, &[("freqHz", 1_000.0), ("q", 3.0)]);
    let (l2, r2) = run(&mut k, &x, &x);
    assert_eq!(l, l2);
    assert_eq!(l2, r2);
}
