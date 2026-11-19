//! What the Echo parity test plays, shared word for word with the harness that
//! recorded MultiGrainLive's Echo doing the same, so both sides render exactly
//! the same thing.
//!
//! Every move lands between 512-frame blocks, because that is where
//! MultiGrainLive applies them. Only the delay time and glide move: the prism
//! Echo glides wet, dry, feedback and damping over 5 ms where MultiGrainLive
//! steps them at the block edge, so a move of those is meant to differ.

#![allow(dead_code)]

pub const BLOCK: usize = 512;

pub enum Step {
    Set(&'static str, f32),
    Run(usize),
    Reset,
}

pub struct Scenario {
    pub name: &'static str,
    pub sample_rate: u32,
    pub steps: &'static [Step],
}

use Step::*;

pub static SCENARIOS: &[Scenario] = &[
    Scenario { name: "defaults", sample_rate: 48_000, steps: &[Run(48_000)] },
    Scenario {
        name: "short, dark, negative feedback, all wet",
        sample_rate: 48_000,
        steps: &[
            Set("delayMs", 3.7),
            Set("feedback", -0.9),
            Set("damping", 0.7),
            Set("wet", 1.0),
            Set("dry", 0.0),
            Run(24_000),
        ],
    },
    Scenario {
        name: "longest delay, most feedback",
        sample_rate: 48_000,
        steps: &[Set("delayMs", 4_000.0), Set("feedback", 0.98), Set("damping", 0.0), Run(200_000)],
    },
    Scenario {
        name: "delay moves glide",
        sample_rate: 48_000,
        steps: &[
            Set("delayMs", 200.0),
            Set("glideMs", 200.0),
            Run(24_064),
            Set("delayMs", 400.0),
            Run(24_064),
            Set("delayMs", 37.0),
            Run(24_064),
        ],
    },
    Scenario {
        name: "no glide jumps",
        sample_rate: 48_000,
        steps: &[Set("glideMs", 0.0), Set("delayMs", 100.0), Run(4_608), Set("delayMs", 300.0), Run(4_608)],
    },
    Scenario {
        name: "glide changed mid-move",
        sample_rate: 48_000,
        steps: &[
            Set("delayMs", 1_000.0),
            Set("glideMs", 4_000.0),
            Run(9_728),
            Set("delayMs", 10.0),
            Run(9_728),
            Set("glideMs", 50.0),
            Run(9_728),
        ],
    },
    Scenario {
        name: "reset, then a new time",
        sample_rate: 48_000,
        steps: &[Run(12_288), Reset, Set("delayMs", 50.0), Run(12_288)],
    },
    Scenario {
        name: "44.1 kHz",
        sample_rate: 44_100,
        steps: &[Set("delayMs", 123.4), Set("feedback", 0.6), Run(44_100)],
    },
];

/// Every `(sample_rate, max_frames)` pair needed to cover `SCENARIOS`, sorted
/// by sample rate. Every scenario at a given sample rate reads a prefix of the
/// same sequence `0..max_frames`.
pub fn fixture_specs() -> Vec<(u32, usize)> {
    let mut by_rate: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
    for scenario in SCENARIOS {
        let frames: usize = scenario
            .steps
            .iter()
            .map(|step| match *step {
                Run(n) => n,
                Set(_, _) | Reset => 0,
            })
            .sum();
        let entry = by_rate.entry(scenario.sample_rate).or_insert(0);
        if frames > *entry {
            *entry = frames;
        }
    }
    by_rate.into_iter().collect()
}

pub fn fixture_name(sample_rate: u32, frames: usize) -> String {
    format!("echo-scenarios-{sample_rate}-{frames}.f32")
}

/// The input at frame `n` of a scenario: a click, then a tone, then noise
/// bursts, different on each side.
///
/// Kept only for the MacBook fixture recorder so every machine feeds the
/// recorded hashes the exact same sample bits.
pub fn input_computed(n: usize, sample_rate: u32) -> (f32, f32) {
    let t = n as f32 / sample_rate as f32;
    if n == 0 {
        return (1.0, -0.8);
    }
    let mut x = (n as u32).wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    let noise = (x as f32 / u32::MAX as f32) * 2.0 - 1.0;
    match (n / 6_000) % 4 {
        0 => (0.5 * (2.0 * core::f32::consts::PI * 440.0 * t).sin(), 0.4 * (2.0 * core::f32::consts::PI * 660.0 * t).sin()),
        1 => (0.3 * noise, -0.3 * noise),
        2 => (0.0, 0.0),
        _ => (0.6 * (2.0 * core::f32::consts::PI * 3_000.0 * t).sin(), 0.2 * noise),
    }
}

fn load_fixture(sample_rate: u32) -> Vec<(f32, f32)> {
    let frames = fixture_specs()
        .into_iter()
        .find(|&(sr, _)| sr == sample_rate)
        .map(|(_, f)| f)
        .unwrap_or_else(|| panic!("no echo-scenarios fixture configured for sample rate {sample_rate}"));
    let path: std::path::PathBuf =
        [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", &fixture_name(sample_rate, frames)].iter().collect();
    let bytes = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "missing fixture {}: {e} (record on the MacBook with: cargo test --release -p prism-fx record_recorded_inputs -- --ignored)",
            path.display()
        )
    });
    let expected_bytes = frames * 2 * core::mem::size_of::<f32>();
    assert_eq!(
        bytes.len(),
        expected_bytes,
        "fixture {} has {} bytes, expected {expected_bytes}",
        path.display(),
        bytes.len(),
    );
    bytes
        .chunks_exact(8)
        .map(|frame| (f32::from_le_bytes(frame[..4].try_into().unwrap()), f32::from_le_bytes(frame[4..].try_into().unwrap())))
        .collect()
}

