//! The suite layer (SUITE-PLAN §2.0, P1): eleven modules, each a mode selector
//! over existing kernels, held to the catalogue's contract and to the PAR
//! golden renders through the module path.

use prism_fx::contract;
use prism_fx::suite::{self, Exactness, ModuleKernel, SUITES};
use prism_fx::{par, DynKernel, CATALOG};
use std::collections::{HashMap, HashSet};

const SR: f32 = contract::SAMPLE_RATE;

#[test]
fn there_are_eleven_suites_in_the_plans_order() {
    let ids: Vec<_> = SUITES.iter().map(|s| s.id).collect();
    assert_eq!(
        ids,
        ["reverb", "spectral", "cassette", "delay", "modulation", "pitch", "dynamics", "saturation", "filter", "stereo", "utility"]
    );
}

#[test]
fn every_kernel_is_exactly_one_mode_and_every_mode_is_a_kernel() {
    let mut seen: HashMap<&str, (&str, &str)> = HashMap::new();
    for s in SUITES {
        let mut ids = HashSet::new();
        for f in s.families {
            for m in f.modes {
                assert!(ids.insert(m.id), "{}: mode id {} twice", s.id, m.id);
                assert!(prism_fx::find(m.kernel).is_some(), "{}/{}: no kernel {}", s.id, m.id, m.kernel);
                assert!(!m.source.is_empty(), "{}/{}: no source", s.id, m.id);
                assert!(m.rev >= 1);
                if let Some(prev) = seen.insert(m.kernel, (s.id, m.id)) {
                    panic!("kernel {} is both {:?} and {}/{}", m.kernel, prev, s.id, m.id);
                }
            }
        }
    }
    for e in CATALOG {
        assert!(seen.contains_key(e.descriptor.id), "kernel {} is in no suite", e.descriptor.id);
    }
}

#[test]
fn locate_finds_each_kernels_mode() {
    for e in CATALOG {
        let (s, m) = suite::locate(e.descriptor.id).unwrap();
        assert_eq!(m.kernel, e.descriptor.id);
        assert!(suite::find_mode(s.id, m.id).is_some());
    }
    assert!(suite::locate("no-such").is_none());
}

#[test]
fn the_unlisted_modes_are_the_plans() {
    // SUITE-PLAN §1.4–1.5 and §2: kept so old documents open, never offered.
    let mut unlisted: Vec<_> = SUITES.iter().flat_map(|s| s.modes()).filter(|m| !m.listed).map(|m| m.kernel).collect();
    unlisted.sort();
    let mut want = vec![
        "invert", "swap", "width", "dattorro-notch", "dattorro-resonator", "regalia", "damping-filter",
        "decimator", "multi-delay", "hp-single-delay", "stereo-matrix",
    ];
    want.sort();
    assert_eq!(unlisted, want);
}

#[test]
fn every_listed_suite_offers_a_mode_and_cassette_waits_for_its_kernels() {
    for s in SUITES {
        let listed = s.modes().filter(|m| m.listed).count();
        if s.id == "cassette" {
            assert_eq!(listed, 0, "Cassette has no kernel until P6");
            assert!(!s.offered());
        } else {
            assert!(listed > 0, "{} offers nothing", s.id);
            assert!(s.offered());
        }
    }
}

#[test]
fn exactness_is_never_claimed_for_ours() {
    // the LXP voicings are this crate's own design, not Lexicon's
    for id in ["lxp-chamber", "lxp-hall", "lxp-plate", "lxp-room", "echo", "tremolo", "ring"] {
        let (_, m) = suite::locate(id).unwrap();
        assert_eq!(m.class, Exactness::Ours, "{id}");
    }
    let (_, m) = suite::locate("hp-noise-gate").unwrap();
    assert!(matches!(m.class, Exactness::Port(suite::Verdict::Matches)));
}

fn module(s: &'static str, m: &'static str) -> Box<dyn DynKernel> {
    Box::new(ModuleKernel::new(suite::find_mode(s, m).unwrap(), SR).unwrap())
}

#[test]
fn every_mode_keeps_the_contract_through_the_module() {
    for s in SUITES {
        for m in s.modes() {
            let make = || module(s.id, m.id);
            assert_eq!(contract::check_kernel(&make), Vec::<String>::new(), "{}/{}", s.id, m.id);
        }
    }
}

