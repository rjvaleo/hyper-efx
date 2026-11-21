//! Stretching a block at a time, for the audio callback.
//!
//! The engines in this crate were written as whole-buffer renders: hand them a
//! file, get a file back. That is the wrong shape for playback. An audio
//! callback is handed a few hundred frames, must return before the device wants
//! them, and must not allocate on the way — so an engine that needs the whole
//! input before it can produce the first sample can only ever be heard by
//! rendering it first and playing the result.
//!
//! A streaming engine keeps its own state between blocks instead. WSOLA's is
//! small and obvious: where it read from last, where it is writing to, and the
//! stretch of waveform it expects to follow what it just laid down. Everything
//! else it needs is either in the source or in the parameters.
//!
//! Two rules shape everything here, and both are load-bearing.
//!
//! **No allocation once built.** Every buffer is sized at construction from the
//! widest settings the controls allow, not from the current ones, because the
//! current ones can change between two blocks and a resize in the callback is a
//! dropout. Anything that genuinely must allocate — the transient map, which is
//! computed from the whole file — is built on the caller's thread and handed
//! over, the same way the rack is.
//!
//! **The offline render drives the same streamer.** `stretch::wsola` is now a
//! loop over this, so "what you hear is what you export" is true by
//! construction rather than by a test that has to be remembered. If the two
//! ever disagree it is a bug in one caller, not a difference between two
//! algorithms.

use crate::stretch::{VocoderParams, WinShape, WsolaParams};
use crate::Grain;

/// The longest window any control allows, in milliseconds. Buffers are sized
/// from this rather than from the current setting, because the setting can move
/// between blocks.
const MAX_WINDOW_MS: f32 = 2000.0;

/// Half-open frame interval `[lo, hi)` of ring slots that may hold non-zero
/// data since the last clear.
///
/// Every ring begins all zeros. Writes extend `[lo, hi)`; an emit loop that
/// zeroes consecutive slots from the start of the dirty span without wrap
/// aliasing advances `lo` via [`Self::cleared_prefix`]. On a seek, [`Self::clear`]
/// zeroes only `[lo, hi)` (at most two contiguous slices, or the whole ring if
/// `hi - lo >= ring`) and resets the span to empty.
#[derive(Clone, Copy, Default)]
pub(crate) struct RingDirty {
    lo: u64,
    hi: u64,
}

impl RingDirty {
    #[inline]
    pub(crate) fn mark(&mut self, start: u64, end: u64) {
        if start >= end {
            return;
        }
        if self.lo == self.hi {
            self.lo = start;
            self.hi = end;
        } else {
            self.lo = self.lo.min(start);
            self.hi = self.hi.max(end);
        }
    }

    #[inline]
    pub(crate) fn cleared_prefix(&mut self, from: u64, to: u64, ring: usize) {
        if self.lo >= self.hi || from >= to {
            return;
        }
        if from <= self.lo && to > self.lo && self.hi - self.lo <= ring as u64 {
            self.lo = to.min(self.hi);
            if self.lo == self.hi {
                self.lo = 0;
                self.hi = 0;
            }
        }
    }

    #[inline]
    pub(crate) fn clear(&mut self, ring: usize, mut zero: impl FnMut(usize, usize)) {
        if self.lo >= self.hi || ring == 0 {
            self.lo = 0;
            self.hi = 0;
            return;
        }
        let span = self.hi - self.lo;
        if span >= ring as u64 {
            zero(0, ring);
        } else {
            let count = span as usize;
            let start = (self.lo % ring as u64) as usize;
            let first = count.min(ring - start);
            zero(start, start + first);
            if first < count {
                zero(0, count - first);
            }
        }
        self.lo = 0;
        self.hi = 0;
    }
}

/// Everything a streaming engine needs that is not the audio itself.
///
/// Copied into the callback each block, so it must stay cheap to copy — which
/// is why the transient map is not in here but held by the streamer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StretchParams {
    pub ratio: f32,
    pub window_ms: f32,
    pub sample_rate: u32,
    pub wsola: WsolaParams,
    pub vocoder: VocoderParams,
    pub grain: Grain,
    /// SoundHack's time scale: which version it behaves like.
    pub soundhack: crate::soundhack::SoundHackParams,
}

/// How a block-at-a-time engine is driven.
///
/// Deliberately the same shape as `engine::BlockRenderer`, which is the grain
/// cloud's version of exactly this — one of these can stand in for it.
pub trait Streamer: Send {
    /// Fill `out` (interleaved, `channels` wide) with the next block, reading
    /// from `input`. Must not allocate.
    fn render(&mut self, out: &mut [f32], channels: usize, input: &[f32], p: &StretchParams);

    /// Move to an output frame. Anything in flight belongs to where we were.
    fn seek(&mut self, out_frame: u64, input_frames: usize, p: &StretchParams);

    /// The source is `factor` times shorter than the one any transient map
    /// was built on (a copy an octave or more up, `octave.rs`): read the map
    /// that much further along, and divide what it says by as much.
    fn set_octave(&mut self, _factor: f64) {}

    /// Output frames produced so far.
    fn position(&self) -> u64;

    /// Read further on (back, if negative), keeping everything the engine
    /// holds: what a loop does when it goes round, in place of a seek, which
    /// starts the engine over (`Stretcher::wrap`). The distance comes in both
    /// forms an engine may keep its place in: `nominal`, frames of the sound
    /// before Scan (where an anchor works), and `swept`, after it (where a read
    /// that already moves at Scan's speed works). Nothing is allocated, and the
    /// output carries on unbroken.
    fn shift_read(&mut self, nominal: f64, swept: f64);
}

/// Waveform-similarity overlap-add, a block at a time.
///
/// The overlap-add is the only awkward part. A window laid down at the current
/// write position runs on past the end of this block, so the sum cannot be
/// divided by its window total until every window that touches a frame has been
/// laid down. Frames are therefore accumulated into a ring long enough to hold
/// the widest window, and only handed out once the write pointer has passed
/// them — at which point nothing further can contribute.
pub struct WsolaStream {
    /// Accumulated output, interleaved, not yet normalised.
    acc: Vec<f32>,
    /// Summed window height per frame, alongside `acc`.
    norm: Vec<f32>,
    /// Frames in the ring.
    ring: usize,
    channels: usize,
    dirty: RingDirty,

    /// The segment expected to follow what was just written; the next window is
    /// chosen to resemble it.
    expect: Vec<f32>,
    /// Precomputed window, when nothing varies its length.
    window: Vec<f32>,
    /// The shape `window` was built for, so it is rebuilt only when it changes.
    window_for: Option<(usize, WinShape, u32)>,

    /// Where the next window reads from.
    read: usize,
    /// Output frame the next window is laid at.
    write: u64,
    /// Output frames already handed out.
    emitted: u64,
    /// Grain index, which is what all the randomness is addressed by.
    index: u64,
    /// How far along its wander the pitch drift has got.
    drift: crate::grain::DriftClock,
    first: bool,

    /// Where each output instant comes from, when transients are being held at
    /// their original rate. `None` is the ordinary case and means a straight
    /// line, which is arithmetic rather than a lookup — worth the branch
    /// because the alternative is rebuilding a map every time the ratio moves,
    /// and the ratio moves under the pointer.
    ///
    /// Built off the audio thread when it is needed at all, because deriving it
    /// runs an onset detector over the whole file and allocates.
    map: Option<std::sync::Arc<crate::transient::TimeMap>>,
    /// How far along the map a frame of this stream's timeline is. One, except
    /// on a voice with its own pitch: see `Stretcher::set_map_pitch`.
    map_scale: f64,
    /// How many times shorter the source is than the one the map was built
    /// on: one, except above C8 (`octave.rs`).
    map_octave: f64,
    /// Keeps the read continuous when the ratio moves. See `anchor.rs`.
    anchor: crate::anchor::Anchor,
    search_job: Option<PendingSearch>,
    #[cfg(test)]
    immediate_search: bool,
    /// Dropped because the ring could not hold the window. Surfaced rather than
    /// silently degrading, exactly as the voice pool's overflow is.
    pub overflows: u64,
}