/// Loads frame `n` from the recorded MacBook input for `sample_rate`
/// (`tests/fixtures/echo-scenarios-<sr>-<frames>.f32`, interleaved
/// little-endian `f32`).
pub fn input(n: usize, sample_rate: u32) -> (f32, f32) {
    thread_local! {
        static CACHE: std::cell::RefCell<std::collections::HashMap<u32, Vec<(f32, f32)>>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    CACHE.with(|cache| {
        let mut map = cache.borrow_mut();
        let buf = map.entry(sample_rate).or_insert_with(|| load_fixture(sample_rate));
        *buf.get(n)
            .unwrap_or_else(|| panic!("frame {n} out of range for echo-scenarios fixture at {sample_rate} Hz (len {})", buf.len()))
    })
}

/// An Echo as the scenarios drive it: stereo, planar, in blocks.
pub trait Echo {
    fn set(&mut self, key: &str, value: f32);
    fn process(&mut self, left: &mut [f32], right: &mut [f32]);
    fn reset(&mut self);
}

/// Play a scenario and fingerprint every output bit, frame by frame.
pub fn play(echo: &mut dyn Echo, scenario: &Scenario) -> u64 {
    let mut hash = Fnv::new();
    let mut n = 0;
    for step in scenario.steps {
        match *step {
            Set(key, value) => echo.set(key, value),
            Reset => echo.reset(),
            Run(frames) => {
                let end = n + frames;
                while n < end {
                    let len = BLOCK.min(end - n);
                    let (mut left, mut right): (Vec<f32>, Vec<f32>) =
                        (n..n + len).map(|i| input(i, scenario.sample_rate)).unzip();
                    echo.process(&mut left, &mut right);
                    for (l, r) in left.iter().zip(&right) {
                        hash.add(l.to_bits());
                        hash.add(r.to_bits());
                    }
                    n += len;
                }
            }
        }
    }
    hash.0
}

pub struct Fnv(pub u64);

impl Fnv {
    pub fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
    pub fn add(&mut self, word: u32) {
        for byte in word.to_le_bytes() {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}
