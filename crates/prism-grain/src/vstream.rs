//! The phase vocoder, a block at a time.
//!
//! Frequency-domain engines look harder to stream than time-domain ones and are
//! mostly easier. All the state a vocoder carries between frames is per-bin and
//! fixed in size: the previous analysis phase, the accumulated synthesis phase,
//! and the held magnitudes. Nothing about it needs the whole file — that was
//! only ever true of *how it was written*, which took a buffer and returned a
//! buffer.
//!
//! Two things did genuinely have to change.
//!
//! **The overlap-add cannot be normalised at the end**, because there is no
//! end. A frame laid at the write pointer runs on past this block, so a frame
//! of output is only finished once the write pointer has passed it — the same
//! ring the WSOLA streamer uses, for the same reason.
//!
//! **The normalisation floor cannot be a maximum over the output.** The offline
//! renderer took `max(norm) × NORM_FLOOR`, which needs every frame before it
//! can divide the first one. It is computed here from the window and the hop
//! instead: lay frames down until the overlap is complete and take the peak of
//! *that*, which is what the maximum was converging to anyway. It is also the
//! more honest quantity — a share of what the overlap sums to when it is full,
//! rather than a share of whatever the biggest number in one particular render
//! happened to be.
//!
//! Both stereo modes live here. They differ only in where the phase comes from:
//! independently per channel, or once from the channel sum and applied as a
//! shared correction. Sharing the schedule between them is what keeps the two
//! from drifting into different engines.

use crate::fft::{fft, FftStepper};

use crate::stream::{RingDirty, StretchParams, Streamer};

const TWO_PI: f32 = std::f32::consts::TAU;

/// The largest transform any control allows. Buffers are sized from this, not
/// from the current setting, because the setting moves between blocks.
const MAX_FFT: usize = 8192;

/// A share of the full overlap below which the normalisation stops dividing.
///
/// The overlap-add divides by the summed *square* of the window, which tails
/// toward nothing at an edge — and dividing by nearly nothing turns a correctly
/// quiet edge into a peak. The same constant the offline renderer used.
const NORM_FLOOR: f32 = 0.05;

/// A phase vocoder that can be driven from an audio callback.
pub struct VocoderStream {
    channels: usize,

    // Per channel, `bins` apart. Sized for the largest transform.
    prev_phase: Vec<f32>,
    sum_phase: Vec<f32>,
    held: Vec<f32>,
    mag: Vec<f32>,
    phase: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,

    // The linked mode's shared spectrum, and the correction it produces.
    ref_mag: Vec<f32>,
    ref_phase: Vec<f32>,
    prev_ref: Vec<f32>,
    link_sum: Vec<f32>,
    corr: Vec<f32>,

    scratch: Vec<f32>,
    peak_idx: Vec<usize>,
    /// How far along its wander the pitch drift has got.
    drift: crate::grain::DriftClock,

    /// The analysis and synthesis window, rebuilt only when its shape changes.
    win: Vec<f32>,
    win_for: Option<(usize, u32)>,
    /// The full-overlap peak of the summed squared window, likewise.
    floor: f32,
    floor_for: Option<(usize, usize, u32)>,
    /// Room to lay frames down in while deriving the floor above.
    floor_scratch: Vec<f32>,

    // The schedule, shared by both stereo modes.
    read: f64,
    prev_start: isize,
    write: u64,
    emitted: u64,
    index: u64,
    first: bool,
    /// The source ran out at the nominal hop. There is nothing further to lay
    /// down, and the rest of the output is whatever the ring still holds.
    ended: bool,

    // Output waiting to be handed out.
    acc: Vec<f32>,
    norm: Vec<f32>,
    ring: usize,
    dirty: RingDirty,
    /// Largest bin count written to the per-bin state since the last clear.
    dirty_bins: usize,
    /// In-flight next analysis/resynthesis frame, stepped across blocks.
    job: Option<VocoderJob>,
    /// Voice/layer phase offset (0..8) so polyphonic voices stagger stage transitions.
    stagger: usize,
}

/// One vocoder frame being computed incrementally ahead of its commit.
///
/// Holds no heap allocations; all buffers live in [`VocoderStream`]. Parameters
/// are snapshotted when the frame begins so every stage sees one consistent
/// setting, and the frame is committed into `acc`/`norm` only once `write < need`
/// so the output ring never wraps over unread frames.
#[derive(Clone, Copy)]
struct VocoderJob {
    channels: usize,
    n: usize,
    bins: usize,
    hs: usize,
    start: usize,
    ha: f32,
    rate: f32,
    gl: f32,
    gr: f32,
    reverse: bool,
    advance: f64,
    next_hop: u64,
    s: crate::vocoder::Settings,
    stage: VocoderStage,
    /// Set to `true` after the job's first staggered slice has run.
    started: bool,
}

#[derive(Clone, Copy)]
enum VocoderStage {
    Window { ch: usize, i: usize },
    FwdFft { ch: usize, stepper: FftStepper },
    Polar { ch: usize, k: usize },
    LinkMid { k: usize },
    LinkPropInit,
    LinkPropPeaks { p: usize, bins_done: usize },
    LinkPropAll { k: usize },
    LinkPropFinish,
    LinkCart { ch: usize, k: usize },
    IndepPropInit { ch: usize },
    IndepPropPeaks { ch: usize, p: usize, bins_done: usize },
    IndepPropAll { ch: usize, k: usize },
    IndepPropFinish { ch: usize },
    IndepCart { ch: usize, k: usize },
    InvMirror { ch: usize },
    InvFft { ch: usize, stepper: FftStepper },
    Ready,
}

// Empirically calibrated butterfly-equivalent work units (~1.1 ns/unit on x86_64):
// - Polar (`sqrt + atan2`): ~10 units/bin
// - Propagate (`vocoder::wrap` per bin): ~64 units/bin
// - Cartesian (`cos + sin`): ~7 units/bin
const POLAR_WEIGHT: usize = 10;
const CART_WEIGHT: usize = 7;
const PROP_WEIGHT: usize = 64;

impl VocoderJob {
    fn is_ready(&self) -> bool {
        matches!(self.stage, VocoderStage::Ready)
    }

    fn channel_analysis_work(n: usize, bins: usize) -> usize {
        n / 2 + FftStepper::total_work(n, false) + bins * POLAR_WEIGHT + bins
    }

    fn channel_inv_work(n: usize) -> usize {
        n / 4 + FftStepper::total_work(n, true)
    }

    fn phase_work(bins: usize, channels: usize, stereo_link: bool) -> usize {
        if stereo_link && channels > 1 {
            bins * POLAR_WEIGHT + bins * PROP_WEIGHT + channels * bins * CART_WEIGHT
        } else {
            channels * (bins * PROP_WEIGHT + bins * CART_WEIGHT)
        }
    }

