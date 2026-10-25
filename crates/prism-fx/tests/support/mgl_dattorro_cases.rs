// What the Dattorro family's golden renders were made from, as plain data.
//
// Included, not compiled as a module of its own: by `mgl_dattorro.rs` for the
// tests, and by the standalone generator that renders MultiGrainLive's own
// effects into `tests/fixtures/`. One table, so the two cannot drift apart.
//
// Every case builds the effect, sets `start` the way MultiGrainLive's
// `shape::make` does (after `new`, no reset), runs the input in blocks of
// `BLOCK` frames, and sets `moved` just before the block at `MOVE_AT`.

/// The render's rate.
pub const RATE: u32 = 48_000;
/// Frames rendered: one second.
pub const FRAMES: usize = 48_000;
/// The host block the originals were rendered with.
pub const BLOCK: usize = 256;
/// Where the mid-render move lands: a block boundary, 94 blocks in.
pub const MOVE_AT: usize = 94 * BLOCK;

/// Whether frame `i` is kept in a fixture.
///
/// Every 61st frame across the whole render (prime, so it walks across the
/// block boundaries instead of sitting on them), and every frame of a short
/// window around the move. Keeping all 48 000 frames for every case would be
/// several megabytes; any drift in a delay line or a filter state shows up at
/// the next kept frame anyway.
pub fn stored(i: usize) -> bool {
    i.is_multiple_of(61) || (MOVE_AT - 32..MOVE_AT + 96).contains(&i)
}

pub struct Case {
    pub name: &'static str,
    pub start: &'static [(&'static str, f32)],
    pub moved: &'static [(&'static str, f32)],
}

pub struct Family {
    /// The prism id, and the fixture folder.
    pub id: &'static str,
    /// MultiGrainLive's `ShapeKind::as_str`.
    pub kind: &'static str,
    pub cases: &'static [Case],
}

const DEFAULTS: Case = Case { name: "defaults", start: &[], moved: &[] };

