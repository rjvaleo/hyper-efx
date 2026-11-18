//! Which engine the callback runs, and switching between them mid-flight.
//!
//! For a long time this decision did not exist: the callback was a grain
//! scheduler and nothing else, so choosing WSOLA or the vocoder in the
//! interface changed the exported file and never changed what came out of the
//! speakers. The picker looked like a performance control and was not one.
//!
//! Every engine that can run here is held at once, built when the device opens
//! and never allocated again. That is not thrift — an engine built on demand is
//! an allocation in the audio callback, and switching engines is exactly the
//! moment you least want a dropout.
//!
//! Only the engine that is selected is asked for audio. The others keep
//! whatever state they had, which is why a switch has to re-seek the one being
//! switched *to*: it may have been sitting at a position from minutes ago, or
//! never have run at all.

use crate::grain::{GrainEvent, StreamParams};
use crate::hstream::{Parts, PitchedHybrid};
use crate::pstream::PitchedPvsola;
use crate::soundhack::SoundHackStream;
use crate::stream::{Pitched, StretchParams, WsolaStream};
use crate::stretch::Algorithm;
use crate::vstream::VocoderStream;

use crate::render::{BlockRenderer, Source};
use std::sync::Arc;

/// Extra engine instances, for layers past the first.
///
/// Built off the audio thread and handed over, because sixteen of anything here
/// is megabytes and a callback may not allocate. Only the engine actually in
/// use is built: holding sixteen of all five would be a hundred and sixty
/// megabytes for a control that is usually at one.
///
/// Until a bank arrives the callback plays with whatever layers it has, which
/// is thinner than asked for but never silent — the same arrangement the
/// hybrid's separated source uses.
pub struct LayerBank {
    pub algorithm: Algorithm,
    pub layers: u32,
    wsola: Vec<Pitched<WsolaStream>>,
    vocoder: Vec<Pitched<VocoderStream>>,
    pvsola: Vec<PitchedPvsola>,
    hybrid: Vec<PitchedHybrid>,
}

impl LayerBank {
    /// Build the extra layers for one engine. Allocates; never call this from
    /// the audio thread.
    pub fn build(
        algorithm: Algorithm,
        layers: u32,
        max_block: usize,
        channels: usize,
        sample_rate: u32,
    ) -> LayerBank {
        let extra = layers.clamp(1, crate::render::MAX_LAYERS as u32) as usize - 1;
        let mut bank = LayerBank {
            algorithm,
            layers,
            wsola: Vec::new(),
            vocoder: Vec::new(),
            pvsola: Vec::new(),
            hybrid: Vec::new(),
        };
        for _ in 0..extra {
            match algorithm {
                Algorithm::Wsola => bank.wsola.push(Pitched::new(
                    WsolaStream::new(max_block, channels, sample_rate),
                    max_block,
                    channels,
                )),
                Algorithm::Vocoder => bank.vocoder.push(Pitched::new(
                    VocoderStream::new(max_block, channels),
                    max_block,
                    channels,
                )),
                Algorithm::Pvsola => bank.pvsola.push(PitchedPvsola::new(max_block, channels)),
                Algorithm::Hybrid => {
                    bank.hybrid
                        .push(PitchedHybrid::new(max_block, channels, sample_rate))
                }
                // The grain cloud layers inside its own renderer: a layer there
                // is another schedule, not another engine. SoundHack has no
                // layers, as the program had none.
                Algorithm::Granular | Algorithm::SoundHack => {}
            }
        }
        bank
    }

    fn count(&self) -> usize {
        match self.algorithm {
            Algorithm::Wsola => self.wsola.len(),
            Algorithm::Vocoder => self.vocoder.len(),
            Algorithm::Pvsola => self.pvsola.len(),
            Algorithm::Hybrid => self.hybrid.len(),
            Algorithm::Granular | Algorithm::SoundHack => 0,
        }
    }
}

