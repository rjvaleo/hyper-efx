//! EX-S for Ambient FDN: Stone Voices' Ambient Reverb as statically decoded
//! (`sources/reports/001-ambientreverb.md`). Every number here is the
//! decode's: its worked lengths, its tap tables, its laws. Not yet measured
//! against the plug-in (the decode's §9: it is not installed).

use prism_fx::effects::ambient::{bell, coefficients, lengths, level, size_factor, PROGRAMS, G, TA, TN};
use prism_fx::{make, DynKernel};

#[test]
fn the_lengths_are_the_decodes_worked_values() {
    // §2.2's worked values, verbatim.
    assert_eq!(lengths(48_000.0, 100.0), ([977, 1172, 1517, 1312, 1099, 1406, 646, 918], [7350, 10099, 6136, 12331, 8386, 9231, 6000, 10560]));
    assert_eq!(lengths(48_000.0, 50.0), ([513, 615, 796, 689, 577, 738, 339, 482], [3859, 5302, 3221, 6474, 4403, 4846, 3150, 5544]));
    assert_eq!(lengths(44_100.0, 100.0), ([897, 1077, 1394, 1205, 1010, 1292, 593, 843], [6753, 9278, 5638, 11329, 7705, 8481, 5513, 9702]));
}

fn kernel(set: &[(&str, f32)]) -> Box<dyn DynKernel> {
    let mut k = make("ambient-fdn", 48_000.0).unwrap();
    for (key, v) in set {
        let i = k.descriptor().params.iter().position(|p| p.key == *key).unwrap();
        k.set_param(i, *v);
    }
    k
}

/// An impulse on the left after two blocks of quiet, so the gains' first
/// fade-in (from 0, over one block, as the original's) is done.
fn hit(k: &mut Box<dyn DynKernel>, n: usize) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
    l[512] = 1.0;
    k.process(&mut l, &mut r);
    (l[512..].to_vec(), r[512..].to_vec())
}

#[test]
fn the_early_reflections_come_from_the_input_cross_fed_at_the_tables_taps() {
    // Late off, dry off, ER at 0 dB, Wide 150 (identity), and a predelay the
    // reflections must not wait for.
    let mut k = kernel(&[("lateDb", -200.0), ("dryDb", -200.0), ("erDb", 0.0), ("predelayLMs", 100.0), ("predelayRMs", 100.0)]);
    let (l, r) = hit(&mut k, 2_000);
    let sf = size_factor(50.0);
    let t_r0 = (sf * 48_000.0 * 0.0043 + 0.5).floor() as usize;
    let t_l0 = (sf * 48_000.0 * 0.0133 + 0.5).floor() as usize;
    assert_eq!((t_r0, t_l0), (108, 335));
    assert!(r[..t_r0].iter().all(|&x| x == 0.0) && l[..t_r0].iter().all(|&x| x == 0.0));
    // The right hears 0.3 of the left at its first tap (4.3 ms), gain 0.841.
    assert!((r[t_r0] - 0.841 * 0.3).abs() < 1e-6, "{}", r[t_r0]);
    // The left hears 0.5 of itself at its first tap (13.3 ms).
    assert!((l[t_l0] - 0.841 * 0.5).abs() < 1e-6, "{}", l[t_l0]);
}

#[test]
fn the_late_network_is_silent_until_the_first_line_returns_through_its_allpass() {
    // The ±0.6 all-passes' direct terms cancel in both taps; the first sound
    // is line 0's all-pass delay A0, worth 2·0.3·(1 − 0.36) = 0.384.
    let mut k = kernel(&[("erDb", -200.0), ("dryDb", -200.0), ("lateDb", 0.0), ("size", 100.0)]);
    let (l, r) = hit(&mut k, 2_000);
    let (a, _) = lengths(48_000.0, 100.0);
    assert!(l[..a[0]].iter().all(|&x| x == 0.0), "{:?}", l[..a[0]].iter().position(|&x| x != 0.0));
    assert!(r[..a[0]].iter().all(|&x| x == 0.0));
    assert!((l[a[0]] - 0.384).abs() < 1e-6, "{}", l[a[0]]);
}

#[test]
fn wide_is_mono_at_0_identity_at_150_and_side_times_one_and_a_half_at_200() {
    let er_only = [("lateDb", -200.0), ("dryDb", -200.0), ("erDb", 0.0)];
    let at = |wide: f32| {
        let mut set = er_only.to_vec();
        set.push(("wide", wide));
        let (l, r) = hit(&mut kernel(&set), 1_000);
        (l[108], r[108])
    };
    let rp = 0.841 * 0.3;
    let (l, r) = at(0.0);
    assert!((l - r).abs() < 1e-7 && (l - rp * 0.5).abs() < 1e-6);
    let (l, r) = at(150.0);
    assert!(l.abs() < 1e-7 && (r - rp).abs() < 1e-6);
    let (l, r) = at(200.0);
    assert!((r - 1.25 * rp).abs() < 1e-6 && (l + 0.25 * rp).abs() < 1e-6);
}