pub const FAMILIES: &[Family] = &[
    Family {
        id: "dattorro-notch",
        kind: "dattorro_notch",
        cases: &[
            DEFAULTS,
            Case { name: "narrow", start: &[("frequency", 440.0), ("bandwidth", 30.0), ("mix", 0.8)], moved: &[] },
            Case {
                name: "moved",
                start: &[("frequency", 150.0)],
                moved: &[("frequency", 3000.0), ("bandwidth", 800.0), ("gainDb", 12.0), ("mix", 0.5)],
            },
        ],
    },
    Family {
        id: "dattorro-resonator",
        kind: "dattorro_resonator",
        cases: &[
            DEFAULTS,
            Case { name: "narrow", start: &[("frequency", 2500.0), ("bandwidth", 12.0), ("mix", 0.9)], moved: &[] },
            Case {
                name: "moved",
                start: &[("bandwidth", 1.0)],
                moved: &[("frequency", 90.0), ("bandwidth", 10_000.0), ("mix", 0.4)],
            },
        ],
    },
    Family {
        id: "regalia",
        kind: "regalia_mitra",
        cases: &[
            DEFAULTS,
            Case { name: "cut", start: &[("gainDb", -18.0), ("frequency", 250.0), ("bandwidth", 100.0)], moved: &[] },
            Case {
                name: "moved",
                start: &[("gainDb", 3.0)],
                moved: &[("frequency", 6000.0), ("bandwidth", 3000.0), ("gainDb", 24.0), ("mix", 0.7)],
            },
        ],
    },
    Family {
        id: "chamberlin",
        kind: "chamberlin",
        cases: &[
            DEFAULTS,
            Case {
                name: "all-four",
                start: &[
                    ("lowFreq", 300.0),
                    ("lowQ", 4.0),
                    ("lowAmp", 0.6),
                    ("bandOn", 1.0),
                    ("bandFreq", 1500.0),
                    ("bandQ", 8.0),
                    ("bandAmp", 0.8),
                    ("highOn", 1.0),
                    ("highFreq", 6000.0),
                    ("highQ", 2.0),
                    ("highAmp", 0.5),
                    ("notchOn", 1.0),
                    ("notchFreq", 900.0),
                    ("notchQ", 3.0),
                    ("notchAmp", 0.4),
                    ("drive", 3.0),
                ],
                moved: &[],
            },
            Case {
                name: "moved",
                start: &[],
                moved: &[
                    ("lowFreq", 15_000.0),
                    ("lowQ", 10.0),
                    ("drive", 0.25),
                    ("highOn", 1.0),
                    ("highAmp", 1.0),
                    ("highFreq", 100.0),
                ],
            },
        ],
    },
    Family {
        id: "white-chorus",
        kind: "white_chorus",
        cases: &[
            DEFAULTS,
            Case {
                name: "allpass",
                start: &[
                    ("allpass", 1.0),
                    ("stereo", 0.25),
                    ("feedback", -0.7),
                    ("rateHz", 3.0),
                    ("depthMs", 20.0),
                    ("delayMs", 30.0),
                ],
                moved: &[],
            },
            Case {
                name: "moved",
                start: &[],
                moved: &[
                    ("delayMs", 60.0),
                    ("depthMs", 40.0),
                    ("rateHz", 12.0),
                    ("feedback", 0.9),
                    ("wet", 0.5),
                    ("dry", 0.3),
                    ("allpass", 1.0),
                ],
            },
        ],
    },
    Family {
        id: "flanger",
        kind: "dattorro_flanger",
        cases: &[
            DEFAULTS,
            Case {
                name: "deep",
                start: &[("feedback", -0.95), ("rateHz", 0.8), ("depthMs", 2.0), ("delayMs", 0.4), ("stereo", 1.0)],
                moved: &[],
            },
            Case {
                name: "moved",
                start: &[("allpass", 1.0)],
                moved: &[("delayMs", 5.0), ("rateHz", 7.0), ("feedback", 0.98), ("allpass", 0.0), ("stereo", 0.0)],
            },
        ],
    },
    Family {
        id: "vibrato",
        kind: "dattorro_vibrato",
        cases: &[
            DEFAULTS,
            Case {
                name: "linear",
                start: &[("allpass", 0.0), ("rateHz", 6.0), ("depthMs", 4.0), ("dry", 0.5), ("feedback", 0.3)],
                moved: &[],
            },
            Case {
                name: "moved",
                start: &[],
                moved: &[("rateHz", 18.0), ("depthMs", 10.0), ("delayMs", 12.0), ("stereo", 0.0)],
            },
        ],
    },
    Family {
        id: "pn-noise",
        kind: "pn_noise",
        cases: &[
            DEFAULTS,
            Case {
                name: "slow",
                start: &[("clockHz", 7000.0), ("level", 0.5), ("mix", 0.5), ("seed", 12345.0)],
                moved: &[],
            },
            Case {
                name: "moved",
                start: &[("clockHz", 3000.0)],
                moved: &[("clockHz", 450.0), ("seed", 777.0), ("level", 0.8), ("mix", 0.9)],
            },
        ],
    },
    Family {
        id: "pn-noise-eq",
        kind: "pn_noise_eq",
        cases: &[
            DEFAULTS,
            Case {
                name: "slow",
                start: &[("clockHz", 7000.0), ("level", 0.5), ("mix", 0.5), ("seed", 12345.0)],
                moved: &[],
            },
            Case {
                name: "moved",
                start: &[("clockHz", 3000.0)],
                moved: &[("clockHz", 450.0), ("seed", 777.0), ("level", 0.8), ("mix", 0.9)],
            },
        ],
    },
    Family {
        id: "dattorro-plate",
        kind: "dattorro_plate",
        cases: &[
            DEFAULTS,
            Case {
                name: "voiced",
                start: &[
                    ("predelayMs", 30.0),
                    ("decay", 0.85),
                    ("damping", 0.3),
                    ("modDepth", 16.0),
                    ("modRateHz", 1.2),
                    ("wet", 0.7),
                    ("dry", 0.5),
                    ("crossfeed", 0.6),
                    ("bandwidth", 0.7),
                ],
                moved: &[],
            },
            Case {
                name: "moved",
                start: &[],
                moved: &[
                    ("decay", 0.95),
                    ("predelayMs", 120.0),
                    ("inputDiffusion1", 0.2),
                    ("inputDiffusion2", 0.9),
                    ("decayDiffusion1", 0.9),
                    ("decayDiffusion2", 0.1),
                    ("wet", 1.0),
                    ("dry", 0.0),
                ],
            },
        ],
    },
    Family {
        id: "allpass-diffuser",
        kind: "allpass_diffuser",
        cases: &[
            DEFAULTS,
            Case {
                name: "voiced",
                start: &[("coefficient", -0.8), ("delayMs", 3.3), ("wet", 0.6), ("dry", 0.4)],
                moved: &[],
            },
            Case { name: "moved", start: &[], moved: &[("delayMs", 50.0), ("coefficient", 0.95)] },
        ],
    },
    Family {
        id: "damping-filter",
        kind: "damping_filter",
        cases: &[
            DEFAULTS,
            Case { name: "dark", start: &[("cutoffHz", 300.0), ("wet", 0.7), ("dry", 0.3)], moved: &[] },
            Case { name: "moved", start: &[("cutoffHz", 40.0)], moved: &[("cutoffHz", 12_000.0)] },
        ],
    },
    Family {
        id: "leslie",
        kind: "leslie",
        cases: &[
            DEFAULTS,
            Case {
                name: "fast",
                start: &[
                    ("rateHz", 6.5),
                    ("depthMs", 5.0),
                    ("amDepth", 0.8),
                    ("width", 0.4),
                    ("baseMs", 35.0),
                    ("dry", 0.3),
                ],
                moved: &[],
            },
            Case { name: "moved", start: &[], moved: &[("rateHz", 0.2), ("depthMs", 8.0), ("baseMs", 1.0)] },
        ],
    },
    Family {
        id: "single-bit-pn",
        kind: "single_bit_pn",
        cases: &[
            DEFAULTS,
            Case {
                name: "slow",
                start: &[("clockHz", 3300.0), ("level", 0.3), ("mix", 0.4), ("seed", 4242.0)],
                moved: &[],
            },
            Case { name: "moved", start: &[], moved: &[("clockHz", 123.0), ("level", 0.05), ("seed", 9.0)] },
        ],
    },
    Family {
        id: "harmonizer",
        kind: "harmonizer",
        cases: &[
            DEFAULTS,
            Case {
                name: "down",
                start: &[("semitones", -12.0), ("windowMs", 25.0), ("feedback", 0.6), ("wet", 1.0), ("dry", 0.0)],
                moved: &[],
            },
            Case {
                name: "moved",
                start: &[],
                moved: &[("semitones", 5.0), ("windowMs", 200.0), ("feedback", 0.9)],
            },
        ],
    },
    Family {
        id: "detune",
        kind: "detune",
        cases: &[
            DEFAULTS,
            Case {
                name: "wide",
                start: &[("cents", 60.0), ("windowMs", 15.0), ("wet", 1.0), ("dry", 0.2)],
                moved: &[],
            },
            Case { name: "moved", start: &[], moved: &[("cents", 3.0), ("windowMs", 240.0)] },
        ],
    },
    Family {
        id: "doubler",
        kind: "doubler",
        cases: &[
            DEFAULTS,
            Case {
                name: "close",
                start: &[("delayMs", 8.0), ("variationMs", 15.0), ("rateHz", 3.0)],
                moved: &[],
            },
            Case {
                name: "moved",
                start: &[],
                moved: &[("delayMs", 90.0), ("variationMs", 0.0), ("rateHz", 0.07), ("wet", 1.0)],
            },
        ],
    },
    Family {
        id: "doppler",
        kind: "doppler",
        cases: &[
            DEFAULTS,
            Case {
                name: "receding",
                start: &[("velocityMps", 60.0), ("distanceMs", 100.0), ("wet", 1.0), ("dry", 0.3)],
                moved: &[],
            },
            Case {
                name: "moved",
                start: &[("velocityMps", -30.0)],
                moved: &[("distanceMs", 500.0), ("velocityMps", 150.0), ("speedMps", 40.0)],
            },
        ],
    },
    Family {
        id: "dattorro-filter-bank",
        kind: "dattorro_filter_bank",
        cases: &[
            DEFAULTS,
            Case {
                name: "voiced",
                start: &[
                    ("notchHz", 120.0),
                    ("notchQ", 18.0),
                    ("resonatorHz", 700.0),
                    ("resonatorQ", 0.2),
                    ("resonatorAmp", 1.0),
                    ("regaliaOn", 0.0),
                    ("dampingHz", 2000.0),
                    ("dampingQ", 4.0),
                    ("dampingAmp", 0.6),
                ],
                moved: &[],
            },
            Case {
                name: "moved",
                start: &[],
                moved: &[
                    ("notchOn", 0.0),
                    ("regaliaHz", 400.0),
                    ("regaliaQ", 12.0),
                    ("regaliaAmp", 0.2),
                    ("dampingQ", 18.0),
                    ("resonatorOn", 0.0),
                ],
            },
        ],
    },
];