/// Every engine the audio thread can run, all resident.
pub struct Stretcher {
    /// The most layers the machine can carry, measured. Never written into
    /// `StreamParams` — see `set_layer_cap`.
    layer_cap: u32,
    grain: BlockRenderer,
    /// Each engine, or nothing where a voice has no use for it: see
    /// [`Stretcher::voice`]. The deck's own has them all.
    wsola: Option<Pitched<WsolaStream>>,
    vocoder: Option<Pitched<VocoderStream>>,
    pvsola: Option<PitchedPvsola>,
    hybrid: Option<PitchedHybrid>,
    soundhack: Option<Pitched<SoundHackStream>>,
    /// The source, split three ways. Handed over from off the audio thread; an
    /// empty one simply means the hybrid has nothing to play yet.
    parts: std::sync::Arc<Parts>,
    /// What was running last block, so a change can be noticed and acted on.
    current: Algorithm,
    /// The engine being faded out of, and how many frames of the fade are left.
    ///
    /// Switching outright puts a step in the waveform — the new engine starts
    /// cold at the playhead and its first sample has nothing to do with the
    /// last one the old engine produced — and a step is a click. So the old
    /// engine keeps running for a moment and the two are mixed.
    fading: Option<(Algorithm, usize)>,
    /// Somewhere to render the outgoing engine while the incoming one fills
    /// `out`. Sized once, like everything else here.
    scratch: Vec<f32>,
    /// And somewhere to render the grain cloud when it is layered over an
    /// engine rather than being the engine.
    cloud_buf: Vec<f32>,
    /// Frames left of the short fade-in after a rescale on an engine with no
    /// grains to carry: its seek starts mid-wave, which is a click otherwise.
    declick: u32,
    /// The grain renderer is playing for a Hybrid still waiting on its
    /// separation, so the separation has a place to come in at.
    stand_in: bool,
    /// Feedback over whatever is playing, the same stage the offline render
    /// ends with (`feedback.rs`). A voice has none: its notes go through the
    /// deck's, after they have been summed.
    feedback: Option<crate::feedback::Feedback>,
    /// And somewhere to render each extra layer before it is added in.
    layer_buf: Vec<f32>,
    /// The extra layers, if any have been handed over yet.
    bank: Option<LayerBank>,
    /// Where each extra layer's own playhead is, so a layer is only re-seeked
    /// when it has actually lost its place.
    layer_at: Vec<u64>,
    /// Output frame the whole thing is at. Kept here rather than read from
    /// whichever engine is live, because a switch must not appear to seek.
    position: u64,
    /// The transient map WSOLA is holding, for the voices to share.
    map: Option<Arc<crate::transient::TimeMap>>,
}

/// How long a switch between engines takes to cross over.
///
/// About twenty milliseconds at the usual rates — long enough that the step
/// which would otherwise be a click is spread below hearing, short enough that
/// the change still feels immediate under the finger.
const FADE_FRAMES: usize = 1024;
/// The fade-in after a rescale, about five milliseconds at 48 kHz.
const DECLICK_FRAMES: u32 = 240;

/// Which engines run here at all.
///
/// Every engine runs here now.
///
/// Kept as a function rather than deleted: it is what the interface mirrors,
/// and the next engine added will not be streaming on the day it lands.
pub fn is_live(_alg: Algorithm) -> bool {
    true
}

/// What actually runs for a requested engine.
fn resolve(alg: Algorithm) -> Algorithm {
    if is_live(alg) {
        alg
    } else {
        Algorithm::Granular
    }
}

/// The hop each engine's layers are spaced by.
///
/// Every engine means something different by a window — a splice for WSOLA, an
/// analysis frame for the vocoder — so each computes its own. These mirror what
/// the offline renderer passes to `stretch::layered`, and they have to: get one
/// wrong and the layers sit at different places live than they do in the file.
fn layer_hop(alg: Algorithm, sp: &StreamParams) -> u64 {
    let sr = sp.sample_rate.max(1) as f32;
    let hop = match alg {
        Algorithm::Wsola => {
            let win = (((sp.window_ms.clamp(5.0, 2000.0) / 1000.0) * sr) as usize).max(64);
            crate::stretch::hop_frames(&sp.grain, win, sr)
        }
        Algorithm::Vocoder | Algorithm::Hybrid => {
            let n = crate::stretch::fft_size_for(sp.vocoder.window_ms, sp.sample_rate);
            crate::stretch::hop_frames(&sp.grain, n, sr)
        }
        Algorithm::Pvsola => {
            (crate::stretch::fft_size_for(sp.vocoder.window_ms, sp.sample_rate) / 4).max(1)
        }
        Algorithm::Granular => sp.plan().hop,
        // Never layered (its bank is empty), but it has a hop all the same:
        // the synthesis one `FindBestRatio` picks.
        Algorithm::SoundHack => {
            crate::soundhack::find_best_ratio(
                sp.soundhack.version,
                sp.ratio.clamp(0.01, 100.0) as f64,
                crate::soundhack::DEFAULT_CONFIG.nw,
            )
            .1
        }
    };
    hop.max(1) as u64
}

fn stretch_params(sp: &StreamParams) -> StretchParams {
    StretchParams {
        ratio: sp.ratio,
        window_ms: sp.window_ms,
        sample_rate: sp.sample_rate,
        wsola: sp.wsola,
        vocoder: sp.vocoder,
        grain: sp.grain,
        soundhack: sp.soundhack,
    }
}