#[test]
fn every_mode_renders_its_kernels_golden_bits_through_the_module() {
    let golden = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/par.tsv")).unwrap();
    let g: HashMap<(&str, &str), &str> = golden
        .lines()
        .map(|l| {
            let mut f = l.split('\t');
            ((f.next().unwrap(), f.next().unwrap()), f.next().unwrap())
        })
        .collect();
    let mut bad = Vec::new();
    for s in SUITES {
        for m in s.modes() {
            let make = || module(s.id, m.id);
            for (sc, h) in par::scenarios(&make) {
                let h = format!("{h:016x}");
                if g.get(&(m.kernel, sc.as_str())) != Some(&h.as_str()) {
                    bad.push(format!("{}/{} {sc}", s.id, m.id));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} module renders differ from the kernel's golden:\n{}", bad.len(), bad.join("\n"));
}

// ---- switching modes -------------------------------------------------------------

fn sine(frames: usize, hz: f32) -> (Vec<f32>, Vec<f32>) {
    let l: Vec<f32> = (0..frames).map(|n| 0.5 * (std::f32::consts::TAU * hz * n as f32 / SR).sin()).collect();
    (l.clone(), l)
}

#[test]
fn a_switch_crossfades_for_twenty_ms_without_a_click() {
    let from = suite::find_mode("modulation", "tremolo").unwrap();
    let to = suite::find_mode("utility", "gain").unwrap();
    let make = |m: &suite::Mode| (prism_fx::find(m.kernel).unwrap().make)(SR);
    let (input, _) = sine(4800, 220.0);
    // each mode alone, for the steps the signal itself makes
    let alone = |m: &suite::Mode| {
        let mut k = make(m);
        let (mut l, mut r) = (input.clone(), input.clone());
        k.process(&mut l, &mut r);
        l
    };
    let (a, b) = (alone(from), alone(to));
    let mut k = ModuleKernel::new(from, SR).unwrap();
    assert!(k.begin_switch(to, make(to)).is_ok());
    assert!(k.switching());
    // a second switch while one runs is handed back, not dropped on the audio thread
    assert!(k.begin_switch(to, make(to)).is_err());
    let (mut l, mut r) = (input.clone(), input.clone());
    k.process(&mut l, &mut r);
    assert!(!k.switching(), "20 ms is 960 frames; done by 4800");
    assert_eq!(k.mode().id, "gain");
    assert!(k.take_retired().is_some(), "the old kernel waits to be dropped off the audio thread");
    assert!(k.take_retired().is_none());
    // No step over −60 dBFS beyond what the two modes' own steps explain. An
    // equal-power fade of two correlated signals sums to at most √2 of either
    // (cos + sin ≤ √2), so that is the honest bound for the signal's own motion.
    for n in 1..4800 {
        let own = (a[n] - a[n - 1]).abs().max((b[n] - b[n - 1]).abs());
        assert!((l[n] - l[n - 1]).abs() <= std::f32::consts::SQRT_2 * own + 0.001, "click at {n}: {} → {} (own step {own})", l[n - 1], l[n]);
    }
}

#[test]
fn after_the_fade_the_new_mode_is_bit_identical_to_a_fresh_one_with_its_memory() {
    let from = suite::find_mode("delay", "echo").unwrap();
    let to = suite::find_mode("reverb", "schroeder-reverb").unwrap();
    // the values the slot remembers for the mode being returned to
    let memory = [(0usize, 0.3f32), (1, 0.7)];
    let build = || {
        let mut n = (prism_fx::find(to.kernel).unwrap().make)(SR);
        for &(i, v) in &memory {
            n.set_param(i, v * (n.descriptor().params[i].max - n.descriptor().params[i].min) + n.descriptor().params[i].min);
        }
        n.reset();
        n
    };
    let (input_l, input_r) = par::input();
    let at = 4_000;
    let mut k = ModuleKernel::new(from, SR).unwrap();
    let (mut l, mut r) = (input_l.clone(), input_r.clone());
    k.process(&mut l[..at], &mut r[..at]);
    k.begin_switch(to, build()).ok().unwrap();
    k.process(&mut l[at..], &mut r[at..]);
    let mut fresh = build();
    let (mut fl, mut fr) = (input_l[at..].to_vec(), input_r[at..].to_vec());
    fresh.process(&mut fl, &mut fr);
    let fade = suite::fade_frames(SR) as usize;
    assert_eq!(fade, 960);
    for n in fade..fl.len() {
        assert_eq!(l[at + n].to_bits(), fl[n].to_bits(), "frame {n} after the switch");
        assert_eq!(r[at + n].to_bits(), fr[n].to_bits(), "frame {n} after the switch");
    }
}

#[test]
fn the_module_reports_the_running_modes_latency_and_tail() {
    let k = ModuleKernel::new(suite::find_mode("reverb", "schroeder-reverb").unwrap(), SR).unwrap();
    let raw = prism_fx::make("schroeder-reverb", SR).unwrap();
    assert_eq!(k.latency_frames(), raw.latency_frames());
    assert_eq!(k.tail_seconds(), raw.tail_seconds());
    assert_eq!(k.descriptor().id, "schroeder-reverb");
}
