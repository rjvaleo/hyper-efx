//! The golden renders for MultiGrainLive's shaping family: the test signal,
//! the settings each effect is played at, and how a render is kept.
//!
//! One file, read word for word by two programs: the generator that played
//! MultiGrainLive's own `fx` effects and wrote `tests/fixtures/`, and the tests
//! that play the prism kernels against what it wrote. Change anything here and
//! the fixtures no longer describe the same renders — regenerate them.
//!
//! A render is 1.5 s of stereo at 48 kHz, played in blocks of 256 frames, with
//! the case's second set of values written between two blocks halfway in. The
//! whole render is fingerprinted (FNV-1a over the bits of every sample), and
//! every 61st frame is kept as little-endian f32, left then right. The
//! fingerprint holds every sample to the bit; the kept frames say by how much,
//! and where, a render that misses it differs. Keeping every frame of 31
//! renders would be 18 MB; this is about 300 KB.

#![allow(dead_code)]

pub const SAMPLE_RATE: u32 = 48_000;
pub const FRAMES: usize = 72_000;
/// The block size MultiGrainLive's rack was played at.
pub const BLOCK: usize = 256;
/// Where the second set of values is written: between blocks 140 and 141.
pub const MOVE_AT: usize = 141 * BLOCK;
/// Every this-many-th frame is kept. Prime, so it walks across the block size
/// and every periodic thing in the signal.
pub const STRIDE: usize = 61;

pub struct Case {
    /// The prism id, which is also MultiGrainLive's kind.
    pub id: &'static str,
    pub name: &'static str,
    /// Written before the first frame, in order.
    pub start: &'static [(&'static str, f32)],
    /// Written at `MOVE_AT`, in order.
    pub moves: &'static [(&'static str, f32)],
}

const fn case(
    id: &'static str,
    name: &'static str,
    start: &'static [(&'static str, f32)],
    moves: &'static [(&'static str, f32)],
) -> Case {
    Case { id, name, start, moves }
}

pub const CASES: &[Case] = &[
    case("invert", "defaults", &[], &[]),
    case("swap", "defaults", &[], &[]),
    case("width", "collapse-to-wide", &[("width", 0.0)], &[("width", 2.0)]),
    case("width", "half", &[("width", 0.5)], &[("width", 1.3)]),
    case("dc", "defaults", &[], &[("hz", 60.0)]),
    case("dc", "lowest", &[("hz", 1.0)], &[("hz", 20.0)]),
    case("ring", "defaults", &[], &[("hz", 1000.0)]),
    case("ring", "sweep-folds", &[("hz", 600.0), ("sweep", -2000.0), ("mix", 0.8)], &[("mix", 0.4), ("sweep", 1500.0)]),
    case("ring", "slow-half", &[("hz", 30.0), ("mix", 0.5)], &[("hz", 8000.0)]),
    case("rappify", "defaults", &[], &[("amount", 1.0)]),
    case("rappify", "bright-slow", &[("amount", 0.3), ("hz", 1500.0), ("speed", 5.0)], &[("hz", 100.0), ("speed", 200.0)]),
    case("rappify", "off-then-on", &[("amount", 0.0)], &[("amount", 0.8)]),
    case("boomerang", "defaults", &[], &[("throwMs", 1500.0)]),
    case("boomerang", "short-all-wet", &[("throwMs", 20.0), ("mix", 1.0)], &[("mix", 0.2)]),
    case("boomerang", "longest", &[("throwMs", 2000.0), ("mix", 0.7)], &[("throwMs", 100.0)]),
    case("fit", "defaults", &[], &[("grainMs", 200.0)]),
    case("fit", "hard", &[("amount", 1.0), ("grainMs", 5.0), ("floorDb", -20.0)], &[("amount", 0.0)]),
    case("fit", "off-then-on", &[("amount", 0.0)], &[("amount", 0.9), ("floorDb", -80.0)]),
    case("gate", "defaults", &[], &[("thresholdDb", -10.0)]),
    case(
        "gate",
        "fast-half",
        &[("thresholdDb", -20.0), ("attackMs", 0.1), ("releaseMs", 5.0), ("depth", 0.5)],
        &[("releaseMs", 2000.0)],
    ),
    case("gate", "slow", &[("attackMs", 200.0)], &[("thresholdDb", -80.0), ("depth", 0.25)]),
    case("utility", "defaults", &[], &[("gainDb", 6.0)]),
    case(
        "utility",
        "flipped-wide",
        &[("invert", 1.0), ("swap", 1.0), ("width", 2.0), ("gainDb", -6.0)],
        &[("width", 0.0), ("swap", 0.0)],
    ),
    case("utility", "fit", &[("ampFit", 1.0), ("grainMs", 10.0), ("amount", 1.0)], &[("ampFit", 0.0)]),
    case("utility", "fit-later", &[], &[("ampFit", 1.0), ("floorDb", -30.0)]),
    case("maximizer", "defaults", &[], &[("amount", 1.0)]),
    case("maximizer", "off-then-on", &[("amount", 0.0)], &[("amount", 0.6)]),
    case(
        "maximizer",
        "manual",
        &[("autoLevel", 0.0), ("autoComp", 0.0), ("ceilingDb", -6.0), ("amount", 0.8)],
        &[("autoComp", 1.0)],
    ),
    case("maximizer", "gentle", &[("amount", 0.3), ("ceilingDb", -12.0)], &[("ceilingDb", -1.0), ("autoLevel", 0.0)]),
    case("gain", "unity-to-min", &[("db", 0.0)], &[("db", -24.0)]),
    case("gain", "boost", &[("db", 12.0)], &[("db", 24.0)]),
];