struct PendingSearch {
    key: (usize, usize, usize, usize, usize, usize, WsolaParams),
    sweep: crate::stretch::OffsetSearch,
}

impl WsolaStream {
    /// Build one sized for the widest settings the controls allow.
    ///
    /// `max_block` is the largest block the device may ask for. Everything is
    /// allocated here and nothing after.
    pub fn new(max_block: usize, channels: usize, sample_rate: u32) -> Self {
        let channels = channels.max(1);
        let max_win = (((MAX_WINDOW_MS / 1000.0) * sample_rate.max(1) as f32) as usize).max(64);
        // A window laid at the far end of a block still has to fit, so the ring
        // holds a block and a window and one spare frame to keep the write
        // pointer from ever meeting the read pointer exactly.
        let ring = max_block.max(1) + max_win + 1;
        WsolaStream {
            acc: vec![0.0; ring * channels],
            norm: vec![0.0; ring],
            ring,
            channels,
            dirty: RingDirty::default(),
            expect: vec![0.0; max_win * channels],
            window: Vec::with_capacity(max_win),
            window_for: None,
            read: 0,
            write: 0,
            emitted: 0,
            index: 0,
            drift: crate::grain::DriftClock::default(),
            first: true,
            map: None,
            map_scale: 1.0,
            map_octave: 1.0,
            anchor: crate::anchor::Anchor::default(),
            search_job: None,
            #[cfg(test)]
            immediate_search: false,
            overflows: 0,
        }
    }

    /// Hand over a freshly built transient map, or `None` for a straight line.
    ///
    /// Separate from `render` because building one walks the whole file. The
    /// caller does it on its own thread and leaves the result here, which is
    /// the same arrangement the rack uses for the same reason.
    pub fn set_map(&mut self, map: Option<crate::transient::TimeMap>) {
        self.map = map.map(std::sync::Arc::new);
    }

    /// The same, for a map somebody else holds too.
    pub fn adopt_map(&mut self, map: Option<std::sync::Arc<crate::transient::TimeMap>>) {
        self.map = map;
    }

    pub fn set_map_scale(&mut self, scale: f64) {
        self.map_scale = scale;
    }

    /// Where the read pointer nominally sits for an output frame.
    ///
    /// Without transient preservation this is a straight line and needs no map
    /// at all, which is what lets the ratio move freely without anything being
    /// rebuilt off-thread.
    ///
    /// Before Scan: under a Scan of less than one the sweep needs output frames
    /// past the document to reach the end of the file, so the line may carry on
    /// that far before it is held.
    #[inline]
    fn nominal(&self, out_frame: f64, ratio: f32, in_frames: usize, scan: f32) -> f32 {
        let reach = in_frames as f64 / (scan.abs().clamp(1e-3, 1.0) as f64);
        match &self.map {
            Some(m) => {
                let o = self.map_octave;
                let along = out_frame * self.map_scale * o;
                let last = m.out_end();
                if along > last && scan.abs() < 1.0 {
                    ((m.input_at(last) + (along - last) / ratio.max(1e-6) as f64) / o).clamp(0.0, reach) as f32
                } else if o == 1.0 {
                    m.input_at(along) as f32
                } else {
                    (m.input_at(along) / o) as f32
                }
            }
            None => self.anchor.source(out_frame, ratio).clamp(0.0, reach) as f32,
        }
    }

    /// The map the current parameters imply. Allocates — never call from the
    /// audio thread.
    pub fn build_map(
        input: &[f32],
        channels: usize,
        sample_rate: u32,
        ratio: f32,
        hop_out: usize,
        p: &WsolaParams,
    ) -> Option<crate::transient::TimeMap> {
        if !p.preserve_transients {
            return None;
        }
        let in_frames = input.len() / channels.max(1);
        let hits = crate::transient::onsets(input, channels, sample_rate, p.sensitivity, p.floor);
        let guard = ((hop_out as f32) * p.guard_hops.clamp(1.0, 16.0)) as usize;
        Some(crate::transient::TimeMap::with_transients(
            in_frames,
            ratio,
            &hits,
            guard.max(1),
        ))
    }

    #[cfg(test)]
    pub(crate) fn search_at_deadline(&mut self) {
        self.immediate_search = true;
    }

    fn prepare_search(
        &mut self,
        input: &[f32],
        channels: usize,
        search: usize,
        hop: usize,
        params: WsolaParams,
    ) {
        let key = (
            input.as_ptr() as usize,
            input.len(),
            channels,
            self.read,
            search,
            hop,
            params,
        );
        if self.search_job.as_ref().is_none_or(|job| job.key != key) {
            self.search_job = Some(PendingSearch {
                key,
                sweep: crate::stretch::OffsetSearch::new(
                    input.len(),
                    channels,
                    self.read,
                    search,
                    hop,
                    params,
                ),
            });
        }
    }

    fn clear_ring(&mut self) {
        let ch = self.channels;
        self.dirty.clear(self.ring, |a, b| {
            self.acc[a * ch..b * ch].fill(0.0);
            self.norm[a..b].fill(0.0);
        });
    }

    /// Rebuild the precomputed window only when its shape actually changed.
    fn ensure_window(&mut self, len: usize, shape: WinShape, skew: f32) {
        let key = (len, shape, skew.to_bits());
        if self.window_for == Some(key) {
            return;
        }
        self.window.clear();
        let power = crate::stretch::window_power(skew);
        for i in 0..len {
            self.window
                .push(crate::stretch::shape_with_power(i, len, shape, power));
        }
        self.window_for = Some(key);
    }
}

impl Streamer for WsolaStream {
    fn position(&self) -> u64 {
        self.emitted
    }

    fn set_octave(&mut self, factor: f64) {
        self.map_octave = factor;
    }

    fn shift_read(&mut self, nominal: f64, swept: f64) {
        // The next window reads from the new place, and every one after it,
        // through the anchor (before Scan, as `nominal` is); the windows already
        // laid stay, so the splice search joins the two as it joins every window.
        self.anchor.shift(nominal);
        self.read = (self.read as f64 + swept).max(0.0) as usize;
        self.search_job = None;
    }

    fn seek(&mut self, out_frame: u64, input_frames: usize, p: &StretchParams) {
        self.clear_ring();
        self.search_job = None;
        self.emitted = out_frame;
        self.write = out_frame;
        self.first = true;
        self.drift.reset();
        let sr = p.sample_rate.max(1) as f32;
        let win = (((p.window_ms.clamp(5.0, MAX_WINDOW_MS) / 1000.0) * sr) as usize).max(64) & !1;
        let hop = crate::stretch::hop_frames(&p.grain, win, sr).max(1);
        // The index is derived from where we are rather than from how many
        // windows have been laid, so seeking to a moment gives the same splices
        // as playing to it. The grain cloud does the same, for the same reason.
        self.index = out_frame / hop as u64;
        self.anchor.reset(p.ratio.clamp(0.01, 100.0));
        let scan = p.grain.scan.clamp(-4.0, 4.0);
        let nominal = self.nominal(out_frame as f64, p.ratio, input_frames, scan);
        let swept = if scan < 0.0 {
            input_frames as f32 + nominal * scan
        } else {
            nominal * scan
        };
        // This layer's own throw, so layers read different audio rather than
        // the same instant laid down a fixed offset apart — which is a delay
        // line and combs.
        self.read = crate::stretch::place(swept + p.grain.layer_read, input_frames, p.grain.wrap);
    }

