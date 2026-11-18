//! The source an octave or more up, for the top of the keyboard.
//!
//! Five engines pitch by stretching and then reading the result back faster
//! ([`crate::stream::PitchRing`]): to sound a note `P` times higher they have
//! to make `P` times as many frames as they hand out. At G9, note 127, that is
//! 48 times the work of the same note unpitched, and PVSOLA, the hybrid and
//! SoundHack took two to three times longer to make a block than it takes to
//! play (RJ, 17 Nov 2026: "fix the issue you just said").
//!
//! Above C8 they read one of these instead: the source low-passed and halved
//! `k` times, so it is `2^k` times shorter and every frequency in it is `2^k`
//! times higher *per frame*. Told the sample rate is `2^k` times lower, an
//! engine stretches it exactly as it would the original — the same windows in
//! milliseconds, the same ratio — but makes `2^k` times fewer frames, and the
//! ring reads them back at `P / 2^k`. The pitch and the length come out the
//! same; the work stays where it was at C8.
//!
//! At C8 and below nothing here is used and nothing changes.
//!
//! Built on demand, off the audio thread, only once a note above C8 asks for
//! it: most sounds never go up there, and the copies together are most of the
//! size of the sound again.

use crate::hstream::Parts;

/// How many halvings are kept: sixteen times shorter, so a rate of 256, the
/// widest there is (`stream::MAX_PITCH`), is read at 16.
pub const LEVELS: u32 = 4;

/// The fastest read the ring is left to do before the next copy up is used.
/// C8, a rate of 16, so everything at or below C8 is exactly as it was.
const TOP: f32 = 16.0;

/// The level a note at `pitch` starts on: none at C8 or below, otherwise the
/// fewest halvings that bring the read back to C8's rate or under.
pub fn level_for(pitch: f32) -> u32 {
    if pitch <= TOP {
        return 0;
    }
    ((pitch / TOP).log2().ceil() as u32).min(LEVELS)
}

/// The level to be on while a note is sounding, given the one it is on.
///
/// Moving level is a re-seek of the engine underneath, so a pitch that wanders
/// (a bend, a glide, an LFO) stays where it is while the read is between a
/// quarter and one and a half times C8's rate, and only crosses when it leaves
/// that.
pub fn level_while(pitch: f32, now: u32) -> u32 {
    let rate = pitch / factor(now) as f32;
    let low = if now == 0 { 0.0 } else { TOP / 4.0 };
    if rate >= low && rate <= TOP * 1.5 {
        now
    } else {
        level_for(pitch)
    }
}

/// `2^k`.
pub fn factor(level: u32) -> u32 {
    1 << level
}

/// Frames of a source `frames` long, `level` halvings up.
pub fn frames_at(frames: usize, level: u32) -> usize {
    let d = factor(level) as usize;
    frames.div_ceil(d)
}

/// The source, and the hybrid's separated parts if it has them, halved again
/// and again.
#[derive(Default)]
pub struct Octaves {
    /// Which source these were made from: where its samples are and how many.
    of: (usize, usize),
    /// And which separation, the same way.
    parts_of: (usize, usize),
    pub channels: usize,
    /// `levels[k - 1]` is the source halved `k` times.
    pub levels: Vec<Vec<f32>>,
    /// `parts[k - 1]` the same for the separated parts; empty if there were none.
    pub parts: Vec<Parts>,
}

fn id(v: &[f32]) -> (usize, usize) {
    (v.as_ptr() as usize, v.len())
}

impl Octaves {
    /// Make them. Allocates and walks the whole sound several times: never
    /// call this from the audio thread.
    pub fn build(samples: &[f32], channels: usize, parts: Option<&Parts>) -> Octaves {
        let channels = channels.max(1);
        let mut levels: Vec<Vec<f32>> = Vec::with_capacity(LEVELS as usize);
        for k in 0..LEVELS as usize {
            let next = halve(if k == 0 { samples } else { &levels[k - 1] }, channels);
            levels.push(next);
        }
        let parts_list = match parts {
            Some(p) if !p.is_empty() => {
                let mut out: Vec<Parts> = Vec::with_capacity(LEVELS as usize);
                for k in 0..LEVELS as usize {
                    let from = if k == 0 { p } else { &out[k - 1] };
                    let ch = from.channels.max(1);
                    let next = Parts {
                        harmonic: halve(&from.harmonic, ch),
                        percussive: halve(&from.percussive, ch),
                        residual: halve(&from.residual, ch),
                        channels: from.channels,
                        split: from.split,
                    };
                    out.push(next);
                }
                out
            }
            _ => Vec::new(),
        };
        Octaves {
            of: id(samples),
            parts_of: parts.map_or((0, 0), |p| id(&p.harmonic)),
            channels,
            levels,
            parts: parts_list,
        }
    }