    fn frame_work(n: usize, channels: usize, stereo_link: bool) -> usize {
        let bins = n / 2 + 1;
        channels * Self::channel_analysis_work(n, bins)
            + Self::phase_work(bins, channels, stereo_link)
            + channels * Self::channel_inv_work(n)
    }

    fn remaining_work(&self) -> usize {
        let n = self.n;
        let bins = self.bins;
        let chs = self.channels;
        let link = self.s.stereo_link && chs > 1;
        let all_inv = chs * Self::channel_inv_work(n);
        let all_phase = Self::phase_work(bins, chs, link);
        let one_an = Self::channel_analysis_work(n, bins);
        let one_inv = Self::channel_inv_work(n);
        match self.stage {
            VocoderStage::Window { ch, i } => {
                (n - i).div_ceil(2)
                    + FftStepper::total_work(n, false)
                    + bins * POLAR_WEIGHT
                    + bins
                    + (chs - 1 - ch) * one_an
                    + all_phase
                    + all_inv
            }
            VocoderStage::FwdFft { ch, stepper } => {
                stepper.remaining_work()
                    + bins * POLAR_WEIGHT
                    + bins
                    + (chs - 1 - ch) * one_an
                    + all_phase
                    + all_inv
            }
            VocoderStage::Polar { ch, k } => {
                (bins - k) * POLAR_WEIGHT
                    + bins
                    + (chs - 1 - ch) * one_an
                    + all_phase
                    + all_inv
            }
            VocoderStage::LinkMid { k } => {
                (bins - k) * POLAR_WEIGHT
                    + bins * PROP_WEIGHT
                    + chs * bins * CART_WEIGHT
                    + all_inv
            }
            VocoderStage::LinkPropInit => {
                bins * PROP_WEIGHT + chs * bins * CART_WEIGHT + all_inv
            }
            VocoderStage::LinkPropPeaks { bins_done, .. } => {
                bins.saturating_sub(bins_done) * PROP_WEIGHT + chs * bins * CART_WEIGHT + all_inv
            }
            VocoderStage::LinkPropAll { k } => {
                (bins - k) * PROP_WEIGHT + chs * bins * CART_WEIGHT + all_inv
            }
            VocoderStage::LinkPropFinish => {
                bins * 2 + chs * bins * CART_WEIGHT + all_inv
            }
            VocoderStage::LinkCart { ch, k } => {
                (bins - k) * CART_WEIGHT + (chs - 1 - ch) * bins * CART_WEIGHT + all_inv
            }
            VocoderStage::IndepPropInit { ch } => {
                bins * PROP_WEIGHT
                    + bins * CART_WEIGHT
                    + (chs - 1 - ch) * (bins * PROP_WEIGHT + bins * CART_WEIGHT)
                    + all_inv
            }
            VocoderStage::IndepPropPeaks { ch, bins_done, .. } => {
                bins.saturating_sub(bins_done) * PROP_WEIGHT
                    + bins * CART_WEIGHT
                    + (chs - 1 - ch) * (bins * PROP_WEIGHT + bins * CART_WEIGHT)
                    + all_inv
            }
            VocoderStage::IndepPropAll { ch, k } => {
                (bins - k) * PROP_WEIGHT
                    + bins * CART_WEIGHT
                    + (chs - 1 - ch) * (bins * PROP_WEIGHT + bins * CART_WEIGHT)
                    + all_inv
            }
            VocoderStage::IndepPropFinish { ch } => {
                bins * 2
                    + bins * CART_WEIGHT
                    + (chs - 1 - ch) * (bins * PROP_WEIGHT + bins * CART_WEIGHT)
                    + all_inv
            }
            VocoderStage::IndepCart { ch, k } => {
                (bins - k) * CART_WEIGHT
                    + (chs - 1 - ch) * (bins * PROP_WEIGHT + bins * CART_WEIGHT)
                    + all_inv
            }
            VocoderStage::InvMirror { ch } => {
                n / 4 + FftStepper::total_work(n, true) + (chs - 1 - ch) * one_inv
            }
            VocoderStage::InvFft { ch, stepper } => {
                stepper.remaining_work() + (chs - 1 - ch) * one_inv
            }
            VocoderStage::Ready => 0,
        }
    }
}

impl Streamer for VocoderStream {
    fn position(&self) -> u64 {
        VocoderStream::position(self)
    }
    fn seek(&mut self, out_frame: u64, input_frames: usize, p: &StretchParams) {
        VocoderStream::seek(self, out_frame, input_frames, p)
    }
    fn render(&mut self, out: &mut [f32], channels: usize, input: &[f32], p: &StretchParams) {
        VocoderStream::render(self, out, channels, input, p)
    }
}

impl VocoderStream {
    pub fn new(max_block: usize, channels: usize) -> Self {
        let channels = channels.max(1);
        let bins = MAX_FFT / 2 + 1;
        // A frame laid at the far end of a block still has to fit.
        let ring = max_block.max(1) + MAX_FFT + 1;
        VocoderStream {
            channels,
            prev_phase: vec![0.0; channels * bins],
            sum_phase: vec![0.0; channels * bins],
            held: vec![0.0; channels * bins],
            mag: vec![0.0; channels * bins],
            phase: vec![0.0; channels * bins],
            re: vec![0.0; channels * MAX_FFT],
            im: vec![0.0; channels * MAX_FFT],
            ref_mag: vec![0.0; bins],
            ref_phase: vec![0.0; bins],
            prev_ref: vec![0.0; bins],
            link_sum: vec![0.0; bins],
            corr: vec![0.0; bins],
            scratch: vec![0.0; bins],
            peak_idx: Vec::with_capacity(bins / 4),
            win: Vec::with_capacity(MAX_FFT),
            win_for: None,
            floor: 0.0,
            floor_for: None,
            floor_scratch: vec![0.0; 3 * MAX_FFT],
            read: 0.0,
            prev_start: -1,
            write: 0,
            emitted: 0,
            index: 0,
            drift: crate::grain::DriftClock::default(),
            first: true,
            ended: false,
            acc: vec![0.0; ring * channels],
            norm: vec![0.0; ring],
            ring,
            dirty: RingDirty::default(),
            dirty_bins: 0,
            job: None,
            stagger: 0,
        }
    }

    /// Assign a voice/layer index so polyphonic chords stagger stage transitions.
    pub fn set_stagger(&mut self, id: usize) {
        self.stagger = id & 7;
    }

    fn ensure_window(&mut self, n: usize, skew: f32) {
        let key = (n, skew.to_bits());
        if self.win_for == Some(key) {
            return;
        }
        crate::vocoder::write_skewed_window(&mut self.win, n, skew);
        self.win_for = Some(key);
    }

