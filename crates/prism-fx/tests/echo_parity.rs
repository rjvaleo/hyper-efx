//! The prism Echo renders exactly what MultiGrainLive's Echo rendered, bit for
//! bit, so documents that use it load and sound the same once it moves.
//!
//! The fingerprints were recorded from MultiGrainLive's `fx::dattorro::Echo` at
//! commit ba66823, by a harness that plays the same scenarios file through it.
//! A fingerprint that stops matching means the prism Echo no longer sounds like
//! the one documents were made with.

#[path = "support/echo_scenarios.rs"]
mod scenarios;

use scenarios::{play, Echo, SCENARIOS};

struct Prism(Box<dyn prism_fx::DynKernel>);

impl Echo for Prism {
    fn set(&mut self, key: &str, value: f32) {
        let i = self.0.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        self.0.set_param(i, value);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.0.process(left, right);
    }
    fn reset(&mut self) {
        self.0.reset();
    }
}

/// MultiGrainLive's output, scenario by scenario, in `SCENARIOS` order.
const RECORDED: &[(&str, u64)] = &[
    ("defaults", 0xf58d492b011b4ab1),
    ("short, dark, negative feedback, all wet", 0x42456409dcb2532d),
    ("longest delay, most feedback", 0x35f539bff13cdd9b),
    ("delay moves glide", 0xa15ba568e80e6dbd),
    ("no glide jumps", 0x172cd6637bebe9da),
    ("glide changed mid-move", 0xddaaad3f02e7c5d4),
    ("reset, then a new time", 0xdf2103ef6d1e3926),
    ("44.1 kHz", 0x284c41cf8bf75808),
];

#[test]
fn the_prism_echo_matches_multigrainlives_bit_for_bit() {
    assert_eq!(SCENARIOS.len(), RECORDED.len());
    let mut differ = Vec::new();
    for (scenario, &(name, recorded)) in SCENARIOS.iter().zip(RECORDED) {
        assert_eq!(scenario.name, name, "scenarios and recordings are out of step");
        let k = prism_fx::make("echo", scenario.sample_rate as f32).expect("echo is in the catalogue");
        let got = play(&mut Prism(k), scenario);
        if got != recorded {
            differ.push(format!("{name}: {got:#018x}, recorded {recorded:#018x}"));
        }
    }
    assert!(differ.is_empty(), "{differ:#?}");
}

#[test]
fn it_keeps_every_key_multigrainlive_documents_store() {
    let d = prism_fx::find("echo").expect("echo is in the catalogue").descriptor;
    let keys: Vec<&str> = d.params.iter().map(|p| p.key).collect();
    assert_eq!(keys, ["delayMs", "feedback", "damping", "wet", "dry", "glideMs"]);
    // Ranges and defaults as MultiGrainLive has them, so stored values clamp
    // the same way.
    let ranges: Vec<(f32, f32, f32, bool)> = d.params.iter().map(|p| (p.min, p.max, p.default, p.log)).collect();
    assert_eq!(
        ranges,
        [
            (1.0, 4_000.0, 350.0, true),
            (-0.98, 0.98, 0.35, false),
            (0.0, 1.0, 0.2, false),
            (0.0, 1.0, 0.5, false),
            (0.0, 1.0, 1.0, false),
            (0.0, 4_000.0, 220.0, false),
        ]
    );
}
