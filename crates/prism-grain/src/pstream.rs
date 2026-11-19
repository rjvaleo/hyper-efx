//! PVSOLA, a block at a time.
//!
//! The engine is the vocoder run in short bursts with the waveform put back
//! under it in between, so streaming it is streaming the vocoder plus one piece
//! of bookkeeping: a run has to be *ready* by the time the previous one reaches
//! its anchor, because the splice is chosen by correlating the two.
//!
//! The offline renderer produces a whole round at a time — anchor to anchor —
//! and there is no reason a callback cannot do the same, so it does. A round is
//! laid into a ring and handed out block by block; when the ring runs low the
//! next round is produced. The lookahead that costs is not the round but the
//! *run-up*: each run discards a window of output before the material it is
//! actually for, because those first frames are the vocoder's overlap-add
//! coming up to depth and splicing onto them puts in more disorder than the
//! drift being prevented.
//!
//! Two vocoder runs are held rather than one. Nothing carries across an anchor
//! — that is the entire point of the engine — so the second is not continuing
//! the first, it is starting fresh from the input's own phase while the first
//! is still sounding. They swap at each anchor rather than being rebuilt,
//! because rebuilding one in a callback is an allocation.

use crate::stream::{PitchRing, RingDirty, StretchParams};
use crate::pvsola::PvsolaParams;
use crate::vstream::VocoderStream;

/// The widest round the controls allow, in output frames: sixty-four anchor
/// frames of a quarter of the largest transform.
const MAX_SPAN: usize = 64 * (8192 / 4);
/// The widest run-up, search and fade, likewise.
const MAX_LEAD: usize = 8192 + 9600 + 8192;

pub struct PvsolaStream {
    /// Frames between the places the splice search tries: four, or fewer on
    /// a copy of the sound an octave or more up, whose frames are longer
    /// (`octave.rs`), so the search keeps the same step in time.
    stride: usize,
    /// The run currently sounding, and the one being prepared. They swap at
    /// every anchor; neither is ever rebuilt.
    a: VocoderStream,
    b: VocoderStream,
    /// One round of the run being prepared, before it is spliced in.
    piece: Vec<f32>,
    piece_frames: usize,
    /// The splice search's two sounds mixed to one channel, made once a
    /// search. Room for a whole round each, so the search never grows them.
    mono_want: Vec<f32>,
    mono_piece: Vec<f32>,
    /// How much of that round has been produced so far, and what it is for.
    ///
    /// A round is a whole vocoder run and takes far longer to make than one
    /// block takes to play — measured at 89% of the real-time budget in a
    /// single callback, which is a dropout on any machine with something else
    /// running. So it is produced a slice at a time, spread across the blocks
    /// the *previous* round is playing for, and spliced in when it is ready.
    prep: Option<Prep>,

    /// The plan the round in flight was started under.
    ///
    /// A run is defined from its anchor: `written` and `read` accumulate under
    /// one plan, and the splice reads back into what the previous round left.
    /// Recomputing the plan mid-round — which is what moving the re-anchor or
    /// the analysis window used to do — leaves the bookkeeping describing a
    /// round that was never made, and every splice after it lands in the wrong
    /// place. So a change waits for the next anchor, which is a boundary this
    /// engine already has and the only place it can safely take one.
    held: Option<Plan>,

    /// And the parameters it was started under.
    ///
    /// The plan is not the whole story: the two vocoder runs inside this take
    /// their own window from `StretchParams`, so deferring only the plan left
    /// the analysis window changing under a round that had already been
    /// measured for a different one. A run is defined from its anchor, and
    /// that has to mean all of it.
    held_p: Option<StretchParams>,

    /// Finished output waiting to be handed out.
    acc: Vec<f32>,
    ring: usize,
    channels: usize,
    dirty: RingDirty,

    /// Output frames laid into the ring.
    written: u64,
    /// Output frames handed out.
    emitted: u64,
    /// Input frame the next run is anchored at.
    read: usize,
    first: bool,
    ended: bool,
    /// Voice/layer phase offset (0..8) for staggering round prep and search.
    stagger: usize,
}

impl PvsolaStream {
    pub fn new(max_block: usize, channels: usize) -> Self {
        let channels = channels.max(1);
        // A round is laid down in one go, so the ring has to hold one plus
        // whatever the callback has not yet collected.
        let ring = max_block.max(1) + MAX_SPAN + MAX_LEAD + 1;
        PvsolaStream {
            stride: 4,
            a: VocoderStream::new(MAX_SPAN + MAX_LEAD, channels),
            b: VocoderStream::new(MAX_SPAN + MAX_LEAD, channels),
            piece: vec![0.0; (MAX_SPAN + MAX_LEAD) * channels],
            piece_frames: 0,
            mono_want: vec![0.0; MAX_SPAN + MAX_LEAD],
            mono_piece: vec![0.0; MAX_SPAN + MAX_LEAD],
            prep: None,
            held: None,
            held_p: None,
            acc: vec![0.0; ring * channels],
            ring,
            channels,
            dirty: RingDirty::default(),
            written: 0,
            emitted: 0,
            read: 0,
            first: true,
            ended: false,
            stagger: 0,
        }
    }