    /// The peak of the summed squared window once the overlap is complete.
    ///
    /// Derived by laying frames down until the middle has settled, rather than
    /// assumed from a formula — the window is adjustable and the hop can be any
    /// integer, so there is no closed form worth trusting.
    fn ensure_floor(&mut self, n: usize, hs: usize, skew: f32) {
        let key = (n, hs, skew.to_bits());
        if self.floor_for == Some(key) {
            return;
        }
        let hs = hs.max(1);
        if hs >= n {
            // Frames do not reach each other, so nothing ever overlaps and the
            // peak is one window's own. Worth the branch: it is also what keeps
            // the scratch below bounded, since the hop can be far longer than
            // the window when density is set very low.
            self.floor = self.win[..n].iter().fold(0f32, |m, &w| m.max(w * w)) * NORM_FLOOR;
            self.floor_for = Some(key);
            return;
        }
        let span = 3 * n;
        let acc = &mut self.floor_scratch[..span];
        acc.fill(0.0);
        let mut at = 0usize;
        while at + n <= span {
            for i in 0..n {
                acc[at + i] += self.win[i] * self.win[i];
            }
            at += hs;
        }
        // The middle, where every frame that can contribute has.
        let peak = acc[n..2 * n].iter().fold(0f32, |m, &x| m.max(x));
        self.floor = peak * NORM_FLOOR;
        self.floor_for = Some(key);
    }

    fn clear(&mut self) {
        let ch = self.channels;
        self.dirty.clear(self.ring, |a, b| {
            self.acc[a * ch..b * ch].fill(0.0);
            self.norm[a..b].fill(0.0);
        });
        let bins = self.dirty_bins;
        if bins > 0 {
            let stride = MAX_FFT / 2 + 1;
            for c in 0..ch {
                let bo = c * stride;
                self.prev_phase[bo..bo + bins].fill(0.0);
                self.sum_phase[bo..bo + bins].fill(0.0);
                self.held[bo..bo + bins].fill(0.0);
            }
            self.prev_ref[..bins].fill(0.0);
            self.link_sum[..bins].fill(0.0);
            self.dirty_bins = 0;
        }
    }

    pub fn position(&self) -> u64 {
        self.emitted
    }

    /// Move to an output frame.
    ///
    /// A vocoder cannot resume mid-stream any more than WSOLA can: the
    /// synthesis phase is an accumulation from wherever it started, so arriving
    /// at a moment is not the same as having played to it. The read pointer is
    /// put where the schedule says and the phase starts afresh, which is
    /// audible as a moment of re-anchoring and nothing worse.
    pub fn seek(&mut self, out_frame: u64, input_frames: usize, p: &StretchParams) {
        self.job = None;
        self.clear();
        self.emitted = out_frame;
        self.write = out_frame;
        self.first = true;
        self.ended = false;
        self.prev_start = -1;
        self.drift.reset();

        let n = crate::stretch::fft_size_for(p.vocoder.window_ms, p.sample_rate);
        let hs = crate::stretch::hop_frames(&p.grain, n, p.sample_rate.max(1) as f32).max(1);
        self.index = out_frame / hs as u64;

        let ratio = p.ratio.clamp(0.01, 100.0);
        let skew = p.grain.scan.clamp(-4.0, 4.0) as f64;
        let advance = (hs as f64 / ratio as f64) * skew;
        let span = input_frames.saturating_sub(n).max(1);
        self.read = if skew < 0.0 {
            span as f64 + advance * self.index as f64
        } else {
            advance * self.index as f64
        }
        .rem_euclid(span as f64);
    }

    /// Snapshot the next vocoder frame's schedule and parameters into `self.job`.
    /// Returns `false` if the stream has reached the end of the source.
    fn begin_job(&mut self, in_frames: usize, channels: usize, p: &StretchParams) -> bool {
        let n = crate::stretch::fft_size_for(p.vocoder.window_ms, p.sample_rate).min(MAX_FFT);
        let sr = p.sample_rate.max(1) as f32;
        let bins = n / 2 + 1;
        let hs = crate::stretch::hop_frames(&p.grain, n, sr).max(1);
        let ratio = p.ratio.clamp(0.01, 100.0);
        let g = p.grain;
        let s = crate::vocoder::settings_for(p);

        self.ensure_window(n, g.envelope);
        self.ensure_floor(n, hs, g.envelope);

        let skew = g.scan.clamp(-4.0, 4.0) as f64;
        let advance = (hs as f64 / ratio as f64) * skew;
        let span = in_frames.saturating_sub(n).max(1);
        let pos_jitter = (g.position_jitter_ms / 1000.0) * sr;

        let jitter = if pos_jitter > 0.0 {
            (pos_jitter * g.rand_bipolar(self.index, g.salt(5))) as f64
        } else {
            0.0
        };
        // The jitter, plus this layer's own throw — layers that read the
        // same instant and are laid down a fixed offset apart are a delay
        // line, not a cloud.
        let mut start = (self.read + jitter + g.layer_read as f64).max(0.0).round() as usize;
        if start + n > in_frames {
            // At the nominal hop, running out of source is the end of the
            // job — a stretch shorter than the source reaches the end of
            // the input before the end of the output, and the rest is
            // silence rather than a wrap nobody asked for.
            //
            // Off it, the read pointer is sweeping at a speed with nothing
            // to do with the output length, so wrap and keep going.
            if (skew - 1.0).abs() < 1e-6 && !g.wrap {
                self.ended = true;
                return false;
            }
            self.read = self.read.rem_euclid(span as f64);
            start = self.read.round() as usize;
            if start + n > in_frames {
                self.ended = true;
                return false;
            }
            // A wrap is a discontinuity; a hop measured across it would be
            // a large negative number and the phase estimate nonsense.
            self.prev_start = -1;
        }
        let start = start.min(in_frames.saturating_sub(n));

        let ha = if self.prev_start < 0 {
            hs as f32
        } else {
            (start as isize - self.prev_start) as f32
        };
        let ha = if ha.abs() < 1e-6 { hs as f32 } else { ha };
        let x = self.drift.place(self.write, sr, g.drift_rate_hz);
        let rate = crate::stretch::grain_rate(&g, self.index, x);
        let (gl, gr) = crate::grain::pan_gains(&g, self.index, channels);
        let next_hop = crate::stretch::grain_size(&g, self.index, hs).max(1) as u64;

        self.dirty_bins = self.dirty_bins.max(bins);
        self.job = Some(VocoderJob {
            channels,
            n,
            bins,
            hs,
            start,
            ha,
            rate,
            gl,
            gr,
            reverse: g.reverse,
            advance,
            next_hop,
            s,
            stage: VocoderStage::Window { ch: 0, i: 0 },
            started: false,
        });
        true
    }