    fn render(&mut self, out: &mut [f32], channels: usize, input: &[f32], p: &StretchParams) {
        let channels = channels.max(1).min(self.channels);
        let frames = out.len() / channels.max(1);
        out.fill(0.0);
        let in_frames = input.len() / channels.max(1);
        if frames == 0 || in_frames == 0 {
            return;
        }

        let sr = p.sample_rate.max(1) as f32;
        let ratio = p.ratio.clamp(0.01, 100.0);
        // A new ratio reads on from where the next window was going to read,
        // rather than from wherever `frame / ratio` now lands.
        self.anchor.retune(self.write, ratio);
        let win = (((p.window_ms.clamp(5.0, MAX_WINDOW_MS) / 1000.0) * sr) as usize).max(64) & !1;
        let hop_out = crate::stretch::hop_frames(&p.grain, win, sr).max(1);
        // How far a splice may be moved to find a better join.
        //
        // Bounded by the hop, and that bound is the fix for a real fault: at
        // 200 ms the search was wide enough that one window could splice from
        // 180 ms *before* the nominal position and the next from 180 ms after.
        // Both joins correlate well — the search is doing its job — but the two
        // windows are then overlap-adding material a third of a second apart,
        // and what you hear is the sound jumping about inside itself.
        //
        // The read pointer does not drift, because it is re-derived from the
        // output position every hop rather than accumulated. So this is not a
        // time-base problem; it is adjacent windows being allowed to disagree
        // about where they are.
        //
        // One hop is the bound. Consecutive splices are a hop apart, so a
        // search of one hop lets adjacent windows reach each other's nominal
        // position and no further — they can disagree by a window, which is
        // what the algorithm is for, but not by a third of a second, which is
        // what it was doing.
        //
        // Half a hop was tried first and is the tighter classic figure. It is
        // also, at the default window and overlap, ten milliseconds — exactly
        // the control's own default, so Search would have done nothing at all
        // above it. A control that looks live and is not is the worse fault.
        let want = (((p.wsola.search_ms.clamp(0.0, 200.0)) / 1000.0) * sr) as usize;
        let search = want.min(hop_out.max(1));
        let g = &p.grain;
        let steady = g.size_jitter.abs() < 1e-6;
        let pos_jitter = (g.position_jitter_ms / 1000.0) * sr;
        let scan = g.scan.clamp(-4.0, 4.0);

        if steady {
            self.ensure_window(win, p.wsola.shape, g.envelope);
        }

        // Lay down every window that starts before the end of what this block
        // has to hand out. A window reaching past that is fine — the ring holds
        // it and the next block finishes it.
        let need = self.emitted + frames as u64;
        while self.write < need {
            let pos = if self.first {
                self.first = false;
                self.read
            } else {
                // A changed search key restarts it; finish at this deadline.
                self.prepare_search(input, channels, search, hop_out, p.wsola);
                let job = self.search_job.as_mut().unwrap();
                job.sweep
                    .advance(input, channels, &self.expect, hop_out, p.wsola, usize::MAX);
                job.sweep.result()
            };

            let len = crate::stretch::grain_size(g, self.index, win);
            let take = if pos_jitter > 0.0 {
                let j = pos_jitter * g.rand_bipolar(self.index, g.salt(5));
                crate::stretch::place(pos as f32 + j, in_frames, g.wrap)
            } else {
                pos
            };
            let x = self.drift.place(self.write, sr, g.drift_rate_hz);
            let rate = crate::stretch::grain_rate(g, self.index, x);
            let (gl, gr) = crate::grain::pan_gains(g, self.index, channels);
            let span = (len as f32) * rate;
            let power = crate::stretch::window_power(g.envelope);

            if len >= self.ring {
                // Wider than the ring can hold. Only reachable if a device asks
                // for a far larger block than it promised; counted rather than
                // written past the end of the buffer.
                self.overflows += 1;
            } else {
                let stride = self.channels;
                let mut done = 0usize;
                let mut slot = (self.write % self.ring as u64) as usize;
                while done < len {
                    let run = (len - done).min(self.ring - slot);
                    let acc = &mut self.acc[slot * stride..(slot + run) * stride];
                    let norm = &mut self.norm[slot..slot + run];
                    for (k, (a, nm)) in acc
                        .chunks_exact_mut(stride)
                        .zip(norm.iter_mut())
                        .enumerate()
                    {
                        let i = done + k;
                        let w = if steady {
                            self.window[i]
                        } else {
                            crate::stretch::shape_with_power(i, len, p.wsola.shape, power)
                        };
                        let step = if g.reverse {
                            span - (i as f32) * rate
                        } else {
                            (i as f32) * rate
                        };
                        let src = take as f32 + step;
                        if src >= (in_frames - 1) as f32 || src < 0.0 {
                            continue;
                        }
                        for ch in 0..channels {
                            let pan = if ch == 0 { gl } else { gr };
                            a[ch] += crate::stretch::read_at(
                                input, channels, ch, src, in_frames,
                            ) * w * pan;
                        }
                        *nm += w;
                    }
                    done += run;
                    slot = 0;
                }
                self.dirty.mark(self.write, self.write + len as u64);
            }

            // What naturally follows the window just taken.
            let tail = pos + hop_out;
            for i in 0..hop_out {
                for ch in 0..channels {
                    let s = (tail + i) * channels + ch;
                    self.expect[i * channels + ch] =
                        if s < input.len() { input[s] } else { 0.0 };
                }
            }

            self.search_job = None;
            self.write += hop_out as u64;
            self.index += 1;
            let nominal = self.nominal(self.write as f64, ratio, in_frames, scan);
            let swept = if scan < 0.0 {
                in_frames as f32 + nominal * scan
            } else {
                nominal * scan
            };
            self.read = crate::stretch::place(swept + g.layer_read, in_frames, g.wrap);
        }

        // Source files are immutable between calls. Search the next join over
        // the remaining hop, with complete candidates in their original order.
        // Replacing a source or changing search controls invalidates the job.
        #[cfg(test)]
        let ahead = !self.immediate_search;
        #[cfg(not(test))]
        let ahead = true;
        if ahead {
            self.prepare_search(input, channels, search, hop_out, p.wsola);
            let calls = ((self.write.saturating_sub(self.emitted) as usize).div_ceil(frames)).max(1);
            let job = self.search_job.as_mut().unwrap();
            let count = job.sweep.remaining().div_ceil(calls);
            job.sweep
                .advance(input, channels, &self.expect, hop_out, p.wsola, count);
        }

        // Hand out the finished frames and clear them for reuse. Nothing can
        // contribute to a frame the write pointer has already passed.
        {
            let stride = self.channels;
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
                    let n = *nm;
                    for ch in 0..channels {
                        let v = a[ch];
                        o[ch] = if n > 1e-6 { v / n } else { v };
                    }
                    a.fill(0.0);
                    *nm = 0.0;
                }
                done += run;
                slot = 0;
            }
        }
        self.dirty.cleared_prefix(self.emitted, need, self.ring);
        self.emitted = need;
    }
}

/// A streaming engine with pitch shifting on the end of it.
///
/// Pitch is time stretching plus resampling: stretch by the pitch factor on top
/// of the ratio, then read the result back that much faster. The two length
/// changes cancel and the duration is the ratio's alone. The offline renderer
/// does exactly this, so streaming has to as well or the two stop matching —
/// pitch is not something that can be folded into the splice and still be the
/// same sound.
///
/// The resampler pulls: to hand out N frames it needs about N × pitch frames of
/// stretched audio, which it takes from the inner engine in chunks and keeps in
/// a ring. The ring is a fixed size, so nothing is allocated once it exists; a
/// read faster than it was sized for is taken in pieces ([`PitchRing::pull`]).
pub struct Pitched<S: Streamer> {
    inner: S,
    ring: PitchRing,
    scratch: Vec<f32>,
}