    pub fn set_stagger(&mut self, id: usize) {
        self.stagger = id & 7;
        self.a.set_stagger(id);
        self.b.set_stagger(id);
    }

    pub fn position(&self) -> u64 {
        self.emitted
    }

    /// Everything a round depends on, worked out once so the two halves of the
    /// engine cannot disagree about it.
    fn plan(p: &StretchParams, pv: &PvsolaParams) -> Plan {
        let sr = p.sample_rate.max(1) as f32;
        let n = crate::stretch::fft_size_for(p.vocoder.window_ms, p.sample_rate);
        let hop = (n / 4).max(1);
        let anchors = pv.anchor_frames.clamp(1, 64) as usize;
        let out_span = anchors * hop;
        // Scan is the sweep's speed through the source, so a round covers that
        // much more or less of it. The vocoder inside it reads Scan itself.
        let scan = p.grain.scan.clamp(-4.0, 4.0);
        let ratio = if scan.abs() == 1.0 { p.ratio } else { p.ratio / scan.abs().max(1e-3) }.clamp(0.01, 100.0);
        Plan {
            n,
            ratio,
            out_span,
            in_span: ((out_span as f32) / ratio).round().max(if scan == 0.0 { 0.0 } else { 1.0 }) as usize,
            search: ((pv.search_ms.clamp(0.0, 200.0) / 1000.0) * sr) as usize,
            blend: ((pv.blend.clamp(0.0, 1.0) * n as f32) as usize).min(out_span),
            pre_out: n,
            pre_in: ((n as f32) / ratio).ceil() as usize,
            backward: scan < 0.0,
        }
    }

    pub fn seek(&mut self, out_frame: u64, input_frames: usize, p: &StretchParams, pv: &PvsolaParams) {
        let pl = Self::plan(p, pv);
        let ch = self.channels;
        self.dirty.clear(self.ring, |a, b| self.acc[a * ch..b * ch].fill(0.0));
        self.emitted = out_frame;
        // Land on a round boundary at or before the target. Anywhere else and
        // the first round would have to be produced partly, which is the one
        // thing this engine cannot do — a run is defined from its anchor.
        let round = out_frame / pl.out_span.max(1) as u64;
        self.written = round * pl.out_span as u64;
        let swept = (round as usize).saturating_mul(pl.in_span);
        // Backward, the first round is anchored at the end of the file.
        self.read = if pl.backward { input_frames.saturating_sub(swept) } else { swept };
        self.first = true;
        self.ended = false;
        self.piece_frames = 0;
        self.prep = None;
        // A seek is a fresh start, so it adopts whatever is set now.
        self.held = None;
        self.held_p = None;
    }

    /// Fill one block. Must not allocate.
    pub fn render(
        &mut self,
        out: &mut [f32],
        channels: usize,
        input: &[f32],
        p: &StretchParams,
        pv: &PvsolaParams,
    ) {
        let channels = channels.max(1).min(self.channels);
        let frames = out.len() / channels;
        out.fill(0.0);
        let in_frames = input.len() / channels;
        if frames == 0 || in_frames == 0 {
            return;
        }

        // The plan the round in flight was started under, or a new one if no
        // round is in flight. See `held`.
        let (pl, hp) = match (self.held, self.held_p) {
            (Some(pl), Some(hp)) if self.prep.is_some() => (pl, hp),
            _ => {
                let fresh = Self::plan(p, pv);
                self.held = Some(fresh);
                self.held_p = Some(*p);
                (fresh, *p)
            }
        };
        // Everything below runs under the round's own parameters, not under
        // whatever the slider is at this instant.
        let p = &hp;
        let need = self.emitted + frames as u64;

        // Steady-state spreading: while the previous round still covers this
        // block plus the crossfade margin, advance the next round's vocoder
        // frames and splice correlation in bounded per-block steps so no single
        // callback pays a whole FFT frame or correlation search.
        // On `self.first` (right after a seek), `self.written < need + blend`
        // enters the catch-up loop below immediately, leaving seek timing and
        // bits untouched.
        if !self.first && self.written >= need + pl.blend as u64 && !self.ended {
            let rem_frames = (self.written - (need + pl.blend as u64)) as usize;
            let blocks_left = (rem_frames / frames.max(1)) + 1;
            let round_blocks = (pl.out_span / frames.max(1)).max(1);
            self.prepare_step(input, channels, in_frames, p, &pl, blocks_left, round_blocks);
        }

        // A round fades *backwards* into the one before it, so a frame is not
        // finished when the write pointer reaches it — it is finished when the
        // write pointer is a fade-length past it. Handing one out any earlier
        // means the next round crossfades into a frame that has already gone,
        // and the sound then depends on how big a block the device asked for.
        while self.written < need + pl.blend as u64 && !self.ended {
            // Not ready yet — finish it now rather than hand out a gap. Only
            // reachable on the first round, or if the device suddenly asks for
            // far more than it has been.
            self.prepare(input, channels, in_frames, p, &pl, usize::MAX);
            if self.ended {
                break;
            }
            self.splice(channels, in_frames, &pl);
        }

        let stride = self.channels;
        let mut done = 0usize;
        let mut slot = (self.emitted % self.ring as u64) as usize;
        while done < frames {
            let run = (frames - done).min(self.ring - slot);
            let acc = &mut self.acc[slot * stride..(slot + run) * stride];
            let dst = &mut out[done * channels..(done + run) * channels];
            if channels == stride {
                dst.copy_from_slice(acc);
                acc.fill(0.0);
            } else {
                for (a, o) in acc
                    .chunks_exact_mut(stride)
                    .zip(dst.chunks_exact_mut(channels))
                {
                    o.copy_from_slice(&a[..channels]);
                    a[..channels].fill(0.0);
                }
            }
            done += run;
            slot = 0;
        }
        if channels == self.channels {
            self.dirty.cleared_prefix(self.emitted, need, self.ring);
        }
        self.emitted = need;
    }