impl Stretcher {
    pub fn new(max_block: usize, channels: usize, sample_rate: u32) -> Self {
        Stretcher {
            grain: BlockRenderer::new(max_block),
            wsola: Some(Pitched::new(
                WsolaStream::new(max_block, channels, sample_rate),
                max_block,
                channels,
            )),
            vocoder: Some(Pitched::new(VocoderStream::new(max_block, channels), max_block, channels)),
            pvsola: Some(PitchedPvsola::new(max_block, channels)),
            hybrid: Some(PitchedHybrid::new(max_block, channels, sample_rate)),
            soundhack: Some(Pitched::new(SoundHackStream::new(max_block, channels), max_block, channels)),
            parts: std::sync::Arc::new(Parts::default()),
            current: Algorithm::Granular,
            fading: None,
            layer_cap: crate::render::MAX_LAYERS as u32,
            scratch: vec![0.0; max_block.max(1) * channels.max(1)],
            layer_buf: vec![0.0; max_block.max(1) * channels.max(1)],
            cloud_buf: vec![0.0; max_block.max(1) * channels.max(1)],
            declick: 0,
            stand_in: false,
            feedback: Some(crate::feedback::Feedback::new()),
            bank: None,
            layer_at: vec![u64::MAX; crate::render::MAX_LAYERS],
            position: 0,
            map: None,
        }
    }

    /// One note's worth of engine: `algorithm` alone, its layers, and a cloud.
    ///
    /// The Grain page's Poly runs one of these per held key, each at its own
    /// semitones. Only the engine in use is built, for the reason [`LayerBank`]
    /// is built that way, and with no feedback loop of its own, because the
    /// deck's runs over the sum. Allocates; never call this from the audio
    /// thread.
    pub fn voice(
        algorithm: Algorithm,
        layers: u32,
        max_block: usize,
        channels: usize,
        sample_rate: u32,
    ) -> Self {
        let wide = max_block.max(1) * channels.max(1);
        let is = |a: Algorithm| algorithm == a;
        Stretcher {
            grain: BlockRenderer::new(max_block),
            wsola: is(Algorithm::Wsola).then(|| {
                Pitched::new(WsolaStream::new(max_block, channels, sample_rate), max_block, channels)
            }),
            vocoder: is(Algorithm::Vocoder)
                .then(|| Pitched::new(VocoderStream::new(max_block, channels), max_block, channels)),
            pvsola: is(Algorithm::Pvsola).then(|| PitchedPvsola::new(max_block, channels)),
            hybrid: is(Algorithm::Hybrid).then(|| PitchedHybrid::new(max_block, channels, sample_rate)),
            soundhack: is(Algorithm::SoundHack)
                .then(|| Pitched::new(SoundHackStream::new(max_block, channels), max_block, channels)),
            parts: Arc::new(Parts::default()),
            current: algorithm,
            fading: None,
            layer_cap: crate::render::MAX_LAYERS as u32,
            scratch: vec![0.0; wide],
            layer_buf: vec![0.0; wide],
            cloud_buf: vec![0.0; wide],
            declick: 0,
            stand_in: false,
            feedback: None,
            bank: Some(LayerBank::build(algorithm, layers, max_block, channels, sample_rate)),
            layer_at: vec![u64::MAX; crate::render::MAX_LAYERS],
            position: 0,
            map: None,
        }
    }

    pub fn position(&self) -> u64 {
        self.position
    }

    /// Hand WSOLA a freshly built transient map, or `None` for a straight line.
    /// Built off the audio thread; see `crate::stream`.
    pub fn set_map(&mut self, map: Option<crate::transient::TimeMap>) {
        self.adopt_map(map.map(Arc::new));
    }

    /// The same map, shared: a voice takes what the deck has been given, and a
    /// clone of the map itself would be an allocation on the audio thread.
    pub fn adopt_map(&mut self, map: Option<Arc<crate::transient::TimeMap>>) {
        self.map = map.clone();
        if let Some(w) = self.wsola.as_mut() {
            w.inner_mut().adopt_map(map.clone());
        }
        // The hybrid stretches its percussive part with preservation held on,
        // so it wants a map too — of that part rather than of the whole sound,
        // which is what having separated it is for. The caller decides which.
        if let Some(h) = self.hybrid.as_mut() {
            h.adopt_map(map);
        }
    }

    /// [`Self::adopt_map`], handing back the map it replaces. The engines let
    /// go of the old map first, so the caller ends up its last holder here
    /// and can pass it somewhere it is safe to free: dropped on the audio
    /// thread, the last hold on a map is a free there. Voices sharing the
    /// map must take the new one before the caller lets the old one go.
    pub fn replace_map(
        &mut self,
        map: Option<Arc<crate::transient::TimeMap>>,
    ) -> Option<Arc<crate::transient::TimeMap>> {
        let old = self.map.take();
        self.adopt_map(map);
        old
    }

