//! Plays a prism kernel through the golden scenarios in `mgl_golden.rs` and
//! compares it with what MultiGrainLive's original rendered.
//!
//! The kernel is driven in uneven blocks of 97 frames, split where a control
//! moves, so a match also says the host's block size does not matter.

#![allow(dead_code)]

use super::golden::{self, Render};
use std::path::PathBuf;

fn fixture(id: &str, name: &str) -> Vec<f32> {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", id, &format!("{name}.f32")].iter().collect();
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(bytes.len() % 4, 0, "{} is not whole f32s", path.display());
    bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
}

fn set(k: &mut Box<dyn prism_fx::DynKernel>, settings: golden::Settings) {
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
}

/// The prism kernel's render of one scenario, interleaved.
pub fn render(id: &str, r: &Render, input: &[f32]) -> Vec<f32> {
    let mut k = prism_fx::make(id, golden::SAMPLE_RATE as f32).unwrap_or_else(|| panic!("{id} is in the catalogue"));
    set(&mut k, r.start);
    let frames = input.len() / 2;
    let (mut left, mut right): (Vec<f32>, Vec<f32>) = input.chunks_exact(2).map(|f| (f[0], f[1])).unzip();
    let mut at = 0;
    while at < frames {
        for &(when, settings) in r.moves {
            if when == at {
                set(&mut k, settings);
            }
        }
        let next_move = r.moves.iter().map(|m| m.0).filter(|&m| m > at).min().unwrap_or(frames);
        let end = (at + 97).min(next_move).min(frames);
        k.process(&mut left[at..end], &mut right[at..end]);
        at = end;
    }
    left.iter().zip(&right).flat_map(|(&l, &r)| [l, r]).collect()
}

/// Every render of `id` matches the original within `tolerance`, sample for
/// sample. Returns the largest difference seen.
pub fn check(id: &str, tolerance: f32) -> f32 {
    let input = fixture(id, "input");
    assert_eq!(input.len(), golden::FRAMES * 2, "{id}: input is the wrong length");
    let renders = golden::renders(id);
    assert!(!renders.is_empty(), "{id}: no scenarios");
    let mut worst = 0.0f32;
    let mut failures = Vec::new();
    for r in renders {
        let want = fixture(id, r.name);
        assert_eq!(want.len(), input.len(), "{id}/{}: the wrong length", r.name);
        let got = render(id, r, &input);
        let mut first = None;
        let mut largest = 0.0f32;
        for (i, (a, b)) in got.iter().zip(&want).enumerate() {
            let d = (a - b).abs();
            if (d > tolerance || d.is_nan()) && first.is_none() {
                first = Some((i / 2, i % 2, *a, *b));
            }
            if d > largest || d.is_nan() {
                largest = if d.is_nan() { f32::INFINITY } else { d };
            }
        }
        worst = worst.max(largest);
        if let Some((frame, side, a, b)) = first {
            failures.push(format!(
                "{}: frame {frame} side {side} is {a}, the original {b}; largest difference {largest:e}",
                r.name
            ));
        }
    }
    assert!(failures.is_empty(), "{id} differs from MultiGrainLive's:\n{}", failures.join("\n"));
    worst
}