/// The widest pitch shift there is, in semitones each way: eight octaves, the
/// grain cloud's own limit (a rate of 256, `grain.rs`). A key's pitch rides on
/// top of the document's, and the top of MIDI is 67 semitones above middle C,
/// so this is what lets note 127 (G9) play at its own pitch (RJ, 16 Nov 2026:
/// the keys stopped at C8, where the old ±48 ran out).
pub const MAX_SEMITONES: f32 = 96.0;

/// [`MAX_SEMITONES`] as a rate multiplier.
pub const MAX_PITCH: f32 = 256.0;

/// The read rate the ring is sized for: a block at this rate or slower is one
/// read, exactly as it always was. A faster one is read in pieces, each small
/// enough to fit, so the ring does not have to be sized for the fastest rate
/// (at 256 that would be megabytes per voice for a rate nobody holds).
const RING_PITCH: f32 = 20.0;

/// The resampling half of pitch shifting, with no engine attached.
///
/// [`Pitched`] is this plus a [`Streamer`]. PVSOLA and the hybrid cannot be
/// `Streamer`s — one needs parameters of its own and the other reads a
/// separated source rather than the input — so they drive this directly
/// instead. **One implementation either way**, which is the whole point: two
/// resamplers is two different sounds, and it would be the third time this
/// project had the same thing implemented twice and drifting.
///
/// It **pulls**. To hand out N frames it needs about N × pitch frames of
/// stretched audio, which the owner renders in chunks and pushes in. Sized for
/// [`RING_PITCH`], so nothing is allocated once it exists; [`PitchRing::pull`]
/// takes a faster read in pieces that fit.
pub struct PitchRing {
    /// Stretched audio waiting to be read back at a rate.
    buf: Vec<f32>,
    ring: usize,
    channels: usize,
    dirty: RingDirty,
    /// Stretched frames pushed in so far, counted from the start of the file.
    made: u64,
    /// Where the read position was when the pitch last changed.
    base: f64,
    /// Output frames read since then.
    ///
    /// The read position is *derived* from these two rather than accumulated,
    /// so that at a steady pitch it is exactly `f × pitch` — the same
    /// expression the offline resampler uses. Accumulating would drift away
    /// from it over a long render.
    since: u64,
    /// The pitch being aimed at.
    last_pitch: f32,
    /// The pitch actually being read at.
    ///
    /// A change in read rate is a corner in the waveform, and a big enough one
    /// is a click. Automation never showed it because it moves the pitch in
    /// slivers a hundred times a second; a hand on the slider arrives every
    /// ninety milliseconds in whole steps, and each one clicked.
    ///
    /// So the rate glides to its target over a few milliseconds instead of
    /// stepping. While it is travelling the read position has to be
    /// accumulated, since the closed form below assumes a constant rate — but
    /// only while travelling, so a steady pitch keeps the exact `f × pitch`
    /// the offline resampler uses and cannot drift from it.
    glide: f32,
    /// Output frames handed out.
    emitted: u64,
    /// Whether anything has been read out of the ring since the last seek.
    engaged: bool,
    /// The earliest stretched frame this run holds.
    ///
    /// The interpolator reaches one frame back, and after a seek there is
    /// nothing there. The offline resampler clamps its index to the start of
    /// the buffer; this clamps to the start of the run, which is the same
    /// thing said in streaming terms.
    origin: u64,
    /// How much the engine is asked for at a time.
    chunk: usize,
    /// How many octaves up the source the engine is reading (`octave.rs`).
    /// Nought at C8 and below, always.
    level: u32,
    /// A note wanted a copy of the source further up than there was: whoever
    /// owns the copies should make them.
    wants: bool,
    /// The last frame handed out, and the step to smooth away at the start of
    /// the next read after a change of level.
    last: Vec<f32>,
    join: Vec<f32>,
    joining: u32,
}

/// Frames over which the step at a change of level is smoothed away.
const JOIN_FRAMES: u32 = 256;

/// How long the read rate takes to reach a new pitch, in frames.
///
/// In frames rather than milliseconds so the ring needs no sample rate — about
/// 12 ms at 48 kHz, 13 at 44.1 and 6 at 96. Every one of those is well above a
/// click and below a portamento, which is the whole window that matters, so
/// threading a rate through four constructors to be exact about it would buy
/// nothing.
///
/// A tape machine glides for the same reason: a read rate that arrives
/// instantly is a corner in the waveform, and a big enough corner is a click.
const GLIDE_FRAMES: f32 = 576.0;

impl PitchRing {
    pub fn new(max_block: usize, channels: usize) -> Self {
        let channels = channels.max(1);
        let chunk = max_block.max(1);
        // Room for a whole block read at `RING_PITCH`, plus one chunk so a
        // pull always has somewhere to land, plus the four frames the
        // interpolator spans. Faster reads are taken in pieces (`pull`).
        let ring = ((max_block as f32 * RING_PITCH) as usize) + chunk + 8;
        PitchRing {
            buf: vec![0.0; ring * channels],
            ring,
            channels,
            dirty: RingDirty::default(),
            made: 0,
            base: 0.0,
            since: 0,
            last_pitch: 1.0,
            glide: 1.0,
            emitted: 0,
            engaged: false,
            origin: 0,
            chunk,
            level: 0,
            wants: false,
            last: vec![0.0; channels],
            join: vec![0.0; channels],
            joining: 0,
        }
    }

    /// The fractional read position along the stretched timeline.
    fn pos(&self) -> f64 {
        self.base + self.since as f64 * self.last_pitch as f64
    }

    /// Aim at a new pitch. The rate travels there; the read does not jump.
    fn retune(&mut self, pitch: f32) {
        if (pitch - self.last_pitch).abs() > 1e-9 {
            self.base = self.pos();
            self.since = 0;
            self.last_pitch = pitch;
        }
    }

    /// Whether this pitch can skip the ring and take the engine's own frames.
    ///
    /// Only until something has been read through it. After that the engine is
    /// a few frames ahead of the read, so coming home to nought and bypassing
    /// the ring there skips those frames, and is heard. The ring carries on at
    /// a rate of one until the next seek.
    pub fn bypassed(&self, pitch: f32) -> bool {
        !self.engaged && (pitch - 1.0).abs() < 1e-6
    }

    /// Whether the rate is still on its way somewhere.
    ///
    /// The threshold is what the ear would notice, not what a float can tell
    /// apart. A rate within 1e-4 of its target is a tenth of a cent out, which
    /// is inaudible — and a one-pole in `f32` cannot do better than that
    /// anyway: the step it takes each sample is the distance times `k`, and
    /// once that falls under an ulp of the rate itself the addition stops
    /// moving. At these rates it parks around 7e-5 and stays there forever, so
    /// a tighter threshold would mean gliding for the rest of the file.
    fn gliding(&self) -> bool {
        (self.glide - self.last_pitch).abs() > 1e-4
    }

    /// Semitones as a rate multiplier, within [`MAX_SEMITONES`] each way.
    pub fn factor(semitones: f32) -> f32 {
        2f32.powf(semitones / 12.0).clamp(1.0 / MAX_PITCH, MAX_PITCH)
    }

    /// The most output frames one read at `pitch` can take, so that every
    /// stretched frame it spans is still in the ring when it is read.
    ///
    /// A read of `n` frames spans `(n - 1) × rate` stretched frames plus the
    /// interpolator's four; the ring holds `RING_PITCH` blocks' worth plus a
    /// chunk and eight. At `RING_PITCH` or slower this is at least a whole
    /// block, so a block is one read, as it always was.
    fn reach(&self, pitch: f32) -> usize {
        let rate = self.glide.max(self.last_pitch).max(pitch).max(1.0) as f64;
        let room = (self.ring - self.chunk - 8) as f64;
        ((room / rate).floor() as usize).max(1)
    }

