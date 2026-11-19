//! Noise morphing, a block at a time.
//!
//! The offline renderer measures the whole residual's envelope, then
//! synthesises. A callback cannot do the first half, but it does not need to:
//! the envelope for an output frame comes from the two analysis frames either
//! side of `frame / ratio` in the source, and the source is all in memory. So
//! the two transforms are done on demand rather than looked up from a table
//! that took a pass over the file to build.
//!
//! The level correction is the awkward part and the reason this is not simply
//! the vocoder again. Random phases overlap-add incoherently, so the
//! synthesised level is short by an amount that depends on the overlap and the
//! spectrum — the offline renderer measures the shortfall afterwards, walking
//! both signals. Here it is measured a hop at a time against the source at the
//! matching instant, which is the same comparison made locally instead of
//! globally, and ramped between measurements so nothing steps.

use crate::fft::FftStepper;

use crate::noise::Morph;
use crate::stream::{RingDirty, StretchParams};

const MAX_FFT: usize = 8192;

/// Fresh noise shaped like the noise that was there.
pub struct NoiseStream {
    channels: usize,

    win: Vec<f32>,
    win_for: Option<usize>,
    win_sq_sum: f32,
    re: Vec<f32>,
    im: Vec<f32>,
    out_re: Vec<f32>,
    mag: Vec<f32>,
    env_a: Vec<f32>,
    env_b: Vec<f32>,

    acc: Vec<f32>,
    norm: Vec<f32>,
    ring: usize,
    dirty: RingDirty,

    /// Output frame the next synthesis frame is laid at, and how many have
    /// been handed out.
    write: u64,
    emitted: u64,
    index: u64,

    /// The corrective gain either side of the current hop, ramped between.
    gain_from: f32,
    gain_to: f32,
    gain_at: u64,
    /// Keeps the read continuous when the ratio moves. See `anchor.rs`.
    anchor: crate::anchor::Anchor,

    /// In-flight next noise-morph frame, stepped across blocks.
    job: Option<NoiseJob>,
    /// Voice/layer phase offset (0..8) for staggering stage boundaries.
    stagger: usize,
}

#[derive(Clone, Copy)]
struct NoiseJob {
    channels: usize,
    n: usize,
    hop: usize,
    bins: usize,
    span: usize,
    a_start: usize,
    b_start: usize,
    t: f32,
    seed: u32,
    stage: NoiseStage,
    started: bool,
}

#[derive(Clone, Copy)]
enum NoiseStage {
    EnvWin { ch: usize, is_b: bool, i: usize },
    EnvFft { ch: usize, is_b: bool, stepper: FftStepper },
    EnvMag { ch: usize, is_b: bool, k: usize },
    EnvSmooth { ch: usize, is_b: bool, k: usize },
    SynthSpec { ch: usize, k: usize },
    SynthMirror { ch: usize },
    SynthIfft { ch: usize, stepper: FftStepper },
    Ready,
}

const NOISE_SPEC_WEIGHT: usize = 14;
const NOISE_MAG_WEIGHT: usize = 2;

impl NoiseJob {
    fn is_ready(&self) -> bool {
        matches!(self.stage, NoiseStage::Ready)
    }

    fn one_env_work(n: usize, bins: usize, span: usize) -> usize {
        n / 2 + FftStepper::total_work(n, false) + bins * NOISE_MAG_WEIGHT + (bins * span.max(1)) / 4
    }

    fn one_synth_work(n: usize, bins: usize) -> usize {
        bins * NOISE_SPEC_WEIGHT + n / 4 + FftStepper::total_work(n, true)
    }

    fn one_channel_work(n: usize, bins: usize, span: usize) -> usize {
        2 * Self::one_env_work(n, bins, span) + Self::one_synth_work(n, bins)
    }

