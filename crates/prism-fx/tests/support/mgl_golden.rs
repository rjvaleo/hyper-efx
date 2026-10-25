//! Golden renders of MultiGrainLive's own effects: the test signal, the
//! settings each render uses, and how a prism kernel is checked against them.
//!
//! The same file is compiled into the generator that recorded the fixtures
//! (it drove MultiGrainLive's `fx` crate, 256 frames a block, stereo, 48 kHz),
//! so the scenarios cannot drift apart from what was recorded. Controls move
//! only on block boundaries, where the originals read them.
//!
//! Fixtures live in `tests/fixtures/<id>/`: `input.f32` and one file per
//! render, interleaved stereo, little-endian f32.

#![allow(dead_code)]

pub const SAMPLE_RATE: u32 = 48_000;
/// Half a second: enough for every move to show, small enough to keep.
pub const FRAMES: usize = 24_000;
/// The block size the originals were driven with.
pub const BLOCK: usize = 256;
/// Where the input goes silent, so tails and releases show.
pub const SILENT_FROM: usize = 18_432;

pub type Settings = &'static [(&'static str, f32)];

pub struct Render {
    pub name: &'static str,
    /// Set before the first frame.
    pub start: Settings,
    /// Set at these frames, each a multiple of `BLOCK`.
    pub moves: &'static [(usize, Settings)],
}

/// Deterministic noise and an exponential sine sweep, different on each side,
/// interleaved.
pub fn input() -> Vec<f32> {
    let mut state = 0x1234_5678u32;
    let mut noise = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as f32 / u32::MAX as f32 - 0.5
    };
    let sr = SAMPLE_RATE as f64;
    let seconds = SILENT_FROM as f64 / sr;
    let (f0, f1) = (20.0f64, 20_000.0f64);
    let k = (f1 / f0).ln() / seconds;
    let mut out = Vec::with_capacity(FRAMES * 2);
    for n in 0..FRAMES {
        if n >= SILENT_FROM {
            out.push(0.0);
            out.push(0.0);
            continue;
        }
        let t = n as f64 / sr;
        // Phase of an exponential sweep, f0 → f1 over `seconds`.
        let up = (std::f64::consts::TAU * f0 * ((k * t).exp() - 1.0) / k).sin() as f32;
        let down = (std::f64::consts::TAU * f1 * (1.0 - (-k * t).exp()) / k).sin() as f32;
        let (a, b) = (noise(), noise());
        out.push(0.5 * up + 0.5 * a);
        out.push(0.4 * down + 0.6 * b);
    }
    out
}

pub fn renders(id: &str) -> &'static [Render] {
    match id {
        "schroeder-reverb" => SCHROEDER,
        "moorer-reverb" => MOORER,
        "phaser" => PHASER,
        "eq" => EQ,
        "compressor" => COMPRESSOR,
        _ => &[],
    }
}

pub const IDS: [&str; 5] = ["schroeder-reverb", "moorer-reverb", "phaser", "eq", "compressor"];

const SCHROEDER: &[Render] = &[
    Render { name: "defaults", start: &[], moves: &[] },
    Render {
        name: "big-then-small-then-frozen",
        start: &[
            ("decay", 0.95),
            ("size", 2.5),
            ("diffusion", 0.5),
            ("wet", 0.8),
            ("dry", 0.2),
            ("spread", 0.15),
            ("predelayMs", 40.0),
        ],
        moves: &[(8_192, &[("size", 0.4), ("decay", 0.6)]), (16_384, &[("freeze", 1.0)])],
    },
    Render {
        name: "tiny-endless-frozen-thawed",
        start: &[("size", 0.1), ("diffusion", 0.95), ("decay", 0.9995), ("wet", 1.0), ("dry", 0.0), ("spread", 0.0)],
        moves: &[
            (8_192, &[("freeze", 1.0)]),
            (16_384, &[("freeze", 0.0), ("predelayMs", 250.0), ("spread", 0.2), ("size", 8.0)]),
        ],
    },
];

const MOORER: &[Render] = &[
    Render { name: "defaults", start: &[], moves: &[] },
    Render {
        name: "damped-then-bright-then-frozen",
        start: &[
            ("decay", 0.95),
            ("damping", 2.0),
            ("size", 1.7),
            ("predelayMs", 30.0),
            ("spread", 0.2),
            ("wet", 0.7),
            ("dry", 0.3),
            ("diffusion", 0.4),
        ],
        moves: &[(8_192, &[("damping", 0.0), ("size", 0.5)]), (16_384, &[("freeze", 1.0)])],
    },
    Render {
        name: "huge-frozen-thawed",
        start: &[
            ("decay", 0.5),
            ("damping", 0.5),
            ("size", 8.0),
            ("diffusion", 0.9),
            ("wet", 1.0),
            ("dry", 0.0),
            ("spread", 0.0),
        ],
        moves: &[(8_192, &[("freeze", 1.0)]), (16_384, &[("freeze", 0.0), ("size", 3.0), ("predelayMs", 120.0)])],
    },
];