    /// Hand out `out` at `pitch`, asking `make` for stretched frames as they
    /// are needed: `make(buf)` fills `buf` (from `scratch`, at most a chunk)
    /// with the engine's next frames.
    ///
    /// A block read faster than the ring was sized for is taken in pieces,
    /// each one filled and read before the next, so the frames a read spans
    /// are never overwritten by the frames that come after them. The read
    /// position carries on exactly from piece to piece, so a split block is
    /// the same sound as a whole one.
    pub fn pull(
        &mut self,
        out: &mut [f32],
        channels: usize,
        pitch: f32,
        scratch: &mut [f32],
        mut make: impl FnMut(&mut [f32]),
    ) {
        let ch = channels.max(1);
        let frames = out.len() / ch;
        if frames == 0 {
            self.need(0, pitch);
            self.read(out, ch, pitch);
            return;
        }
        let mut at = 0;
        while at < frames {
            let n = (frames - at).min(self.reach(pitch));
            let need = self.need(n, pitch);
            while self.made < need {
                let k = self.chunk.min((need - self.made) as usize);
                make(&mut scratch[..k * ch]);
                self.push(scratch, k, ch);
            }
            self.read(&mut out[at * ch..(at + n) * ch], ch, pitch);
            at += n;
        }
        self.smooth(out, ch);
    }

    /// After a change of level the first frame of the new read is not where
    /// the last one of the old read was. Take that step out, fading the
    /// correction away, and remember where this block ended.
    fn smooth(&mut self, out: &mut [f32], ch: usize) {
        let frames = out.len() / ch;
        if frames == 0 {
            return;
        }
        let chans = ch.min(self.last.len());
        if self.joining == JOIN_FRAMES {
            for c in 0..chans {
                self.join[c] = self.last[c] - out[c];
            }
        }
        let mut f = 0;
        while self.joining > 0 && f < frames {
            let g = self.joining as f32 / JOIN_FRAMES as f32;
            for c in 0..chans {
                out[f * ch + c] += self.join[c] * g;
            }
            self.joining -= 1;
            f += 1;
        }
        for c in 0..chans {
            self.last[c] = out[(frames - 1) * ch + c];
        }
    }

    /// How many octaves up the source the engine is reading.
    pub fn level(&self) -> u32 {
        self.level
    }

    /// Whether a note has asked for a copy of the source that was not there,
    /// since this was last asked. The owner builds them off the audio thread.
    pub fn take_wants(&mut self) -> bool {
        std::mem::take(&mut self.wants)
    }

    /// The level a seek at `pitch` starts on, given the copies there are
    /// (`have` levels of them).
    pub fn start_level(&mut self, pitch: f32, have: u32) -> u32 {
        let want = crate::octave::level_for(pitch);
        if want > have {
            self.wants = true;
        }
        want.min(have)
    }

    /// Whether the level should change now, mid-note, and to what. Copies that
    /// have gone (a new sound) send it back to nought.
    pub fn next_level(&mut self, pitch: f32, have: u32) -> Option<u32> {
        let mut want = crate::octave::level_while(pitch, self.level);
        if want > have {
            self.wants = true;
            want = if self.level <= have { self.level } else { have };
        }
        (want != self.level).then_some(want)
    }

    /// The rate the ring reads at for `pitch`, on the level it is on.
    pub fn rate(&self, pitch: f32) -> f32 {
        pitch / crate::octave::factor(self.level) as f32
    }

    /// Seek on `level`. Returns the frame of the engine's timeline to seek it
    /// to, on that level.
    pub fn seek_level(&mut self, out_frame: u64, pitch: f32, level: u32) -> u64 {
        self.level = level;
        self.joining = 0;
        self.seek(out_frame, pitch / crate::octave::factor(level) as f32)
    }

    /// Move to `level` where the read is now, mid-note. The engine underneath
    /// is re-seeked to the answer; the step that leaves in the waveform is
    /// smoothed away over the next few milliseconds.
    pub fn change_level(&mut self, pitch: f32, level: u32) -> u64 {
        let here = self.position();
        let at = self.seek_level(here, pitch, level);
        self.joining = JOIN_FRAMES;
        at
    }

    /// The engine at `pitch` on `level`: stretched as far as the ratio and the
    /// pitch together ask, at the sample rate of the copy it is reading.
    pub fn inner_params_at(p: &StretchParams, pitch: f32, level: u32) -> StretchParams {
        let mut inner = Self::inner_params(p, pitch);
        if level > 0 {
            let d = crate::octave::factor(level);
            inner.sample_rate = (p.sample_rate / d).max(1);
            // WSOLA's search steps in frames, and a frame of the copy is `d`
            // frames of the sound: the same step in time is `d` times fewer
            // frames. Left at four, a splice two octaves up could only land
            // within sixteen frames of the best join, the joins wandered out
            // of phase hop after hop, and a held tone cancelled itself out.
            inner.wsola.stride = (p.wsola.stride / d).max(1);
        }
        inner
    }

    /// The ratio the engine underneath must be driven at.
    ///
    /// Pitch shifting is stretching plus reading back faster, so the engine
    /// runs at ratio × pitch and the extra length is taken back out by the
    /// read rate. The two cancel and the duration is the ratio's alone.
    pub fn inner_params(p: &StretchParams, pitch: f32) -> StretchParams {
        let mut inner = *p;
        inner.ratio = (p.ratio * pitch).clamp(0.01, 100.0);
        inner
    }

    pub fn chunk(&self) -> usize {
        self.chunk
    }

    pub fn made(&self) -> u64 {
        self.made
    }

    pub fn position(&self) -> u64 {
        self.emitted
    }

    /// How many stretched frames must be in hand before `frames` can be read.
    ///
    /// Three past the last one the read lands on, because the interpolator
    /// spans four frames and reaches two forward.
    pub fn need(&mut self, frames: usize, pitch: f32) -> u64 {
        self.retune(pitch);
        if frames == 0 {
            return self.made;
        }
        // The faster of where the rate is and where it is going: while it is
        // gliding the block is read at something between the two, and asking
        // for too much costs nothing while asking for too little is a gap.
        let rate = self.glide.max(self.last_pitch) as f64;
        let last = self.pos() + (frames - 1) as f64 * rate;
        last.floor() as u64 + 3
    }

    /// Take `n` freshly stretched frames from `src`.
    pub fn push(&mut self, src: &[f32], n: usize, channels: usize) {
        let channels = channels.max(1).min(self.channels);
        let stride = self.channels;
        let mut done = 0usize;
        let mut slot = (self.made % self.ring as u64) as usize;
        while done < n {
            let run = (n - done).min(self.ring - slot);
            let dst = &mut self.buf[slot * stride..(slot + run) * stride];
            let s = &src[done * channels..(done + run) * channels];
            if channels == stride {
                dst.copy_from_slice(s);
            } else {
                for (d, src_f) in dst.chunks_exact_mut(stride).zip(s.chunks_exact(channels)) {
                    d[..channels].copy_from_slice(src_f);
                }
            }
            done += run;
            slot = 0;
        }
        self.dirty.mark(self.made, self.made + n as u64);
        self.made += n as u64;
    }