    /// Where the source these were made from is, and how many samples.
    pub fn source_id(&self) -> (usize, usize) {
        self.of
    }

    /// Whether these were made from `samples`.
    pub fn is_for(&self, samples: &[f32]) -> bool {
        !self.levels.is_empty() && self.of == id(samples)
    }

    /// Whether these were made from a source `frames` long, for a seek, which
    /// is told the length and not handed the source.
    pub fn is_length(&self, frames: usize) -> bool {
        !self.levels.is_empty() && self.channels > 0 && self.of.1 / self.channels == frames
    }

    /// Whether the separated parts here are `parts`'.
    pub fn is_for_parts(&self, parts: &Parts) -> bool {
        !self.parts.is_empty() && self.parts_of == id(&parts.harmonic)
    }

    /// The source `level` halvings up, or the source itself at level 0.
    pub fn source<'a>(&'a self, samples: &'a [f32], level: u32) -> &'a [f32] {
        if level == 0 {
            samples
        } else {
            &self.levels[level as usize - 1]
        }
    }

    pub fn parts_at<'a>(&'a self, parts: &'a Parts, level: u32) -> &'a Parts {
        if level == 0 {
            parts
        } else {
            &self.parts[level as usize - 1]
        }
    }

    /// The size of the copies, in bytes.
    pub fn bytes(&self) -> usize {
        let parts: usize = self.parts.iter().map(|p| p.harmonic.len() + p.percussive.len() + p.residual.len()).sum();
        (self.levels.iter().map(Vec::len).sum::<usize>() + parts) * std::mem::size_of::<f32>()
    }
}

/// Half-band low-pass taps: a 63-tap Blackman-windowed sinc cut at 0.225 of
/// the rate, so what is left after halving is below its new Nyquist, with a
/// tenth of it to roll off in.
fn taps() -> [f32; 63] {
    let mut h = [0f32; 63];
    let m = 31.0f64;
    let fc = 0.225f64;
    let mut sum = 0.0;
    for (i, t) in h.iter_mut().enumerate() {
        let n = i as f64 - m;
        let sinc = if n == 0.0 { 2.0 * fc } else { (2.0 * std::f64::consts::PI * fc * n).sin() / (std::f64::consts::PI * n) };
        let x = std::f64::consts::PI * i as f64 / (2.0 * m);
        let w = 0.42 - 0.5 * (2.0 * x).cos() + 0.08 * (4.0 * x).cos();
        *t = (sinc * w) as f32;
        sum += sinc * w;
    }
    for t in h.iter_mut() {
        *t = (*t as f64 / sum) as f32;
    }
    h
}

