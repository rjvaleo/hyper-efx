//! Grain feedback: the stretch re-reading what it has just played.
//!
//! RJ, 4 Nov 2026: grains that persist and form misty textures, and with the
//! blur up, smears. Whatever the engine plays goes into a ring; the loop reads
//! it back as grains — two overlapping voices, each grain starting its read
//! somewhere between `Time` and `Time` plus a scatter that `Smear` sets — then
//! blurs them through all-pass diffusers (`Smear` again) and darkens them with
//! a one-pole low-pass (`Damp`). That return is heard and written into the
//! ring again at the `Feedback` level, so each pass is quieter and the cloud
//! fades — unless the loop is held, when nothing new goes in and what is in it
//! goes round at full level. `Shift` moves each re-read grain in pitch.
//!
//! **Every engine.** It sits at the end of the `Stretcher` and of the offline
//! `Stretch::process`, after the engine and the grain cloud, so it reaches
//! WSOLA, the vocoder, PVSOLA and the hybrid as well as the cloud (RJ: "I
//! don't hear any feedback at all in the stretching"). Both paths run the
//! same struct sample by sample, so a block boundary changes nothing and what
//! you hear is what you export.
//!
//! **Off is untouched.** With Feedback at zero and the loop not held, `process`
//! returns before reading or writing anything, so the sound is bit-for-bit
//! what it was before this existed.
//!
//! **No allocation once built.** The ring is sized for the longest Time and
//! scatter at the highest sample rate the engine runs at, and the loop works
//! on the first two channels; a third or later channel passes through dry.

use crate::grain::Grain;

/// The longest Time any control allows.
pub const MAX_TIME_MS: f32 = 2000.0;
/// The highest sample rate the ring is sized for.
const MAX_RATE: f32 = 192_000.0;
/// Channels the loop runs on.
const LOOP_CHANNELS: usize = 2;
/// The shifter's window, in milliseconds, at most.
const SHIFT_WINDOW_MS: f32 = 40.0;
/// The diffusers' lengths, in milliseconds: mutually prime-ish, so they smear
/// rather than ring.
const DIFFUSER_MS: [f32; 4] = [4.7, 7.1, 11.3, 16.9];
/// Each diffuser's ring, sized for the longest at the highest rate.
const DIFFUSER_MAX: usize = (17.0 * MAX_RATE / 1000.0) as usize + 2;
/// The loop's ceiling. A held loop at full level with damping off cannot grow
/// past this, so a stuck setting is loud, not a runaway.
const LOOP_LIMIT: f32 = 4.0;

/// How far the re-read grains may scatter, as a share of Time, at full Smear.
const SCATTER: f32 = 0.5;

/// The loop's own state. One per renderer.
pub struct Feedback {
    ring: Vec<f32>,
    len: usize,
    write: usize,
    /// Two grain voices re-reading the ring, half a grain apart: where each is
    /// in its grain (0..1), how many grains it has started (the scatter is a
    /// pure function of that count, so the same render scatters the same way),
    /// and how far back each channel of it is reading.
    phase: [f32; 2],
    count: [u64; 2],
    delay: [[f32; LOOP_CHANNELS]; 2],
    diff: [[Vec<f32>; 4]; LOOP_CHANNELS],
    diff_at: [usize; 4],
    damp: [f32; LOOP_CHANNELS],
}

impl Default for Feedback {
    fn default() -> Self {
        Self::new()
    }
}

impl Feedback {
    pub fn new() -> Self {
        let len = ((MAX_TIME_MS * (1.0 + SCATTER) + 2.0 * SHIFT_WINDOW_MS) / 1000.0 * MAX_RATE) as usize + 4;
        let d = || [vec![0.0; DIFFUSER_MAX], vec![0.0; DIFFUSER_MAX], vec![0.0; DIFFUSER_MAX], vec![0.0; DIFFUSER_MAX]];
        Feedback {
            ring: vec![0.0; len * LOOP_CHANNELS],
            len,
            write: 0,
            phase: [0.0, 0.5],
            count: [0, 0],
            delay: [[-1.0; LOOP_CHANNELS]; 2],
            diff: [d(), d()],
            diff_at: [0; 4],
            damp: [0.0; LOOP_CHANNELS],
        }
    }

    /// Empty the loop: a seek, or a fresh render.
    pub fn reset(&mut self) {
        self.ring.fill(0.0);
        for ch in &mut self.diff {
            for d in ch.iter_mut() {
                d.fill(0.0);
            }
        }
        self.write = 0;
        self.phase = [0.0, 0.5];
        self.count = [0, 0];
        self.delay = [[-1.0; LOOP_CHANNELS]; 2];
        self.diff_at = [0; 4];
        self.damp = [0.0; LOOP_CHANNELS];
    }

    /// Whether these settings do anything at all.
    pub fn active(g: &Grain) -> bool {
        g.fb_hold || g.fb_amount > 0.0
    }