    /// Read `out.len() / channels` frames back at `pitch`.
    pub fn read(&mut self, out: &mut [f32], channels: usize, pitch: f32) {
        self.retune(pitch);
        self.engaged = true;
        let channels = channels.max(1).min(self.channels);
        let frames = out.len() / channels.max(1);
        let base = self.base;
        let since = self.since;
        // One pole per sample toward the target rate. `GLIDE_SECONDS` of it is
        // below a portamento and well above a click.
        let k = 1.0 - (-1.0f32 / GLIDE_FRAMES).exp();
        let mut walking = self.base;
        let mut glided = false;
        let ring = self.ring;
        let stride = self.channels;
        let origin = self.origin as i64;
        // Track the ring slot of `i` incrementally instead of dividing by `ring`
        // on every tap and channel.
        let mut tracked_i = origin;
        let mut tracked_slot = (self.origin % ring as u64) as usize;
        for f in 0..frames {
            let at = if self.gliding() {
                // Travelling: the position has to be accumulated, because the
                // rate is different on every frame.
                self.glide += (self.last_pitch - self.glide) * k;
                if (self.glide - self.last_pitch).abs() < 1e-4 {
                    self.glide = self.last_pitch;
                }
                glided = true;
                let here = walking;
                walking += self.glide as f64;
                here
            } else if glided {
                // Arrived part way through this block. The closed form below
                // counts from the top of the block, where the read no longer
                // is, so it reads on from where the glide got to. Going back to
                // the closed form here was a jump of the frames travelled so
                // far, and a second, backwards, when the next block began from
                // the frame the glide had stopped on.
                let here = walking;
                walking += pitch as f64;
                here
            } else {
                base + (since + f as u64) as f64 * pitch as f64
            };
            let i = at.floor() as i64;
            let t = (at - i as f64) as f32;
            let out_f = &mut out[f * channels..(f + 1) * channels];
            if i > origin {
                let base_idx = i - 1;
                if base_idx >= tracked_i {
                    let adv = (base_idx - tracked_i) as usize;
                    let mut s = tracked_slot + adv;
                    if s >= ring {
                        s -= ring;
                        if s >= ring {
                            s %= ring;
                        }
                    }
                    tracked_i = base_idx;
                    tracked_slot = s;
                } else {
                    tracked_i = base_idx;
                    tracked_slot = (base_idx as u64 % ring as u64) as usize;
                }
                let s_m1 = tracked_slot;
                if s_m1 + 3 < ring {
                    // Contiguous four-frame span in the ring: no wrap, no modulo.
                    let span = &self.buf[s_m1 * stride..(s_m1 + 4) * stride];
                    for ch in 0..channels {
                        let m1 = span[ch];
                        let p0 = span[stride + ch];
                        let p1 = span[2 * stride + ch];
                        let p2 = span[3 * stride + ch];
                        out_f[ch] = crate::stretch::hermite(m1, p0, p1, p2, t);
                    }
                } else {
                    let s0 = if s_m1 + 1 < ring { s_m1 + 1 } else { 0 };
                    let s1 = if s0 + 1 < ring { s0 + 1 } else { 0 };
                    let s2 = if s1 + 1 < ring { s1 + 1 } else { 0 };
                    let (b_m1, b0, b1, b2) = (
                        s_m1 * stride,
                        s0 * stride,
                        s1 * stride,
                        s2 * stride,
                    );
                    for ch in 0..channels {
                        out_f[ch] = crate::stretch::hermite(
                            self.buf[b_m1 + ch],
                            self.buf[b0 + ch],
                            self.buf[b1 + ch],
                            self.buf[b2 + ch],
                            t,
                        );
                    }
                }
            } else {
                // First frame after a seek: `i - 1` clamps to `origin`.
                let slot_of = |k: i64| -> usize {
                    let idx = (i + k).max(origin) as u64;
                    (idx % ring as u64) as usize * stride
                };
                let (b_m1, b0, b1, b2) = (slot_of(-1), slot_of(0), slot_of(1), slot_of(2));
                for ch in 0..channels {
                    // The same four-point Hermite the offline renderer uses. It
                    // was linear here once, which is why a pitched stream and a
                    // pitched export were audibly the same and numerically not.
                    out_f[ch] = crate::stretch::hermite(
                        self.buf[b_m1 + ch],
                        self.buf[b0 + ch],
                        self.buf[b1 + ch],
                        self.buf[b2 + ch],
                        t,
                    );
                }
            }
        }
        // A glide leaves the read somewhere the closed form does not describe,
        // so hand the accumulated position back as the new base.
        if glided {
            self.base = walking;
            self.since = 0;
        } else {
            self.since += frames as u64;
        }
        self.emitted += frames as u64;
    }

    /// Keep the counters in step while the ring is being bypassed.
    ///
    /// At unity pitch the engine already produces at the right rate, so the
    /// ring is skipped entirely — but its idea of where it is has to keep up,
    /// or the first block after the slider leaves unity reads from the wrong
    /// place. Deliberately not `seek`: that clears the buffer, and clearing
    /// twenty thousand samples on every block of a callback that is doing no
    /// resampling at all is work for nothing.
    pub fn advance_unpitched(&mut self, frames: usize) {
        self.emitted += frames as u64;
        self.base = self.emitted as f64;
        self.since = 0;
        self.last_pitch = 1.0;
        self.glide = 1.0;
        self.made = self.emitted;
        self.origin = self.emitted;
    }

    /// Move to an output frame. Returns the stretched frame the engine
    /// underneath must be seeked to.
    pub fn seek(&mut self, out_frame: u64, pitch: f32) -> u64 {
        // Derived from the output frame, not from wherever the read had got
        // to, so a seek to frame zero lands on exactly `f × pitch` — which is
        // what the offline renderer computes for the same frame.
        self.base = 0.0;
        self.since = out_frame;
        self.last_pitch = pitch;
        // A seek starts *at* its rate rather than gliding into it. The glide is
        // for a hand moving the slider mid-flight; a fresh render that eased
        // into its pitch would not match what the offline renderer writes for
        // the same frames, which is the one thing that must never differ.
        self.glide = pitch;
        self.engaged = false;
        let at = self.pos();
        self.made = at as u64;
        self.origin = at as u64;
        self.emitted = out_frame;
        let ch = self.channels;
        self.dirty.clear(self.ring, |a, b| self.buf[a * ch..b * ch].fill(0.0));
        at as u64
    }
}

impl<S: Streamer> Pitched<S> {
    /// [`Streamer::shift_read`] for the engine inside: `by` is in frames of
    /// the sound, and the engine reads the copy an octave level down, whose
    /// frames are that many of the sound's.
    pub fn shift_read(&mut self, nominal: f64, swept: f64) {
        let d = crate::octave::factor(self.ring.level()) as f64;
        self.inner.shift_read(nominal / d, swept / d);
    }

    /// Wrap an engine. The engine is built by the caller because each has its
    /// own idea of what it needs; everything after that is common.
    pub fn new(inner: S, max_block: usize, channels: usize) -> Self {
        let channels = channels.max(1);
        Pitched {
            inner,
            ring: PitchRing::new(max_block, channels),
            scratch: vec![0.0; max_block.max(1) * channels],
        }
    }

    /// The engine underneath, for whatever it alone understands — WSOLA's
    /// transient map, for instance.
    pub fn inner_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    pub fn inner(&self) -> &S {
        &self.inner
    }

    /// Render `frames` of output at `semitones`, reading from `input`.
    pub fn render_pitched(
        &mut self,
        out: &mut [f32],
        channels: usize,
        input: &[f32],
        p: &StretchParams,
        semitones: f32,
    ) {
        self.render_octaves(out, channels, input, None, p, semitones);
    }

