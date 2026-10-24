//! Hyperprism Sonic Decimator: Hyperprism-PPC 1.2.1's process 38 (class
//! ESonicDecimator, routine `Decimator`), as its code does it.
//!
//! - **Rate.** A sample-and-hold on a fractional counter: whenever the
//!   counter reaches 1 / rate it takes the current input and drops by that
//!   much, and it gains one a sample. So at 30 % holds of 3 and 4 samples
//!   alternate. No filter before or after. The first sample is the initial
//!   hold, 0.
//! - **Bits.** In the original's 16-bit integers (a float of 1 is 32768): the
//!   held value is truncated to a word, then floored to k = int(bits) bits by
//!   an arithmetic shift, `(v >> (16 − k)) << (16 − k)` — 2^k levels, zero
//!   among them, negatives rounding down — and to k + 1 bits the same way,
//!   and the two are crossfaded by the fractional bit. At 16 bits the k + 1
//!   shift is −1, which the PowerPC's shift turns to 0; the crossfade weight
//!   is then 0 too.
//! - **Smoothing.** Rate and bits glide by Hyperprism's smoothing: every 45th
//!   sample, `a·target + (1 − a)·value`. 100 % wet, no mix.
//!
//! The rate is a fraction of the sample rate, so it needs no scaling at other
//! rates. A mono input in the original is the same as equal sides here.
//!
//! Changed from the original: the word saturates at −32768…32767 instead of
//! wrapping. The original cast to a 16-bit short, so a float of +1 (32768)
//! came out as −32768, and anything past full scale folded over; from 16-bit
//! files it never saw such values, from a float host it does.

use super::hp_stereo_shared::{Stepper, FULL_SCALE};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const RATE: usize = 0;
const BITS: usize = 1;
const SMOOTHING: usize = 2;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("rate", "Sample Rate", 1.0, 100.0, 50.0).unit("%").smooth(0.0),
    ParamSpec::new("bits", "Bit Depth", 1.0, 16.0, 8.0).unit("bits").smooth(0.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 30.0).unit("%").kind(ParamKind::Int).smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-sonic-decimator",
    code: 64,
    version: 1,
    label: "Hyperprism Sonic Decimator",
    category: Category::Distortion,
    params: PARAMS,
    panel: Panel {
        // The original: the X/Y pad, sample rate across and bit depth up, and
        // the smoothing slider in its settings.
        groups: &[
            Group { label: "Decimate", keys: &["rate", "bits"] },
            Group { label: "Settings", keys: &["smoothing"] },
        ],
        primary: &["rate", "bits"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "rate", x_range: (1.0, 100.0), y: "bits", y_range: (1.0, 16.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Sonic Decimator",
    sound: None,
};

/// `(v >> shift) << shift` on a 16-bit word, with the PowerPC's 6-bit shift
/// count: 32 and over shift everything out.
#[inline]
fn floor_to(v: i32, shift: i32) -> i32 {
    let s = shift & 0x3f;
    if s >= 32 {
        0
    } else {
        ((v >> s) << s) as i16 as i32
    }
}

pub struct HpSonicDecimator {
    values: [f32; 3],
    /// Targets: rate as a fraction, bits.
    rate: f32,
    bits: f32,
    rate_s: f32,
    bits_s: f32,
    /// 1 / rate, and the whole bits, as the routine derives them.
    hold_for: f32,
    whole: i32,
    held: [f32; 2],
    count: f32,
    step: Stepper,
}

impl HpSonicDecimator {
    fn targets(&mut self) {
        self.rate = (self.values[RATE] as f64 * 0.01f32 as f64) as f32;
        self.bits = self.values[BITS];
    }

    fn derive(&mut self) {
        self.hold_for = (1.0 / self.rate_s as f64) as f32;
        self.whole = self.bits_s as i32;
    }
}

impl Kernel for HpSonicDecimator {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 3];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpSonicDecimator {
            values,
            rate: 0.0,
            bits: 0.0,
            rate_s: 0.0,
            bits_s: 0.0,
            hold_for: 1.0,
            whole: 16,
            held: [0.0; 2],
            count: 0.0,
            step: Stepper::new(sample_rate, values[SMOOTHING]),
        };
        k.targets();
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        self.values[index] = spec.clamp(value);
        match index {
            SMOOTHING => self.step.set_smoothing(self.values[SMOOTHING]),
            _ => self.targets(),
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    /// `SetProcess`: holds and counter cleared, glides on their targets.
    fn reset(&mut self) {
        self.rate_s = self.rate;
        self.bits_s = self.bits;
        self.derive();
        self.held = [0.0; 2];
        self.count = 0.0;
        self.step.restart();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.step.fires() {
            self.rate_s = self.step.toward(self.rate_s, self.rate);
            self.bits_s = self.step.toward(self.bits_s, self.bits);
            self.derive();
        }
        if self.hold_for <= self.count {
            self.held = [frame[0] * FULL_SCALE, frame[1] * FULL_SCALE];
            self.count -= self.hold_for;
        }
        self.count += 1.0;
        let (coarse, fine) = (16 - self.whole, 15 - self.whole);
        let fraction = (self.bits_s as f64 - self.whole as i16 as f64) as f32;
        for (out, held) in frame.iter_mut().zip(self.held) {
            let v = (held as i32).clamp(i16::MIN as i32, i16::MAX as i32);
            let q1 = floor_to(v, coarse);
            let q2 = floor_to(v, fine);
            *out = ((q2 - q1) as f32 * fraction + q1 as f32) / FULL_SCALE;
        }
    }
}