    /// Run `buf` (interleaved, `channels` wide) through the loop, in place.
    ///
    /// The return is two overlapping grain voices reading the ring, each grain
    /// a Hann window long. Each grain starts its read somewhere between Time
    /// and Time plus the scatter — Smear sets both how far apart the grains
    /// are thrown and how much the diffusers blur them — so the returns are a
    /// cloud of what was just played rather than one clean echo. Shift moves
    /// each grain's read at its own speed, which is its pitch.
    pub fn process(&mut self, buf: &mut [f32], channels: usize, g: &Grain, sample_rate: u32) {
        if !Self::active(g) {
            return;
        }
        let channels = channels.max(1);
        let frames = buf.len() / channels;
        let sr = (sample_rate.max(1) as f32).min(MAX_RATE);
        let hold = g.fb_hold;
        let amount = if hold { 1.0 } else { g.fb_amount.clamp(0.0, 0.95) };
        let time = (g.fb_time_ms.clamp(20.0, MAX_TIME_MS) / 1000.0) * sr;
        let smear = g.fb_smear.clamp(0.0, 1.0);
        // The grain: never longer than the Time it sits inside.
        let window = (SHIFT_WINDOW_MS / 1000.0 * sr).min(time * 0.8).max(8.0);
        let diff_len = DIFFUSER_MS.map(|ms| ((ms / 1000.0 * sr) as usize).clamp(1, DIFFUSER_MAX - 1));
        let diff_delay: f32 = diff_len.iter().sum::<usize>() as f32;
        let shift = g.fb_shift.clamp(-12.0, 12.0);
        let ratio = 2f32.powf(shift / 12.0);
        // Where a grain starts reading, so that unshifted and unscattered a
        // grain returns exactly one Time later. A grain read faster than it is
        // written closes on the write head as it plays, so it starts that much
        // further back.
        let base = (time - diff_delay).max(1.0) + ((ratio - 1.0) * window).max(0.0);
        let scatter = SCATTER * smear * time;
        let drift = 1.0 - ratio;
        let coeff = 0.75 * smear;
        let lp = 1.0 - 0.9 * g.fb_damp.clamp(0.0, 1.0);
        let lc = channels.min(LOOP_CHANNELS);
        let len = self.len;
        let step = 1.0 / window;

        for f in 0..frames {
            // Each voice moves through its grain; a voice that finishes one
            // starts the next from a new place.
            for k in 0..2 {
                self.phase[k] += step;
                if self.phase[k] >= 1.0 || self.delay[k][0] < 0.0 {
                    if self.phase[k] >= 1.0 {
                        self.phase[k] -= 1.0;
                    }
                    self.count[k] += 1;
                    for ch in 0..lc {
                        self.delay[k][ch] = base + scatter * unit(self.count[k], k as u64, ch as u64);
                    }
                }
            }
            for ch in 0..lc {
                let mut ret = 0.0;
                for k in 0..2 {
                    let gain = (std::f32::consts::PI * self.phase[k]).sin().powi(2);
                    ret += gain * self.tap(ch, self.delay[k][ch]);
                    self.delay[k][ch] += drift;
                }
                // Smear: each pass blurred into the next.
                for (i, n) in diff_len.iter().enumerate() {
                    let line = &mut self.diff[ch][i];
                    let at = self.diff_at[i];
                    let read = (at + DIFFUSER_MAX - n) % DIFFUSER_MAX;
                    let delayed = line[read];
                    let v = ret + coeff * delayed;
                    line[at] = v;
                    ret = delayed - coeff * v;
                }
                // Damp: each pass darker than the last.
                self.damp[ch] += (ret - self.damp[ch]) * lp;
                let ret = self.damp[ch].clamp(-LOOP_LIMIT, LOOP_LIMIT);

                let x = buf[f * channels + ch];
                let into = if hold { ret } else { x + amount * ret };
                self.ring[self.write * LOOP_CHANNELS + ch] = into.clamp(-LOOP_LIMIT, LOOP_LIMIT);
                buf[f * channels + ch] = x + amount * ret;
            }
            for a in &mut self.diff_at {
                *a = (*a + 1) % DIFFUSER_MAX;
            }
            self.write = (self.write + 1) % len;
        }
    }

    /// The ring `d` frames behind the write head, interpolated.
    #[inline]
    fn tap(&self, ch: usize, d: f32) -> f32 {
        let len = self.len;
        let d = d.clamp(1.0, (len - 2) as f32);
        let back = d.floor();
        let t = d - back;
        let i0 = (self.write + len - back as usize) % len;
        let i1 = (i0 + len - 1) % len;
        let a = self.ring[i0 * LOOP_CHANNELS + ch];
        let b = self.ring[i1 * LOOP_CHANNELS + ch];
        a + (b - a) * t
    }
}