/// Low-pass and keep every other frame. Frame `n` of the result is centred on
/// frame `2n` of the input, so the two line up in time.
fn halve(x: &[f32], channels: usize) -> Vec<f32> {
    let ch = channels.max(1);
    let frames = x.len() / ch;
    let out_frames = frames.div_ceil(2);
    let h = taps();
    let mut out = vec![0f32; out_frames * ch];
    for n in 0..out_frames {
        let c = 2 * n as isize;
        for c_i in 0..ch {
            let mut acc = 0f32;
            for (j, t) in h.iter().enumerate() {
                let i = c + j as isize - 31;
                if i >= 0 && (i as usize) < frames {
                    acc += t * x[i as usize * ch + c_i];
                }
            }
            out[n * ch + c_i] = acc;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level_of(x: &[f32], f: f64, sr: f64) -> f64 {
        let (mut re, mut im) = (0f64, 0f64);
        for (i, s) in x.iter().enumerate() {
            let w = std::f64::consts::TAU * f * i as f64 / sr;
            re += *s as f64 * w.cos();
            im += *s as f64 * w.sin();
        }
        (re * re + im * im).sqrt() * 2.0 / x.len() as f64
    }

    /// Halving keeps what fits under the new Nyquist and throws out what would
    /// fold back over it.
    #[test]
    fn halving_keeps_the_low_and_drops_what_would_fold() {
        let sr = 48_000.0;
        let n = 48_000;
        let tone = |f: f64| (0..n).map(|i| (std::f64::consts::TAU * f * i as f64 / sr).sin() as f32 * 0.5).collect::<Vec<_>>();
        let low = halve(&tone(2_000.0), 1);
        assert!((level_of(&low, 2_000.0, sr / 2.0) - 0.5).abs() < 0.01, "a 2 kHz tone lost level");
        let high = halve(&tone(18_000.0), 1);
        // 18 kHz at 24 kHz would fold to 6 kHz.
        assert!(level_of(&high, 6_000.0, sr / 2.0) < 0.005, "18 kHz folded back");
    }

    #[test]
    fn levels_stay_put_at_c8_and_below_and_cover_the_top() {
        assert_eq!(level_for(1.0), 0);
        assert_eq!(level_for(16.0), 0);
        assert_eq!(level_for(2f32.powf(67.0 / 12.0)), 2);
        assert_eq!(level_for(256.0), LEVELS);
        assert_eq!(level_while(20.0, 0), 0, "a little over C8 stays where it is");
        assert_eq!(level_while(30.0, 0), 1);
        assert_eq!(level_while(10.0, 1), 1, "a little under stays up");
        assert_eq!(level_while(3.0, 1), 0);
    }

    #[test]
    fn a_copy_is_the_length_it_says() {
        let o = Octaves::build(&vec![0.1; 2 * 1001], 2, None);
        for k in 1..=LEVELS {
            assert_eq!(o.levels[k as usize - 1].len(), frames_at(1001, k) * 2);
        }
        assert!(o.is_length(1001));
    }
}

#[cfg(test)]
mod engines {
    use super::*;
    use crate::stream::{Pitched, StretchParams, WsolaStream};
    use crate::stretch::Stretch;
    use crate::vstream::VocoderStream;

    const SR: u32 = 48_000;

    fn params() -> StretchParams {
        let s = Stretch::default();
        StretchParams {
            ratio: 1.0,
            window_ms: 40.0,
            sample_rate: SR,
            wsola: s.wsola,
            vocoder: s.vocoder,
            grain: s.grain,
            soundhack: s.soundhack,
        }
    }

    /// How much of `f` there is, coherently, over all of `x`: a tone whose
    /// splices wander out of phase cancels itself here even at full level.
    fn level(x: &[f32], f: f64) -> f32 {
        let n = x.len() as f64;
        let (mut re, mut im) = (0f64, 0f64);
        for (i, s) in x.iter().enumerate() {
            let hann = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n).cos();
            let w = std::f64::consts::TAU * f * i as f64 / SR as f64;
            re += *s as f64 * hann * w.cos();
            im += *s as f64 * hann * w.sin();
        }
        ((re * re + im * im).sqrt() * 4.0 / n) as f32
    }

    /// A note read from the copy an octave up is the same note, as loud and as
    /// steady, as read from the sound itself, on the splice engine and the
    /// spectral one.
    #[test]
    fn a_note_from_a_copy_is_as_loud_as_from_the_sound() {
        let frames = SR as usize * 4;
        let src: Vec<f32> = (0..frames).map(|i| (std::f32::consts::TAU * 100.0 * i as f32 / SR as f32).sin() * 0.5).collect();
        let oct = Octaves::build(&src, 1, None);
        let p = params();
        for semis in [24.0f32, 50.0, 67.0] {
            let mut levels = Vec::new();
            // From the sound; from the copy; and from the copy only once the
            // note is under way, which is a change of level mid-note.
            for with in 0..3 {
                let o = (with > 0).then_some(&oct);
                let at_seek = if with == 2 { None } else { o };
                let mut w = Pitched::new(WsolaStream::new(512, 1, SR), 512, 1);
                let mut v = Pitched::new(VocoderStream::new(512, 1), 512, 1);
                w.seek_octaves(0, frames, at_seek, &p, semis);
                v.seek_octaves(0, frames, at_seek, &p, semis);
                let (mut wo, mut vo) = (Vec::new(), Vec::new());
                let mut buf = vec![0f32; 512];
                for _ in 0..60 {
                    w.render_octaves(&mut buf, 1, &src, o, &p, semis);
                    wo.extend_from_slice(&buf);
                    v.render_octaves(&mut buf, 1, &src, o, &p, semis);
                    vo.extend_from_slice(&buf);
                }
                let f = 100.0 * 2f64.powf(semis as f64 / 12.0);
                levels.push((level(&wo[10_240..], f), level(&vo[10_240..], f)));
                if with == 2 {
                    // The change of level, in the first block, leaves no step:
                    // no frame-to-frame jump bigger than the tone's own.
                    let jump = |x: &[f32]| x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max);
                    for (name, x) in [("wsola", &wo), ("vocoder", &vo)] {
                        let (start, steady) = (jump(&x[..4096]), jump(&x[10_240..]));
                        assert!(start < steady * 1.3 + 0.02, "{name} at {semis}: a jump of {start} where the level changed, against {steady} held");
                    }
                }
            }
            eprintln!("{semis} up: wsola {:?}, vocoder {:?}", levels.iter().map(|l| l.0).collect::<Vec<_>>(), levels.iter().map(|l| l.1).collect::<Vec<_>>());
            for late in [1, 2] {
                for (k, (a, b)) in [(levels[0].0, levels[late].0), (levels[0].1, levels[late].1)].into_iter().enumerate() {
                    assert!(b > a * 0.7 && b < a * 1.4, "{} at {semis}: {a} from the sound, {b} from the copy ({})", ["wsola", "vocoder"][k], ["", "from the start", "from part way"][late]);
                }
            }
        }
    }
}