    /// Ensure `self.prep` has been initialised for the next round.
    fn ensure_prep(
        &mut self,
        channels: usize,
        in_frames: usize,
        p: &StretchParams,
        pl: &Plan,
    ) {
        if self.ended || self.prep.is_some() {
            return;
        }
        let want = pl.piece_len().min(self.piece.len() / channels);
        // A backward run reads down from the top of its segment, so its
        // run-up is above the anchor rather than below it.
        let (from, lead) = if pl.backward {
            let top = (self.read + pl.pre_in + pl.n).min(in_frames);
            let lead = top.saturating_sub(self.read + pl.n);
            let used = pl.n + ((want as f32 / pl.ratio).ceil() as usize);
            (top.saturating_sub(used), lead)
        } else {
            // Back up for the run-up, taking whatever is there.
            let from = self.read.saturating_sub(pl.pre_in);
            (from, self.read - from)
        };
        let at = if lead == pl.pre_in {
            pl.pre_out
        } else {
            ((lead as f32) * pl.ratio).round() as usize
        };

        // The segment is bounded exactly as the offline renderer bounds it.
        // Handing the vocoder the rest of the file instead would give it a
        // different idea of where the end is, and its wrap and stop
        // conditions are measured against that — so the runs would diverge
        // near the end of every file, which is the one place nobody listens.
        let need_out = pl.out_span + pl.blend + pl.search;
        let need_in = ((need_out as f32) / pl.ratio).ceil() as usize + pl.n;
        let take = if pl.backward {
            // Everything from the bottom of what the run will read to the
            // top it starts at.
            (self.read + lead + pl.n).min(in_frames).saturating_sub(from)
        } else {
            (lead + need_in).min(in_frames.saturating_sub(from))
        };
        if take < pl.n || at >= want {
            self.ended = true;
            return;
        }

        // The run being prepared starts fresh from the input's own phase.
        // That is the engine: nothing carries across an anchor, so nothing
        // has time to drift.
        self.b.seek(0, take, p);
        self.prep = Some(Prep {
            from,
            take,
            at,
            want,
            done: 0,
            started: false,
            search: None,
            best_off: None,
        });
    }

