//! Before and after for the eight-chain splice search: WSOLA renders of real
//! recordings, written out to listen to, and a comparison of two sets of
//! them. Measurements, run by hand, so ignored:
//!
//! - `WSOLA_TRIAL_SONGS` names 48 kHz WAV files of 32-bit floats, separated
//!   by `;`, and `WSOLA_TRIAL_OUT` the folder the renders go in;
//! - with `WSOLA_TRIAL_REF` naming another such folder, `compare` sets each
//!   render beside the one of the same name there.

use prism_grain::stretch::{Algorithm, Stretch};

const RATE: u32 = 48_000;
/// Where in each recording the passage starts, and how long it is.
const FROM_S: f32 = 40.0;
const SECONDS: f32 = 20.0;

/// (name, ratio, semitones)
const SETTINGS: [(&str, f32, f32); 3] = [("half-length", 0.5, 0.0), ("up-3-semitones", 1.0, 3.0), ("twice-as-long", 2.0, 0.0)];

#[test]
#[ignore]
fn render() {
    let (Ok(songs), Ok(out)) = (std::env::var("WSOLA_TRIAL_SONGS"), std::env::var("WSOLA_TRIAL_OUT")) else { return };
    std::fs::create_dir_all(&out).unwrap();
    for path in songs.split(';').filter(|p| !p.is_empty()) {
        let (channels, all) = read(path);
        let frames = all.len() / channels;
        let start = ((FROM_S * RATE as f32) as usize).min(frames.saturating_sub(1));
        let end = (start + (SECONDS * RATE as f32) as usize).min(frames);
        // A recording shorter than the start point is taken from its start.
        let (start, end) = if end - start < RATE as usize { (0, frames.min((SECONDS * RATE as f32) as usize)) } else { (start, end) };
        let passage = &all[start * channels..end * channels];
        let name = std::path::Path::new(path).file_stem().unwrap().to_string_lossy().replace(' ', "-");
        for (setting, ratio, semitones) in SETTINGS {
            let stretch = Stretch { ratio, semitones, algorithm: Algorithm::Wsola, ..Default::default() };
            let rendered = stretch.process(passage, channels, RATE);
            let file = format!("{out}/{name}--{setting}.wav");
            write(&file, channels, &rendered);
            println!("{file}: {:.1} s", rendered.len() as f32 / channels as f32 / RATE as f32);
        }
    }
}

#[test]
#[ignore]
fn compare() {
    let (Ok(out), Ok(reference)) = (std::env::var("WSOLA_TRIAL_OUT"), std::env::var("WSOLA_TRIAL_REF")) else { return };
    let mut names: Vec<_> = std::fs::read_dir(&out).unwrap().flatten().map(|e| e.file_name()).collect();
    names.sort();
    for name in names {
        let name = name.to_string_lossy().to_string();
        if !name.ends_with(".wav") {
            continue;
        }
        let (channels, new) = read(&format!("{out}/{name}"));
        let (_, old) = read(&format!("{reference}/{name}"));
        assert_eq!(new.len(), old.len(), "{name}: the same length");
        let same = new.iter().zip(&old).filter(|(a, b)| a.to_bits() == b.to_bits()).count();
        let first = new.iter().zip(&old).position(|(a, b)| a.to_bits() != b.to_bits());
        let (mut diff, mut level, mut worst) = (0f64, 0f64, 0f64);
        for (a, b) in new.iter().zip(&old) {
            let d = (*a as f64) - (*b as f64);
            diff += d * d;
            level += (*b as f64) * (*b as f64);
            worst = worst.max(d.abs());
        }
        let db = |x: f64| if x > 0.0 { format!("{:.1} dB", 10.0 * x.log10()) } else { "silent".into() };
        println!(
            "{name}: {:.2}% of samples the same; first change at {}; difference {} below the music, loudest difference {:.4}",
            100.0 * same as f64 / new.len().max(1) as f64,
            first.map_or("none".into(), |i| format!("{:.3} s", (i / channels) as f64 / RATE as f64)),
            db(level / diff.max(f64::MIN_POSITIVE)),
            worst
        );
    }
}

fn read(path: &str) -> (usize, Vec<f32>) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let (mut at, mut channels, mut data) = (12usize, 0usize, None);
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &bytes[at + 8..(at + 8 + size).min(bytes.len())];
        if id == b"fmt " {
            assert_eq!(u16::from_le_bytes([body[0], body[1]]), 3, "{path}: 32-bit floats");
            channels = u16::from_le_bytes([body[2], body[3]]) as usize;
            assert_eq!(u32::from_le_bytes(body[4..8].try_into().unwrap()), RATE, "{path}: 48 kHz");
        } else if id == b"data" {
            data = Some(body.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect());
        }
        at += 8 + size + (size & 1);
    }
    (channels, data.expect("a data chunk"))
}

fn write(path: &str, channels: usize, samples: &[f32]) {
    let data = (samples.len() * 4) as u32;
    let mut b = Vec::with_capacity(44 + data as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&3u16.to_le_bytes());
    b.extend_from_slice(&(channels as u16).to_le_bytes());
    b.extend_from_slice(&RATE.to_le_bytes());
    b.extend_from_slice(&(RATE * 4 * channels as u32).to_le_bytes());
    b.extend_from_slice(&((4 * channels) as u16).to_le_bytes());
    b.extend_from_slice(&32u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}
