//! What Hyperprism-PPC 1.2.1's two delay classes (EDelay and EDelay2) share:
//! how a time in ms becomes samples, the "Param. Speed Ch." and "Smoothing"
//! laws, the rule for other sample rates, and the line itself.
//!
//! The original ran at 44.1 kHz (or the 22 and 11 kHz rates below it) and
//! counted everything in samples. Here every number is the original's at
//! 44.1 kHz; at another rate a count of samples is scaled by rate / 44 100 and
//! a per-step pole is raised to 44 100 / rate, so each takes the same time.

/// The rate the original's numbers are for.
pub(crate) const ORIGINAL_RATE: f64 = 44_100.0;

/// A time in ms as the original's SetParameter makes it samples:
/// `0.001 × (float)(ms × rate)`, kept as a float.
pub(crate) fn samples(ms: f32, sample_rate: f32) -> f32 {
    (0.001 * ((ms as f64 * sample_rate as f64) as f32) as f64) as f32
}

/// A count of samples at 44.1 kHz, at this rate.
pub(crate) fn count(n: i32, sample_rate: f32) -> i32 {
    if sample_rate as f64 == ORIGINAL_RATE {
        n
    } else {
        ((n as f64 * sample_rate as f64 / ORIGINAL_RATE).round() as i32).max(1)
    }
}

/// A one-pole's `(a, b)` per step at 44.1 kHz, at this rate.
fn at_rate(a: f64, b: f64, sample_rate: f32) -> (f64, f64) {
    if sample_rate as f64 == ORIGINAL_RATE {
        (a, b)
    } else {
        let b = b.powf(ORIGINAL_RATE / sample_rate as f64);
        (1.0 - b, b)
    }
}

/// "Param. Speed Ch." (EDelay: EchoTranz, Multi Delay), 1–100: the smoother's
/// `(a, b)` per sample. `N = int(10^(4v/100))`, `a = 1e-8 × N`, `b = 1 − a`,
/// except that 100 gives `N = 0`, which means `a = 1`: no glide at all.
pub(crate) fn speed(v: f32, sample_rate: f32) -> (f64, f64) {
    let v = v as i32;
    let n = if v == 100 { 0 } else { 10f64.powf(4.0 * v as f64 / 100.0) as i32 };
    if n == 0 {
        (1.0, 0.0)
    } else {
        let a = 1e-8 * n as f64;
        at_rate(a, 1.0 - a, sample_rate)
    }
}

/// CEffect's "Smoothing (%)", 0–100 (Comb Filter, Doppler): `(a, b)` per
/// update, as the floats the routines use. `N = int(10^(0.04 × (100 − s)))`,
/// `a = 1e-4 × N`: 1 at no smoothing, 1e-4 at full.
pub(crate) fn smoothing(s: f32, sample_rate: f32) -> (f32, f32) {
    let n = 10f64.powf(0.01 * 4.0 * (100 - s as i32) as f64) as i32;
    let a = 0.0001 * n as f64;
    let (a, b) = at_rate(a, 1.0 - a, sample_rate);
    (a as f32, b as f32)
}

/// A stereo line read by age: age 1 is the frame written last; age 0 is the
/// slot about to be written, which holds the oldest frame, as the original's
/// pointer arithmetic gives when a tap lands on the write pointer.
pub(crate) struct Line {
    frames: Vec<[f32; 2]>,
    write: usize,
}

impl Line {
    /// Room for ages up to `longest` and one more for interpolation.
    pub(crate) fn new(longest: usize) -> Self {
        Line { frames: vec![[0.0; 2]; longest + 2], write: 0 }
    }

    pub(crate) fn clear(&mut self) {
        self.frames.fill([0.0; 2]);
        self.write = 0;
    }

    #[inline]
    pub(crate) fn at(&self, age: usize) -> [f32; 2] {
        let len = self.frames.len();
        let age = if age < len { age } else { age % len };
        let p = if self.write >= age { self.write - age } else { self.write + len - age };
        self.frames[p]
    }

    #[inline]
    pub(crate) fn push(&mut self, frame: [f32; 2]) {
        self.frames[self.write] = frame;
        let next = self.write + 1;
        self.write = if next == self.frames.len() { 0 } else { next };
    }

    /// The longest age that reads a frame of its own.
    pub(crate) fn longest(&self) -> usize {
        self.frames.len() - 2
    }
}

/// What the Hyperprism ports' bit-exact tests share: a fixed stereo input and
/// a hash of every output bit, so a render at the defaults can be held to the
/// one the unchanged code gave.
#[cfg(test)]
pub(crate) mod golden {
    use crate::Kernel;
    use std::path::PathBuf;

    /// Every `(seconds, sample_rate)` pair requested by the Hyperprism unit tests.
    pub(crate) const FIXTURES: &[(f32, f32)] = &[
        (3.0, 44_100.0),
        (3.0, 48_000.0),
        (4.0, 44_100.0),
        (4.0, 48_000.0),
    ];

    pub(crate) fn fixture_name(seconds: f32, sample_rate: f32) -> String {
        let sr = sample_rate as u32;
        let frames = (seconds * sample_rate) as usize;
        format!("hp-golden-{sr}-{frames}.f32")
    }

