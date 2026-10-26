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
    Entry { descriptor: &effects::lxp_chamber::DESCRIPTOR, make: boxed::<effects::LxpChamber> },
    Entry { descriptor: &effects::lxp_hall::DESCRIPTOR, make: boxed::<effects::LxpHall> },
    Entry { descriptor: &effects::lxp_plate::DESCRIPTOR, make: boxed::<effects::LxpPlate> },
    Entry { descriptor: &effects::lxp_room::DESCRIPTOR, make: boxed::<effects::LxpRoom> },
    Entry { descriptor: &effects::hp_low_pass::DESCRIPTOR, make: boxed::<effects::HpLowPass> },
    Entry { descriptor: &effects::hp_high_pass::DESCRIPTOR, make: boxed::<effects::HpHighPass> },
    Entry { descriptor: &effects::hp_band_pass::DESCRIPTOR, make: boxed::<effects::HpBandPass> },
    Entry { descriptor: &effects::hp_band_reject::DESCRIPTOR, make: boxed::<effects::HpBandReject> },
    Entry { descriptor: &effects::hp_more_stereo::DESCRIPTOR, make: boxed::<effects::HpMoreStereo> },
    Entry { descriptor: &effects::hp_medium_room::DESCRIPTOR, make: boxed::<effects::HpMediumRoom> },
    Entry { descriptor: &effects::hp_hall::DESCRIPTOR, make: boxed::<effects::HpHall> },
    Entry { descriptor: &effects::hp_single_delay::DESCRIPTOR, make: boxed::<effects::HpSingleDelay> },
    Entry { descriptor: &effects::hp_echo::DESCRIPTOR, make: boxed::<effects::HpEcho> },
    Entry { descriptor: &effects::hp_echotranz::DESCRIPTOR, make: boxed::<effects::HpEchoTranz> },
    Entry { descriptor: &effects::hp_multi_delay::DESCRIPTOR, make: boxed::<effects::HpMultiDelay> },
    Entry { descriptor: &effects::hp_comb_filter::DESCRIPTOR, make: boxed::<effects::HpCombFilter> },
    Entry { descriptor: &effects::hp_doppler::DESCRIPTOR, make: boxed::<effects::HpDoppler> },
    Entry { descriptor: &effects::hp_chorus::DESCRIPTOR, make: boxed::<effects::HpChorus> },
    Entry { descriptor: &effects::hp_flanger::DESCRIPTOR, make: boxed::<effects::HpFlanger> },
    Entry { descriptor: &effects::hp_ring_modulator::DESCRIPTOR, make: boxed::<effects::HpRingModulator> },
    Entry { descriptor: &effects::hp_vibrato::DESCRIPTOR, make: boxed::<effects::HpVibrato> },
    Entry { descriptor: &effects::hp_tremolo::DESCRIPTOR, make: boxed::<effects::HpTremolo> },
    Entry { descriptor: &effects::hp_frequency_modulator::DESCRIPTOR, make: boxed::<effects::HpFrequencyModulator> },
    Entry { descriptor: &effects::hp_phaser::DESCRIPTOR, make: boxed::<effects::HpPhaser> },
    Entry { descriptor: &effects::hp_pitch_shifter::DESCRIPTOR, make: boxed::<effects::HpPitchShifter> },
    Entry { descriptor: &effects::hp_pan::DESCRIPTOR, make: boxed::<effects::HpPan> },
    Entry { descriptor: &effects::hp_auto_pan::DESCRIPTOR, make: boxed::<effects::HpAutoPan> },
    Entry { descriptor: &effects::hp_stereo_dynamics::DESCRIPTOR, make: boxed::<effects::HpStereoDynamics> },
    Entry { descriptor: &effects::hp_quasi_stereo::DESCRIPTOR, make: boxed::<effects::HpQuasiStereo> },
    Entry { descriptor: &effects::hp_ms_matrix::DESCRIPTOR, make: boxed::<effects::HpMsMatrix> },
    Entry { descriptor: &effects::hp_hyper_stereo::DESCRIPTOR, make: boxed::<effects::HpHyperStereo> },
    Entry { descriptor: &effects::hp_noise_gate::DESCRIPTOR, make: boxed::<effects::HpNoiseGate> },
    Entry { descriptor: &effects::hp_compressor::DESCRIPTOR, make: boxed::<effects::HpCompressor> },
    Entry { descriptor: &effects::hp_sonic_decimator::DESCRIPTOR, make: boxed::<effects::HpSonicDecimator> },
    Entry { descriptor: &effects::invert::DESCRIPTOR, make: boxed::<effects::Invert> },
    Entry { descriptor: &effects::swap::DESCRIPTOR, make: boxed::<effects::Swap> },
    Entry { descriptor: &effects::width::DESCRIPTOR, make: boxed::<effects::Width> },
    Entry { descriptor: &effects::dc::DESCRIPTOR, make: boxed::<effects::Dc> },
    Entry { descriptor: &effects::ring::DESCRIPTOR, make: boxed::<effects::Ring> },
    Entry { descriptor: &effects::rappify::DESCRIPTOR, make: boxed::<effects::Rappify> },
    Entry { descriptor: &effects::boomerang::DESCRIPTOR, make: boxed::<effects::Boomerang> },
    Entry { descriptor: &effects::fit::DESCRIPTOR, make: boxed::<effects::Fit> },
    Entry { descriptor: &effects::gate::DESCRIPTOR, make: boxed::<effects::Gate> },
    Entry { descriptor: &effects::utility::DESCRIPTOR, make: boxed::<effects::Utility> },
    Entry { descriptor: &effects::maximizer::DESCRIPTOR, make: boxed::<effects::Maximizer> },
    Entry { descriptor: &effects::gain::DESCRIPTOR, make: boxed::<effects::Gain> },
    Entry { descriptor: &effects::mgl_dattorro_notch::DESCRIPTOR, make: boxed::<effects::MglDattorroNotch> },
    Entry { descriptor: &effects::mgl_dattorro_resonator::DESCRIPTOR, make: boxed::<effects::MglDattorroResonator> },
    Entry { descriptor: &effects::mgl_regalia::DESCRIPTOR, make: boxed::<effects::MglRegalia> },
    Entry { descriptor: &effects::mgl_chamberlin::DESCRIPTOR, make: boxed::<effects::MglChamberlin> },
    Entry { descriptor: &effects::mgl_white_chorus::DESCRIPTOR, make: boxed::<effects::MglWhiteChorus> },
    Entry { descriptor: &effects::mgl_flanger::DESCRIPTOR, make: boxed::<effects::MglFlanger> },
    Entry { descriptor: &effects::mgl_vibrato::DESCRIPTOR, make: boxed::<effects::MglVibrato> },
    Entry { descriptor: &effects::mgl_pn_noise::DESCRIPTOR, make: boxed::<effects::MglPnNoise> },
    Entry { descriptor: &effects::mgl_pn_noise_eq::DESCRIPTOR, make: boxed::<effects::MglPnNoiseEq> },
    Entry { descriptor: &effects::mgl_dattorro_plate::DESCRIPTOR, make: boxed::<effects::MglDattorroPlate> },
    Entry { descriptor: &effects::mgl_allpass_diffuser::DESCRIPTOR, make: boxed::<effects::MglAllpassDiffuser> },
    Entry { descriptor: &effects::mgl_damping_filter::DESCRIPTOR, make: boxed::<effects::MglDampingFilter> },
    Entry { descriptor: &effects::mgl_leslie::DESCRIPTOR, make: boxed::<effects::MglLeslie> },
    Entry { descriptor: &effects::mgl_single_bit_pn::DESCRIPTOR, make: boxed::<effects::MglSingleBitPn> },
    Entry { descriptor: &effects::mgl_harmonizer::DESCRIPTOR, make: boxed::<effects::MglHarmonizer> },
    Entry { descriptor: &effects::mgl_detune::DESCRIPTOR, make: boxed::<effects::MglDetune> },
    Entry { descriptor: &effects::mgl_doubler::DESCRIPTOR, make: boxed::<effects::MglDoubler> },
    Entry { descriptor: &effects::mgl_doppler::DESCRIPTOR, make: boxed::<effects::MglDoppler> },
    Entry { descriptor: &effects::mgl_dattorro_filter_bank::DESCRIPTOR, make: boxed::<effects::MglDattorroFilterBank> },
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