    fn remaining_work(&self) -> usize {
        let n = self.n;
        let bins = self.bins;
        let span = self.span;
        let one_env = Self::one_env_work(n, bins, span);
        let one_synth = Self::one_synth_work(n, bins);
        let one_ch = Self::one_channel_work(n, bins, span);
        let ch_tail = |ch: usize| (self.channels - 1 - ch) * one_ch;
        let env_tail = |ch: usize, is_b: bool| (if is_b { 0 } else { one_env }) + one_synth + ch_tail(ch);
        match self.stage {
            NoiseStage::EnvWin { ch, is_b, i } => {
                (n - i).div_ceil(2)
                    + FftStepper::total_work(n, false)
                    + bins * NOISE_MAG_WEIGHT
                    + (bins * span) / 4
                    + env_tail(ch, is_b)
            }
            NoiseStage::EnvFft { ch, is_b, stepper } => {
                stepper.remaining_work() + bins * NOISE_MAG_WEIGHT + (bins * span) / 4 + env_tail(ch, is_b)
            }
            NoiseStage::EnvMag { ch, is_b, k } => {
                (bins - k) * NOISE_MAG_WEIGHT + (bins * span) / 4 + env_tail(ch, is_b)
            }
            NoiseStage::EnvSmooth { ch, is_b, k } => {
                ((bins - k) * span) / 4 + env_tail(ch, is_b)
            }
            NoiseStage::SynthSpec { ch, k } => {
                (bins - k) * NOISE_SPEC_WEIGHT + n / 4 + FftStepper::total_work(n, true) + ch_tail(ch)
            }
            NoiseStage::SynthMirror { ch } => {
                n / 4 + FftStepper::total_work(n, true) + ch_tail(ch)
            }
            NoiseStage::SynthIfft { ch, stepper } => stepper.remaining_work() + ch_tail(ch),
            NoiseStage::Ready => 0,
        }
    }
}

impl NoiseStream {
    pub fn new(max_block: usize, channels: usize) -> Self {
        let channels = channels.max(1);
        let bins = MAX_FFT / 2 + 1;
        let ring = max_block.max(1) + MAX_FFT + 1;
        NoiseStream {
            channels,
            win: Vec::with_capacity(MAX_FFT),
            win_for: None,
            win_sq_sum: 0.0,
            re: vec![0.0; MAX_FFT],
            im: vec![0.0; MAX_FFT],
            out_re: vec![0.0; channels * MAX_FFT],
            mag: vec![0.0; bins],
            env_a: vec![0.0; bins],
            env_b: vec![0.0; bins],
            acc: vec![0.0; ring * channels],
            norm: vec![0.0; ring],
            ring,
            dirty: RingDirty::default(),
            write: 0,
            emitted: 0,
            index: 0,
            gain_from: 1.0,
            gain_to: 1.0,
            gain_at: 0,
            anchor: crate::anchor::Anchor::default(),
            job: None,
            stagger: 0,
        }
    }

    pub fn set_stagger(&mut self, id: usize) {
        self.stagger = id & 7;
    }

    pub fn seek(&mut self, out_frame: u64, p: &StretchParams, m: Morph) {
        self.job = None;
        let n = (m.fft_size.max(64).next_power_of_two()).min(MAX_FFT);
        let hop = (n / 4).max(1);
        let ch = self.channels;
        self.dirty.clear(self.ring, |a, b| {
            self.acc[a * ch..b * ch].fill(0.0);
            self.norm[a..b].fill(0.0);
        });
        self.emitted = out_frame;
        // Synthesis frames sit on a grid, so start on the one at or before the
        // target rather than part way through a window.
        self.index = out_frame / hop as u64;
        self.write = self.index * hop as u64;
        self.gain_from = 1.0;
        self.gain_to = 1.0;
        self.gain_at = self.write;
        self.anchor.reset(p.ratio.clamp(0.01, 100.0));
    }

    pub fn position(&self) -> u64 {
        self.emitted
    }

    /// Where in the source an output frame reads, under Scan: the same sweep the
    /// other parts of the hybrid follow, from the end of the file when it runs
    /// backward.
    fn source_at(&self, frame: f64, ratio: f32, scan: f32, in_frames: usize) -> f64 {
        let along = self.anchor.source(frame, ratio);
        if scan == 1.0 {
            along
        } else if scan < 0.0 {
            in_frames as f64 + along * scan as f64
        } else {
            along * scan as f64
        }
    }