    /// The transient map in force.
    pub fn map(&self) -> Option<&Arc<crate::transient::TimeMap>> {
        self.map.as_ref()
    }

    /// The source, split three ways, as this was last given it.
    pub fn parts(&self) -> &Arc<Parts> {
        &self.parts
    }

    /// A voice's own semitones under a map built for no pitch at all: its
    /// engine's timeline runs that much faster, and the map is read that much
    /// further along.
    pub fn set_map_pitch(&mut self, semitones: f32) {
        let pitch = crate::stream::PitchRing::factor(semitones) as f64;
        if let Some(w) = self.wsola.as_mut() {
            w.inner_mut().set_map_scale(1.0 / pitch);
        }
        if let Some(h) = self.hybrid.as_mut() {
            h.set_map_scale(1.0 / pitch);
        }
    }

    /// The deck's feedback over a block that came from nowhere else: every note
    /// is a voice and the transport is silent. The same stage [`Self::render`]
    /// ends with.
    pub fn feedback_only(&mut self, out: &mut [f32], channels: usize, sp: &StreamParams) {
        if let Some(fb) = self.feedback.as_mut() {
            fb.process(out, channels, &sp.grain, sp.sample_rate);
        }
    }

    /// Adopt a bank of extra layers. Built off the audio thread. The bank it
    /// replaces is freed here: on the audio thread, use
    /// [`Self::replace_bank`].
    pub fn set_bank(&mut self, bank: LayerBank) {
        drop(self.replace_bank(bank));
    }

    /// [`Self::set_bank`], handing back the bank it replaces, for the caller
    /// to free somewhere it is safe to: a bank is megabytes.
    pub fn replace_bank(&mut self, bank: LayerBank) -> Option<LayerBank> {
        let old = self.bank.replace(bank);
        // A fresh bank knows nothing about where the transport is.
        self.layer_at.iter_mut().for_each(|v| *v = u64::MAX);
        old
    }

    /// What the callback can actually layer right now, which is not always what
    /// was asked for — a bank takes a moment to build.
    /// The most layers the machine has been measured to carry.
    ///
    /// Set from `Shared::layer_cap` once a block. Deliberately *not* folded into
    /// `StreamParams`: the moment a cap is written back into the parameters, the
    /// next decision is made against the capped number instead of the requested
    /// one, and it walks itself to one layer. That is the bug this whole
    /// mechanism was withdrawn for once already.
    pub fn set_layer_cap(&mut self, cap: u32) {
        self.layer_cap = cap.max(1);
    }

    pub fn live_layers(&self, sp: &StreamParams) -> u32 {
        let want = sp.grain.layers.clamp(1, crate::render::MAX_LAYERS as u32)
            .min(self.layer_cap);
        if resolve(sp.algorithm) == Algorithm::Granular {
            return want;
        }
        match &self.bank {
            Some(b) if b.algorithm == resolve(sp.algorithm) => {
                want.min(b.count() as u32 + 1)
            }
            _ => 1,
        }
    }

    /// Adopt a freshly separated source. Built off the audio thread.
    pub fn set_parts(&mut self, parts: std::sync::Arc<Parts>) {
        self.parts = parts;
    }

    /// The Hybrid is the engine and there is nothing separated for it yet, so
    /// the grain renderer is playing in its place.
    fn standing_in(&self) -> bool {
        self.current == Algorithm::Hybrid && self.parts.is_empty()
    }

    pub fn overflows(&self) -> u64 {
        self.grain.overflows + self.wsola.as_ref().map_or(0, |w| w.inner().overflows)
    }

    pub fn seek(&mut self, out_frame: u64, sp: &StreamParams) {
        self.position = out_frame;
        // A seek starts the loop empty, as a render from the top does. A
        // rescale keeps it: the stretch moved, not the playhead.
        if let Some(fb) = self.feedback.as_mut() {
            fb.reset();
        }
        self.current = resolve(sp.algorithm);
        // A seek is a jump anyway; there is nothing to fade from.
        self.fading = None;
        self.stand_in = false;
        self.layer_at.iter_mut().for_each(|v| *v = u64::MAX);
        self.seek_current(out_frame, sp);
    }

