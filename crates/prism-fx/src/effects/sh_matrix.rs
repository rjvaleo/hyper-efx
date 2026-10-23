//! ++matrix: SoundHack Freesound's mid/side matrix (Tom Erbe, v9.10), as its
//! code does it.
//!
//! One switch, LR>MS or MS>LR:
//!
//! - **Encode** (LR>MS): left becomes mid, `½(L + R) × mid gain`, and right
//!   becomes side, `½(L − R) × side gain`. Not clipped, so a sound can be
//!   encoded, processed as mid and side, and decoded again.
//! - **Decode** (MS>LR): left is `M × mid + S × side`, right `M × mid −
//!   S × side`, clipped at ±1.5.
//!
//! Input and output gain wrap both. Encode then decode at unity gives the
//! sound back. The original steps its mid and side gains at each host block;
//! here they glide, so the result does not depend on the block size.

use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec, Smoothed};

const MID: usize = 0;
const SIDE: usize = 1;
const DIRECTION: usize = 2;
const IN_DB: usize = 3;
const OUT_DB: usize = 4;

const CLIP: f32 = 1.5;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("midDb", "Mid", -12.0, 12.0, 0.0).unit("dB"),
    ParamSpec::new("sideDb", "Side", -12.0, 12.0, 0.0).unit("dB"),
    ParamSpec::new("direction", "Direction", 0.0, 1.0, 0.0).kind(ParamKind::Choice(&["LR>MS", "MS>LR"])).smooth(0.0),
    ParamSpec::new("inDb", "In", -20.0, 20.0, 0.0).unit("dB"),
    ParamSpec::new("outDb", "Out", -20.0, 20.0, 0.0).unit("dB"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "sh-matrix",
    code: 9,
    version: 1,
    label: "++matrix",
    category: Category::Stereo,
    params: PARAMS,
    panel: Panel {
        // The original: mid and side dials, the direction switch, in and out.
        groups: &[
            Group { label: "Matrix", keys: &["midDb", "sideDb", "direction"] },
            Group { label: "Level", keys: &["inDb", "outDb"] },
        ],
        primary: &["midDb", "sideDb", "direction"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "midDb", x_range: (-12.0, 12.0), y: "sideDb", y_range: (-12.0, 12.0) }),
    },
    source: "SoundHack: ++matrix (Freesound 9.10)",
    sound: None,
};

pub struct ShMatrix {
    values: [f32; 5],
    mid: Smoothed,
    side: Smoothed,
    in_db: Smoothed,
    out_db: Smoothed,
}

impl ShMatrix {
    fn glides(&mut self) -> [(&mut Smoothed, usize); 4] {
        [(&mut self.mid, MID), (&mut self.side, SIDE), (&mut self.in_db, IN_DB), (&mut self.out_db, OUT_DB)]
    }
}

impl Kernel for ShMatrix {
    fn new(sample_rate: f32) -> Self {
        let mut values = [0.0; 5];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let glide = |i: usize| Smoothed::new(sample_rate, PARAMS[i].smooth_ms);
        let mut k = ShMatrix { values, mid: glide(MID), side: glide(SIDE), in_db: glide(IN_DB), out_db: glide(OUT_DB) };
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        let v = spec.clamp(value);
        self.values[index] = v;
        for (glide, i) in self.glides() {
            if i == index {
                glide.set(v);
            }
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        let values = self.values;
        for (glide, i) in self.glides() {
            glide.jump(values[i]);
        }
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let db = |d: f32| 10f32.powf(d * 0.05);
        let (mid, side) = (db(self.mid.next()), db(self.side.next()));
        let (gin, gout) = (db(self.in_db.next()), db(self.out_db.next()));
        let (a, b) = (frame[0] * gin, frame[1] * gin);
        if self.values[DIRECTION] < 0.5 {
            frame[0] = (a + b) * mid * 0.5 * gout;
            frame[1] = gout * ((a - b) * side * 0.5);
        } else {
            frame[0] = ((a * mid + b * side) * gout).clamp(-CLIP, CLIP);
            frame[1] = (gout * (a * mid - b * side)).clamp(-CLIP, CLIP);
        }
    }
}