/// A stable 0..1 for grain `n` of voice `k` on channel `ch`.
#[inline]
fn unit(n: u64, k: u64, ch: u64) -> f32 {
    let mut x = n.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ k.wrapping_mul(0xD1B5_4A32_D192_ED03) ^ ch.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 29;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn g(f: impl FnOnce(&mut Grain)) -> Grain {
        let mut g = Grain::default();
        f(&mut g);
        g
    }

    fn click(frames: usize) -> Vec<f32> {
        let mut v = vec![0.0; frames * 2];
        v[0] = 1.0;
        v[1] = 1.0;
        v
    }

    #[test]
    fn off_leaves_the_cloud_untouched() {
        let mut fb = Feedback::new();
        let mut buf: Vec<f32> = (0..2000).map(|i| (i as f32 * 0.01).sin()).collect();
        let before = buf.clone();
        fb.process(&mut buf, 2, &Grain::default(), SR);
        assert_eq!(buf, before);
    }

    #[test]
    fn a_grain_returns_one_time_later_at_the_feedback_level_and_fades() {
        let mut fb = Feedback::new();
        let p = g(|g| { g.fb_amount = 0.5; g.fb_time_ms = 100.0; g.fb_smear = 0.0; g.fb_damp = 0.0; });
        let mut buf = click(20_000);
        fb.process(&mut buf, 2, &p, SR);
        let left: Vec<f32> = buf.iter().step_by(2).copied().collect();
        let window = |c: usize| left[c - 30..c + 30].iter().map(|v| v.abs()).fold(0.0, f32::max);
        let first = window(4_800);
        let second = window(9_600);
        assert!((first - 0.5).abs() < 0.05, "first return {first}");
        assert!((second - 0.25).abs() < 0.05, "second return {second}");
        // Nothing between the returns.
        assert!(left[1_000..4_700].iter().all(|v| v.abs() < 1e-3));
    }

    #[test]
    fn the_same_sound_in_any_block_size() {
        let p = g(|g| { g.fb_amount = 0.7; g.fb_time_ms = 60.0; g.fb_smear = 0.6; g.fb_damp = 0.3; g.fb_shift = 5.0; });
        let src: Vec<f32> = (0..24_000).map(|i| ((i as f32 * 0.013).sin() * 0.5)).collect();
        let mut whole = src.clone();
        Feedback::new().process(&mut whole, 2, &p, SR);
        let mut blocks = src.clone();
        let mut fb = Feedback::new();
        for chunk in blocks.chunks_mut(2 * 256) {
            fb.process(chunk, 2, &p, SR);
        }
        assert_eq!(whole, blocks);
    }

    #[test]
    fn smear_spreads_a_return_and_damp_darkens_it() {
        let run = |smear: f32, damp: f32| {
            let p = g(|g| { g.fb_amount = 0.6; g.fb_time_ms = 100.0; g.fb_smear = smear; g.fb_damp = damp; });
            let mut buf = click(12_000);
            Feedback::new().process(&mut buf, 2, &p, SR);
            buf.iter().step_by(2).copied().collect::<Vec<f32>>()
        };
        let busy = |v: &[f32]| v[4_000..9_000].iter().filter(|x| x.abs() > 1e-3).count();
        assert!(busy(&run(0.9, 0.0)) > 10 * busy(&run(0.0, 0.0)).max(1), "smear spreads the return");
        let energy = |v: &[f32]| v[4_000..9_000].iter().map(|x| x * x).sum::<f32>();
        assert!(energy(&run(0.0, 0.9)) < energy(&run(0.0, 0.0)), "damp takes the top off");
    }

    #[test]
    fn held_it_keeps_sounding_and_lets_nothing_new_in() {
        let p = g(|g| { g.fb_amount = 0.5; g.fb_time_ms = 50.0; g.fb_smear = 0.0; g.fb_damp = 0.0; });
        let mut fb = Feedback::new();
        let mut a = click(4_800);
        fb.process(&mut a, 2, &p, SR);
        let held = g(|g| { g.fb_hold = true; g.fb_time_ms = 50.0; g.fb_smear = 0.0; g.fb_damp = 0.0; });
        // New input while held never enters the loop.
        let mut b: Vec<f32> = vec![0.3; 48_000 * 2];
        fb.process(&mut b, 2, &held, SR);
        let loop_part: Vec<f32> = b.iter().step_by(2).map(|v| v - 0.3).collect();
        let late = loop_part[40_000..48_000].iter().map(|v| v.abs()).fold(0.0, f32::max);
        assert!(late > 0.1, "still sounding a second later: {late}");
        assert!(late <= 1.0 + 1e-3, "and not growing: {late}");
    }

    #[test]
    fn full_feedback_stays_bounded() {
        let p = g(|g| { g.fb_amount = 0.95; g.fb_time_ms = 20.0; g.fb_smear = 1.0; g.fb_shift = 12.0; });
        let mut buf = vec![1.0; 96_000 * 2];
        Feedback::new().process(&mut buf, 2, &p, SR);
        assert!(buf.iter().all(|v| v.is_finite() && v.abs() <= 1.0 + 0.95 * LOOP_LIMIT));
    }
}
