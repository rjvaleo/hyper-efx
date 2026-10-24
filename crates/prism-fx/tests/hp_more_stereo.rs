//! Hyperprism More Stereo: EFilter's side-above-the-low-cut widener, as the
//! 1.2.1 code does it.

#[path = "support/hp_fr_measure.rs"]
mod m;
use m::*;

const ID: &str = "hp-more-stereo";

/// The Butterworth high-pass on L − R: unity passband.
fn highpass(cut: f64, w: f64) -> (f64, f64) {
    let sr = SR as f64;
    let cut = cut.clamp(0.00022675 * sr, 0.46031 * sr);
    let wc = std::f64::consts::TAU * cut / sr;
    let (s, c) = (wc.sin(), wc.cos());
    let beta = 0.5 * (1.0 - s / 2f64.sqrt()) / (1.0 + s / 2f64.sqrt());
    let gamma = (0.5 + beta) * c;
    let alpha = 0.25 * (0.5 + beta + gamma);
    let z1 = (w.cos(), -w.sin());
    let z2 = ((2.0 * w).cos(), -(2.0 * w).sin());
    let num = (2.0 * alpha * (1.0 - 2.0 * z1.0 + z2.0), 2.0 * alpha * (-2.0 * z1.1 + z2.1));
    let den = (1.0 - 2.0 * gamma * z1.0 + 2.0 * beta * z2.0, -2.0 * gamma * z1.1 + 2.0 * beta * z2.1);
    let d = den.0 * den.0 + den.1 * den.1;
    ((num.0 * den.0 + num.1 * den.1) / d, (num.1 * den.0 - num.0 * den.1) / d)
}

/// The side's gain: S' = S + 2g·HP(S), g = (level − 100)/200.
fn side_db(level: f64, cut: f64, hz: f64) -> f64 {
    let g = (level - 100.0) / 200.0;
    let h = highpass(cut, w_of(hz, SR));
    let (re, im) = (1.0 + 2.0 * g * h.0, 2.0 * g * h.1);
    20.0 * (re * re + im * im).sqrt().log10()
}

fn measured_side_db(level: f32, cut: f32, hz: f64) -> f64 {
    let mut k = make(ID, SR, &[("level", level), ("lowCutHz", cut)]);
    let frames = 2 * SR as usize;
    let s = sine(hz, frames, SR, 0.25);
    let minus: Vec<f32> = s.iter().map(|v| -v).collect();
    let (l, r) = run(&mut k, &s, &minus);
    let side: Vec<f32> = l.iter().zip(&r).map(|(a, b)| a - b).collect();
    20.0 * (amplitude(&side[SR as usize..], hz, SR) / 0.5).log10()
}

#[test]
fn the_side_follows_the_decoded_law() {
    for (level, cut) in [(300.0, 100.0), (0.0, 100.0), (150.0, 1_000.0), (250.0, 6_000.0), (50.0, 0.0)] {
        for hz in [20.0, 50.0, 200.0, 1_000.0, 3_000.0, 10_000.0] {
            let want = side_db(level, cut, hz);
            if want < -60.0 {
                continue;
            }
            let got = measured_side_db(level as f32, cut as f32, hz);
            assert!((got - want).abs() < 0.1, "level {level} cut {cut} at {hz} Hz: {got:.3} dB, law {want:.3}");
        }
    }
}

#[test]
fn level_is_the_side_gain_above_the_cut_and_below_it_nothing_changes() {
    let triple = measured_side_db(300.0, 100.0, 5_000.0);
    assert!((triple - 20.0 * 3f64.log10()).abs() < 0.1, "{triple}");
    let mono = measured_side_db(0.0, 100.0, 5_000.0);
    assert!(mono < -30.0, "{mono}");
    let below = measured_side_db(300.0, 6_000.0, 50.0);
    assert!(below.abs() < 0.1, "{below}");
}

#[test]
fn the_mid_is_untouched() {
    let x = sine(700.0, 8_000, SR, 0.4);
    let y = sine(1_900.0, 8_000, SR, 0.3);
    let (l, r) = run(&mut make(ID, SR, &[("level", 300.0), ("lowCutHz", 200.0)]), &x, &y);
    for i in 0..x.len() {
        assert!(((l[i] + r[i]) - (x[i] + y[i])).abs() < 1e-6, "sample {i}");
    }
    // A centred sound has no side: it passes exactly.
    let (l, r) = run(&mut make(ID, SR, &[("level", 300.0)]), &x, &x);
    assert_eq!(l, x);
    assert_eq!(r, x);
}

#[test]
fn the_start_point_and_pad_are_the_originals() {
    let k = prism_fx::make(ID, SR).unwrap();
    let d = k.descriptor();
    assert_eq!(k.param(d.index_of("level").unwrap()), 150.0);
    assert_eq!(k.param(d.index_of("lowCutHz").unwrap()), 0.0);
    let pad = d.panel.pad.unwrap();
    assert_eq!((pad.x, pad.x_range, pad.y, pad.y_range), ("level", (0.0, 300.0), "lowCutHz", (1.0, 6_000.0)));
    assert_eq!(d.label, "Hyperprism More Stereo");
    assert_eq!(d.category, prism_fx::Category::Stereo);
}

/// The level's glide, read back sample by sample: the output's left is
/// L + g·h, and h does not depend on g, so a run at a fixed level gives h.
fn glide(smoothing: f32, from: f32, to: f32) -> Vec<f64> {
    let frames = 600;
    let l = sine(3_000.0, frames, SR, 0.5);
    let r = vec![0.0; frames];
    let settings = [("level", from), ("lowCutHz", 10.0), ("smoothing", smoothing)];
    let (fixed, _) = run(&mut make(ID, SR, &settings), &l, &r);
    let mut k = make(ID, SR, &settings);
    set(&mut k, &[("level", to)]);
    let (moved, _) = run(&mut k, &l, &r);
    let g0 = (from as f64 - 100.0) / 200.0;
    (0..frames)
        .map(|i| {
            let h = (fixed[i] - l[i]) as f64 / g0;
            if h.abs() < 0.05 { f64::NAN } else { (moved[i] - l[i]) as f64 / h }
        })
        .collect()
}

#[test]
fn the_level_glides_a_step_every_forty_five_samples() {
    for (smoothing, a) in [(25.0, 0.1), (0.0, 1.0), (50.0, 0.01), (10.0, 0.3981)] {
        let g = glide(smoothing, 150.0, 300.0);
        let (g0, g1) = (0.25, 1.0);
        let mut want = g0;
        for (i, &got) in g.iter().enumerate() {
            if i % 45 == 44 {
                want = a * g1 + (1.0 - a) * want;
            }
            if got.is_finite() {
                assert!((got - want).abs() < 2e-4, "smoothing {smoothing}: sample {i}: {got} against {want}");
            }
        }
    }
}
