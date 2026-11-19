//! Seeded finite-input stress across every effect and its complete control range.
use prism_fx::{make, CATALOG};
use std::panic::{catch_unwind, AssertUnwindSafe};
const SEED: u32 = 0x0025_a17e;
fn next(s: &mut u32) -> u32 { *s = s.wrapping_mul(1664525).wrapping_add(1013904223); *s }
#[test]
fn every_effect_stays_finite_for_two_hundred_parameter_sets() {
    let mut bad = Vec::new();
    for entry in CATALOG {
        let id = entry.descriptor.id;
        let mut seed = SEED;
        let mut effect = make(id, 48_000.0).unwrap();
        if entry.descriptor.sound.is_some() {
            let (s, channels, rate) = prism_fx::contract::test_sound();
            effect.set_sound(&s, channels, rate);
        }
        for case in 0..200 {
            let case_seed = seed;
            let settings: Vec<_> = entry.descriptor.params.iter().enumerate().map(|(i, p)| {
                let t = match case { 0 => 0.0, 1 => 1.0, _ => (next(&mut seed) >> 8) as f32 / 16777215.0 };
                (i, p.min + (p.max - p.min) * t)
            }).collect();
            let n = match case % 10 { 0 => 1, 1 => 4096, _ => 1 + next(&mut seed) as usize % 4096 };
            let mut l = vec![0.0; n];
            let mut r = vec![0.0; n];
            for i in 0..n {
                l[i] = match case % 3 { 0 => 0.0, 1 => if i % 2 == 0 { 1.0 } else { -1.0 }, _ => (next(&mut seed) >> 8) as f32 / 8388607.5 - 1.0 };
                r[i] = -l[i];
            }
            let result = catch_unwind(AssertUnwindSafe(|| {
                for &(i, v) in &settings { effect.set_param(i, v); }
                effect.process(&mut l, &mut r);
            }));
            if result.is_err() || !l.iter().chain(&r).all(|x| x.is_finite()) {
                bad.push(format!("{id}: seed={SEED:#x}, case={case}, case_seed={case_seed:#x}, frames={n}, settings={settings:?}, panic={}", result.is_err()));
                break;
            }
        }
    }
    assert!(bad.is_empty(), "{} effects failed:\n{}", bad.len(), bad.join("\n"));
}
