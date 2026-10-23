//! Audio effects written once, for MultiGrainLive and idMLab.
//!
//! Each effect is a [`Kernel`] — stereo, in place, a frame at a time — with a
//! [`Descriptor`] that holds its parameters and its shared panel. Hosts find
//! effects in [`CATALOG`] and build them with [`make`]. MultiGrainLive drives a
//! kernel a block at a time from its rack; idMLab drives it a sample at a time
//! from its graph, compiled to wasm32. See MultiGrainLive's `docs/SHARED-FX.md`.
//!
//! Rules every effect keeps, checked by [`contract`] over the whole catalogue:
//! frames and blocks agree, block size does not matter, every control reaches
//! the output, the output stays finite, reset is a fresh start, and nothing
//! allocates after `new`.

#![forbid(unsafe_code)]

pub mod contract;
pub mod effects;
pub mod fft;
mod kernel;
mod smooth;
mod spec;

pub use kernel::{DynKernel, Kernel};
pub use smooth::Smoothed;
pub use spec::{Category, Descriptor, Display, Group, PadPair, Panel, ParamKind, ParamSpec, SoundInput};

/// One effect in the catalogue: what it is, and how to build it.
#[derive(Clone, Copy)]
pub struct Entry {
    pub descriptor: &'static Descriptor,
    pub make: fn(sample_rate: f32) -> Box<dyn DynKernel>,
}

/// Every effect, in the order hosts list them. Ids and codes are unique.
pub static CATALOG: &[Entry] = &[
    Entry { descriptor: &effects::tremolo::DESCRIPTOR, make: boxed::<effects::Tremolo> },
    Entry { descriptor: &effects::decimator::DESCRIPTOR, make: boxed::<effects::Decimator> },
    Entry { descriptor: &effects::echo::DESCRIPTOR, make: boxed::<effects::Echo> },
    Entry { descriptor: &effects::multi_delay::DESCRIPTOR, make: boxed::<effects::MultiDelay> },
    Entry { descriptor: &effects::stereo_matrix::DESCRIPTOR, make: boxed::<effects::StereoMatrix> },
    Entry { descriptor: &effects::spectral_dynamics::DESCRIPTOR, make: boxed::<effects::SpectralDynamics> },
    Entry { descriptor: &effects::convolution::DESCRIPTOR, make: boxed::<effects::Convolution> },
    Entry { descriptor: &effects::sh_decimate::DESCRIPTOR, make: boxed::<effects::ShDecimate> },
    Entry { descriptor: &effects::sh_matrix::DESCRIPTOR, make: boxed::<effects::ShMatrix> },
    Entry { descriptor: &effects::sh_phasor::DESCRIPTOR, make: boxed::<effects::ShPhasor> },
    Entry { descriptor: &effects::sh_delay::DESCRIPTOR, make: boxed::<effects::ShDelay> },
    Entry { descriptor: &effects::sh_pitchdelay::DESCRIPTOR, make: boxed::<effects::ShPitchDelay> },
    Entry { descriptor: &effects::sh_flipper::DESCRIPTOR, make: boxed::<effects::ShFlipper> },
    Entry { descriptor: &effects::sh_bubbler::DESCRIPTOR, make: boxed::<effects::ShBubbler> },
    Entry { descriptor: &effects::sh_chebyshev::DESCRIPTOR, make: boxed::<effects::ShChebyshev> },
    Entry { descriptor: &effects::sh_compand::DESCRIPTOR, make: boxed::<effects::ShCompand> },
];

fn boxed<K: Kernel + 'static>(sample_rate: f32) -> Box<dyn DynKernel> {
    Box::new(K::new(sample_rate))
}

/// The catalogue entry with this id.
pub fn find(id: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|e| e.descriptor.id == id)
}

/// Build the effect with this id at this sample rate.
pub fn make(id: &str, sample_rate: f32) -> Option<Box<dyn DynKernel>> {
    find(id).map(|e| (e.make)(sample_rate))
}