    /// Advance the next round's vocoder preparation or splice correlation by
    /// one bounded per-block slice, paced across `blocks_left` blocks.
    #[allow(clippy::too_many_arguments)]
    fn prepare_step(
        &mut self,
        input: &[f32],
        channels: usize,
        in_frames: usize,
        p: &StretchParams,
        pl: &Plan,
        blocks_left: usize,
        round_blocks: usize,
    ) {
        self.ensure_prep(channels, in_frames, p, pl);
        let Some(mut pr) = self.prep.take() else { return };

        if pr.done < pr.want {
            // Reserve a fixed tail of blocks at the end of the round for the
            // incremental splice correlation search (computed from the round's
            // total length so it does not shrink as `blocks_left` counts down).
            let search_blocks = if pl.search > 0 {
                (16 + (self.stagger % 4) * 2).min(round_blocks / 4)
            } else {
                0
            };
            let prep_blocks = blocks_left.saturating_sub(search_blocks).max(1);
            let rem_work = self.b.remaining_work_for(pr.want, channels, p);
            let mut budget = if prep_blocks == 1 {
                // Final prep block before search begins: finish any rounding
                // residual cleanly so search gets its full reserved window.
                rem_work.saturating_add(rem_work / 4 + 4096)
            } else {
                rem_work.div_ceil(prep_blocks).max(1)
            };
            if !pr.started && self.stagger > 0 && prep_blocks > 1 {
                budget = ((budget * (8 - self.stagger)) / 8).max(1);
            }
            pr.started = true;
            let seg = &input[pr.from * channels..(pr.from + pr.take) * channels];
            let got = self.b.render_budgeted(
                &mut self.piece[pr.done * channels..pr.want * channels],
                channels,
                seg,
                p,
                budget,
            );
            pr.done += got;
            self.prep = Some(pr);
            return;
        }

        // `self.piece` is complete; spread the splice correlation search across
        // the remaining blocks before `splice()` runs.
        if pr.best_off.is_none() {
            self.piece_frames = pr.want;
            let join = self.written.saturating_sub(pl.blend as u64);
            let span = pl.blend.max(1);
            if pr.search.is_none() {
                match self.init_search(channels, join, span, pr.at, pl.search) {
                    Ok(job) => pr.search = Some(job),
                    Err(done_off) => pr.best_off = Some(done_off),
                }
            }
            if let Some(mut sj) = pr.search {
                let rem_offsets = ((sj.hi - sj.off) / self.stride) + 1;
                let step = rem_offsets.div_ceil(blocks_left).max(1);
                if let Some(best) = self.step_search(&mut sj, step) {
                    pr.best_off = Some(best);
                    pr.search = None;
                } else {
                    pr.search = Some(sj);
                }
            }
        }
        self.prep = Some(pr);
    }

    /// Make up to `quota` more frames of the round being prepared, starting a
    /// new one if there is none.
    fn prepare(
        &mut self,
        input: &[f32],
        channels: usize,
        in_frames: usize,
        p: &StretchParams,
        pl: &Plan,
        quota: usize,
    ) {
        self.ensure_prep(channels, in_frames, p, pl);
        let Some(mut pr) = self.prep.take() else { return };
        let step = quota.min(pr.want - pr.done);
        if step > 0 {
            let seg = &input[pr.from * channels..(pr.from + pr.take) * channels];
            let to = (pr.done + step) * channels;
            self.b
                .render(&mut self.piece[pr.done * channels..to], channels, seg, p);
            pr.done += step;
        }
        self.prep = Some(pr);
    }

    /// Splice the prepared round onto what is written.
    fn splice(&mut self, channels: usize, _in_frames: usize, pl: &Plan) {
        let Some(mut pr) = self.prep.take() else {
            self.ended = true;
            return;
        };
        let at = pr.at;
        let want = pr.want;
        self.piece_frames = want;

        if self.first {
            let len = (want - at).min(pl.out_span);
            let stride = self.channels;
            let mut done = 0usize;
            let mut slot = (self.written % self.ring as u64) as usize;
            while done < len {
                let run = (len - done).min(self.ring - slot);
                let acc = &mut self.acc[slot * stride..(slot + run) * stride];
                let src = &self.piece[(at + done) * channels..(at + done + run) * channels];
                if channels == stride {
                    acc.copy_from_slice(src);
                } else {
                    for (a, s) in acc
                        .chunks_exact_mut(stride)
                        .zip(src.chunks_exact(channels))
                    {
                        a[..channels].copy_from_slice(s);
                    }
                }
                done += run;
                slot = 0;
            }
            self.dirty.mark(self.written, self.written + len as u64);
            self.written += len as u64;
            self.first = false;
        } else {
            // Where this run should join. The search moves it either side of
            // the anchor to whichever offset best continues what is already
            // written — the same normalised correlation WSOLA uses, and the
            // reason the joins are not audible as joins.
            let join = self.written.saturating_sub(pl.blend as u64);
            let off = if let Some(best) = pr.best_off {
                best
            } else if let Some(mut sj) = pr.search.take() {
                self.step_search(&mut sj, usize::MAX).unwrap_or(sj.best)
            } else {
                self.best_offset(channels, join, pl.blend.max(1), at, pl.search)
            };
            let len = (want - off).min(pl.out_span + pl.blend);
            let stride = self.channels;
            let mut done = 0usize;
            let mut slot = (join % self.ring as u64) as usize;
            while done < len {
                let run = (len - done).min(self.ring - slot);
                let acc = &mut self.acc[slot * stride..(slot + run) * stride];
                let src = &self.piece[(off + done) * channels..(off + done + run) * channels];
                for (k, (a, s)) in acc
                    .chunks_exact_mut(stride)
                    .zip(src.chunks_exact(channels))
                    .enumerate()
                {
                    let i = done + k;
                    // Linear, not equal power. Equal power is right for two signals
                    // that are unrelated, and the search has just spent its whole
                    // effort making these two agree.
                    let w = if pl.blend > 0 && i < pl.blend {
                        (i as f32 + 0.5) / pl.blend as f32
                    } else {
                        1.0
                    };
                    for ch in 0..channels {
                        let old = if i < pl.blend { a[ch] * (1.0 - w) } else { 0.0 };
                        a[ch] = old + s[ch] * w;
                    }
                }
                done += run;
                slot = 0;
            }
            self.dirty.mark(join, join + len as u64);
            self.written = join + len as u64;
        }

        if pl.backward {
            if self.read == 0 {
                self.ended = true;
            }
            self.read = self.read.saturating_sub(pl.in_span);
        } else {
            self.read += pl.in_span;
            // Checked here rather than on the way in, which is where the offline
            // renderer checks it. The difference is one round's worth of output at
            // the very end of a file — 0.4% of the frames, and every one of them
            // wrong, which is exactly the kind of thing that never shows up until
            // someone stretches something short.
            if self.read + pl.n >= _in_frames {
                self.ended = true;
            }
        }
        std::mem::swap(&mut self.a, &mut self.b);
    }