    /// Move to `out_frame` because the stretch changed, keeping the grain
    /// cloud's sounding grains (see `BlockRenderer::rescale`). The splice and
    /// spectral engines, and their extra layers, read on from where they are
    /// (see below); the cloud layered over one of them is carried like the
    /// cloud on its own.
    pub fn rescale(&mut self, out_frame: u64, sp: &StreamParams) {
        let delta = out_frame as i64 - self.position as i64;
        self.position = out_frame;
        self.current = resolve(sp.algorithm);
        self.fading = None;
        let mapped = self.current == Algorithm::Wsola && sp.wsola.preserve_transients;
        // The extra layers move with the playhead rather than losing their
        // place: forgetting where they were makes each one seek, and a stretch
        // moving every block was a seek of every layer every block.
        for v in self.layer_at.iter_mut() {
            *v = if mapped || *v == u64::MAX { u64::MAX } else { (*v as i64 + delta).max(0) as u64 };
        }
        if self.current == Algorithm::Granular {
            self.grain.rescale(out_frame, sp);
        } else {
            // The splice and spectral engines read on from where they are: the
            // vocoder and PVSOLA advance their read themselves, and WSOLA and
            // the noise stream hold it with an anchor (`anchor.rs`). So a new
            // ratio relabels the playhead and nothing else: no seek, nothing to
            // fade in. A seek and a fade here, every block of a moving stretch,
            // was heard as chopping. A WSOLA reading through a transient map is
            // the one that still seeks, because the map, not the anchor, says
            // where it reads.
            if mapped {
                self.seek_one(self.current, out_frame, sp);
                self.declick = DECLICK_FRAMES;
            }
            if sp.cloud || self.standing_in() {
                self.grain.rescale(out_frame, sp);
            }
        }
    }

    fn seek_current(&mut self, out_frame: u64, sp: &StreamParams) {
        let alg = self.current;
        self.seek_one(alg, out_frame, sp);
    }

    fn seek_one(&mut self, alg: Algorithm, out_frame: u64, sp: &StreamParams) {
        match alg {
            Algorithm::Wsola => {
                if let Some(e) = self.wsola.as_mut() {
                    e.seek(out_frame, sp.in_frames, &stretch_params(sp), sp.semitones);
                }
            }
            Algorithm::Vocoder => {
                if let Some(e) = self.vocoder.as_mut() {
                    e.seek(out_frame, sp.in_frames, &stretch_params(sp), sp.semitones);
                }
            }
            Algorithm::Pvsola => {
                if let Some(e) = self.pvsola.as_mut() {
                    e.seek(out_frame, sp.in_frames, &stretch_params(sp), &sp.pvsola, sp.semitones);
                }
            }
            Algorithm::Hybrid => {
                let parts = std::sync::Arc::clone(&self.parts);
                if let Some(e) = self.hybrid.as_mut() {
                    e.seek(out_frame, &parts, &stretch_params(sp), sp.hybrid, sp.semitones);
                }
            }
            Algorithm::SoundHack => {
                if let Some(e) = self.soundhack.as_mut() {
                    e.seek(out_frame, sp.in_frames, &stretch_params(sp), sp.semitones);
                }
            }
            _ => self.grain.seek(out_frame, sp),
        }
    }

    /// Fill one block from whichever engine is selected.
    ///
    /// `events` collects the grains that started, for the visualiser. Only the
    /// grain cloud has any; the others report none, which is honest — there is
    /// no cloud to draw when a splice engine is running.
    pub fn render(
        &mut self,
        out: &mut [f32],
        channels: usize,
        src: &Source,
        sp: &StreamParams,
        events: &mut [GrainEvent],
    ) -> usize {
        self.render_with(out, channels, src, sp, events, None)
    }