/// The input: a sine sweep with deterministic noise under it, different on
/// each side, then silence with one click in it so the tails show.
///
/// The generator writes this to `fixtures/mgl-dattorro/input.f32`, and the
/// tests read that file back rather than calling this, so the comparison does
/// not lean on two builds agreeing about `sin`.
pub fn input() -> (Vec<f32>, Vec<f32>) {
    let mut seed: u32 = 0x1357_9bdf;
    let mut noise = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    };
    let sweep_end = 36_000usize;
    let (f0, f1) = (40.0f64, 12_000.0f64);
    let span = sweep_end as f64 / RATE as f64;
    let k = (f1 / f0).ln();
    let (mut left, mut right) = (Vec::with_capacity(FRAMES), Vec::with_capacity(FRAMES));
    for n in 0..FRAMES {
        let (nl, nr) = (noise(), noise());
        if n < sweep_end {
            let t = n as f64 / RATE as f64;
            let phase = core::f64::consts::TAU * f0 * span / k * ((k * t / span).exp() - 1.0);
            left.push((0.35 * phase.sin()) as f32 + 0.15 * nl);
            right.push((0.3 * (phase * 1.01 + 0.7).sin()) as f32 + 0.15 * nr);
        } else if n == 40_000 {
            left.push(0.8);
            right.push(-0.5);
        } else {
            left.push(0.0);
            right.push(0.0);
        }
    }
    (left, right)
}
