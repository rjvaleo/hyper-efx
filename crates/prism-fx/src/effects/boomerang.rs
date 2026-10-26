//! Boomerang: MultiGrainLive's live reverse boomerang (`fx::shape::ReverseMix`),
//! moved here unchanged.
//!
//! The input is written into a two-second ring, and read back from a distance
//! behind the write head that grows by two frames per frame — so the read
//! point walks backwards through the recent past at normal speed — and
//! returns to zero once it reaches the throw. Where the read passes the write
//! head the two sides of the jump are faded out over a sixteenth of the throw
//! (at least 32 frames). The reversed signal is blended with the dry by the
//! mix. The throw is held to 64 frames…the ring's length.

use crate::{Category, Descriptor, Group, Kernel, Panel, ParamSpec};

const THROW: usize = 0;
const MIX: usize = 1;

/// The ring's length, in seconds: the longest throw.
const MAX_THROW_S: f32 = 2.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("throwMs", "Throw", 20.0, 2000.0, 400.0).log().unit("ms").smooth(0.0),
    ParamSpec::new("mix", "Mix", 0.0, 1.0, 0.5).smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "boomerang",
    code: 71,
    version: 1,
    label: "Boomerang",
    category: Category::Delay,
    params: PARAMS,
    panel: Panel {
        groups: &[Group { label: "Reverse", keys: &["throwMs", "mix"] }],
        primary: &["throwMs", "mix"],
        displays: &[],
        pad: None,
    },
    source: "MultiGrainLive: Boomerang (fx::shape::ReverseMix)",
    sound: None,
};

pub struct Boomerang {
    sample_rate: f32,
    values: [f32; 2],
    throw: usize,
    /// Interleaved stereo, `frames` long.
    ring: Vec<f32>,
    frames: usize,
    write: usize,
    read: f32,
}

impl Boomerang {
    fn derive(&mut self) {
        let ms = self.values[THROW].clamp(20.0, 2000.0);
        self.throw = (((ms / 1000.0) * self.sample_rate) as usize).clamp(64, self.frames);
    }
}

impl Kernel for Boomerang {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let frames = ((MAX_THROW_S * sample_rate) as usize).max(64);
        let mut k = Boomerang {
            sample_rate,
            values: [PARAMS[THROW].default, PARAMS[MIX].default],
            throw: 64,
            ring: vec![0.0; frames * 2],
            frames,
            write: 0,
            read: 0.0,
        };
        k.derive();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(spec) = PARAMS.get(index) {
            self.values[index] = spec.clamp(value);
            self.derive();
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.ring.fill(0.0);
        self.write = 0;
        self.read = 0.0;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let mix = self.values[MIX].clamp(0.0, 1.0);
        let throw = self.throw;
        self.ring[self.write * 2] = frame[0];
        self.ring[self.write * 2 + 1] = frame[1];

        let d = (self.read as usize).min(throw.saturating_sub(1));
        let at = (self.write + self.frames - d) % self.frames;
        let edge = (throw / 16).max(32);
        let near = d.min(throw.saturating_sub(d));
        let fade = (near as f32 / edge as f32).clamp(0.0, 1.0);
        for (ch, s) in frame.iter_mut().enumerate() {
            let v = self.ring[at * 2 + ch] * fade;
            *s = *s * (1.0 - mix) + v * mix;
        }

        self.write = (self.write + 1) % self.frames;
        self.read += 2.0;
        if self.read >= throw as f32 {
            self.read = 0.0;
        }
    }

    fn tail_seconds(&self) -> f32 {
        self.throw as f32 / self.sample_rate
    }
}
