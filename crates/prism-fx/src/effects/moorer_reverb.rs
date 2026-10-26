//! Moorer reverb: MultiGrainLive's `fx::reverb::Moorer`, sample for sample.
//!
//! James A. Moorer's 1979 reverberator ("About This Reverberation
//! Business"): six combs side by side with a one-pole low-pass inside each
//! loop for the air's absorption, their mean through one 6 ms all-pass.
//!
//! - **Combs.** 50, 56, 61, 68, 72 and 78 ms (Moorer's Table 2). Each comb's
//!   damping `g₁` comes from the table's 25 kHz and 50 kHz columns, read
//!   between them by sample rate and held flat outside, then scaled by **Air
//!   absorption** and kept under 0.98. The loop multiplies the low-passed
//!   signal by the decay `g`, so the loop gain is `g(1 − g₁)` and turning the
//!   damping up can never make it unstable.
//! - **Size** scales every comb; the buffers are cut once for the largest room
//!   and the read point moves inside them.
//! - **Stereo spread** lengthens the right side's combs by that fraction; its
//!   buffers are 20 % longer to make room.
//! - **Diffusion** is the all-pass gain (Moorer's "0.7 or so"); its 6 ms
//!   length is fixed, as he insisted.
//! - **Freeze** holds the loops at unity, damping and all taken out, and stops
//!   feeding them.
//!
//! The original read its controls once per block; here a control acts from
//! the next sample, which is the same thing wherever a host moves it between
//! blocks.

use super::comb_lines::{tail, Line};
use crate::{Category, Descriptor, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const DECAY: usize = 0;
const DAMPING: usize = 1;
const SIZE: usize = 2;
const PREDELAY: usize = 3;
const SPREAD: usize = 4;
const WET: usize = 5;
const DRY: usize = 6;
const DIFFUSION: usize = 7;
const FREEZE: usize = 8;

/// Delay in ms, then the damping at 25 kHz and at 50 kHz: Moorer's Table 2.
const COMBS: [(f32, f32, f32); 6] = [
    (50.0, 0.24, 0.46),
    (56.0, 0.26, 0.48),
    (61.0, 0.28, 0.50),
    (68.0, 0.29, 0.52),
    (72.0, 0.30, 0.53),
    (78.0, 0.32, 0.55),
];
const ALLPASS_MS: f32 = 6.0;
const LARGEST: f32 = 8.0;
const STRETCH: f32 = 0.2;
const PREDELAY_BUFFER_MS: f32 = 1000.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("decay", "Decay", 0.0, 0.9995, 0.83).smooth(0.0),
    ParamSpec::new("damping", "Air absorption", 0.0, 2.0, 1.0).smooth(0.0),
    ParamSpec::new("size", "Size", 0.1, 8.0, 1.0).smooth(0.0),
    ParamSpec::new("predelayMs", "Predelay", 0.0, 1000.0, 0.0).unit("ms").smooth(0.0),
    ParamSpec::new("spread", "Stereo spread", 0.0, 0.2, 0.06).smooth(0.0),
    ParamSpec::new("wet", "Wet", 0.0, 1.0, 0.3).smooth(0.0),
    ParamSpec::new("dry", "Dry", 0.0, 1.0, 1.0).smooth(0.0),
    ParamSpec::new("diffusion", "Diffusion", 0.0, 0.9, 0.7).smooth(0.0),
    ParamSpec::new("freeze", "Freeze", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "moorer-reverb",
    code: 98,
    version: 1,
    label: "Moorer reverb",
    category: Category::Reverb,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Tank", keys: &["decay", "damping", "size", "diffusion", "freeze"] },
            Group { label: "Space", keys: &["predelayMs", "spread"] },
            Group { label: "Mix", keys: &["wet", "dry"] },
        ],
        primary: &["decay", "damping", "size", "wet"],
        displays: &[],
        pad: Some(PadPair { x: "wet", x_range: (0.0, 1.0), y: "decay", y_range: (0.0, 0.9995) }),
    },
    source: "MultiGrainLive: Moorer reverb (fx::reverb::Moorer)",
    sound: None,
};

/// One comb: its line and the low-pass inside its loop.
struct Comb {
    line: Line,
    lp: f32,
}

