//! PAR: every kernel in the catalogue renders bit-identically to the golden set
//! recorded before the suite layer was built (SUITE-PLAN §5, P0).
//!
//! Record once, review, commit: `PAR_RECORD=1 cargo test -p prism-fx --release --test par`.
//! Never re-record to make a failure go away: a changed hash is a changed sound.

use prism_fx::{par, CATALOG};
use std::fmt::Write as _;

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/par.tsv");

fn render_all() -> String {
    let mut out = String::new();
    for e in CATALOG {
        let make = || (e.make)(prism_fx::contract::SAMPLE_RATE);
        for (name, h) in par::scenarios(&make) {
            writeln!(out, "{}\t{}\t{:016x}", e.descriptor.id, name, h).unwrap();
        }
    }
    out
}

#[test]
fn every_kernel_renders_as_recorded() {
    let now = render_all();
    if std::env::var_os("PAR_RECORD").is_some() {
        std::fs::write(GOLDEN, &now).unwrap();
        return;
    }
    let golden = std::fs::read_to_string(GOLDEN).expect("no golden set: record it first (PAR_RECORD=1)");
    let mut bad = Vec::new();
    let g: std::collections::HashMap<(&str, &str), &str> =
        golden.lines().map(|l| { let mut f = l.split('\t'); ((f.next().unwrap(), f.next().unwrap()), f.next().unwrap()) }).collect();
    for l in now.lines() {
        let mut f = l.split('\t');
        let (id, sc, h) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
        match g.get(&(id, sc)) {
            Some(x) if *x == h => {}
            Some(x) => bad.push(format!("{id} {sc}: {h} (golden {x})")),
            None => bad.push(format!("{id} {sc}: not in the golden set")),
        }
    }
    assert!(golden.lines().count() == now.lines().count(), "scenario count changed: {} now, {} golden", now.lines().count(), golden.lines().count());
    assert!(bad.is_empty(), "{} renders changed:\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn renders_are_deterministic() {
    for e in CATALOG.iter().take(86) {
        let make = || (e.make)(prism_fx::contract::SAMPLE_RATE);
        let a = par::scenarios(&make);
        let b = par::scenarios(&make);
        assert_eq!(a[..3], b[..3], "{}", e.descriptor.id);
    }
}