    /// [`Self::render`], with `add` summed in before the feedback: the keys'
    /// voices, which reach the feedback and the rack with the rest of the block.
    pub fn render_with(
        &mut self,
        out: &mut [f32],
        channels: usize,
        src: &Source,
        sp: &StreamParams,
        events: &mut [GrainEvent],
        add: Option<&[f32]>,
    ) -> usize {
        let want = resolve(sp.algorithm);
        if want != self.current {
            // The engine being switched to may be anywhere, or nowhere. Put it
            // where the transport actually is before asking it for audio, or
            // the switch is heard as a jump.
            //
            // The old one is deliberately left alone and kept running, so there
            // is something to fade out of.
            let from = self.current;
            self.current = want;
            // Unless they are one renderer: a Hybrid with nothing separated yet
            // is the grain cloud carrying on, and fading it into itself would
            // ask it for every frame twice.
            self.fading = if from == Algorithm::Granular && self.standing_in() {
                None
            } else {
                Some((from, FADE_FRAMES))
            };
            self.seek_one(want, self.position, sp);
            self.stand_in = false;
        }
        // The separation has arrived under a transport that was already playing
        // (it takes a few hundred milliseconds, and a file starts at once). The
        // engine it is for was last told where to be before there was anything
        // to read, at the top of the file: left alone it starts there, which is
        // the sound going back to the beginning and staying that far behind the
        // playhead. Put it where the transport is, and cross into it from the
        // cloud that has been covering. With the cloud on, the cloud simply
        // carries on, and the engine fades in under it.
        if self.stand_in && !self.parts.is_empty() {
            self.stand_in = false;
            self.seek_one(Algorithm::Hybrid, self.position, sp);
            self.layer_at.iter_mut().for_each(|v| *v = u64::MAX);
            if sp.cloud {
                self.declick = DECLICK_FRAMES;
            } else {
                self.fading = Some((Algorithm::Granular, FADE_FRAMES));
            }
        }

        let frames = out.len() / channels.max(1);
        let mut reported = self.render_one(self.current, out, channels, src, sp, events);
        // A rescaled splice or spectral engine fades in over a few
        // milliseconds rather than starting mid-wave.
        if self.declick > 0 {
            for f in 0..frames {
                if self.declick == 0 {
                    break;
                }
                let g = 1.0 - self.declick as f32 / DECLICK_FRAMES as f32;
                for ch in 0..channels {
                    out[f * channels + ch] *= g;
                }
                self.declick -= 1;
            }
        }
        self.add_layers(out, channels, src, sp);
        reported = reported.max(self.add_cloud(out, channels, src, sp, events));

        // Mix in the tail of the engine being left behind.
        if let Some((from, left)) = self.fading {
            let n = frames.min(self.scratch.len() / channels.max(1));
            let mut evs: [GrainEvent; 0] = [];
            // Lifted out and put straight back. `Vec::default` is empty and
            // allocates nothing, and the buffer returns to the same place with
            // the same capacity — this is a borrow dance, not a reallocation.
            let mut scratch = std::mem::take(&mut self.scratch);
            self.render_one(from, &mut scratch[..n * channels], channels, src, sp, &mut evs);
            for f in 0..n {
                let done = FADE_FRAMES - left + f;
                let t = (done as f32 / FADE_FRAMES as f32).clamp(0.0, 1.0);
                // Equal power, because two engines rendering the same instant
                // agree about what is there and not at all about its phase.
                // This is the opposite choice from PVSOLA's splice, where the
                // search spends its whole effort correlating the two sides
                // first and a linear fade is then the right one.
                let (a, b) = (
                    (t * std::f32::consts::FRAC_PI_2).sin(),
                    (t * std::f32::consts::FRAC_PI_2).cos(),
                );
                for ch in 0..channels {
                    let i = f * channels + ch;
                    out[i] = out[i] * a + scratch[i] * b;
                }
            }
            self.scratch = scratch;
            self.fading = match left.checked_sub(n) {
                Some(0) | None => None,
                Some(rest) => Some((from, rest)),
            };
        }

        if let Some(add) = add {
            for (o, a) in out.iter_mut().zip(add) {
                *o += *a;
            }
        }

        // Feedback last, over the engine, its layers and the cloud together,
        // as the offline render ends.
        if let Some(fb) = self.feedback.as_mut() {
            fb.process(&mut out[..frames * channels], channels, &sp.grain, sp.sample_rate);
        }

        self.position += frames as u64;
        reported
    }

    /// Mix the grain cloud in over whichever engine is running.
    ///
    /// The cloud is not a layer in the sense `add_layers` means. That stack is
    /// the *same* engine several times over, staggered by a fraction of a hop
    /// and normalised by root-N; it thickens one sound. This is a second sound
    /// entirely, reading the same source at the same ratio and mixed in.
    ///
    /// Nothing happens when the cloud is switched off, and nothing happens
    /// when the cloud already *is* the engine — layering it over itself would
    /// only make it louder.
    ///
    /// The grains are reported to the visualiser, so the cloud can be seen
    /// over a WSOLA document exactly as it is seen when it is the engine.
    fn add_cloud(
        &mut self,
        out: &mut [f32],
        channels: usize,
        src: &Source,
        sp: &StreamParams,
        events: &mut [GrainEvent],
    ) -> usize {
        if !sp.cloud || self.current == Algorithm::Granular {
            return 0;
        }
        // Waiting on a separation the engine *is* the cloud, one renderer that
        // has already made these frames. Asked for them again it lost its
        // place, sought, and dropped every sounding grain, once a block. All
        // there is to do is hold it at the level the cloud will have when the
        // separation comes in beneath it.
        if self.standing_in() {
            let (_, wet) = crate::stretch::cloud_gains(sp.cloud_mix);
            for s in out.iter_mut() {
                *s *= wet;
            }
            return 0;
        }
        let frames = out.len() / channels.max(1);
        let n = frames.min(self.cloud_buf.len() / channels.max(1));
        if n == 0 {
            return 0;
        }
        // Switched on mid-flight, or seeked while it was off: the cloud has no
        // idea where the transport went. Put it there before asking for audio.
        if self.grain.position() != self.position {
            self.grain.seek(self.position, sp);
        }
        let mut buf = std::mem::take(&mut self.cloud_buf);
        let reported = self
            .grain
            .render(&mut buf[..n * channels], channels, src, sp, events);
        let (dry, wet) = crate::stretch::cloud_gains(sp.cloud_mix);
        for k in 0..n * channels {
            out[k] = out[k] * dry + buf[k] * wet;
        }
        self.cloud_buf = buf;
        reported
    }