    /// Advance the in-flight frame by at most `budget` butterfly-equivalent
    /// work units, returning how many units were spent.
    fn step_job(&mut self, input: &[f32], budget: usize) -> usize {
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

        while spent < cap {
            let left = cap - spent;
            match job.stage {
                VocoderStage::Ready => break,
                VocoderStage::Window { ch, i } => {
                    let ro = ch * MAX_FFT;
                    let take = (left.saturating_mul(2)).max(1);
                    let end = i.saturating_add(take).min(n);
                    let win = &self.win[..n];
                    let re = &mut self.re[ro..ro + n];
                    let im = &mut self.im[ro..ro + n];
                    if job.reverse {
                        for idx in i..end {
                            let j = n - 1 - idx;
                            re[idx] = input[(job.start + j) * channels + ch] * win[idx];
                            im[idx] = 0.0;
                        }
                    } else {
                        for idx in i..end {
                            re[idx] = input[(job.start + idx) * channels + ch] * win[idx];
                            im[idx] = 0.0;
                        }
                    }
                    spent = spent.saturating_add((end - i).div_ceil(2));
                    job.stage = if end == n {
                        VocoderStage::FwdFft { ch, stepper: FftStepper::new(n, false) }
                    } else {
                        VocoderStage::Window { ch, i: end }
                    };
                }
                VocoderStage::FwdFft { ch, mut stepper } => {
                    let ro = ch * MAX_FFT;
                    spent = spent.saturating_add(stepper.step(
                        &mut self.re[ro..ro + n],
                        &mut self.im[ro..ro + n],
                        left,
                    ));
                    job.stage = if stepper.is_done() {
                        VocoderStage::Polar { ch, k: 0 }
                    } else {
                        VocoderStage::FwdFft { ch, stepper }
                    };
                }
                VocoderStage::Polar { ch, k } => {
                    let ro = ch * MAX_FFT;
                    let bo = ch * (MAX_FFT / 2 + 1);
                    let take = (left / POLAR_WEIGHT).max(1);
                    let end = k.saturating_add(take).min(bins);
                    for idx in k..end {
                        let (r, i) = (self.re[ro + idx], self.im[ro + idx]);
                        self.mag[bo + idx] = (r * r + i * i).sqrt();
                        self.phase[bo + idx] = i.atan2(r);
                    }
                    spent = spent.saturating_add((end - k) * POLAR_WEIGHT);
                    if end == bins {
                        crate::vocoder::shape_magnitudes(
                            &mut self.mag[bo..bo + bins],
                            &mut self.held[bo..bo + bins],
                            &mut self.scratch[..bins],
                            &job.s,
                            self.first,
                        );
                        spent = spent.saturating_add(bins);
                        job.stage = if ch + 1 < channels {
                            VocoderStage::Window { ch: ch + 1, i: 0 }
                        } else if job.s.stereo_link && channels > 1 {
                            VocoderStage::LinkMid { k: 0 }
                        } else {
                            VocoderStage::IndepPropInit { ch: 0 }
                        };
                    } else {
                        job.stage = VocoderStage::Polar { ch, k: end };
                    }
                }
                VocoderStage::LinkMid { k } => {
                    let take = (left / POLAR_WEIGHT).max(1);
                    let end = k.saturating_add(take).min(bins);
                    for idx in k..end {
                        let (mut sr, mut si) = (0.0f32, 0.0f32);
                        for c in 0..channels {
                            sr += self.re[c * MAX_FFT + idx];
                            si += self.im[c * MAX_FFT + idx];
                        }
                        self.ref_mag[idx] = (sr * sr + si * si).sqrt();
                        self.ref_phase[idx] = si.atan2(sr);
                    }
                    spent = spent.saturating_add((end - k) * POLAR_WEIGHT);
                    job.stage = if end == bins {
                        VocoderStage::LinkPropInit
                    } else {
                        VocoderStage::LinkMid { k: end }
                    };
                }
                VocoderStage::LinkPropInit => {
                    if self.first {
                        self.link_sum[..bins].copy_from_slice(&self.ref_phase[..bins]);
                        spent = spent.saturating_add(bins);
                        job.stage = VocoderStage::LinkPropFinish;
                    } else if !job.s.phase_lock {
                        job.stage = VocoderStage::LinkPropAll { k: 0 };
                    } else {
                        crate::vocoder::peaks(
                            &self.ref_mag[..bins],
                            job.s.peak_width.clamp(1, 32),
                            &mut self.peak_idx,
                        );
                        spent = spent.saturating_add(bins * 2);
                        job.stage = if self.peak_idx.is_empty() {
                            VocoderStage::LinkPropAll { k: 0 }
                        } else {
                            VocoderStage::LinkPropPeaks { p: 0, bins_done: 0 }
                        };
                    }
                }
                VocoderStage::LinkPropPeaks { mut p, mut bins_done } => {
                    let trust = job.s.freq_trust.clamp(0.0, 4.0);
                    let spread = job.s.phase_spread.clamp(0.0, 4.0);
                    let width = job.s.lock_width.clamp(0.0, 4.0);
                    let total_peaks = self.peak_idx.len();
                    while p < total_peaks && spent < cap {
                        let touched = crate::vocoder::propagate_peaks_range(
                            &self.ref_phase[..bins],
                            &self.prev_ref[..bins],
                            &mut self.link_sum[..bins],
                            &self.peak_idx,
                            p,
                            p + 1,
                            n,
                            job.ha,
                            job.hs as f32,
                            trust,
                            spread,
                            width,
                        );
                        p += 1;
                        bins_done = bins_done.saturating_add(touched).min(bins);
                        spent = spent.saturating_add(touched * PROP_WEIGHT);
                    }
                    job.stage = if p >= total_peaks {
                        VocoderStage::LinkPropFinish
                    } else {
                        VocoderStage::LinkPropPeaks { p, bins_done }
                    };
                }
                VocoderStage::LinkPropAll { k } => {
                    let take = (left / PROP_WEIGHT).max(1);
                    let end = k.saturating_add(take).min(bins);
                    crate::vocoder::propagate_all_range(
                        &self.ref_phase[..bins],
                        &self.prev_ref[..bins],
                        &mut self.link_sum[..bins],
                        k,
                        end,
                        n,
                        job.ha,
                        job.hs as f32,
                        &job.s,
                    );
                    spent = spent.saturating_add((end - k) * PROP_WEIGHT);
                    job.stage = if end == bins {
                        VocoderStage::LinkPropFinish
                    } else {
                        VocoderStage::LinkPropAll { k: end }
                    };
                }
                VocoderStage::LinkPropFinish => {
                    if (job.rate - 1.0).abs() > 1e-6 {
                        for k in 0..bins {
                            self.link_sum[k] = wrap(self.link_sum[k] * job.rate);
                        }
                    }
                    for k in 0..bins {
                        self.corr[k] = wrap(self.link_sum[k] - self.ref_phase[k]);
                    }
                    self.prev_ref[..bins].copy_from_slice(&self.ref_phase[..bins]);
                    spent = spent.saturating_add(bins * 2);
                    job.stage = VocoderStage::LinkCart { ch: 0, k: 0 };
                }
                VocoderStage::LinkCart { ch, k } => {
                    let bo = ch * (MAX_FFT / 2 + 1);
                    let ro = ch * MAX_FFT;
                    let take = (left / CART_WEIGHT).max(1);
                    let end = k.saturating_add(take).min(bins);
                    for idx in k..end {
                        let p = self.phase[bo + idx] + self.corr[idx];
                        let m = self.mag[bo + idx];
                        self.re[ro + idx] = m * p.cos();
                        self.im[ro + idx] = m * p.sin();
                    }
                    spent = spent.saturating_add((end - k) * CART_WEIGHT);
                    job.stage = if end == bins {
                        if ch + 1 < channels {
                            VocoderStage::LinkCart { ch: ch + 1, k: 0 }
                        } else {
                            VocoderStage::InvMirror { ch: 0 }
                        }
                    } else {
                        VocoderStage::LinkCart { ch, k: end }
                    };
                }
                VocoderStage::IndepPropInit { ch } => {
                    let bo = ch * (MAX_FFT / 2 + 1);
                    if self.first {
                        self.sum_phase[bo..bo + bins].copy_from_slice(&self.phase[bo..bo + bins]);
                        spent = spent.saturating_add(bins);
                        job.stage = VocoderStage::IndepPropFinish { ch };
                    } else if !job.s.phase_lock {
                        job.stage = VocoderStage::IndepPropAll { ch, k: 0 };
                    } else {
                        crate::vocoder::peaks(
                            &self.mag[bo..bo + bins],
                            job.s.peak_width.clamp(1, 32),
                            &mut self.peak_idx,
                        );
                        spent = spent.saturating_add(bins * 2);
                        job.stage = if self.peak_idx.is_empty() {
                            VocoderStage::IndepPropAll { ch, k: 0 }
                        } else {
                            VocoderStage::IndepPropPeaks { ch, p: 0, bins_done: 0 }
                        };
                    }
                }
                VocoderStage::IndepPropPeaks { ch, mut p, mut bins_done } => {
                    let bo = ch * (MAX_FFT / 2 + 1);
                    let trust = job.s.freq_trust.clamp(0.0, 4.0);
                    let spread = job.s.phase_spread.clamp(0.0, 4.0);
                    let width = job.s.lock_width.clamp(0.0, 4.0);
                    let total_peaks = self.peak_idx.len();
                    while p < total_peaks && spent < cap {
                        let touched = crate::vocoder::propagate_peaks_range(
                            &self.phase[bo..bo + bins],
                            &self.prev_phase[bo..bo + bins],
                            &mut self.sum_phase[bo..bo + bins],
                            &self.peak_idx,
                            p,
                            p + 1,
                            n,
                            job.ha,
                            job.hs as f32,
                            trust,
                            spread,
                            width,
                        );
                        p += 1;
                        bins_done = bins_done.saturating_add(touched).min(bins);
                        spent = spent.saturating_add(touched * PROP_WEIGHT);
                    }
                    job.stage = if p >= total_peaks {
                        VocoderStage::IndepPropFinish { ch }
                    } else {
                        VocoderStage::IndepPropPeaks { ch, p, bins_done }
                    };
                }
                VocoderStage::IndepPropAll { ch, k } => {
                    let bo = ch * (MAX_FFT / 2 + 1);
                    let take = (left / PROP_WEIGHT).max(1);
                    let end = k.saturating_add(take).min(bins);
                    crate::vocoder::propagate_all_range(
                        &self.phase[bo..bo + bins],
                        &self.prev_phase[bo..bo + bins],
                        &mut self.sum_phase[bo..bo + bins],
                        k,
                        end,
                        n,
                        job.ha,
                        job.hs as f32,
                        &job.s,
                    );
                    spent = spent.saturating_add((end - k) * PROP_WEIGHT);
                    job.stage = if end == bins {
                        VocoderStage::IndepPropFinish { ch }
                    } else {
                        VocoderStage::IndepPropAll { ch, k: end }
                    };
                }
                VocoderStage::IndepPropFinish { ch } => {
                    let bo = ch * (MAX_FFT / 2 + 1);
                    if (job.rate - 1.0).abs() > 1e-6 {
                        for k in 0..bins {
                            self.sum_phase[bo + k] = wrap(self.sum_phase[bo + k] * job.rate);
                        }
                    }
                    self.prev_phase[bo..bo + bins].copy_from_slice(&self.phase[bo..bo + bins]);
                    spent = spent.saturating_add(bins * 2);
                    job.stage = VocoderStage::IndepCart { ch, k: 0 };
                }
                VocoderStage::IndepCart { ch, k } => {
                    let bo = ch * (MAX_FFT / 2 + 1);
                    let ro = ch * MAX_FFT;
                    let take = (left / CART_WEIGHT).max(1);
                    let end = k.saturating_add(take).min(bins);
                    for idx in k..end {
                        let m = self.mag[bo + idx];
                        let p = self.sum_phase[bo + idx];
                        self.re[ro + idx] = m * p.cos();
                        self.im[ro + idx] = m * p.sin();
                    }
                    spent = spent.saturating_add((end - k) * CART_WEIGHT);
                    job.stage = if end == bins {
                        if ch + 1 < channels {
                            VocoderStage::IndepPropInit { ch: ch + 1 }
                        } else {
                            VocoderStage::InvMirror { ch: 0 }
                        }
                    } else {
                        VocoderStage::IndepCart { ch, k: end }
                    };
                }
                VocoderStage::InvMirror { ch } => {
                    let ro = ch * MAX_FFT;
                    for k in bins..n {
                        self.re[ro + k] = self.re[ro + n - k];
                        self.im[ro + k] = -self.im[ro + n - k];
                    }
                    self.im[ro] = 0.0;
                    if n % 2 == 0 {
                        self.im[ro + n / 2] = 0.0;
                    }
                    spent = spent.saturating_add(n / 4);
                    job.stage = VocoderStage::InvFft { ch, stepper: FftStepper::new(n, true) };
                }
                VocoderStage::InvFft { ch, mut stepper } => {
                    let ro = ch * MAX_FFT;
                    spent = spent.saturating_add(stepper.step(
                        &mut self.re[ro..ro + n],
                        &mut self.im[ro..ro + n],
                        left,
                    ));
                    job.stage = if stepper.is_done() {
                        if ch + 1 < channels {
                            VocoderStage::InvMirror { ch: ch + 1 }
                        } else {
                            VocoderStage::Ready
                        }
                    } else {
                        VocoderStage::InvFft { ch, stepper }
                    };
                }
            }
        }
        self.job = Some(job);
        spent
    }

