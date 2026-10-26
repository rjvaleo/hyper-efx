//! Schroeder reverb: MultiGrainLive's `fx::reverb::Schroeder`, sample for
//! sample.
//!
//! The 1961 design as Moorer drew it in his Fig. 2b: four plain combs side by
//! side, 29.7, 37.1, 41.1 and 43.7 ms (mutually prime, so their repeats do not
//! pile up on one pitch), their mean through two all-passes in series, 5 and
//! 1.7 ms. Every comb feeds back by the one decay gain.
//!
//! - **Size** scales every comb. The buffers are sized once for the largest
//!   room and the read point moves inside them, so a move neither allocates
//!   nor cuts the tail.
//! - **Stereo spread** lengthens the right side's combs by that fraction; the
//!   right side's buffers are 17 % longer to make room.
//! - **Predelay** is a plain delay ahead of the combs, at least one sample.
//! - **Freeze** holds the combs at unity and stops feeding them, which is the
//!   only way to a tail that never ends: a unity loop still being fed would
//!   grow without bound.
//!
//! The original read its controls once per block; here a control acts from
//! the next sample, which is the same thing wherever a host moves it between
//! blocks.

use super::comb_lines::{tail, Line};
use crate::{Category, Descriptor, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const DECAY: usize = 0;
const SIZE: usize = 1;
const DIFFUSION: usize = 2;
const WET: usize = 3;
const DRY: usize = 4;
const SPREAD: usize = 5;
const PREDELAY: usize = 6;
const FREEZE: usize = 7;

const COMBS_MS: [f32; 4] = [29.7, 37.1, 41.1, 43.7];
const ALLPASSES_MS: [f32; 2] = [5.0, 1.7];
/// The largest size the control allows, which the buffers are cut for.
const LARGEST: f32 = 8.0;
/// How much longer each side's buffers are than the one before.
const STRETCH: f32 = 0.17;
const PREDELAY_BUFFER_MS: f32 = 1000.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("decay", "Decay", 0.0, 0.9995, 0.805).smooth(0.0),
    ParamSpec::new("size", "Size", 0.1, 8.0, 1.0).smooth(0.0),
    ParamSpec::new("diffusion", "Diffusion", 0.0, 0.95, 0.7).smooth(0.0),
    ParamSpec::new("wet", "Wet", 0.0, 1.0, 0.3).smooth(0.0),
    ParamSpec::new("dry", "Dry", 0.0, 1.0, 1.0).smooth(0.0),
    ParamSpec::new("spread", "Stereo spread", 0.0, 0.2, 0.05).smooth(0.0),
    ParamSpec::new("predelayMs", "Predelay", 0.0, 1000.0, 0.0).unit("ms").smooth(0.0),
    ParamSpec::new("freeze", "Freeze", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "schroeder-reverb",
    code: 97,
    version: 1,
    label: "Schroeder reverb",
    category: Category::Reverb,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Tank", keys: &["decay", "size", "diffusion", "freeze"] },
            Group { label: "Space", keys: &["spread", "predelayMs"] },
            Group { label: "Mix", keys: &["wet", "dry"] },
        ],
        primary: &["decay", "size", "wet"],
        displays: &[],
        pad: Some(PadPair { x: "wet", x_range: (0.0, 1.0), y: "decay", y_range: (0.0, 0.9995) }),
    },
    source: "MultiGrainLive: Schroeder reverb (fx::reverb::Schroeder)",
    sound: None,
};

pub struct SchroederReverb {
    sample_rate: f32,
    values: [f32; 8],
    combs: [[Line; 4]; 2],
    allpasses: [[Line; 2]; 2],
    pre: [Line; 2],
    /// How far back each comb reads, from size and spread.
    taps: [[usize; 4]; 2],
    /// How far back the predelay reads.
    pre_tap: usize,
}

impl SchroederReverb {
    fn taps(&mut self) {
        let (size, spread, sr) = (self.values[SIZE], self.values[SPREAD], self.sample_rate);
        for ch in 0..2 {
            let side = 1.0 + ch as f32 * spread;
            for (i, tap) in self.taps[ch].iter_mut().enumerate() {
                let want = (COMBS_MS[i] * size * side * 0.001 * sr) as usize;
                *tap = want.clamp(2, self.combs[ch][i].len() - 1);
            }
        }
    }

    fn predelay(&mut self) {
        let pre = ((self.values[PREDELAY] * 0.001 * self.sample_rate) as usize).max(1);
        self.pre_tap = pre.min(self.pre[0].len() - 1);
    }
}

impl Kernel for SchroederReverb {
    fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let mut values = [0.0; 8];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let bank = |ch: usize| {
            let stretch = 1.0 + ch as f32 * STRETCH;
            core::array::from_fn(|i| Line::ms(COMBS_MS[i] * LARGEST * stretch, sr))
        };
        let mut k = SchroederReverb {
            sample_rate: sr,
            values,
            combs: [bank(0), bank(1)],
            allpasses: core::array::from_fn(|_| core::array::from_fn(|i| Line::ms(ALLPASSES_MS[i], sr))),
            pre: core::array::from_fn(|_| Line::ms(PREDELAY_BUFFER_MS, sr)),
            taps: [[2; 4]; 2],
            pre_tap: 1,
        };
        k.taps();
        k.predelay();
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
            _ => {}
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        let combs = self.combs.iter_mut().flatten();
        let allpasses = self.allpasses.iter_mut().flatten();
        for line in combs.chain(allpasses).chain(self.pre.iter_mut()) {
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
            for (comb, &tap) in self.combs[ch].iter_mut().zip(&self.taps[ch]) {
                let y = comb.read(tap);
                comb.write(if frozen { y } else { delayed + y * g });
                sum += y;
            }
            sum /= 4.0;
            let mut y = sum;
            for a in &mut self.allpasses[ch] {
                y = a.allpass(y, diffusion);
            }
            *out = x * dry + y * wet;
        }
    }

    fn tail_seconds(&self) -> f32 {
        let longest = COMBS_MS[3] * self.values[SIZE] * (1.0 + self.values[SPREAD]) * 0.001;
        tail(longest, self.values[DECAY], self.values[PREDELAY] * 0.001, self.values[FREEZE] >= 0.5)
    }
}