    /// Prepare the mono mix buffers and energy for an incremental splice search.
    /// Returns `Err(offset)` if no search loop is needed, or `Ok(SearchJob)`.
    fn init_search(
        &mut self,
        channels: usize,
        join: u64,
        span: usize,
        at: usize,
        search: usize,
    ) -> Result<SearchJob, usize> {
        if span == 0 || self.piece_frames <= span + at {
            return Err(at.min(self.piece_frames.saturating_sub(1)));
        }
        if search == 0 {
            return Err(at);
        }
        let lo = at.saturating_sub(search);
        let hi = (at + search).min(self.piece_frames - span - 1);
        if hi <= lo {
            return Err(at.min(hi));
        }

        let fits = span <= self.mono_want.len() && hi + span <= self.mono_piece.len();
        if !fits {
            return Err(at);
        }

        let stride = self.channels;
        let mut done = 0usize;
        let mut slot = (join % self.ring as u64) as usize;
        while done < span {
            let run = (span - done).min(self.ring - slot);
            let acc = &self.acc[slot * stride..(slot + run) * stride];
            for (dst, a) in self.mono_want[done..done + run]
                .iter_mut()
                .zip(acc.chunks_exact(stride))
            {
                let mut sum = 0.0;
                for &v in &a[..channels] {
                    sum += v;
                }
                *dst = sum / channels as f32;
            }
            done += run;
            slot = 0;
        }
        for f in lo..hi + span {
            let mut acc = 0.0;
            for ch in 0..channels {
                acc += self.piece[f * channels + ch];
            }
            self.mono_piece[f] = acc / channels as f32;
        }
        let mut want_energy = 0f32;
        for &v in &self.mono_want[..span] {
            want_energy += v * v;
        }
        let want_energy = want_energy.sqrt();
        if want_energy < 1e-9 {
            return Err(at);
        }
        Ok(SearchJob {
            span,
            hi,
            off: lo,
            want_energy,
            best: at,
            best_score: f32::NEG_INFINITY,
        })
    }

    /// Evaluate up to `max_offsets` candidate offsets in an incremental search.
    /// Returns `Some(best)` once `sj.off > sj.hi`.
    fn step_search(&self, sj: &mut SearchJob, max_offsets: usize) -> Option<usize> {
        let want = &self.mono_want[..sj.span];
        let mono = &self.mono_piece;
        let mut done = 0usize;
        while sj.off <= sj.hi && done < max_offsets {
            let mut dot = 0f32;
            let mut energy = 0f32;
            for (w, v) in want.iter().zip(&mono[sj.off..sj.off + sj.span]) {
                dot += w * v;
                energy += v * v;
            }
            let score = dot / (sj.want_energy * energy.sqrt() + 1e-9);
            if score > sj.best_score {
                sj.best_score = score;
                sj.best = sj.off;
            }
            sj.off += self.stride;
            done += 1;
        }
        if sj.off > sj.hi {
            Some(sj.best)
        } else {
            None
        }
    }

    /// Which offset into the prepared run best continues what is written.
    ///
    /// Mixed to mono first: the alignment is a property of the moment, not of
    /// one channel, and searching per channel would let a stereo pair land at
    /// two different offsets and smear the image.
    fn best_offset(
        &mut self,
        channels: usize,
        join: u64,
        span: usize,
        at: usize,
        search: usize,
    ) -> usize {
        match self.init_search(channels, join, span, at, search) {
            Err(off) => off,
            Ok(mut sj) => self.step_search(&mut sj, usize::MAX).unwrap_or(sj.best),
        }
    }