    /// The same, reading a copy of `input` an octave or more up when the note
    /// is above C8 and `octaves` has one (`octave.rs`).
    pub fn render_octaves(
        &mut self,
        out: &mut [f32],
        channels: usize,
        input: &[f32],
        octaves: Option<&crate::octave::Octaves>,
        p: &StretchParams,
        semitones: f32,
    ) {
        let pitch = PitchRing::factor(semitones);
        if self.ring.bypassed(pitch) {
            // Nothing to resample; the engine is already producing at the right
            // rate, so do not pay for a copy through the ring.
            let frames = out.len() / channels.max(1);
            self.inner.render(out, channels, input, p);
            self.ring.advance_unpitched(frames);
            return;
        }
        let oct = octaves.filter(|o| o.is_for(input));
        let have = oct.map_or(0, |o| o.levels.len() as u32);
        if let Some(level) = self.ring.next_level(pitch, have) {
            let frames = crate::octave::frames_at(input.len() / channels.max(1), level);
            let at = self.ring.change_level(pitch, level);
            self.inner.set_octave(crate::octave::factor(level) as f64);
            self.inner.seek(at, frames, &PitchRing::inner_params_at(p, pitch, level));
        }
        let level = self.ring.level();
        let src = match oct {
            Some(o) => o.source(input, level),
            None => input,
        };
        let inner = PitchRing::inner_params_at(p, pitch, level);
        let rate = self.ring.rate(pitch);
        let engine = &mut self.inner;
        self.ring.pull(out, channels, rate, &mut self.scratch, |buf| engine.render(buf, channels, src, &inner));
    }

    pub fn position(&self) -> u64 {
        self.ring.position()
    }

    /// Whether a note has asked for copies of the source that were not there.
    pub fn take_wants(&mut self) -> bool {
        self.ring.take_wants()
    }

    /// Move to an output frame. The engine underneath is seeked to the matching
    /// place on the *stretched* timeline, which runs `pitch` times faster.
    pub fn seek(&mut self, out_frame: u64, input_frames: usize, p: &StretchParams, semitones: f32) {
        self.seek_octaves(out_frame, input_frames, None, p, semitones);
    }

    /// The same, starting on a copy of the source an octave or more up when
    /// the note is above C8 and `octaves` has one.
    pub fn seek_octaves(
        &mut self,
        out_frame: u64,
        input_frames: usize,
        octaves: Option<&crate::octave::Octaves>,
        p: &StretchParams,
        semitones: f32,
    ) {
        let pitch = PitchRing::factor(semitones);
        let have = octaves.filter(|o| o.is_length(input_frames)).map_or(0, |o| o.levels.len() as u32);
        let level = self.ring.start_level(pitch, have);
        let at = self.ring.seek_level(out_frame, pitch, level);
        self.inner.set_octave(crate::octave::factor(level) as f64);
        self.inner.seek(at, crate::octave::frames_at(input_frames, level), &PitchRing::inner_params_at(p, pitch, level));
    }
}

#[cfg(test)]
mod tests {

    /// A hand on the pitch slider used to click while automation did not.
    ///
    /// Automation moves the pitch in slivers a hundred times a second, so its
    /// steps were too small to hear. A drag arrives every ninety milliseconds
    /// in whole ones, and each was a step in the *read rate* — a corner in the
    /// waveform, and a big enough corner is a click.
    #[test]
    fn a_step_in_pitch_does_not_put_a_corner_in_the_output() {
        let sr = 48_000u32;
        let mut ring = PitchRing::new(512, 1);
        // Fill it with a slow sine, which is smooth enough that any corner in
        // the output came from the read and not from the material.
        let src: Vec<f32> = (0..sr as usize).map(|i| (i as f32 / 60.0).sin() * 0.5).collect();
        ring.push(&src, src.len(), 1);

        let corner = |v: &[f32]| {
            v.windows(3).map(|w| (w[2] - 2.0 * w[1] + w[0]).abs()).fold(0f32, f32::max)
        };

        let mut steady = vec![0.0f32; 2048];
        ring.read(&mut steady, 1, 1.0);
        let base = corner(&steady);

        // Ask for a fifth up, in one go, the way releasing a slider does.
        let mut moved = vec![0.0f32; 2048];
        ring.read(&mut moved, 1, PitchRing::factor(7.0));
        let mut joined = steady[steady.len() - 2..].to_vec();
        joined.extend_from_slice(&moved);
        let got = corner(&joined);
        assert!(
            got < base * 6.0,
            "a seven-semitone step put a corner of {got:.5} in against a steady {base:.5}"
        );
    }

    /// And it does arrive: a glide that never reaches its target is a bug of a
    /// different kind.
    #[test]
    fn a_glided_pitch_reaches_the_rate_it_was_asked_for() {
        let mut ring = PitchRing::new(512, 1);
        let src: Vec<f32> = (0..48_000).map(|i| (i as f32 / 60.0).sin()).collect();
        ring.push(&src, src.len(), 1);
        let want = PitchRing::factor(7.0);
        let mut out = vec![0.0f32; 512];
        for _ in 0..16 {
            ring.read(&mut out, 1, want);
        }
        assert!(!ring.gliding(), "the read rate never arrived");
    }

    /// The top of MIDI is a read 48 times faster than the source, and the
    /// ring is sized for 20 (RJ, 16 Nov 2026: the keys stopped at C8). A read
    /// too fast for the ring is taken in pieces; if a piece ever outran the
    /// ring, frames it needed would be overwritten before it read them. Every
    /// output frame is checked against the same four-point read of the line
    /// itself, so a single lost frame shows.
    #[test]
    fn a_read_faster_than_the_ring_reads_every_frame_it_spans() {
        let line = |j: i64| ((j.max(0) as f32) * 0.0007).sin() * 0.5;
        for semis in [48.0f32, 60.0, 67.0, MAX_SEMITONES] {
            let pitch = PitchRing::factor(semis);
            let mut ring = PitchRing::new(512, 1);
            ring.seek(0, pitch);
            let mut scratch = vec![0.0f32; 512];
            let mut next = 0i64;
            let mut out = vec![0.0f32; 512];
            let mut f = 0u64;
            for _ in 0..12 {
                ring.pull(&mut out, 1, pitch, &mut scratch, |buf| {
                    for v in buf.iter_mut() {
                        *v = line(next);
                        next += 1;
                    }
                });
                for &got in &out {
                    let at = f as f64 * pitch as f64;
                    let i = at.floor() as i64;
                    let t = (at - i as f64) as f32;
                    let want = crate::stretch::hermite(line(i - 1), line(i), line(i + 1), line(i + 2), t);
                    assert_eq!(got, want, "{semis} semitones up, frame {f}: a frame the read needed was gone");
                    f += 1;
                }
            }
            assert!(next as f64 >= f as f64 * pitch as f64 * 0.99, "the engine was asked for too little at {semis}");
        }
        assert!((PitchRing::factor(67.0) - 2f32.powf(67.0 / 12.0)).abs() < 1e-3, "G9 is not clamped");
        assert_eq!(PitchRing::factor(200.0), MAX_PITCH);
    }
    use super::*;
    use crate::stretch::{Algorithm, Stretch};

    const RATE: u32 = 44_100;

