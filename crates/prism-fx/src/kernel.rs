//! The trait every effect implements.
//!
//! An effect is stereo, in place, one frame at a time. idMLab calls it a frame
//! at a time because its graph is one sample deep; MultiGrainLive calls the block
//! method, whose default is the frame method in a loop. An effect that gains from
//! blocks may override `process`, and the contract tests hold the two to the same
//! output.

use crate::spec::Descriptor;

pub trait Kernel: Send {
    /// Allocate everything here. Nothing allocates after this.
    fn new(sample_rate: f32) -> Self
    where
        Self: Sized;

    fn descriptor() -> &'static Descriptor
    where
        Self: Sized;

    /// Set a parameter by index, in real units. The kernel clamps it with its
    /// spec and glides to it; an index past the end is ignored.
    fn set_param(&mut self, index: usize, value: f32);

    /// The value last set, after clamping.
    fn param(&self, index: usize) -> f32;

    /// Clear all state, as if just made, keeping the parameter values. Glides
    /// land on their targets.
    fn reset(&mut self);

    /// One stereo frame, in place.
    fn tick(&mut self, frame: &mut [f32; 2]);

    /// A block, planar, in place. Must equal `tick` over the same frames.
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut frame = [*l, *r];
            self.tick(&mut frame);
            *l = frame[0];
            *r = frame[1];
        }
    }

    /// Hand over the sound the descriptor's `sound` names: interleaved,
    /// `channels` wide, at its own rate. Called off the audio thread; this is
    /// the one place after `new` an effect may allocate. An effect that takes
    /// no sound ignores it.
    fn set_sound(&mut self, _samples: &[f32], _channels: usize, _sample_rate: f32) {}

    /// How far the output lags the input, for latency compensation.
    fn latency_frames(&self) -> u32 {
        0
    }

    /// How long the effect keeps sounding after its input stops.
    fn tail_seconds(&self) -> f32 {
        0.0
    }
}

/// `Kernel` as a host holds it: `Box<dyn DynKernel>`. Every kernel is one.
pub trait DynKernel: Send {
    fn descriptor(&self) -> &'static Descriptor;
    fn set_param(&mut self, index: usize, value: f32);
    fn param(&self, index: usize) -> f32;
    fn reset(&mut self);
    fn tick(&mut self, frame: &mut [f32; 2]);
    fn process(&mut self, left: &mut [f32], right: &mut [f32]);
    fn set_sound(&mut self, samples: &[f32], channels: usize, sample_rate: f32);
    fn latency_frames(&self) -> u32;
    fn tail_seconds(&self) -> f32;
}

impl<K: Kernel> DynKernel for K {
    fn descriptor(&self) -> &'static Descriptor {
        K::descriptor()
    }
    fn set_param(&mut self, index: usize, value: f32) {
        Kernel::set_param(self, index, value)
    }
    fn param(&self, index: usize) -> f32 {
        Kernel::param(self, index)
    }
    fn reset(&mut self) {
        Kernel::reset(self)
    }
    fn tick(&mut self, frame: &mut [f32; 2]) {
        Kernel::tick(self, frame)
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        Kernel::process(self, left, right)
    }
    fn set_sound(&mut self, samples: &[f32], channels: usize, sample_rate: f32) {
        Kernel::set_sound(self, samples, channels, sample_rate)
    }
    fn latency_frames(&self) -> u32 {
        Kernel::latency_frames(self)
    }
    fn tail_seconds(&self) -> f32 {
        Kernel::tail_seconds(self)
    }
}