const PHASER: &[Render] = &[
    Render { name: "defaults", start: &[], moves: &[] },
    Render {
        name: "dense-resonant-fast",
        start: &[
            ("notches", 8.0),
            ("centreHz", 200.0),
            ("spread", 1.3),
            ("resonance", 0.95),
            ("depth", -0.8),
            ("rateHz", 4.0),
            ("sweep", 3.0),
            ("stereo", 0.5),
            ("feedback", 0.9),
            ("wet", 1.0),
            ("dry", 0.0),
        ],
        moves: &[
            (8_192, &[("notches", 3.0), ("rateHz", 9.5), ("centreHz", 2_000.0)]),
            (16_384, &[("feedback", -0.95), ("resonance", 0.999), ("depth", 1.0)]),
        ],
    },
    Render {
        name: "one-notch-still-then-sweeping",
        start: &[
            ("notches", 1.0),
            ("centreHz", 8_000.0),
            ("spread", 3.0),
            ("resonance", 0.0),
            ("rateHz", 0.0),
            ("sweep", 4.0),
            ("stereo", 0.0),
            ("wet", 0.5),
            ("dry", 0.5),
        ],
        moves: &[
            (8_192, &[("notches", 5.4), ("rateHz", 10.0), ("sweep", 0.0), ("resonance", 0.6)]),
            (16_384, &[("centreHz", 40.0), ("sweep", 4.0), ("stereo", 0.37)]),
        ],
    },
];

const EQ: &[Render] = &[
    Render {
        name: "bells-then-rumble-cut-then-flat",
        start: &[
            ("band.1.gainDb", -6.0),
            ("band.2.gainDb", 9.0),
            ("band.6.gainDb", 4.0),
            ("band.3.enabled", 1.0),
            ("band.3.gainDb", -12.0),
            ("band.3.q", 8.0),
            ("band.3.freq", 333.0),
        ],
        moves: &[
            (8_192, &[("band.0.freq", 120.0), ("band.2.freq", 3_000.0), ("band.6.mode", 3.0)]),
            (
                16_384,
                &[
                    ("band.1.gainDb", 0.0),
                    ("band.2.gainDb", 0.0),
                    ("band.6.gainDb", 0.0),
                    ("band.3.gainDb", 0.0),
                    ("band.0.freq", 10.0),
                ],
            ),
        ],
    },
    Render {
        name: "cuts-anywhere-then-extremes",
        start: &[
            ("band.7.enabled", 1.0),
            ("band.7.freq", 5_000.0),
            ("band.4.enabled", 1.0),
            ("band.4.mode", 0.0),
            ("band.4.freq", 400.0),
            ("band.4.q", 4.0),
            ("band.5.enabled", 1.0),
            ("band.5.mode", 5.0),
            ("band.5.freq", 9_000.0),
            ("band.5.q", 0.3),
            ("band.1.mode", 4.0),
            ("band.1.gainDb", 6.0),
            ("band.1.freq", 2_000.0),
            ("band.1.q", 2.0),
        ],
        moves: &[
            (
                8_192,
                &[("band.7.mode", 0.0), ("band.7.freq", 50.0), ("band.0.mode", 5.0), ("band.0.freq", 15_000.0), ("band.1.enabled", 0.0)],
            ),
            (
                16_384,
                &[
                    ("band.0.mode", 1.0),
                    ("band.2.gainDb", -24.0),
                    ("band.2.q", 0.05),
                    ("band.2.freq", 24_000.0),
                    ("band.3.enabled", 1.0),
                    ("band.3.mode", 3.0),
                    ("band.3.gainDb", 24.0),
                    ("band.3.q", 18.0),
                    ("band.3.freq", 10.0),
                ],
            ),
        ],
    },
    Render {
        name: "flat-then-boost-then-shelves",
        start: &[],
        moves: &[
            (8_192, &[("band.2.gainDb", 12.0)]),
            (
                16_384,
                &[
                    ("band.6.gainDb", -24.0),
                    ("band.6.freq", 10.0),
                    ("band.1.q", 18.0),
                    ("band.1.gainDb", 24.0),
                    ("band.1.freq", 24_000.0),
                    ("band.0.enabled", 0.0),
                    ("band.0.freq", 500.0),
                ],
            ),
        ],
    },
];

const COMPRESSOR: &[Render] = &[
    Render { name: "defaults", start: &[], moves: &[] },
    Render {
        name: "fast-hard-then-slow-then-soft",
        start: &[
            ("thresholdDb", -40.0),
            ("ratio", 20.0),
            ("attackMs", 0.05),
            ("releaseMs", 5.0),
            ("kneeDb", 0.0),
            ("makeupDb", 12.0),
        ],
        moves: &[
            (8_192, &[("attackMs", 500.0), ("releaseMs", 3_000.0)]),
            (16_384, &[("kneeDb", 24.0), ("ratio", 1.5), ("makeupDb", -24.0)]),
        ],
    },
    Render {
        name: "inert-then-squeezed-then-released",
        start: &[("ratio", 1.0), ("makeupDb", 3.0)],
        moves: &[
            (8_192, &[("ratio", 8.0), ("thresholdDb", -30.0), ("kneeDb", 12.0)]),
            (16_384, &[("thresholdDb", 0.0), ("attackMs", 2.0), ("releaseMs", 50.0)]),
        ],
    },
];