    /// Sum in the extra layers, and scale the whole thing for how many sounded.
    ///
    /// Each layer is the same engine reading its own place in the source — see
    /// `Grain::layer_throw` — and laid down its own fraction of a hop later.
    /// That offset is what interleaves the grain onsets, and it also spreads
    /// the work: sixteen vocoder layers all transforming on the same block
    /// spiked to 160% of the real-time budget, and staggered they do not.
    fn add_layers(&mut self, out: &mut [f32], channels: usize, src: &Source, sp: &StreamParams) {
        let live = self.live_layers(sp);
        // The grain cloud does its own layering inside `BlockRenderer`, and one
        // layer needs no help from anyone.
        if live <= 1 || self.current == Algorithm::Granular {
            return;
        }
        let frames = out.len() / channels.max(1);
        let n = frames.min(self.layer_buf.len() / channels.max(1));
        let span = n * channels;

        let hop = layer_hop(self.current, sp);
        let spread = sp.grain.layer_spread.clamp(0.0, 4.0);
        let mut buf = std::mem::take(&mut self.layer_buf);

        for layer in 1..live {
            let mut lp = *sp;
            lp.grain.seed = sp.grain.seed.wrapping_add(layer.wrapping_mul(0x9E37_79B9));
            lp.grain.layer_read = sp.grain.layer_throw(layer, sp.sample_rate);
            // A layer is *delayed* by its share of the hop, exactly as the
            // offline renderer delays it — which is what interleaves the frames
            // rather than having every layer transform on the same block.
            let off = ((((hop * layer as u64) / live as u64) as f32) * spread) as u64;

            // Before its own delay has elapsed a layer has nothing to say, and
            // the offline sum has nothing from it there either.
            let end = self.position + n as u64;
            if end <= off {
                continue;
            }
            let skip = off.saturating_sub(self.position) as usize;
            let take = n - skip;
            let at = (self.position + skip as u64) - off;

            let i = layer as usize - 1;
            // Seek only when the layer has actually lost its place.
            //
            // **Not on an exact match.** `at` is derived from `off`, and `off`
            // is derived from Layer spread — so while that control is moving,
            // `at` differs from where the layer is by a frame or two on *every
            // block*, and an equality test re-seeks every one of them. Seeking
            // re-primes the engine: the splice chain for WSOLA, the whole phase
            // and overlap-add state for the vocoder. Ninety times a second that
            // is not a stretch, it is a stutter — and it is exactly what the
            // comment here already warned about while the code did it anyway.
            //
            // So there is a tolerance, and inside it the layer simply keeps
            // playing from where it is. Its alignment then lags the ideal by
            // under a hop, which is what the layer offset is measured in and is
            // inaudible; losing the engine's state is not.
            let stored = self.layer_at.get(i).copied();
            let slack = hop.max(1);
            let seeking = match stored {
                Some(s) => at.abs_diff(s) > slack,
                None => true,
            };
            if seeking {
                self.seek_layer(i, at, &lp);
            }
            self.render_layer(i, &mut buf[..take * channels], channels, src, &lp);
            if let Some(slot) = self.layer_at.get_mut(i) {
                // Where the layer actually is now, which after a tolerated
                // mismatch is not where `at` said it should be.
                let from = if seeking { at } else { stored.unwrap_or(at) };
                *slot = from + take as u64;
            }
            for k in 0..take * channels {
                out[skip * channels + k] += buf[k];
            }
        }
        self.layer_buf = buf;

        // Blind square root, exactly as the offline renderer applies it, so the
        // file and the transport are the same sound at every layer count.
        //
        // `out` is the raw sum of `live` layers. Decorrelated, that sum is
        // about root-N times one layer, so dividing by root-N puts it back —
        // which is `layer_gain(N) / N`, not `layer_gain(N)`. Getting that the
        // wrong way round is a factor of N and it is not subtle.
        let lift = crate::grain::layer_gain(live) / live as f32;
        for v in out.iter_mut() {
            *v *= lift;
        }
    }