pub struct MoorerReverb {
    sample_rate: f32,
    values: [f32; 9],
    combs: [[Comb; 6]; 2],
    allpasses: [Line; 2],
    pre: [Line; 2],
    taps: [[usize; 6]; 2],
    pre_tap: usize,
    /// Each comb's in-loop damping, `g₁`.
    damping: [f32; 6],
}

impl MoorerReverb {
    fn taps(&mut self) {
        let (size, spread, sr) = (self.values[SIZE], self.values[SPREAD], self.sample_rate);
        for ch in 0..2 {
            let side = 1.0 + ch as f32 * spread;
            for (i, tap) in self.taps[ch].iter_mut().enumerate() {
                let want = (COMBS[i].0 * size * side * 0.001 * sr) as usize;
                *tap = want.clamp(2, self.combs[ch][i].line.len() - 1);
            }
        }
    }

    fn predelay(&mut self) {
        let pre = ((self.values[PREDELAY] * 0.001 * self.sample_rate) as usize).max(1);
        self.pre_tap = pre.min(self.pre[0].len() - 1);
    }

    /// Between the table's two columns by sample rate, held flat outside.
    fn damping(&mut self) {
        let t = ((self.sample_rate - 25_000.0) / 25_000.0).clamp(0.0, 1.0);
        let scale = self.values[DAMPING];
        for (g1, &(_, lo, hi)) in self.damping.iter_mut().zip(&COMBS) {
            *g1 = ((lo + (hi - lo) * t) * scale).clamp(0.0, 0.98);
        }
    }
}

impl Kernel for MoorerReverb {
    fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let mut values = [0.0; 9];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let bank = |ch: usize| {
            let stretch = 1.0 + ch as f32 * STRETCH;
            core::array::from_fn(|i| Comb { line: Line::ms(COMBS[i].0 * LARGEST * stretch, sr), lp: 0.0 })
        };
        let mut k = MoorerReverb {
            sample_rate: sr,
            values,
            combs: [bank(0), bank(1)],
            allpasses: core::array::from_fn(|_| Line::ms(ALLPASS_MS, sr)),
            pre: core::array::from_fn(|_| Line::ms(PREDELAY_BUFFER_MS, sr)),
            taps: [[2; 6]; 2],
            pre_tap: 1,
            damping: [0.0; 6],
        };
        k.taps();
        k.predelay();
        k.damping();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        self.values[index] = spec.clamp(value);
        match index {
            SIZE | SPREAD => self.taps(),
            PREDELAY => self.predelay(),
            DAMPING => self.damping(),
            _ => {}
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        for comb in self.combs.iter_mut().flatten() {
            comb.line.clear();
            comb.lp = 0.0;
        }
        for line in self.allpasses.iter_mut().chain(self.pre.iter_mut()) {
            line.clear();
        }
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let frozen = self.values[FREEZE] >= 0.5;
        let g = if frozen { 1.0 } else { self.values[DECAY] };
        let (diffusion, wet, dry) = (self.values[DIFFUSION], self.values[WET], self.values[DRY]);
        for (ch, out) in frame.iter_mut().enumerate() {
            let x = *out;
            let delayed = self.pre[ch].delay(x, self.pre_tap);
            let mut sum = 0.0;
            for ((comb, &tap), &g1) in self.combs[ch].iter_mut().zip(&self.taps[ch]).zip(&self.damping) {
                let y = comb.line.read(tap);
                comb.lp = y * (1.0 - g1) + comb.lp * g1;
                // Frozen, the damping leaves the loop too: a lossy filter in
                // a unity loop would still decay.
                comb.line.write(if frozen { y } else { delayed + comb.lp * g });
                sum += y;
            }
            sum /= 6.0;
            *out = x * dry + self.allpasses[ch].allpass(sum, diffusion) * wet;
        }
    }

    fn tail_seconds(&self) -> f32 {
        let longest = COMBS[5].0 * self.values[SIZE] * (1.0 + self.values[SPREAD]) * 0.001;
        tail(longest, self.values[DECAY], self.values[PREDELAY] * 0.001, self.values[FREEZE] >= 0.5)
    }
}
