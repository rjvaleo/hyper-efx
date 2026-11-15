//! Comb Hall: Jezar's Freeverb (2000, public domain), bit for bit.
//!
//! Modelled on the decode of Black Water Reverb, which is Freeverb's
//! `revmodel` unchanged (`sources/reports/001-black-water-reverb.md`): the sum
//! of both inputs times 0.015 into eight lowpass-feedback combs per side
//! (1116…1617 samples, the right side 23 longer), their sum through four of
//! Freeverb's all-passes in series (556, 441, 341, 225 at 0.5), then the
//! width matrix and the dry path.
//!
//! As coded, including what the original does that a redesign would not
//! (SUITE-PLAN D10):
//! - **Fixed sample counts.** The lengths are 44.1 kHz's and are not scaled,
//!   so at 48 kHz the room is shorter and brighter, as it is in the original.
//! - **Freeverb's all-pass** is `b − x` with feedback 0.5, which is not a true
//!   all-pass.
//! - **No smoothing.** A control acts from the next sample.
//! - **Denormals flushed** to zero on each comb read, on the comb's filter
//!   store and on each all-pass read, as `undenormalise` does.
//! - **Freeze** sets every comb to feedback 1 and damping 0 and mutes the
//!   input, so the tail holds.
//!
//! The controls are the original's six, on its own 0..1 scale with its laws:
//! Room size 0.7 + 0.28·v, Damping 0.4·v, Width v, Wet 3·v, Dry 2·v.
//!
//! One point the decode leaves open: it reports that Black Water's Wet setter
//! does not recompute the width matrix, while classic Freeverb's does. This
//! follows classic Freeverb, which the decode says it matches in every other
//! respect: moving Wet is heard at once.

use crate::spec::Role;
use crate::{Category, Descriptor, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const FREEZE: usize = 0;
const ROOM: usize = 1;
const DAMP: usize = 2;
const WIDTH: usize = 3;
const WET: usize = 4;
const DRY: usize = 5;

/// `tuning.h`, as the decode reads them from the DLL (§4.1).
pub const COMBS_L: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
pub const ALLPASSES_L: [usize; 4] = [556, 441, 341, 225];
pub const STEREO_SPREAD: usize = 23;
pub const ALLPASS_FEEDBACK: f32 = 0.5;
pub const FIXED_GAIN: f32 = 0.015;
const SCALE_WET: f32 = 3.0;
const SCALE_DRY: f32 = 2.0;
const SCALE_DAMP: f32 = 0.4;
const SCALE_ROOM: f32 = 0.28;
const OFFSET_ROOM: f32 = 0.7;
const FREEZE_MODE: f32 = 0.5;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("freeze", "Freeze", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0).role(Role::Time),
    ParamSpec::new("roomSize", "Room size", 0.0, 1.0, 0.5).smooth(0.0).role(Role::Time),
    ParamSpec::new("damping", "Damping", 0.0, 1.0, 0.5).smooth(0.0).role(Role::Tone),
    ParamSpec::new("width", "Width", 0.0, 1.0, 1.0).smooth(0.0).role(Role::Space),
    ParamSpec::new("wet", "Wet", 0.0, 1.0, 1.0 / 3.0).smooth(0.0).role(Role::Output),
    ParamSpec::new("dry", "Dry", 0.0, 1.0, 0.0).smooth(0.0).role(Role::Output),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "freeverb",
    code: 102,
    version: 1,
    label: "Comb Hall",
    category: Category::Reverb,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Time", keys: &["roomSize", "freeze"] },
            Group { label: "Tone", keys: &["damping"] },
            Group { label: "Space", keys: &["width"] },
            Group { label: "Output", keys: &["wet", "dry"] },
        ],
        primary: &["roomSize", "damping", "wet"],
        displays: &[],
        pad: Some(PadPair { x: "damping", x_range: (0.0, 1.0), y: "roomSize", y_range: (0.0, 1.0) }),
    },
    source: "Jezar's Freeverb (public domain), as copied bit for bit in Black Water Reverb (sources/reports/001-black-water-reverb.md)",
    sound: None,
};

/// `undenormalise`: a value whose exponent bits are all zero becomes zero.
#[inline]
fn flush(x: f32) -> f32 {
    if x.to_bits() & 0x7f80_0000 == 0 {
        0.0
    } else {
        x
    }
}

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    store: f32,
}

impl Comb {
    fn new(len: usize) -> Comb {
        Comb { buf: vec![0.0; len], idx: 0, store: 0.0 }
    }

