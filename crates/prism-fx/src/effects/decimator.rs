//! Decimator: fewer samples and fewer bits.
//!
//! The sample rate drops by holding: a held sample is the input sample itself,
//! kept until the next one is taken, with no anti-alias filter before it and
//! no smoothing after. The aliasing and the steps are the sound.
//!
//! The bit depth drops by rounding down to one of 2^bits levels set midway
//! between steps, so every depth is symmetric and 1 bit is a pure two-level
//! signal. The cost of a level at the middle is that silence becomes a level
//! half a step up, which at low depths is a small offset. 24 bits is the full
//! resolution of the f32 path, and passes the audio through untouched.

use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec, Smoothed};

const RATE: usize = 0;
const BITS: usize = 1;
const MIX: usize = 2;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("rate", "Sample rate", 1.0, 100.0, 100.0).log().unit("%"),
    ParamSpec::new("bits", "Bit depth", 1.0, 24.0, 24.0).kind(ParamKind::Int),
    ParamSpec::new("mix", "Mix", 0.0, 1.0, 1.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "decimator",
    code: 2,
    version: 1,
    label: "Decimator",
    category: Category::Distortion,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Resolution", keys: &["rate", "bits"] },
            Group { label: "Output", keys: &["mix"] },
        ],
        primary: &["rate", "bits"],
        displays: &[Display::Scope],
        // Hyperprism's pad: sample rate across, bit depth up.
        pad: Some(PadPair { x: "rate", x_range: (1.0, 100.0), y: "bits", y_range: (1.0, 24.0) }),
    },
    source: "Hyperprism: Sonic Decimator",
    sound: None,
};

/// At or above this, the bits are the f32 path's own and nothing is rounded.
const FULL_BITS: f32 = 24.0;

pub struct Decimator {
    values: [f32; 3],
    rate: Smoothed,
    /// Glided, so a moved depth sweeps through the fractional depths between
    /// rather than stepping.
    bits: Smoothed,
    mix: Smoothed,
    /// How far through the current hold, in input samples. A new sample is
    /// taken each time it passes 1.
    due: f32,
    held: [f32; 2],
    /// Levels each side of zero for `levels_bits`, kept so `exp2` runs only
    /// while the depth is moving.
    levels: f32,
    levels_bits: f32,
}

impl Decimator {
    fn land(&mut self) {
        self.rate.jump(self.values[RATE]);
        self.bits.jump(self.values[BITS]);
        self.mix.jump(self.values[MIX]);
    }

    fn crush(&mut self, x: f32, bits: f32) -> f32 {
        if bits >= FULL_BITS {
            return x;
        }
        if bits != self.levels_bits {
            self.levels = (bits - 1.0).exp2();
            self.levels_bits = bits;
        }
        let q = self.levels;
        let step = (x * q).floor().clamp(-q, q - 1.0);
        (step + 0.5) / q
    }
}

impl Kernel for Decimator {
    fn new(sample_rate: f32) -> Self {
        let glide = |i: usize| Smoothed::new(sample_rate, PARAMS[i].smooth_ms);
        let mut k = Decimator {
            values: [PARAMS[RATE].default, PARAMS[BITS].default, PARAMS[MIX].default],
            rate: glide(RATE),
            bits: glide(BITS),
            mix: glide(MIX),
            due: 1.0,
            held: [0.0; 2],
            levels: 1.0,
            levels_bits: 1.0,
        };
        k.land();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        let v = spec.clamp(value);
        self.values[index] = v;
        match index {
            RATE => self.rate.set(v),
            BITS => self.bits.set(v),
            _ => self.mix.set(v),
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.land();
        // Due at once, so the first frame after a reset is taken, not held.
        self.due = 1.0;
        self.held = [0.0; 2];
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let step = self.rate.next() / 100.0;
        let bits = self.bits.next();
        let mix = self.mix.next();

        if self.due >= 1.0 {
            self.due -= 1.0;
            self.held = *frame;
        }
        self.due += step;

        for (side, s) in frame.iter_mut().enumerate() {
            let wet = self.crush(self.held[side], bits);
            *s = *s * (1.0 - mix) + wet * mix;
        }
    }
}