    /// Overlap-add a finished `VocoderJob` into the output ring and advance
    /// the frame schedule. Caller ensures `self.write + job.n <= self.emitted + self.ring`.
    fn commit_job(&mut self) {
        let Some(job) = self.job.take() else { return };
        debug_assert!(job.is_ready());
        let n = job.n;
        let stride = self.channels;
        let win = &self.win[..n];
        for c in 0..job.channels {
            let ro = c * MAX_FFT;
            let pan = if c == 0 { job.gl } else { job.gr };
            let re_c = &self.re[ro..ro + n];
            let mut done = 0usize;
            let mut slot = (self.write % self.ring as u64) as usize;
            while done < n {
                let run = (n - done).min(self.ring - slot);
                let acc = &mut self.acc[slot * stride..(slot + run) * stride];
                for ((a, &r), &w) in acc
                    .chunks_exact_mut(stride)
                    .zip(&re_c[done..done + run])
                    .zip(&win[done..done + run])
                {
                    a[c] += r * w * pan;
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

        self.prev_start = job.start as isize;
        self.first = false;
        self.read += job.advance;
        self.write += job.next_hop;
        self.index += 1;
    }

    /// Hand out `frames` finished output samples from the ring and clear their slots.
    fn emit_frames(&mut self, out: &mut [f32], frames: usize, channels: usize) {
        let need = self.emitted + frames as u64;
        let stride = self.channels;
        let floor = self.floor;
        let mut done = 0usize;
        let mut slot = (self.emitted % self.ring as u64) as usize;
        while done < frames {
            let run = (frames - done).min(self.ring - slot);
            let acc = &mut self.acc[slot * stride..(slot + run) * stride];
            let norm = &mut self.norm[slot..slot + run];
            let dst = &mut out[done * channels..(done + run) * channels];
            for ((a, nm), o) in acc
                .chunks_exact_mut(stride)
                .zip(norm.iter_mut())
                .zip(dst.chunks_exact_mut(channels))
            {
                let d = nm.max(floor);
                for ch in 0..channels {
                    let v = a[ch];
                    o[ch] = if d > 1e-6 { v / d } else { v };
                }
                a.fill(0.0);
                *nm = 0.0;
            }
            done += run;
            slot = 0;
        }
        self.dirty.cleared_prefix(self.emitted, need, self.ring);
        self.emitted = need;
    }

    /// Estimated remaining work units to push `self.write` up to `target_write`
    /// output frames (used by [`crate::pstream::PvsolaStream`] to pace rounds).
    pub(crate) fn remaining_work_for(&self, target_write: usize, channels: usize, p: &StretchParams) -> usize {
        if self.ended {
            return 0;
        }
        let channels = channels.max(1).min(self.channels);
        let target = target_write as u64;
        if self.write >= target {
            return 0;
        }
        let n = crate::stretch::fft_size_for(p.vocoder.window_ms, p.sample_rate).min(MAX_FFT);
        let sr = p.sample_rate.max(1) as f32;
        let hs = crate::stretch::hop_frames(&p.grain, n, sr).max(1) as u64;
        let one_frame = VocoderJob::frame_work(n, channels, p.vocoder.stereo_link);
        if let Some(job) = self.job {
            let after_job = self.write + job.next_hop;
            let extra_frames = if after_job < target {
                ((target - after_job) as usize).div_ceil(hs as usize)
            } else {
                0
            };
            job.remaining_work() + extra_frames * one_frame
        } else {
            let frames_needed = ((target - self.write) as usize).div_ceil(hs as usize);
            frames_needed * one_frame
        }
    }

    /// Advance by at most `budget` work units and emit as many finished frames
    /// (up to `out.len() / channels`) as are now ready. Used by PVSOLA to spread
    /// even the first vocoder frame of a round across callbacks.
    ///
    /// Returns the number of output frames written into `out`.
    pub(crate) fn render_budgeted(
        &mut self,
        out: &mut [f32],
        channels: usize,
        input: &[f32],
        p: &StretchParams,
        budget: usize,
    ) -> usize {
        let channels = channels.max(1).min(self.channels);
        let max_frames = out.len() / channels;
        if max_frames == 0 {
            return 0;
        }
        if budget == usize::MAX {
            self.render(out, channels, input, p);
            return max_frames;
        }
        let in_frames = input.len() / channels;
        if in_frames == 0 {
            out.fill(0.0);
            return max_frames;
        }
        let n = crate::stretch::fft_size_for(p.vocoder.window_ms, p.sample_rate).min(MAX_FFT);
        if in_frames < n {
            self.render(out, channels, input, p);
            return max_frames;
        }
        if let Some(job) = self.job {
            if job.channels != channels || job.start + job.n > in_frames {
                self.job = None;
            }
        }

        let target = self.emitted + max_frames as u64;
        let mut spent = 0usize;
        let cap = budget.max(1);
        while spent < cap && self.write < target && !self.ended {
            if self.job.is_none() && !self.begin_job(in_frames, channels, p) {
                break;
            }
            spent = spent.saturating_add(self.step_job(input, cap - spent));
            if let Some(job) = self.job {
                if job.is_ready() {
                    if self.write + job.n as u64 <= self.emitted + self.ring as u64 {
                        self.commit_job();
                    } else {
                        break;
                    }
                }
            }
        }

        let avail = if self.ended {
            max_frames
        } else {
            self.write.saturating_sub(self.emitted).min(max_frames as u64) as usize
        };
        if avail > 0 {
            self.emit_frames(&mut out[..avail * channels], avail, channels);
        }
        avail
    }

    /// Fill one block. Must not allocate.
    pub fn render(&mut self, out: &mut [f32], channels: usize, input: &[f32], p: &StretchParams) {
        let channels = channels.max(1).min(self.channels);
        let frames = out.len() / channels;
        out.fill(0.0);
        let in_frames = input.len() / channels;
        if frames == 0 || in_frames == 0 {
            return;
        }

        let n = crate::stretch::fft_size_for(p.vocoder.window_ms, p.sample_rate).min(MAX_FFT);
        if in_frames < n {
            // Too short to transform. Nothing useful to say about its spectrum,
            // and the offline renderer passes it through for the same reason.
            let take = frames.min(in_frames.saturating_sub(self.emitted as usize));
            for f in 0..take {
                for ch in 0..channels {
                    out[f * channels + ch] = input[(self.emitted as usize + f) * channels + ch];
                }
            }
            self.emitted += frames as u64;
            return;
        }

        if let Some(job) = self.job {
            if job.channels != channels || job.start + job.n > in_frames {
                self.job = None;
            }
        }

        let need = self.emitted + frames as u64;
        while self.write < need && !self.ended {
            if self.job.is_none() && !self.begin_job(in_frames, channels, p) {
                break;
            }
            self.step_job(input, usize::MAX);
            self.commit_job();
        }

        // Hand out what is finished, and clear it for reuse.
        self.emit_frames(out, frames, channels);

        // Spread the next frame across the blocks between now and when `self.write`
        // is needed again. Only begin a brand-new job ahead when there is at least
        // one block of runway (`rem_frames > 0`), so large blocks (`frames >= hop`)
        // never compute two frames in one callback.
        if !self.ended && self.write >= self.emitted {
            let rem_frames = (self.write - self.emitted) as usize;
            if self.job.is_none() && rem_frames > 0 {
                let _ = self.begin_job(in_frames, channels, p);
            }
            if let Some(job) = self.job {
                if !job.is_ready() {
                    let blocks_left = (rem_frames / frames.max(1)) + 1;
                    let rem_work = job.remaining_work();
                    let mut budget = rem_work.div_ceil(blocks_left).max(1);
                    if !job.started && self.stagger > 0 && blocks_left > 1 {
                        budget = ((budget * (8 - self.stagger)) / 8).max(1);
                    }
                    self.step_job(input, budget);
                }
            }
        }
    }
}

#[inline]
fn wrap(p: f32) -> f32 {
    let mut x = p;
    while x > std::f32::consts::PI {
        x -= TWO_PI;
    }
    while x < -std::f32::consts::PI {
        x += TWO_PI;
    }
    x
}

/// Real-output inverse transform by conjugation.
///
/// Every caller (`vstream::render`, `nstream::render`, `decompose`, `noise`,
/// and `vocoder` tests) reconstructs a Hermitian-symmetric real signal, reads
/// only `re`, and overwrites `im` before reusing it — so the final `im` pass
/// is skipped. For power-of-two lengths, multiplying by `1.0 / n` is IEEE-754
/// bit-identical to dividing by `n`.
pub(crate) fn ifft(re: &mut [f32], im: &mut [f32]) {
    for v in im.iter_mut() {
        *v = -*v;
    }
    fft(re, im);
    let len = re.len();
    if len.is_power_of_two() {
        let inv = 1.0 / len as f32;
        for v in re.iter_mut() {
            *v *= inv;
        }
    } else if len > 0 {
        let n = len as f32;
        for v in re.iter_mut() {
            *v /= n;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stretch::{Algorithm, Stretch};

    const RATE: u32 = 44_100;

    fn chord(secs: f32, channels: usize) -> Vec<f32> {
        let n = (RATE as f32 * secs) as usize;
        let mut v = Vec::with_capacity(n * channels);
        for i in 0..n {
            let t = i as f32 / RATE as f32;
            let s = 0.3 * (std::f32::consts::TAU * 220.0 * t).sin()
                + 0.25 * (std::f32::consts::TAU * 277.2 * t).sin()
                + 0.2 * (std::f32::consts::TAU * 329.6 * t).sin();
            for c in 0..channels {
                // Delayed rather than scaled, so the linked mode has something
                // to link. Two scaled copies come out identical either way.
                v.push(if c == 0 { s } else { 0.0 });
            }
        }
        for i in 0..n {
            for c in 1..channels {
                v[i * channels + c] = if i >= 977 { v[(i - 977) * channels] } else { 0.0 };
            }
        }
        v
    }

    fn params(spec: &Stretch) -> StretchParams {
        StretchParams {
            ratio: spec.ratio,
            window_ms: spec.window_ms,
            sample_rate: RATE,
            wsola: spec.wsola,
            vocoder: spec.vocoder,
            grain: spec.grain,
            soundhack: spec.soundhack,
        }
    }

    fn streamed(input: &[f32], channels: usize, spec: &Stretch, block: usize) -> Vec<f32> {
        let p = params(spec);
        let in_frames = input.len() / channels;
        let want = ((in_frames as f64) * spec.ratio as f64).round() as usize;
        let mut s = VocoderStream::new(block, channels);
        s.seek(0, in_frames, &p);
        let mut out = vec![0f32; want * channels];
        let mut at = 0usize;
        let mut buf = vec![0f32; block * channels];
        while at < want {
            let n = block.min(want - at);
            s.render(&mut buf[..n * channels], channels, input, &p);
            out[at * channels..(at + n) * channels].copy_from_slice(&buf[..n * channels]);
            at += n;
        }
        out
    }


    /// A new Drift rate changes the speed and not the place: see `DriftClock`.
    #[test]
    fn a_change_of_rate_leaves_the_drift_where_it_was() {
        let input = vec![0.1f32; RATE as usize * 2];
        let mut sp = spec(1.0);
        sp.grain.pitch_drift_semis = 8.0;
        sp.grain.drift_rate_hz = 0.5;
        let mut p = params(&sp);
        let mut s = VocoderStream::new(512, 1);
        s.seek(RATE as u64 * 100, RATE as usize * 2, &p);
        let mut buf = vec![0f32; 512];
        s.render(&mut buf, 1, &input, &p);
        let sr = RATE as f32;
        let before = s.drift.place(s.write, sr, 0.5);
        p.grain.drift_rate_hz = 0.51;
        s.render(&mut buf, 1, &input, &p);
        let after = s.drift.place(s.write, sr, 0.51);
        assert!((after - before).abs() < 0.05, "the drift moved {} nodes in a block", (after - before).abs());
    }

    fn spec(ratio: f32) -> Stretch {
        Stretch { ratio, algorithm: Algorithm::Vocoder, ..Default::default() }
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len().max(1) as f32).sqrt()
    }

    #[test]
    fn streaming_matches_the_offline_vocoder() {
        let src = chord(0.5, 2);
        for ratio in [0.5f32, 2.0, 4.0] {
            let s = spec(ratio);
            let offline = s.process(&src, 2, RATE);
            let live = streamed(&src, 2, &s, 512);
            assert_eq!(offline.len(), live.len(), "lengths differ at {ratio}x");
            let worst = offline
                .iter()
                .zip(&live)
                .map(|(a, b)| (a - b).abs())
                .fold(0f32, f32::max);
            assert!(worst < 1e-6, "at {ratio}x the two paths differ by {worst:.2e}");
        }
    }

    #[test]
    fn the_block_size_does_not_change_the_sound() {
        let src = chord(0.4, 2);
        let s = spec(3.0);
        let a = streamed(&src, 2, &s, 64);
        let b = streamed(&src, 2, &s, 1024);
        for (i, (x, y)) in a.iter().zip(&b).enumerate() {
            assert_eq!(x.to_bits(), y.to_bits(), "bit mismatch at sample {i}: {x} vs {y}");
        }
    }

    #[test]
    fn spread_frames_and_stagger_are_bit_identical_when_params_hold_still() {
        let src = chord(0.5, 2);
        for &(ratio, link) in &[(0.5f32, false), (1.0, false), (2.0, false), (2.0, true)] {
            let mut s = spec(ratio);
            s.vocoder.stereo_link = link;
            let reference = if (ratio - 1.0).abs() < 1e-6 {
                streamed(&src, 2, &s, 2048)
            } else {
                s.process(&src, 2, RATE)
            };
            let p = params(&s);
            let in_frames = src.len() / 2;
            let want = reference.len() / 2;
            for stagger in [0usize, 3, 7] {
                let mut vs = VocoderStream::new(64, 2);
                vs.set_stagger(stagger);
                vs.seek(0, in_frames, &p);
                let mut out = vec![0f32; want * 2];
                let mut at = 0usize;
                let mut buf = vec![0f32; 64 * 2];
                while at < want {
                    let n = 64.min(want - at);
                    vs.render(&mut buf[..n * 2], 2, &src, &p);
                    out[at * 2..(at + n) * 2].copy_from_slice(&buf[..n * 2]);
                    at += n;
                }
                for (i, (a, b)) in reference.iter().zip(&out).enumerate() {
                    assert_eq!(
                        a.to_bits(),
                        b.to_bits(),
                        "ratio={ratio} link={link} stagger={stagger} sample {i}: {a} vs {b}"
                    );
                }
            }
        }
    }

    #[test]
    fn linking_the_channels_streams_too() {
        let src = chord(0.4, 2);
        let mut s = spec(3.0);
        s.vocoder.stereo_link = true;
        let offline = s.process(&src, 2, RATE);
        let live = streamed(&src, 2, &s, 256);
        let worst = offline.iter().zip(&live).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
        assert!(worst < 1e-6, "linked stereo differs by {worst:.2e}");
    }

    /// Every control on the vocoder's panel has to reach the streaming engine
    /// too, or the live sound and the exported one are different engines.
    #[test]
    fn the_vocoder_controls_reach_the_streaming_engine() {
        let src = chord(0.4, 2);
        let base = spec(3.0);
        let plain = streamed(&src, 2, &base, 256);
        let cases: Vec<(&str, Box<dyn Fn(&mut Stretch)>)> = vec![
            ("windowMs", Box::new(|s: &mut Stretch| s.vocoder.window_ms = 92.0)),
            ("phaseLock", Box::new(|s: &mut Stretch| s.vocoder.phase_lock = false)),
            ("freqTrust", Box::new(|s: &mut Stretch| s.vocoder.freq_trust = 0.2)),
            ("phaseSpread", Box::new(|s: &mut Stretch| s.vocoder.phase_spread = 0.0)),
            ("peakWidth", Box::new(|s: &mut Stretch| s.vocoder.peak_width = 12)),
            ("lockWidth", Box::new(|s: &mut Stretch| s.vocoder.lock_width = 3.0)),
            ("magFreeze", Box::new(|s: &mut Stretch| s.vocoder.mag_freeze = 0.9)),
            ("magBlur", Box::new(|s: &mut Stretch| s.vocoder.mag_blur = 0.8)),
            ("magGate", Box::new(|s: &mut Stretch| s.vocoder.mag_gate = 0.3)),
            ("stereoLink", Box::new(|s: &mut Stretch| s.vocoder.stereo_link = true)),
            ("overlap", Box::new(|s: &mut Stretch| s.grain.overlap = 4.0)),
            ("envelope", Box::new(|s: &mut Stretch| s.grain.envelope = 1.0)),
            ("scan", Box::new(|s: &mut Stretch| s.grain.scan = 0.5)),
            ("reverse", Box::new(|s: &mut Stretch| s.grain.reverse = true)),
        ];
        for (name, apply) in cases {
            let mut s = base;
            apply(&mut s);
            let d: f32 = plain
                .iter()
                .zip(&streamed(&src, 2, &s, 256))
                .map(|(a, b)| (a - b).abs())
                .sum::<f32>()
                / plain.len() as f32;
            assert!(d > 1e-6, "{name} did not reach the streaming vocoder");
        }
    }

    #[test]
    fn silence_streams_to_silence() {
        let out = streamed(&vec![0f32; 20_000], 1, &spec(3.0), 256);
        assert!(out.iter().all(|v| v.abs() < 1e-6));
    }

    #[test]
    fn the_level_survives_a_long_stretch() {
        let src = chord(0.5, 2);
        let out = streamed(&src, 2, &spec(8.0), 512);
        let (a, b) = (rms(&src), rms(&out[20_000..out.len() - 20_000]));
        assert!((b / a) > 0.5 && (b / a) < 2.0, "level moved by {:.2}x", b / a);
    }

    #[test]
    fn repeated_seeks_leave_zero_dirty_slots_in_vocoder() {
        let src = chord(1.0, 2);
        let in_frames = src.len() / 2;
        let mut v = VocoderStream::new(512, 2);
        let mut buf = vec![0.0f32; 128 * 2];
        for &(win_ms, link, at) in &[
            (40.0f32, true, 0u64),
            (90.0, false, 11_111),
            (20.0, true, 3_210),
            (40.0, false, 3_210),
        ] {
            let mut s = spec(1.5);
            s.vocoder.window_ms = win_ms;
            s.vocoder.stereo_link = link;
            let p = params(&s);
            v.seek(at, in_frames, &p);
            assert!(v.acc.iter().all(|&x| x == 0.0));
            assert!(v.norm.iter().all(|&x| x == 0.0));
            assert!(v.prev_phase.iter().all(|&x| x == 0.0));
            assert!(v.sum_phase.iter().all(|&x| x == 0.0));
            assert!(v.held.iter().all(|&x| x == 0.0));
            assert!(v.prev_ref.iter().all(|&x| x == 0.0));
            assert!(v.link_sum.iter().all(|&x| x == 0.0));
            for _ in 0..8 {
                v.render(&mut buf, 2, &src, &p);
            }
        }
        let s = spec(1.5);
        v.seek(0, in_frames, &params(&s));
        assert!(v.acc.iter().all(|&x| x == 0.0));
        assert!(v.norm.iter().all(|&x| x == 0.0));
        assert!(v.prev_phase.iter().all(|&x| x == 0.0));
        assert!(v.sum_phase.iter().all(|&x| x == 0.0));
        assert!(v.held.iter().all(|&x| x == 0.0));
        assert!(v.prev_ref.iter().all(|&x| x == 0.0));
        assert!(v.link_sum.iter().all(|&x| x == 0.0));
    }
}