    #[inline]
    fn process(&mut self, input: f32, feedback: f32, damp1: f32, damp2: f32) -> f32 {
        let output = flush(self.buf[self.idx]);
        self.store = flush(output * damp2 + self.store * damp1);
        self.buf[self.idx] = input + self.store * feedback;
        self.idx += 1;
        if self.idx >= self.buf.len() {
            self.idx = 0;
        }
        output
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
        self.idx = 0;
        self.store = 0.0;
    }
}

struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

impl Allpass {
    fn new(len: usize) -> Allpass {
        Allpass { buf: vec![0.0; len], idx: 0 }
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let bufout = flush(self.buf[self.idx]);
        self.buf[self.idx] = input + bufout * ALLPASS_FEEDBACK;
        self.idx += 1;
        if self.idx >= self.buf.len() {
            self.idx = 0;
        }
        bufout - input
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
        self.idx = 0;
    }
}

pub struct Freeverb {
    sample_rate: f32,
    values: [f32; 6],
    combs: [[Comb; 8]; 2],
    allpasses: [[Allpass; 4]; 2],
    // `revmodel`'s derived state, as `update` leaves it.
    gain: f32,
    feedback: f32,
    damp1: f32,
    damp2: f32,
    wet1: f32,
    wet2: f32,
    dry: f32,
}

impl Freeverb {
    /// `revmodel::update`.
    fn update(&mut self) {
        let wet = self.values[WET] * SCALE_WET;
        let width = self.values[WIDTH];
        self.wet1 = wet * (width / 2.0 + 0.5);
        self.wet2 = wet * ((1.0 - width) / 2.0);
        if self.values[FREEZE] >= FREEZE_MODE {
            self.feedback = 1.0;
            self.damp1 = 0.0;
            self.gain = 0.0;
        } else {
            self.feedback = self.values[ROOM] * SCALE_ROOM + OFFSET_ROOM;
            self.damp1 = self.values[DAMP] * SCALE_DAMP;
            self.gain = FIXED_GAIN;
        }
        self.damp2 = 1.0 - self.damp1;
        self.dry = self.values[DRY] * SCALE_DRY;
    }
}

impl Kernel for Freeverb {
    fn new(sample_rate: f32) -> Self {
        let mut values = [0.0; 6];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let side = |s: usize| {
            (
                core::array::from_fn(|i| Comb::new(COMBS_L[i] + s)),
                core::array::from_fn(|i| Allpass::new(ALLPASSES_L[i] + s)),
            )
        };
        let (cl, al) = side(0);
        let (cr, ar) = side(STEREO_SPREAD);
        let mut k = Freeverb {
            sample_rate: sample_rate.max(1.0),
            values,
            combs: [cl, cr],
            allpasses: [al, ar],
            gain: 0.0,
            feedback: 0.0,
            damp1: 0.0,
            damp2: 0.0,
            wet1: 0.0,
            wet2: 0.0,
            dry: 0.0,
        };
        k.update();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        self.values[index] = spec.clamp(value);
        self.update();
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    /// `mute`: every buffer cleared — unless frozen, when the held tail stays.
    fn reset(&mut self) {
        if self.values[FREEZE] >= FREEZE_MODE {
            return;
        }
        for c in self.combs.iter_mut().flatten() {
            c.clear();
        }
        for a in self.allpasses.iter_mut().flatten() {
            a.clear();
        }
    }

    /// `revmodel::processreplace`, one frame.
    fn tick(&mut self, frame: &mut [f32; 2]) {
        let (in_l, in_r) = (frame[0], frame[1]);
        let input = (in_l + in_r) * self.gain;
        let (mut out_l, mut out_r) = (0.0f32, 0.0f32);
        let (fb, d1, d2) = (self.feedback, self.damp1, self.damp2);
        for i in 0..8 {
            out_l += self.combs[0][i].process(input, fb, d1, d2);
            out_r += self.combs[1][i].process(input, fb, d1, d2);
        }
        for i in 0..4 {
            out_l = self.allpasses[0][i].process(out_l);
            out_r = self.allpasses[1][i].process(out_r);
        }
        frame[0] = out_l * self.wet1 + out_r * self.wet2 + in_l * self.dry;
        frame[1] = out_r * self.wet1 + out_l * self.wet2 + in_r * self.dry;
    }

    fn tail_seconds(&self) -> f32 {
        // The longest comb, in the samples it has at whatever rate this runs.
        let longest = (COMBS_L[7] + STEREO_SPREAD) as f32 / self.sample_rate;
        super::comb_lines::tail(longest, self.feedback, 0.0, self.values[FREEZE] >= FREEZE_MODE)
    }
}