#[test]
fn each_line_falls_sixty_db_in_the_decay_time_halves_it_at_high_damping_and_scales_it_below_the_crossover() {
    let sr = 48_000.0f32;
    for (decay, size, bass, hd) in [(1.5f32, 50.0f32, 1.0f32, 10_000.0f32), (4.62, 90.5, 1.51, 3162.0), (0.2, 8.0, 0.505, 800.0)] {
        let c = coefficients(sr, decay, size, false, bass, 200.0, hd);
        let sf = size_factor(size);
        for k in 0..8 {
            // One pass of the scaled loop is sf·t seconds; after T, 0.001.
            let passes = decay / (sf * TN[k]);
            assert!((c.g[k].powf(passes) - 0.001).abs() < 2e-5, "line {k}");
            // The shelf's DC gain is gLo/g: below the crossover the RT is T·Bass.
            let g_lo = c.g[k] * (1.0 + c.sh[k]);
            assert!((g_lo.powf(passes * bass) - 0.001).abs() < 2e-5, "line {k} bass");
            // The damping low-pass's gain at High damping is g: there the RT halves.
            let w = std::f32::consts::TAU * hd / sr;
            let a = c.cd[k];
            let mag = a / ((1.0 - (1.0 - a) * w.cos()).powi(2) + ((1.0 - a) * w.sin()).powi(2)).sqrt();
            assert!((mag - c.g[k]).abs() < 1e-4, "line {k} damping: {mag} vs {}", c.g[k]);
        }
    }
}

#[test]
fn each_bell_is_g_at_its_centre_and_unity_at_dc_and_nyquist() {
    let sr = 48_000.0f32;
    for (db, f) in [(-6.0f32, 417.0f32), (3.6, 389.0), (-15.0, 3000.0), (2.7, 209.0)] {
        let [c0, c1, c2] = bell(db, f, sr);
        let g = 1.0 + 2.0 * c2;
        // H = (1+G)/2 + (1−G)/2·A2, A2 = (c1 + c0(1+c1)z⁻¹ + z⁻²)/(1 + c0(1+c1)z⁻¹ + c1 z⁻²).
        let h = |hz: f32| {
            let w = std::f32::consts::TAU * hz / sr;
            let (z1, z2) = ((w.cos(), -w.sin()), ((2.0 * w).cos(), -(2.0 * w).sin()));
            let b = c0 * (1.0 + c1);
            let num = (c1 + b * z1.0 + z2.0, b * z1.1 + z2.1);
            let den = (1.0 + b * z1.0 + c1 * z2.0, b * z1.1 + c1 * z2.1);
            let d2 = den.0 * den.0 + den.1 * den.1;
            let a2 = ((num.0 * den.0 + num.1 * den.1) / d2, (num.1 * den.0 - num.0 * den.1) / d2);
            let re = (1.0 + g) / 2.0 + (1.0 - g) / 2.0 * a2.0;
            let im = (1.0 - g) / 2.0 * a2.1;
            (re * re + im * im).sqrt()
        };
        assert!((h(f) - g).abs() < 1e-3 * g, "{db} dB at {f}: {}", h(f));
        assert!((h(0.0) - 1.0).abs() < 1e-5);
        assert!((h(sr / 2.0) - 1.0).abs() < 1e-4);
    }
}

#[test]
fn the_levels_and_the_sixteen_programs_are_the_decodes() {
    assert_eq!(level(-91.0), 0.0);
    assert!((level(-6.0) - 0.501187).abs() < 1e-5);
    let names: Vec<_> = PROGRAMS.iter().map(|p| p.0).collect();
    assert_eq!(names, ["Default", "Cathedral", "Station", "Small Hall", "Medium Hall", "Large Hall", "Stadium", "Far Space", "Long Ambience", "Spring Reverb", "Metal And Vacuum", "Mellow", "Garage", "Input In A Cave", "Dark Ambience", "Deep Underground"]);
    let k = make("ambient-fdn", 48_000.0).unwrap();
    for (name, v) in PROGRAMS {
        for (p, x) in k.descriptor().params.iter().zip(v) {
            assert!(x >= p.min && x <= p.max, "{name}: {} = {x}", p.key);
        }
    }
    assert_eq!(G.len(), 18);
    assert_eq!((TA.len(), TN.len()), (8, 8));
}
