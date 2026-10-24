//! The effects themselves, one file each. Each file holds its descriptor and
//! its kernel; `CATALOG` in the crate root lists them.

pub mod convolution;
pub mod decimator;
pub mod echo;
pub mod hp_comb_filter;
mod hp_delay_law;
pub mod hp_doppler;
pub mod hp_echo;
pub mod hp_echotranz;
pub mod hp_multi_delay;
pub mod hp_single_delay;
pub mod multi_delay;
pub mod sh_bubbler;
pub mod sh_chebyshev;
pub mod sh_compand;
pub mod sh_decimate;
pub mod sh_delay;
pub mod sh_flipper;
pub mod sh_matrix;
pub mod sh_phasor;
pub mod sh_pitchdelay;
pub mod spectral_dynamics;
pub mod stereo_matrix;
pub mod tremolo;

pub use convolution::Convolution;
pub use decimator::Decimator;
pub use echo::Echo;
pub use hp_comb_filter::HpCombFilter;
pub use hp_doppler::HpDoppler;
pub use hp_echo::HpEcho;
pub use hp_echotranz::HpEchoTranz;
pub use hp_multi_delay::HpMultiDelay;
pub use hp_single_delay::HpSingleDelay;
pub use multi_delay::MultiDelay;
pub use sh_bubbler::ShBubbler;
pub use sh_chebyshev::ShChebyshev;
pub use sh_compand::ShCompand;
pub use sh_decimate::ShDecimate;
pub use sh_delay::ShDelay;
pub use sh_flipper::ShFlipper;
pub use sh_matrix::ShMatrix;
pub use sh_phasor::ShPhasor;
pub use sh_pitchdelay::ShPitchDelay;
pub use spectral_dynamics::SpectralDynamics;
pub use stereo_matrix::StereoMatrix;
pub use tremolo::Tremolo;