    /// The source is a copy `factor` times shorter (`octave.rs`).
    pub fn set_octave(&mut self, factor: u32) {
        self.stride = (4 / factor.max(1) as usize).max(1);
    }
}

/// Incremental state for the splice correlation search.
#[derive(Clone, Copy)]
struct SearchJob {
    span: usize,
    hi: usize,
    off: usize,
    want_energy: f32,
    best: usize,
    best_score: f32,
}

/// A round being made, a slice at a time.
struct Prep {
    from: usize,
    take: usize,
    at: usize,
    want: usize,
    done: usize,
    started: bool,
    search: Option<SearchJob>,
    best_off: Option<usize>,
}

#[derive(Clone, Copy)]
struct Plan {
    n: usize,
    ratio: f32,
    out_span: usize,
    in_span: usize,
    search: usize,
    blend: usize,
    pre_out: usize,
    pre_in: usize,
    /// Scan is negative: each round is anchored below the last.
    backward: bool,
}

impl Plan {
    /// How much output one run has to make: the run-up to throw away, room for
    /// the search to move, the fade, and the round itself.
    fn piece_len(&self) -> usize {
        self.pre_out + self.search + self.blend + self.out_span
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stretch::{Algorithm, Stretch};

    const RATE: u32 = 44_100;

    fn chord(secs: f32, channels: usize) -> Vec<f32> {
        let n = (RATE as f32 * secs) as usize;
        let mut mono = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f32 / RATE as f32;
            mono.push(
                0.3 * (std::f32::consts::TAU * 220.0 * t).sin()
                    + 0.25 * (std::f32::consts::TAU * 277.2 * t).sin()
                    + 0.2 * (std::f32::consts::TAU * 329.6 * t).sin(),
            );
        }
        let mut v = vec![0f32; n * channels];
        for i in 0..n {
            v[i * channels] = mono[i];
            for c in 1..channels {
                v[i * channels + c] = if i >= 977 { mono[i - 977] } else { 0.0 };
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
        let mut s = PvsolaStream::new(block, channels);
        s.seek(0, in_frames, &p, &spec.pvsola);
        let mut out = vec![0f32; want * channels];
        let mut at = 0usize;
        let mut buf = vec![0f32; block * channels];
        while at < want {
            let n = block.min(want - at);
            s.render(&mut buf[..n * channels], channels, input, &p, &spec.pvsola);
            out[at * channels..(at + n) * channels].copy_from_slice(&buf[..n * channels]);
            at += n;
        }
        out
    }

    fn spec(ratio: f32) -> Stretch {
        Stretch { ratio, algorithm: Algorithm::Pvsola, ..Default::default() }
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len().max(1) as f32).sqrt()
    }

    #[test]
    fn streaming_matches_the_offline_pvsola() {
        let src = chord(1.0, 2);
        for ratio in [2.0f32, 4.0] {
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
        let src = chord(0.8, 2);
        let s = spec(3.0);
        let a = streamed(&src, 2, &s, 64);
        let b = streamed(&src, 2, &s, 2048);
        for (i, (x, y)) in a.iter().zip(&b).enumerate() {
            assert_eq!(x.to_bits(), y.to_bits(), "bit mismatch at sample {i}: {x} vs {y}");
        }
    }

    #[test]
    fn spread_pvsola_and_stagger_are_bit_identical_when_params_hold_still() {
        let src = chord(1.0, 2);
        let s = spec(2.0);
        let offline = s.process(&src, 2, RATE);
        let p = params(&s);
        let in_frames = src.len() / 2;
        let want = offline.len() / 2;
        for stagger in [0usize, 2, 5] {
            let mut pv = PvsolaStream::new(64, 2);
            pv.set_stagger(stagger);
            pv.seek(0, in_frames, &p, &s.pvsola);
            let mut out = vec![0f32; want * 2];
            let mut at = 0usize;
            let mut buf = vec![0f32; 64 * 2];
            while at < want {
                let n = 64.min(want - at);
                pv.render(&mut buf[..n * 2], 2, &src, &p, &s.pvsola);
                out[at * 2..(at + n) * 2].copy_from_slice(&buf[..n * 2]);
                at += n;
            }
            for (i, (a, b)) in offline.iter().zip(&out).enumerate() {
                assert_eq!(
                    a.to_bits(),
                    b.to_bits(),
                    "pvsola stagger={stagger} sample {i}: {a} vs {b}"
                );
            }
        }
    }

    /// Render while turning a control, the way a hand on a slider does.
    fn streamed_while_turning(
        input: &[f32],
        channels: usize,
        spec: &Stretch,
        block: usize,
        mut turn: impl FnMut(&mut Stretch, usize),
    ) -> Vec<f32> {
        let in_frames = input.len() / channels;
        let want = ((in_frames as f64) * spec.ratio as f64).round() as usize;
        let mut s = PvsolaStream::new(block, channels);
        let mut live = *spec;
        s.seek(0, in_frames, &params(&live), &live.pvsola);
        let mut out = vec![0f32; want * channels];
        let mut at = 0usize;
        let mut buf = vec![0f32; block * channels];
        let mut n_block = 0usize;
        while at < want {
            turn(&mut live, n_block);
            n_block += 1;
            let n = block.min(want - at);
            s.render(&mut buf[..n * channels], channels, input, &params(&live), &live.pvsola);
            out[at * channels..(at + n) * channels].copy_from_slice(&buf[..n * channels]);
            at += n;
        }
        out
    }

    /// Sharpest corner — the second difference, which tells a discontinuity
    /// from an honest slope. See `engine::transport`, where the same measure
    /// is used on the rack's controls.
    fn worst_corner(v: &[f32], channels: usize) -> f32 {
        (0..v.len() / channels)
            .skip(1)
            .take(v.len() / channels - 2)
            .map(|f| {
                let a = v[(f - 1) * channels];
                let b = v[f * channels];
                let c = v[(f + 1) * channels];
                (c - 2.0 * b + a).abs()
            })
            .fold(0f32, f32::max)
    }

    /// Moving the re-anchor or the analysis window used to glitch, because the
    /// plan was recomputed on every block while `written` and `read` had been
    /// accumulated under the old one — so the splice landed somewhere the
    /// previous round had never written. A change now waits for the next
    /// anchor, which is the only boundary this engine can take one at.
    #[test]
    fn turning_the_anchor_or_the_window_does_not_glitch() {
        let src = chord(1.5, 2);
        let base = spec(2.0);
        let steady = worst_corner(&streamed(&src, 2, &base, 256), 2);

        // The re-anchor, swept across its whole range while playing.
        let anchored = streamed_while_turning(&src, 2, &base, 256, |s, i| {
            s.pvsola.anchor_frames = 2 + (i as u32 * 3) % 30;
        });
        let got = worst_corner(&anchored, 2);
        assert!(
            got < steady * 4.0,
            "sweeping the re-anchor put a corner of {got:.4} in against a steady {steady:.4}"
        );

        // And the analysis window, likewise.
        let windowed = streamed_while_turning(&src, 2, &base, 256, |s, i| {
            s.vocoder.window_ms = 20.0 + ((i * 7) % 80) as f32;
        });
        let got = worst_corner(&windowed, 2);
        assert!(
            got < steady * 4.0,
            "sweeping the window put a corner of {got:.4} in against a steady {steady:.4}"
        );
    }

    #[test]
    fn the_anchor_controls_reach_the_streaming_engine() {
        let src = chord(0.8, 2);
        let base = spec(3.0);
        let plain = streamed(&src, 2, &base, 512);
        let cases: Vec<(&str, Box<dyn Fn(&mut Stretch)>)> = vec![
            ("anchorFrames", Box::new(|s: &mut Stretch| s.pvsola.anchor_frames = 20)),
            ("searchMs", Box::new(|s: &mut Stretch| s.pvsola.search_ms = 0.0)),
            ("blend", Box::new(|s: &mut Stretch| s.pvsola.blend = 0.0)),
            ("windowMs", Box::new(|s: &mut Stretch| s.vocoder.window_ms = 92.0)),
            ("phaseLock", Box::new(|s: &mut Stretch| s.vocoder.phase_lock = false)),
            ("magBlur", Box::new(|s: &mut Stretch| s.vocoder.mag_blur = 0.8)),
        ];
        for (name, apply) in cases {
            let mut s = base;
            apply(&mut s);
            let d: f32 = plain
                .iter()
                .zip(&streamed(&src, 2, &s, 512))
                .map(|(a, b)| (a - b).abs())
                .sum::<f32>()
                / plain.len() as f32;
            assert!(d > 1e-6, "{name} did not reach the streaming engine");
        }
    }

    #[test]
    fn silence_streams_to_silence() {
        let out = streamed(&vec![0f32; 60_000], 1, &spec(3.0), 512);
        assert!(out.iter().all(|v| v.abs() < 1e-6));
    }

    #[test]
    fn the_level_survives() {
        let src = chord(1.0, 2);
        let out = streamed(&src, 2, &spec(4.0), 512);
        let (a, b) = (rms(&src), rms(&out[20_000..out.len() - 20_000]));
        assert!((b / a) > 0.5 && (b / a) < 2.0, "level moved by {:.2}x", b / a);
    }

    #[test]
    fn repeated_seeks_leave_zero_dirty_slots_in_pvsola() {
        let src = chord(2.0, 2);
        let s = spec(1.5);
        let p = params(&s);
        let in_frames = src.len() / 2;
        let mut pv = PvsolaStream::new(512, 2);
        let mut buf = vec![0.0f32; 128 * 2];
        // Include mid-round seeks (where written < emitted) and consecutive seeks.
        for &at in &[0u64, 12_345, 37_000, 3_333, 3_333, 50_111] {
            pv.seek(at, in_frames, &p, &s.pvsola);
            assert!(pv.acc.iter().all(|&v| v == 0.0));
            for _ in 0..6 {
                pv.render(&mut buf, 2, &src, &p, &s.pvsola);
            }
        }
        pv.seek(0, in_frames, &p, &s.pvsola);
        assert!(pv.acc.iter().all(|&v| v == 0.0));
    }
}

/// PVSOLA with the pitch stage on the end of it.
///
/// PVSOLA is built *out of* the vocoder and WSOLA rather than beside them, so
/// it takes parameters of its own and cannot be a [`Streamer`](crate::stream::Streamer)
/// — which is how it came to be the one engine with no pitch at all on the
/// audio thread. The control moved the exported file and did nothing to what
/// came out of the speakers.
///
/// It drives the same [`PitchRing`] the others do. Two resamplers would be two
/// different sounds.
pub struct PitchedPvsola {
    inner: PvsolaStream,
    ring: PitchRing,
    scratch: Vec<f32>,
}

impl PitchedPvsola {
    pub fn new(max_block: usize, channels: usize) -> Self {
        let channels = channels.max(1);
        PitchedPvsola {
            inner: PvsolaStream::new(max_block, channels),
            ring: PitchRing::new(max_block, channels),
            scratch: vec![0.0; max_block.max(1) * channels],
        }
    }

    pub fn position(&self) -> u64 {
        self.ring.position()
    }

    pub fn set_stagger(&mut self, id: usize) {
        self.inner.set_stagger(id);
    }

    pub fn render_pitched(
        &mut self,
        out: &mut [f32],
        channels: usize,
        input: &[f32],
        p: &StretchParams,
        pv: &PvsolaParams,
        semitones: f32,
    ) {
        self.render_octaves(out, channels, input, None, p, pv, semitones);
    }

    /// The same, on a copy of the source an octave or more up above C8
    /// (`octave.rs`).
    #[allow(clippy::too_many_arguments)]
    pub fn render_octaves(
        &mut self,
        out: &mut [f32],
        channels: usize,
        input: &[f32],
        octaves: Option<&crate::octave::Octaves>,
        p: &StretchParams,
        pv: &PvsolaParams,
        semitones: f32,
    ) {
        let pitch = PitchRing::factor(semitones);
        if self.ring.bypassed(pitch) {
            // Already at the right rate; do not pay for a trip through the ring.
            let frames = out.len() / channels.max(1);
            self.inner.render(out, channels, input, p, pv);
            self.ring.advance_unpitched(frames);
            return;
        }
        let oct = octaves.filter(|o| o.is_for(input));
        let have = oct.map_or(0, |o| o.levels.len() as u32);
        if let Some(level) = self.ring.next_level(pitch, have) {
            let frames = crate::octave::frames_at(input.len() / channels.max(1), level);
            let at = self.ring.change_level(pitch, level);
            self.inner.set_octave(crate::octave::factor(level));
            self.inner.seek(at, frames, &PitchRing::inner_params_at(p, pitch, level), pv);
        }
        let level = self.ring.level();
        let src = match oct {
            Some(o) => o.source(input, level),
            None => input,
        };
        let inner = PitchRing::inner_params_at(p, pitch, level);
        let rate = self.ring.rate(pitch);
        let engine = &mut self.inner;
        self.ring.pull(out, channels, rate, &mut self.scratch, |buf| engine.render(buf, channels, src, &inner, pv));
    }

    pub fn take_wants(&mut self) -> bool {
        self.ring.take_wants()
    }

    pub fn seek(
        &mut self,
        out_frame: u64,
        input_frames: usize,
        p: &StretchParams,
        pv: &PvsolaParams,
        semitones: f32,
    ) {
        self.seek_octaves(out_frame, input_frames, None, p, pv, semitones);
    }

    pub fn seek_octaves(
        &mut self,
        out_frame: u64,
        input_frames: usize,
        octaves: Option<&crate::octave::Octaves>,
        p: &StretchParams,
        pv: &PvsolaParams,
        semitones: f32,
    ) {
        let pitch = PitchRing::factor(semitones);
        let have = octaves.filter(|o| o.is_length(input_frames)).map_or(0, |o| o.levels.len() as u32);
        let level = self.ring.start_level(pitch, have);
        let at = self.ring.seek_level(out_frame, pitch, level);
        self.inner.set_octave(crate::octave::factor(level));
        self.inner.seek(
            at,
            crate::octave::frames_at(input_frames, level),
            &PitchRing::inner_params_at(p, pitch, level),
            pv,
        );
    }
}
