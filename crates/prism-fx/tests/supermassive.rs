//! EX-M for the Network family: Valhalla Supermassive's twelve networks against
//! renders of the original (SUITE-PLAN §5, P4). The captures are Reaper renders
//! at 48 kHz in 32-bit float, kept with the decode in
//! `effect-algorithms/sources/captures` (outside this repository; the path can
//! be moved with SM_CAPTURES). Tolerances are the measurement report's:
//! first arrivals exact to the sample, and every 0.5 s energy bin within
//! 0.05 dB (`sources/reports/001-valhalla-measured.md` S3, S5) — and, tighter
//! than the report asked, each whole render nulls against the capture to
//! 100 dB or more.

use prism_fx::{make, DynKernel};
use std::path::PathBuf;

const SR: f32 = 48_000.0;
const MODES: [(&str, &str); 12] = [
    ("sm-gemini", "gemini"),
    ("sm-hydra", "hydra"),
    ("sm-centaurus", "centaurus"),
    ("sm-sagittarius", "sagittarius"),
    ("sm-great-annihilator", "great-annihilator"),
    ("sm-andromeda", "andromeda"),
    ("sm-lyra", "lyra"),
    ("sm-capricorn", "capricorn"),
    ("sm-triangulum", "triangulum"),
    ("sm-lmc", "large-magellanic-cloud"),
    ("sm-cirrus-major", "cirrus-major"),
    ("sm-cirrus-minor", "cirrus-minor"),
];

fn captures() -> PathBuf {
    std::env::var_os("SM_CAPTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Volumes/1 TB Funtime/WORK DIR/effect-algorithms/sources/captures"))
}

/// A 32-bit float stereo WAV, as Reaper wrote them.
fn wav(name: &str) -> (Vec<f32>, Vec<f32>) {
    let path = captures().join(name);
    let b = std::fs::read(&path).unwrap_or_else(|e| panic!("capture {} not found ({e}); set SM_CAPTURES", path.display()));
    let mut i = 12;
    loop {
        let id = &b[i..i + 4];
        let size = u32::from_le_bytes(b[i + 4..i + 8].try_into().unwrap()) as usize;
        if id == b"fmt " {
            assert_eq!(u16::from_le_bytes([b[i + 8], b[i + 9]]), 3, "float");
            assert_eq!(u16::from_le_bytes([b[i + 10], b[i + 11]]), 2, "stereo");
        }
        if id == b"data" {
            let d = &b[i + 8..i + 8 + size];
            let s: Vec<f32> = d.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
            return (s.iter().step_by(2).copied().collect(), s.iter().skip(1).step_by(2).copied().collect());
        }
        i += 8 + size + (size & 1);
    }
}

fn kernel(id: &str, set: &[(&str, f32)]) -> Box<dyn DynKernel> {
    let mut k = make(id, SR).unwrap();
    for (key, v) in set {
        let i = k.descriptor().params.iter().position(|p| p.key == *key).unwrap();
        k.set_param(i, *v);
    }
    k
}

/// Render an impulse at `at` (L only, or both) through `k`, `frames` long.
fn render(k: &mut Box<dyn DynKernel>, frames: usize, at: usize, both: bool) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (vec![0.0f32; frames], vec![0.0f32; frames]);
    l[at] = 1.0;
    if both {
        r[at] = 1.0;
    }
    for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
        k.process(a, b);
    }
    (l, r)
}

fn onset(x: &[f32], from: usize, floor: f32) -> Option<usize> {
    x[from..].iter().position(|v| v.abs() > floor)
}

#[test]
fn every_modes_first_arrival_is_the_measured_sample() {
    // MEAS S3: FB 0, Spacing 0, ModDepth 0, Density 0, Delay 100 ms, Mix 100.
    let want = [4833, 9666, 19364, 38696, 19364, 38696, 4833, 4833, 38696, 19364, 9666, 4833];
    for ((id, _), want) in MODES.iter().zip(want) {
        let mut k = kernel(id, &[("mix", 100.0), ("feedback", 0.0), ("modDepth", 0.0), ("delayMs", 100.0)]);
        let (l, _) = render(&mut k, 192_000 + 60_000, 192_000, true);
        assert_eq!(onset(&l, 192_001, 1e-4), Some(want - 1), "{id}");
    }
}

