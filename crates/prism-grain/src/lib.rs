//! MultiGrainLive's sound engine, shared with idMLab.
//!
//! The five ways it plays a file — grains, WSOLA, a phase vocoder, PVSOLA and
//! the hybrid of the three — each as an offline render and as a streamer, the
//! `Stretcher` that holds all five resident and crossfades between them, and
//! the `BlockRenderer` that plays a grain stream a block at a time.
//!
//! Moved here from MultiGrainLive's `fx` and `engine` crates unchanged, so
//! both programs run the same engine. MultiGrainLive re-exports these modules
//! under their old paths; idMLab wraps the `Stretcher` as a rack module. The
//! device, the threads and the locks stayed in MultiGrainLive: everything here
//! is plain DSP with no dependencies, and builds for wasm32.

pub mod decompose;
pub mod fft;
pub mod grain;
pub mod hstream;
pub mod hybrid;
pub mod noise;
pub mod nstream;
pub mod pstream;
pub mod pvsola;
pub mod render;
pub mod stream;
pub mod stretch;
pub mod stretcher;
pub mod transient;
pub mod tuning;
pub mod vocoder;
pub mod vstream;

pub use grain::{Grain, GrainStream, StreamParams};
pub use render::{BlockRenderer, Source};
pub use stretch::Stretch;
pub use stretcher::Stretcher;
pub use vocoder::Settings as VocoderSettings;

/// Progress through a long offline render, and a way to stop it.
///
/// The argument is **frames produced since the last call**, not a total. Only
/// the caller knows how many passes an engine will make, so accumulating is its
/// job; an engine just says what it did.
/// Returning `false` means "give up" — see [`tick`].
pub type Progress<'a> = Option<&'a (dyn Fn(u64) -> bool + Sync)>;

/// Report `n` more frames. `false` back means the caller wants to stop.
///
/// Stopping leaves the rest of the output buffer as the zeros it was allocated
/// with, so the length is still right and the caller gets a partial render
/// rather than a corrupt one.
#[inline]
#[must_use]
pub fn tick(p: Progress, n: u64) -> bool {
    match p {
        Some(f) => f(n),
        None => true,
    }
}