    fn seek_layer(&mut self, i: usize, at: u64, lp: &StreamParams) {
        let Some(bank) = self.bank.as_mut() else { return };
        match bank.algorithm {
            Algorithm::Wsola => {
                if let Some(e) = bank.wsola.get_mut(i) {
                    e.seek(at, lp.in_frames, &stretch_params(lp), lp.semitones);
                }
            }
            Algorithm::Vocoder => {
                if let Some(e) = bank.vocoder.get_mut(i) {
                    e.seek(at, lp.in_frames, &stretch_params(lp), lp.semitones);
                }
            }
            Algorithm::Pvsola => {
                if let Some(e) = bank.pvsola.get_mut(i) {
                    e.seek(at, lp.in_frames, &stretch_params(lp), &lp.pvsola, lp.semitones);
                }
            }
            Algorithm::Hybrid => {
                let parts = std::sync::Arc::clone(&self.parts);
                if let Some(e) = bank.hybrid.get_mut(i) {
                    e.seek(at, &parts, &stretch_params(lp), lp.hybrid, lp.semitones);
                }
            }
            Algorithm::Granular | Algorithm::SoundHack => {}
        }
    }

    fn render_layer(
        &mut self,
        i: usize,
        out: &mut [f32],
        channels: usize,
        src: &Source,
        lp: &StreamParams,
    ) {
        let parts = std::sync::Arc::clone(&self.parts);
        let Some(bank) = self.bank.as_mut() else {
            out.fill(0.0);
            return;
        };
        match bank.algorithm {
            Algorithm::Wsola => {
                if let Some(e) = bank.wsola.get_mut(i) {
                    e.render_pitched(out, channels, &src.samples, &stretch_params(lp), lp.semitones);
                }
            }
            Algorithm::Vocoder => {
                if let Some(e) = bank.vocoder.get_mut(i) {
                    e.render_pitched(out, channels, &src.samples, &stretch_params(lp), lp.semitones);
                }
            }
            Algorithm::Pvsola => {
                if let Some(e) = bank.pvsola.get_mut(i) {
                    e.render_pitched(
                        out,
                        channels,
                        &src.samples,
                        &stretch_params(lp),
                        &lp.pvsola,
                        lp.semitones,
                    );
                }
            }
            Algorithm::Hybrid => {
                if let Some(e) = bank.hybrid.get_mut(i) {
                    e.render_pitched(
                        out,
                        channels,
                        &parts,
                        &stretch_params(lp),
                        lp.hybrid,
                        lp.semitones,
                    );
                }
            }
            Algorithm::Granular | Algorithm::SoundHack => out.fill(0.0),
        }
    }

    fn render_one(
        &mut self,
        alg: Algorithm,
        out: &mut [f32],
        channels: usize,
        src: &Source,
        sp: &StreamParams,
        events: &mut [GrainEvent],
    ) -> usize {
        match alg {
            Algorithm::Wsola => {
                if let Some(e) = self.wsola.as_mut() {
                    e.render_pitched(out, channels, &src.samples, &stretch_params(sp), sp.semitones);
                }
                0
            }
            Algorithm::Vocoder => {
                if let Some(e) = self.vocoder.as_mut() {
                    e.render_pitched(out, channels, &src.samples, &stretch_params(sp), sp.semitones);
                }
                0
            }
            Algorithm::Pvsola => {
                if let Some(e) = self.pvsola.as_mut() {
                    e.render_pitched(out, channels, &src.samples, &stretch_params(sp), &sp.pvsola, sp.semitones);
                }
                0
            }
            Algorithm::Hybrid => {
                // Nothing separated yet — the pass is still running on another
                // thread. The grain cloud covers the gap rather than silence.
                if self.parts.is_empty() {
                    // It has been left wherever it last played: a seek moves
                    // the engine it is standing in for, not it.
                    if alg == self.current {
                        if self.grain.position() != self.position {
                            self.grain.seek(self.position, sp);
                        }
                        self.stand_in = true;
                    }
                    return self.grain.render(out, channels, src, sp, events);
                }
                let parts = std::sync::Arc::clone(&self.parts);
                if let Some(e) = self.hybrid.as_mut() {
                    e.render_pitched(out, channels, &parts, &stretch_params(sp), sp.hybrid, sp.semitones);
                }
                0
            }
            Algorithm::SoundHack => {
                if let Some(e) = self.soundhack.as_mut() {
                    e.render_pitched(out, channels, &src.samples, &stretch_params(sp), sp.semitones);
                }
                0
            }
            _ => self.grain.render(out, channels, src, sp, events),
        }
    }
}