    /// `seconds` of two sines (220 Hz left, 331 Hz right) under fixed-seed
    /// noise, with a quiet stretch in the middle for dynamics to act on.
    ///
    /// Kept only for the MacBook fixture recorder so every machine feeds the
    /// golden hashes the exact same sample bits.
    pub(crate) fn input_computed(seconds: f32, sample_rate: f32) -> Vec<[f32; 2]> {
        let n = (seconds * sample_rate) as usize;
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut noise = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
        };
        (0..n)
            .map(|i| {
                let t = i as f32 / sample_rate;
                let level = if i > n / 3 && i < n / 2 { 0.05 } else { 0.6 };
                let l = level * (core::f32::consts::TAU * 220.0 * t).sin() + 0.1 * noise();
                let r = level * (core::f32::consts::TAU * 331.0 * t).sin() + 0.1 * noise();
                [l, r]
            })
            .collect()
    }

    /// Loads the recorded MacBook stereo input for `(seconds, sample_rate)`
    /// from `tests/fixtures/hp-golden-<sr>-<frames>.f32` (interleaved
    /// little-endian `f32`).
    pub(crate) fn input(seconds: f32, sample_rate: f32) -> Vec<[f32; 2]> {
        let frames = (seconds * sample_rate) as usize;
        let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", &fixture_name(seconds, sample_rate)]
            .iter()
            .collect();
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
            .map(|frame| [f32::from_le_bytes(frame[..4].try_into().unwrap()), f32::from_le_bytes(frame[4..].try_into().unwrap())])
            .collect()
    }

    /// The input through `k`, frame by frame, with `at(k, i)` called before
    /// frame `i` for moves mid-way.
    pub(crate) fn render<K: Kernel>(k: &mut K, input: &[[f32; 2]], mut at: impl FnMut(&mut K, usize)) -> Vec<[f32; 2]> {
        input
            .iter()
            .enumerate()
            .map(|(i, x)| {
                at(k, i);
                let mut frame = *x;
                k.tick(&mut frame);
                frame
            })
            .collect()
    }

    /// FNV-1a over every output bit.
    pub(crate) fn hash(out: &[[f32; 2]]) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for b in out.iter().flat_map(|f| f.iter()).flat_map(|s| s.to_bits().to_le_bytes()) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    /// A kernel made at `sample_rate` with `settings` (index, value), rendered
    /// with `moves` (frame, index, value) applied mid-way, hashed.
    pub(crate) fn hash_of<K: Kernel>(sample_rate: f32, seconds: f32, settings: &[(usize, f32)], moves: &[(usize, usize, f32)]) -> u64 {
        let mut k = K::new(sample_rate);
        for &(i, v) in settings {
            k.set_param(i, v);
        }
        let out = render(&mut k, &input(seconds, sample_rate), |k, n| {
            for &(at, i, v) in moves {
                if at == n {
                    k.set_param(i, v);
                }
            }
        });
        hash(&out)
    }
}

#[cfg(test)]
#[path = "../../tests/support/echo_scenarios.rs"]
mod echo_scenarios;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn write_interleaved(path: &std::path::Path, frames: impl IntoIterator<Item = [f32; 2]>, expected_frames: usize) -> usize {
        let mut bytes = Vec::with_capacity(expected_frames * 8);
        for [l, r] in frames {
            bytes.extend_from_slice(&l.to_le_bytes());
            bytes.extend_from_slice(&r.to_le_bytes());
        }
        assert_eq!(bytes.len(), expected_frames * 8);
        std::fs::write(path, &bytes).unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
        bytes.len()
    }

    #[test]
    #[ignore = "records the Mac's inputs: run on the MacBook only"]
    fn record_recorded_inputs() {
        let out_dir = match std::env::var_os("FIXTURE_OUT") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures"].iter().collect(),
        };
        std::fs::create_dir_all(&out_dir).unwrap_or_else(|e| panic!("failed to create {}: {e}", out_dir.display()));

        for &(seconds, sample_rate) in golden::FIXTURES {
            let name = golden::fixture_name(seconds, sample_rate);
            let frames = golden::input_computed(seconds, sample_rate);
            let count = frames.len();
            let path = out_dir.join(&name);
            let bytes = write_interleaved(&path, frames, count);
            println!("wrote {} ({count} frames, {bytes} bytes)", path.display());
        }

        for (sample_rate, count) in echo_scenarios::fixture_specs() {
            let name = echo_scenarios::fixture_name(sample_rate, count);
            let path = out_dir.join(&name);
            let iter = (0..count).map(|n| {
                let (l, r) = echo_scenarios::input_computed(n, sample_rate);
                [l, r]
            });
            let bytes = write_interleaved(&path, iter, count);
            println!("wrote {} ({count} frames, {bytes} bytes)", path.display());
        }
    }

    #[test]
    fn the_speed_law_at_its_marks() {
        let near = |v: f32, a: f64| (speed(v, 44_100.0).0 / a - 1.0).abs() < 1e-12;
        assert!(near(50.0, 1e-6) && near(25.0, 1e-7) && near(75.0, 1e-5) && near(1.0, 1e-8));
        assert!(near(99.0, 9.12e-5), "int(10^3.96) = 9120");
        assert_eq!(speed(100.0, 44_100.0), (1.0, 0.0));
    }

    #[test]
    fn the_smoothing_law_at_its_marks() {
        let near = |s: f32, a: f32| (smoothing(s, 44_100.0).0 / a - 1.0).abs() < 1e-6;
        assert!(near(50.0, 0.01) && near(0.0, 1.0) && near(100.0, 1e-4) && near(75.0, 0.001));
    }

    #[test]
    fn other_rates_keep_the_time() {
        let (_, b) = speed(75.0, 48_000.0);
        let (_, b0) = speed(75.0, 44_100.0);
        assert!((b.powf(48_000.0) - b0.powf(44_100.0)).abs() < 1e-9);
        assert_eq!(count(4096, 48_000.0), 4458);
        assert_eq!(count(4096, 44_100.0), 4096);
    }

    #[test]
    fn the_line_reads_by_age() {
        let mut line = Line::new(3);
        for v in 1..=4 {
            line.push([v as f32, 0.0]);
        }
        assert_eq!(line.at(1)[0], 4.0);
        assert_eq!(line.at(3)[0], 2.0);
    }
}
