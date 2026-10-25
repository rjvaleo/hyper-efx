//! Plays a prism kernel through the shaping family's golden cases and holds
//! it to what MultiGrainLive's original rendered. See `shape_golden.rs`.

#![allow(dead_code)]

use crate::golden::*;
use std::path::PathBuf;

/// Largest difference allowed on any kept sample.
pub const TOLERANCE: f32 = 1e-5;

/// One control as MultiGrainLive's `ParamSpec` has it: key, label, min, max,
/// default, log, unit.
pub type Spec = (&'static str, &'static str, f32, f32, f32, bool, &'static str);

pub fn make(id: &str) -> Box<dyn prism_fx::DynKernel> {
    prism_fx::make(id, SAMPLE_RATE as f32).unwrap_or_else(|| panic!("{id} is not in the catalogue"))
}

fn set(k: &mut dyn prism_fx::DynKernel, settings: &[(&str, f32)]) {
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("{} has no control {key}", k.descriptor().id));
        k.set_param(i, value);
    }
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read_f32(path: PathBuf) -> Vec<f32> {
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
}

/// Render one case through the prism kernel, as the generator rendered it
/// through the original: blocks of 256, the moves written between blocks.
pub fn render(case: &Case, input: &[f32]) -> Vec<f32> {
    let mut k = make(case.id);
    set(k.as_mut(), case.start);
    let mut left: Vec<f32> = input.iter().step_by(2).copied().collect();
    let mut right: Vec<f32> = input.iter().skip(1).step_by(2).copied().collect();
    for (b, (l, r)) in left.chunks_mut(BLOCK).zip(right.chunks_mut(BLOCK)).enumerate() {
        if b * BLOCK == MOVE_AT {
            set(k.as_mut(), case.moves);
        }
        k.process(l, r);
    }
    left.iter().zip(&right).flat_map(|(&l, &r)| [l, r]).collect()
}

/// Every case for `id` matches the original: each kept sample within
/// `TOLERANCE`, and — on the platform the fixtures were recorded on, whose
/// `sin`, `exp`, `powf` and `tan` the original used — every sample to the bit.
pub fn check(id: &str) {
    let input = signal();
    let stored_input = read_f32(fixtures().join("shape-input.f32"));
    assert_eq!(stored_input, strided(&input), "the test signal is not the one the originals heard");

    let prints = std::fs::read_to_string(fixtures().join(id).join("fingerprints.txt")).expect("fingerprints");
    let print = |name: &str| -> u64 {
        let line = prints.lines().find(|l| l.split(' ').next() == Some(name)).unwrap_or_else(|| panic!("no fingerprint for {name}"));
        u64::from_str_radix(line.split(' ').nth(1).unwrap(), 16).unwrap()
    };
    assert_eq!(print("input"), fingerprint(&input), "the test signal is not the one the originals heard");

    let cases: Vec<&Case> = CASES.iter().filter(|c| c.id == id).collect();
    assert!(!cases.is_empty(), "no golden cases for {id}");
    let mut failures = Vec::new();
    for case in cases {
        let got = render(case, &input);
        assert!(got.iter().all(|s| s.is_finite()), "{id}/{}: non-finite output", case.name);
        let want = read_f32(fixtures().join(id).join(format!("{}.f32", case.name)));
        let kept = strided(&got);
        assert_eq!(kept.len(), want.len(), "{id}/{}: fixture length", case.name);
        let (mut worst, mut at) = (0.0f32, 0usize);
        for (i, (a, b)) in kept.iter().zip(&want).enumerate() {
            let d = (a - b).abs();
            if d > worst {
                worst = d;
                at = i;
            }
        }
        if worst > TOLERANCE {
            failures.push(format!(
                "{}: frame {} side {} is {}, the original {} (off by {worst})",
                case.name,
                (at / 2) * STRIDE,
                at % 2,
                kept[at],
                want[at]
            ));
        }
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) && fingerprint(&got) != print(case.name) {
            failures.push(format!(
                "{}: not bit-exact (fingerprint {:016x}, recorded {:016x}; worst kept difference {worst})",
                case.name,
                fingerprint(&got),
                print(case.name)
            ));
        }
    }
    assert!(failures.is_empty(), "{id} differs from MultiGrainLive:\n{}", failures.join("\n"));
}

/// The descriptor carries MultiGrainLive's controls exactly, and its identity.
pub fn check_descriptor(id: &str, code: u16, label: &str, specs: &[Spec]) {
    let d = prism_fx::find(id).unwrap_or_else(|| panic!("{id} is not in the catalogue")).descriptor;
    assert_eq!((d.id, d.code, d.label), (id, code, label));
    let got: Vec<Spec> = d.params.iter().map(|p| (p.key, p.label, p.min, p.max, p.default, p.log, p.unit)).collect();
    assert_eq!(got, specs);
    // The originals apply a value the moment it is written; so do these.
    assert!(d.params.iter().all(|p| p.smooth_ms == 0.0), "{id}: a control glides");
}