#[test]
fn lyras_first_arrival_at_100_and_200_ms_is_the_captures() {
    // MEAS S1: the 6 s renders. The smoother glides down from its default and
    // stalls in float32 just above the target, as the original's does, so the
    // first sample is the interpolator's T[28] through the High Cut.
    for (ms, at) in [(100.0, 4833usize), (200.0, 9633)] {
        let (cl, _) = wav(&format!("001-valhalla-supermassive-lyra-imp-at6s-d{}ms-fb0-den0-sp0-md0.wav", ms as u32));
        let mut k = kernel("sm-lyra", &[("mix", 100.0), ("feedback", 0.0), ("modDepth", 0.0), ("delayMs", ms)]);
        let (l, _) = render(&mut k, cl.len(), 288_000, true);
        assert_eq!(onset(&cl, 288_001, 1e-4), Some(at - 1));
        assert_eq!(onset(&l, 288_001, 1e-4), Some(at - 1), "{ms} ms");
        for n in 288_000 + at - 2..288_000 + at + 6 {
            assert!((l[n] - cl[n]).abs() < 2e-3, "{ms} ms, +{}: {} vs capture {}", n - 288_000, l[n], cl[n]);
        }
    }
}

/// Energy per 0.5 s bin, in dB, for the 10 s after the impulse.
fn bins(x: &[f32], at: usize) -> Vec<f64> {
    x[at..at + 480_000].chunks(24_000).map(|c| 10.0 * c.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>().max(1e-30).log10()).collect()
}

#[test]
fn every_mode_matches_its_capture_bin_for_bin_at_feedback_50_density_100() {
    // MEAS S5: L-only impulse at 4 s, Delay 100, FB 50, Density 100, Spacing 0,
    // ModDepth 0, Mix 100; bins above −60 dB within 0.05 dB on both channels.
    let mut bad = Vec::new();
    for (id, name) in MODES {
        let (cl, cr) = wav(&format!("001-valhalla-supermassive-{name}-Linimp-at4s-d100ms-fb50-den100-sp0-md0.wav"));
        let mut k = kernel(id, &[("mix", 100.0), ("feedback", 50.0), ("density", 100.0), ("modDepth", 0.0), ("delayMs", 100.0)]);
        let (l, r) = render(&mut k, cl.len(), 192_000, false);
        for (side, (ours, theirs)) in [("L", (&l, &cl)), ("R", (&r, &cr))] {
            assert_eq!(onset(ours, 192_001, 1e-4), onset(theirs, 192_001, 1e-4), "{id} {side}: first arrival");
            // Sample for sample: the difference is 100 dB below the capture's
            // energy (measured: −116 to −127 dB, the original's own anti-
            // denormal offset and float rounding).
            let energy: f64 = theirs[192_000..].iter().map(|&v| (v as f64).powi(2)).sum();
            let residue: f64 = ours[192_000..].iter().zip(&theirs[192_000..]).map(|(x, y)| ((x - y) as f64).powi(2)).sum();
            let null = 10.0 * (residue / energy).log10();
            if null > -100.0 {
                bad.push(format!("{id} {side}: nulls only to {null:.1} dB"));
            }
            let (a, b) = (bins(ours, 192_000), bins(theirs, 192_000));
            let top = b.iter().cloned().fold(f64::MIN, f64::max);
            for (i, (x, y)) in a.iter().zip(&b).enumerate() {
                if *y > top - 60.0 && (x - y).abs() > 0.05 {
                    bad.push(format!("{id} {side} bin {i}: {x:.3} dB vs capture {y:.3} dB"));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} bins off:\n{}", bad.len(), bad.join("\n"));
}