    fn ensure_window(&mut self, n: usize) {
        if self.win_for == Some(n) {
            return;
        }
        self.win.clear();
        if n <= 1 {
            self.win.extend(std::iter::repeat(1.0).take(n));
        } else {
            for i in 0..n {
                self.win
                    .push(0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (n - 1) as f32).cos());
            }
        }
        let mut full = 0f32;
        for i in 0..n {
            full += self.win[i] * self.win[i];
        }
        self.win_sq_sum = full;
        self.win_for = Some(n);
    }

    fn begin_job(&mut self, in_frames: usize, channels: usize, p: &StretchParams, m: Morph) {
        let n = (m.fft_size.max(64).next_power_of_two()).min(MAX_FFT);
        let hop = (n / 4).max(1);
        let ratio = p.ratio.clamp(0.01, 100.0);
        self.anchor.retune(self.write, ratio);
        let span = m.smooth_bins.max(1) | 1;
        let bins = n / 2 + 1;
        let scan = p.grain.scan.clamp(-4.0, 4.0);
        self.ensure_window(n);

        let src = self.source_at(self.write as f64, ratio, scan, in_frames).max(0.0) as f32;
        let a_start = (src as usize).min(in_frames.saturating_sub(n));
        let b_start = (a_start + hop).min(in_frames.saturating_sub(n));
        let t = (src - a_start as f32).clamp(0.0, 1.0);

        self.job = Some(NoiseJob {
            channels,
            n,
            hop,
            bins,
            span,
            a_start,
            b_start,
            t,
            seed: m.seed,
            stage: NoiseStage::EnvWin { ch: 0, is_b: false, i: 0 },
            started: false,
        });
    }

    fn step_job(&mut self, input: &[f32], in_frames: usize, budget: usize) -> usize {
        let Some(mut job) = self.job else { return 0 };
        if job.is_ready() {
            return 0;
        }
        job.started = true;
        let cap = budget.max(1);
        let mut spent = 0usize;
        let n = job.n;
        let bins = job.bins;
        let channels = job.channels;
        let last_in = in_frames.saturating_sub(1);

        while spent < cap {
            let left = cap - spent;
            match job.stage {
                NoiseStage::Ready => break,
                NoiseStage::EnvWin { ch, is_b, i } => {
                    let start = if is_b { job.b_start } else { job.a_start };
                    let take = (left.saturating_mul(2)).max(1);
                    let end = i.saturating_add(take).min(n);
                    let win = &self.win[..n];
                    for idx in i..end {
                        let f = (start + idx).min(last_in);
                        self.re[idx] = input[f * channels + ch] * win[idx];
                        self.im[idx] = 0.0;
                    }
                    spent = spent.saturating_add((end - i).div_ceil(2));
                    job.stage = if end == n {
                        NoiseStage::EnvFft { ch, is_b, stepper: FftStepper::new(n, false) }
                    } else {
                        NoiseStage::EnvWin { ch, is_b, i: end }
                    };
                }
                NoiseStage::EnvFft { ch, is_b, mut stepper } => {
                    spent = spent.saturating_add(stepper.step(&mut self.re[..n], &mut self.im[..n], left));
                    job.stage = if stepper.is_done() {
                        NoiseStage::EnvMag { ch, is_b, k: 0 }
                    } else {
                        NoiseStage::EnvFft { ch, is_b, stepper }
                    };
                }
                NoiseStage::EnvMag { ch, is_b, k } => {
                    let take = (left / NOISE_MAG_WEIGHT).max(1);
                    let end = k.saturating_add(take).min(bins);
                    for idx in k..end {
                        let (r, i) = (self.re[idx], self.im[idx]);
                        self.mag[idx] = (r * r + i * i).sqrt();
                    }
                    spent = spent.saturating_add((end - k) * NOISE_MAG_WEIGHT);
                    job.stage = if end == bins {
                        NoiseStage::EnvSmooth { ch, is_b, k: 0 }
                    } else {
                        NoiseStage::EnvMag { ch, is_b, k: end }
                    };
                }
                NoiseStage::EnvSmooth { ch, is_b, k } => {
                    let half = job.span / 2;
                    let w_per_bin = (job.span / 4).max(1);
                    let take = (left / w_per_bin).max(1);
                    let end = k.saturating_add(take).min(bins);
                    let dst = if is_b { &mut self.env_b } else { &mut self.env_a };
                    for idx in k..end {
                        let lo = idx.saturating_sub(half);
                        let hi = (idx + half + 1).min(bins);
                        let mut acc = 0f32;
                        for kk in lo..hi {
                            acc += self.mag[kk] * self.mag[kk];
                        }
                        dst[idx] = (acc / (hi - lo) as f32).sqrt();
                    }
                    spent = spent.saturating_add((end - k) * w_per_bin);
                    job.stage = if end == bins {
                        if !is_b {
                            NoiseStage::EnvWin { ch, is_b: true, i: 0 }
                        } else {
                            NoiseStage::SynthSpec { ch, k: 0 }
                        }
                    } else {
                        NoiseStage::EnvSmooth { ch, is_b, k: end }
                    };
                }
                NoiseStage::SynthSpec { ch, k } => {
                    let take = (left / NOISE_SPEC_WEIGHT).max(1);
                    let end = k.saturating_add(take).min(bins);
                    let salt = job.seed.wrapping_add(ch as u32 * 7919);
                    let base = self.index * bins as u64;
                    for idx in k..end {
                        let mg = self.env_a[idx] + (self.env_b[idx] - self.env_a[idx]) * job.t;
                        let ph = rand01(base + idx as u64, salt) * std::f32::consts::TAU;
                        self.re[idx] = mg * ph.cos();
                        self.im[idx] = mg * ph.sin();
                    }
                    spent = spent.saturating_add((end - k) * NOISE_SPEC_WEIGHT);
                    job.stage = if end == bins {
                        NoiseStage::SynthMirror { ch }
                    } else {
                        NoiseStage::SynthSpec { ch, k: end }
                    };
                }
                NoiseStage::SynthMirror { ch } => {
                    for k in bins..n {
                        self.re[k] = self.re[n - k];
                        self.im[k] = -self.im[n - k];
                    }
                    self.im[0] = 0.0;
                    if n % 2 == 0 {
                        self.im[n / 2] = 0.0;
                    }
                    spent = spent.saturating_add(n / 4);
                    job.stage = NoiseStage::SynthIfft { ch, stepper: FftStepper::new(n, true) };
                }
                NoiseStage::SynthIfft { ch, mut stepper } => {
                    spent = spent.saturating_add(stepper.step(&mut self.re[..n], &mut self.im[..n], left));
                    if stepper.is_done() {
                        let oo = ch * MAX_FFT;
                        self.out_re[oo..oo + n].copy_from_slice(&self.re[..n]);
                        job.stage = if ch + 1 < channels {
                            NoiseStage::EnvWin { ch: ch + 1, is_b: false, i: 0 }
                        } else {
                            NoiseStage::Ready
                        };
                    } else {
                        job.stage = NoiseStage::SynthIfft { ch, stepper };
                    }
                }
            }
        }
        self.job = Some(job);
        spent
    }

    fn commit_job(&mut self) {
        let Some(job) = self.job.take() else { return };
        debug_assert!(job.is_ready());
        let n = job.n;
        let win = &self.win[..n];
        let stride = self.channels;
        for ch in 0..job.channels {
            let re = &self.out_re[ch * MAX_FFT..ch * MAX_FFT + n];
            let mut done = 0usize;
            let mut slot = (self.write % self.ring as u64) as usize;
            while done < n {
                let run = (n - done).min(self.ring - slot);
                let acc = &mut self.acc[slot * stride..(slot + run) * stride];
                for ((a, &r), &w) in acc
                    .chunks_exact_mut(stride)
                    .zip(&re[done..done + run])
                    .zip(&win[done..done + run])
                {
                    a[ch] += r * w;
                }
                done += run;
                slot = 0;
            }
        }
        {
            let mut done = 0usize;
            let mut slot = (self.write % self.ring as u64) as usize;
            while done < n {
                let run = (n - done).min(self.ring - slot);
                for (dst, &w) in self.norm[slot..slot + run]
                    .iter_mut()
                    .zip(&win[done..done + run])
                {
                    *dst += w * w;
                }
                done += run;
                slot = 0;
            }
        }
        self.dirty.mark(self.write, self.write + n as u64);
        self.write += job.hop as u64;
        self.index += 1;
    }

    /// Fill one block. Must not allocate.
    pub fn render(
        &mut self,
        out: &mut [f32],
        channels: usize,
        input: &[f32],
        p: &StretchParams,
        m: Morph,
    ) {
        let channels = channels.max(1).min(self.channels);
        let frames = out.len() / channels;
        out.fill(0.0);
        let in_frames = input.len() / channels;
        if frames == 0 || in_frames == 0 {
            return;
        }
        let n = (m.fft_size.max(64).next_power_of_two()).min(MAX_FFT);
        let hop = (n / 4).max(1);
        let ratio = p.ratio.clamp(0.01, 100.0);
        let scan = p.grain.scan.clamp(-4.0, 4.0);

        if let Some(job) = self.job {
            if job.channels != channels {
                self.job = None;
            }
        }

        let need = self.emitted + frames as u64;
        while self.write < need {
            if self.job.is_none() {
                self.begin_job(in_frames, channels, p, m);
            }
            self.step_job(input, in_frames, usize::MAX);
            self.commit_job();
        }
        if self.job.is_none() {
            self.ensure_window(n);
            self.anchor.retune(self.write, ratio);
        }

        // The normalisation floor, as a share of the full overlap.
        let (eff_n, eff_hop) = match self.job {
            Some(job) => (job.n, job.hop),
            None => (n, hop),
        };
        let floor = (self.win_sq_sum / (eff_n as f32 / eff_hop as f32).max(1.0)) * 0.05;

        let stride = self.channels;
        let mut done = 0usize;
        let mut slot = (self.emitted % self.ring as u64) as usize;
        while done < frames {
            let run = (frames - done).min(self.ring - slot);
            for k in 0..run {
                let f = done + k;
                let frame = self.emitted + f as u64;
                let s = slot + k;
                let d = self.norm[s].max(floor);

                // Put the level back where the source's was. Random phases sum
                // incoherently, so the synthesised level is short by an amount that
                // depends on the overlap and the spectrum — measured rather than
                // derived, a hop at a time, ramped so nothing steps.
                if frame >= self.gain_at {
                    self.gain_from = self.gain_to;
                    self.gain_to = self.local_gain(input, channels, frame, ratio, hop, d, scan);
                    self.gain_at = frame + hop as u64;
                }
                let along = ((frame + hop as u64 - self.gain_at) as f32) / hop as f32;
                let g = self.gain_from + (self.gain_to - self.gain_from) * along.clamp(0.0, 1.0);

                for ch in 0..channels {
                    let v = self.acc[s * stride + ch];
                    out[f * channels + ch] = if d > 1e-6 { v / d * g } else { 0.0 };
                    self.acc[s * stride + ch] = 0.0;
                }
                self.norm[s] = 0.0;
            }
            done += run;
            slot = 0;
        }
        if channels == self.channels {
            self.dirty.cleared_prefix(self.emitted, need, self.ring);
        }
        self.emitted = need;

        // Spread the next noise-morph frame across the blocks remaining before `self.write`.
        if self.write >= self.emitted {
            let rem_frames = (self.write - self.emitted) as usize;
            if self.job.is_none() && rem_frames > 0 {
                self.begin_job(in_frames, channels, p, m);
            }
            if let Some(job) = self.job {
                if !job.is_ready() {
                    let blocks_left = (rem_frames / frames.max(1)) + 1;
                    let rem_work = job.remaining_work();
                    let mut budget = rem_work.div_ceil(blocks_left).max(1);
                    if !job.started && self.stagger > 0 && blocks_left > 1 {
                        budget = ((budget * (8 - self.stagger)) / 8).max(1);
                    }
                    self.step_job(input, in_frames, budget);
                }
            }
        }
    }

    /// How far the synthesised level is from the source's, right here.
    fn local_gain(
        &self,
        input: &[f32],
        channels: usize,
        frame: u64,
        ratio: f32,
        hop: usize,
        d: f32,
        scan: f32,
    ) -> f32 {
        let in_frames = input.len() / channels;
        let src = self.source_at(frame as f64, ratio, scan, in_frames).max(0.0) as usize;
        if src >= in_frames || d <= 1e-6 {
            return 1.0;
        }
        let hi = (src + hop).min(in_frames);
        let mut want = 0f32;
        for f in src..hi {
            for ch in 0..channels {
                let v = input[f * channels + ch];
                want += v * v;
            }
        }
        let want = (want / ((hi - src).max(1) * channels) as f32).sqrt();

        let mut have = 0f32;
        let stride = self.channels;
        let mut done = 0usize;
        let mut slot = (frame % self.ring as u64) as usize;
        while done < hop {
            let run = (hop - done).min(self.ring - slot);
            let acc = &self.acc[slot * stride..(slot + run) * stride];
            let norm = &self.norm[slot..slot + run];
            for (a, &nm) in acc.chunks_exact(stride).zip(norm) {
                let n = nm.max(d).max(1e-6);
                for &v in &a[..channels] {
                    let x = v / n;
                    have += x * x;
                }
            }
            done += run;
            slot = 0;
        }
        let have = (have / (hop * channels) as f32).sqrt();
        // Never more than twelve decibels of it — beyond that it is amplifying
        // the noise floor of a gap.
        if have > 1e-9 {
            (want / have).clamp(0.0, 4.0)
        } else {
            1.0
        }
    }
}

/// One splitmix64 round. The same construction the grain cloud uses, for the
/// same reason: randomness addressed by index rather than streamed, so three
/// separate renders agree.
fn rand01(index: u64, salt: u32) -> f32 {
    let mut z = index
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(salt as u64 ^ 0xD1B5_4A32_D192_ED03);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    ((z >> 40) as f32) / 16_777_216.0
}