    fn busy(secs: f32, channels: usize) -> Vec<f32> {
        let n = (RATE as f32 * secs) as usize;
        let mut seed = 12345u32;
        let mut v = Vec::with_capacity(n * channels);
        for i in 0..n {
            let t = i as f32 / RATE as f32;
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = ((seed >> 16) as f32 / 32768.0) - 1.0;
            let into = i % 5512;
            let hit = if into < 700 {
                noise * (1.0 - into as f32 / 700.0).powi(2) * 0.7
            } else {
                0.0
            };
            let s = 0.3 * (std::f32::consts::TAU * 220.0 * t).sin() + noise * 0.05 + hit;
            for ch in 0..channels {
                v.push(if ch == 0 { s } else { s * 0.7 });
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

    /// Drive the streamer to produce the whole thing, in blocks.
    fn streamed(input: &[f32], channels: usize, spec: &Stretch, block: usize) -> Vec<f32> {
        let p = params(spec);
        let in_frames = input.len() / channels;
        let want = ((in_frames as f64) * spec.ratio as f64).round() as usize;

        let sr = RATE as f32;
        let win = (((spec.window_ms.clamp(5.0, 2000.0) / 1000.0) * sr) as usize).max(64) & !1;
        let hop = crate::stretch::hop_frames(&spec.grain, win, sr).max(1);

        let mut s = WsolaStream::new(block, channels, RATE);
        s.set_map(WsolaStream::build_map(input, channels, RATE, spec.ratio, hop, &spec.wsola));
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
        let mut s = WsolaStream::new(512, 1, RATE);
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
        Stretch { ratio, algorithm: Algorithm::Wsola, ..Default::default() }
    }

    /// The claim the whole file rests on: what a callback produces block by
    /// block is what the offline render produces in one go.
    #[test]
    fn streaming_matches_the_offline_render() {
        let src = busy(0.5, 2);
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
            assert!(worst < 1e-5, "at {ratio}x the two paths differ by {worst:.2e}");
        }
    }

    /// And it must not depend on the block size, or the sound would change with
    /// the device's buffer setting.
    #[test]
    fn the_block_size_does_not_change_the_sound() {
        let src = busy(0.4, 2);
        let s = spec(3.0);
        let a = streamed(&src, 2, &s, 64);
        let b = streamed(&src, 2, &s, 1024);
        let worst = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).fold(0f32, f32::max);
        assert!(worst < 1e-5, "the block size changed the audio by {worst:.2e}");
    }

    #[test]
    fn the_grain_controls_reach_the_streaming_engine_too() {
        let src = busy(0.4, 2);
        let mut s = spec(3.0);
        let plain = streamed(&src, 2, &s, 256);
        s.grain.overlap = 4.0;
        let moved = streamed(&src, 2, &s, 256);
        let d: f32 = plain.iter().zip(&moved).map(|(a, b)| (a - b).abs()).sum();
        assert!(d > 1e-3, "overlap did not reach the streaming engine");
    }

    /// Seeking lands on the right material, but cannot reproduce the splice
    /// chain that led there.
    ///
    /// This is a property of the algorithm and not a shortcut. Each window's
    /// position is chosen to continue the one before it, so where WSOLA is at
    /// any moment depends on every splice since the last seek — there is no
    /// closed form for it the way there is for a grain, whose randomness is
    /// addressed by index. What can be promised is that the audio after a seek
    /// is the same material at the same instant, at the same level.
    #[test]
    fn seeking_lands_on_the_same_material_even_though_the_splices_differ() {
        let src = busy(0.5, 2);
        let s = spec(2.0);
        let whole = streamed(&src, 2, &s, 256);

        let p = params(&s);
        let in_frames = src.len() / 2;
        let sr = RATE as f32;
        let win = (((s.window_ms / 1000.0) * sr) as usize).max(64) & !1;
        let hop = crate::stretch::hop_frames(&s.grain, win, sr).max(1);
        let mut st = WsolaStream::new(512, 2, RATE);
        st.set_map(WsolaStream::build_map(&src, 2, RATE, s.ratio, hop, &s.wsola));

        // Land on a hop boundary: between them the overlap-add is mid-window,
        // and no engine of this kind can reproduce a partial window it never
        // laid down.
        let at = (hop * 20) as u64;
        st.seek(at, in_frames, &p);
        // Enough blocks to get a window past the seam, where the overlap-add
        // has reached full depth again.
        let want = win + 512;
        let mut buf = vec![0f32; want * 2];
        for c in buf.chunks_mut(512 * 2) {
            let n = c.len() / 2;
            st.render(&mut c[..n * 2], 2, &src, &p);
        }

        let from = at as usize + win;
        let a = &whole[from * 2..(from + 256) * 2];
        let b = &buf[win * 2..(win + 256) * 2];
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let ea: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let eb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        let corr = dot / (ea * eb + 1e-12);
        assert!(corr > 0.8, "seeking landed somewhere else: correlation {corr:.3}");
        assert!(
            (ea / eb - 1.0).abs() < 0.25,
            "seeking changed the level: {ea:.3} against {eb:.3}"
        );
    }

    #[test]
    fn silence_streams_to_silence() {
        let src = vec![0f32; 20_000];
        let out = streamed(&src, 1, &spec(3.0), 256);
        assert!(out.iter().all(|v| v.abs() < 1e-6));
    }

    #[test]
    fn ring_dirty_clears_every_written_slot_across_wrap_and_prefix() {
        let ring = 16usize;
        let mut buf = vec![0.0f32; ring];
        let mut d = RingDirty::default();

        // Write [10, 20) (wraps around 16 -> slots 10..16 and 0..4), emit [10, 14).
        for f in 10u64..20 {
            buf[(f % ring as u64) as usize] = f as f32 + 1.0;
        }
        d.mark(10, 20);
        for f in 10u64..14 {
            buf[(f % ring as u64) as usize] = 0.0;
        }
        d.cleared_prefix(10, 14, ring);
        let mut cleared = 0usize;
        d.clear(ring, |a, b| {
            buf[a..b].fill(0.0);
            cleared += b - a;
        });
        assert_eq!(cleared, 6);
        assert!(buf.iter().all(|&v| v == 0.0));

        // Second clear with no writes touches nothing.
        cleared = 0;
        d.clear(ring, |a, b| {
            buf[a..b].fill(0.0);
            cleared += b - a;
        });
        assert_eq!(cleared, 0);

        // Mid-span emit (from > lo, as in PvsolaStream mid-round seek) must not drop [lo, from).
        for f in 4u64..12 {
            buf[(f % ring as u64) as usize] = 7.0;
        }
        d.mark(4, 12);
        for f in 8u64..10 {
            buf[(f % ring as u64) as usize] = 0.0;
        }
        d.cleared_prefix(8, 10, ring);
        d.clear(ring, |a, b| buf[a..b].fill(0.0));
        assert!(buf.iter().all(|&v| v == 0.0));

        // Span >= ring clears the entire ring.
        for f in 0u64..24 {
            buf[(f % ring as u64) as usize] = 3.0;
        }
        d.mark(0, 24);
        d.cleared_prefix(0, 8, ring);
        d.clear(ring, |a, b| buf[a..b].fill(0.0));
        assert!(buf.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn repeated_seeks_leave_zero_dirty_slots_in_wsola_and_pitch_ring() {
        let n = RATE as usize;
        let mut src = Vec::with_capacity(n * 2);
        for i in 0..n {
            let t = i as f32 / RATE as f32;
            let v = 0.4 * (std::f32::consts::TAU * 220.0 * t).sin();
            src.push(v);
            src.push(v * 0.8);
        }
        let s = spec(1.7);
        let p = params(&s);
        let in_frames = src.len() / 2;
        let mut w = WsolaStream::new(512, 2, RATE);
        let mut buf = vec![0.0f32; 128 * 2];
        for &at in &[0u64, 12_345, 3_210, 40_000, 12_345] {
            w.seek(at, in_frames, &p);
            assert!(w.acc.iter().all(|&v| v == 0.0));
            assert!(w.norm.iter().all(|&v| v == 0.0));
            for _ in 0..8 {
                w.render(&mut buf, 2, &src, &p);
            }
        }
        w.seek(0, in_frames, &p);
        assert!(w.acc.iter().all(|&v| v == 0.0));
        assert!(w.norm.iter().all(|&v| v == 0.0));

        let mut pr = PitchRing::new(512, 2);
        let chunk = vec![0.5f32; 256 * 2];
        pr.seek(100, 1.5);
        assert!(pr.buf.iter().all(|&v| v == 0.0));
        pr.push(&chunk, 256, 2);
        pr.seek(500, 1.0);
        assert!(pr.buf.iter().all(|&v| v == 0.0));
    }
}
