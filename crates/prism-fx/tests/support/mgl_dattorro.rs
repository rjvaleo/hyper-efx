//! Golden renders for the Dattorro family: each port, run over the input the
//! originals were run over, must land within `TOLERANCE` of what
//! MultiGrainLive's own effect rendered, at every kept frame of every case.
//!
//! The fixtures were made by a standalone program that builds
//! MultiGrainLive's `fx` crate (commit a4e456c) and renders each original
//! through `fx::shape::make` in blocks of 256, from the case table in
//! `mgl_dattorro_cases.rs`. Same machine, same arithmetic in the same order,
//! so the match is in practice exact; the tolerance only allows for a libm
//! that rounds a `sin` or `exp` differently.

#![allow(dead_code)]

include!("mgl_dattorro_cases.rs");

/// The most any kept sample may differ from the original's.
pub const TOLERANCE: f32 = 1e-5;

fn fixture(path: &str) -> Vec<f32> {
    let full = format!("{}/tests/fixtures/{path}", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&full).unwrap_or_else(|e| panic!("{full}: {e}"));
    bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
}

fn set(k: &mut Box<dyn prism_fx::DynKernel>, key: &str, value: f32) {
    let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
    k.set_param(i, value);
}

/// Render one case through the port the way the original was rendered: set
/// after `new` with no reset, blocks of `BLOCK`, the move before its block.
/// Returns the kept frames, interleaved, as the fixtures hold them.
pub fn render(id: &str, case: &Case, block: usize) -> Vec<f32> {
    let input = fixture("mgl-dattorro/input.f32");
    assert_eq!(input.len(), FRAMES * 2, "the input fixture is the wrong length");
    let mut left: Vec<f32> = input.iter().step_by(2).copied().collect();
    let mut right: Vec<f32> = input.iter().skip(1).step_by(2).copied().collect();
    let mut k = prism_fx::make(id, RATE as f32).unwrap_or_else(|| panic!("{id} is in the catalogue"));
    for &(key, value) in case.start {
        set(&mut k, key, value);
    }
    let mut at = 0;
    while at < FRAMES {
        if at == MOVE_AT {
            for &(key, value) in case.moved {
                set(&mut k, key, value);
            }
        }
        let end = (at + block).min(FRAMES).min(if at < MOVE_AT { MOVE_AT } else { FRAMES });
        k.process(&mut left[at..end], &mut right[at..end]);
        at = end;
    }
    (0..FRAMES).filter(|&i| stored(i)).flat_map(|i| [left[i], right[i]]).collect()
}

/// Every case of `id` against its fixture.
pub fn matches_multigrainlive(id: &str) {
    let family = FAMILIES.iter().find(|f| f.id == id).unwrap_or_else(|| panic!("no cases for {id}"));
    let mut failures = Vec::new();
    for case in family.cases {
        let want = fixture(&format!("{id}/{}.f32", case.name));
        // The original's block, and a frame at a time: the port must not care.
        for block in [BLOCK, 1] {
            let got = render(id, case, block);
            assert_eq!(got.len(), want.len(), "{id}/{}: kept frame count", case.name);
            let worst = got
                .iter()
                .zip(&want)
                .enumerate()
                .map(|(i, (g, w))| (i, (g - w).abs()))
                .fold((0, 0.0f32), |a, b| if b.1 > a.1 || b.1.is_nan() { b } else { a });
            if worst.1 > TOLERANCE || worst.1.is_nan() {
                let i = worst.0;
                failures.push(format!(
                    "{}, blocks of {block}: kept sample {i} is {} against {} (off by {:e})",
                    case.name, got[i], want[i], worst.1
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{id} departs from MultiGrainLive:\n{}", failures.join("\n"));
}

/// The descriptor carries the original's table: keys, labels, ranges,
/// defaults, log and unit, in order. `table` is `(key, label, min, max,
/// default, log, unit)`.
pub fn carries_the_table(id: &str, label: &str, table: &[(&str, &str, f32, f32, f32, bool, &str)]) {
    let d = prism_fx::find(id).unwrap_or_else(|| panic!("{id} is in the catalogue")).descriptor;
    assert_eq!(d.label, label);
    let got: Vec<_> = d.params.iter().map(|p| (p.key, p.label, p.min, p.max, p.default, p.log, p.unit)).collect();
    assert_eq!(got, table, "{id}: the table differs from MultiGrainLive's");
}