/// The input every case hears, interleaved stereo.
///
/// Exponential sine sweeps (40 Hz to 12 kHz on the left, the other way on the
/// right) with deterministic noise and a DC offset on each side, through a
/// level that is loud, then 30 dB down, then digital silence, then a 40 dB
/// swell with the noise switched in 50 ms bursts — so the gate opens and
/// closes, the fit and the maximiser have dynamics to chase, and the DC filter
/// has an offset to take out.
pub fn signal() -> Vec<f32> {
    let sr = SAMPLE_RATE as f64;
    let seconds = FRAMES as f64 / sr;
    let (lo, hi) = (40.0f64, 12_000.0f64);
    let mut rng = 0x9e37_79b9_7f4a_7c15u64;
    let mut noise = || {
        rng ^= rng >> 12;
        rng ^= rng << 25;
        rng ^= rng >> 27;
        let bits = rng.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11;
        bits as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    };
    let (mut phase_l, mut phase_r) = (0.0f64, 0.0f64);
    let mut out = Vec::with_capacity(FRAMES * 2);
    for n in 0..FRAMES {
        let t = n as f64 / sr;
        let up = lo * (hi / lo).powf(t / seconds);
        let down = hi * (lo / hi).powf(t / seconds);
        phase_l = (phase_l + up / sr).fract();
        phase_r = (phase_r + down / sr).fract();
        let (level, bursts) = if t < 0.35 {
            (1.0, true)
        } else if t < 0.6 {
            (0.03, true)
        } else if t < 0.75 {
            (0.0, false)
        } else {
            (0.01 * 100f64.powf((t - 0.75) / 0.75), (((t - 0.75) / 0.05) as u64).is_multiple_of(2))
        };
        let (nl, nr) = (noise(), noise());
        let (nl, nr) = if bursts { (nl, nr) } else { (0.0, 0.0) };
        let tau = std::f64::consts::TAU;
        let l = level * (0.45 * (tau * phase_l).sin() + 0.12 * nl + 0.04);
        let r = level * (0.35 * (tau * phase_r).sin() + 0.15 * nr - 0.03);
        out.push(l as f32);
        out.push(r as f32);
    }
    out
}

/// FNV-1a over the bits of every sample.
pub fn fingerprint(samples: &[f32]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for s in samples {
        for b in s.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// Every `STRIDE`th frame of an interleaved stereo render.
pub fn strided(samples: &[f32]) -> Vec<f32> {
    samples.chunks(2).step_by(STRIDE).flatten().copied().collect()
}
