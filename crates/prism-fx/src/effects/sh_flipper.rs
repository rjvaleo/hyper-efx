//! ++flipper: SoundHack Delay Trio's reverser (Tom Erbe, v9.11), as its code
//! does it.
//!
//! Every segment, a new head starts at the sample just written and runs
//! backwards, so each segment plays the one before it in reverse. The old head
//! carries on backwards while the new one fades in, linearly, over the
//! overlap. A flip comes when the count reaches the segment length, one sample
//! after it. There is no wet/dry: it is all reversed.
//!
//! Changed from the original: the overlap is a share of the segment. The
//! original took the 0–100 dial as a multiple of the segment, so at its
//! default of 15 the new head reached a fifteenth of its level before the
//! next flip, and every seam stepped. Its tempo lock waits for the rack to
//! have a tempo. The controls glide; the original ramped them across each
//! host block.

use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamSpec, Smoothed};

const FLIP: usize = 0;
const OVERLAP: usize = 1;
const IN_DB: usize = 2;
const OUT_DB: usize = 3;

const CLIP: f32 = 1.5;
/// The old head reads up to two segments back.
const LONGEST_S: f32 = 20.5;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("flipS", "Flip length", 0.1, 10.0, 1.0).log().unit("s"),
    ParamSpec::new("overlap", "Overlap", 0.0, 1.0, 0.15),
    ParamSpec::new("inDb", "In", -20.0, 20.0, 0.0).unit("dB"),
    ParamSpec::new("outDb", "Out", -20.0, 20.0, 0.0).unit("dB"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "sh-flipper",
    code: 13,
    version: 1,
    label: "++flipper",
    category: Category::Delay,
    params: PARAMS,
    panel: Panel {
        // The original: two long sliders and the levels.
        groups: &[
            Group { label: "Flip", keys: &["flipS", "overlap"] },
            Group { label: "Level", keys: &["inDb", "outDb"] },
        ],
        primary: &["flipS", "overlap"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "flipS", x_range: (0.1, 4.0), y: "overlap", y_range: (0.0, 1.0) }),
    },
    source: "SoundHack: ++flipper (Delay Trio 9.11)",
    sound: None,
};

pub struct ShFlipper {
    sample_rate: f32,
    values: [f32; 4],
    flip: Smoothed,
    overlap: Smoothed,
    in_db: Smoothed,
    out_db: Smoothed,
    lines: [Vec<f32>; 2],
    mask: usize,
    write: usize,
    /// The new head and the old one, both running backwards.
    new_head: usize,
    old_head: usize,
    count: u32,
    /// The new head's share.
    fade: f32,
}

impl ShFlipper {
    fn glides(&mut self) -> [(&mut Smoothed, usize); 4] {
        [(&mut self.flip, FLIP), (&mut self.overlap, OVERLAP), (&mut self.in_db, IN_DB), (&mut self.out_db, OUT_DB)]
    }
}

impl Kernel for ShFlipper {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 4];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let glide = |i: usize| Smoothed::new(sample_rate, PARAMS[i].smooth_ms);
        let length = ((sample_rate * LONGEST_S) as usize).next_power_of_two();
        let mut k = ShFlipper {
            sample_rate,
            values,
            flip: glide(FLIP),
            overlap: glide(OVERLAP),
            in_db: glide(IN_DB),
            out_db: glide(OUT_DB),
            lines: [vec![0.0; length], vec![0.0; length]],
            mask: length - 1,
            write: 0,
            new_head: 0,
            old_head: 0,
            count: 0,
            fade: 0.0,
        };
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
        for line in &mut self.lines {
            line.fill(0.0);
        }
        self.write = 0;
        self.new_head = 0;
        self.old_head = 0;
        self.count = 0;
        self.fade = 0.0;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let db = |d: f32| 10f32.powf(d * 0.05);
        let gain_in = db(self.in_db.next());
        let gain_out = db(self.out_db.next());
        let segment = self.flip.next() * self.sample_rate;
        let fade = (self.overlap.next() * segment).max(1.0);

        for (s, out) in frame.iter_mut().enumerate() {
            let line = &mut self.lines[s];
            line[self.write] = *out * gain_in;
            let y = line[self.new_head] * gain_out * self.fade + line[self.old_head] * gain_out * (1.0 - self.fade);
            *out = y.clamp(-CLIP, CLIP);
        }

        if segment <= self.count as f32 {
            self.old_head = self.new_head.wrapping_sub(1) & self.mask;
            self.new_head = self.write;
            self.count = 0;
            self.fade = 0.0;
        } else {
            self.new_head = self.new_head.wrapping_sub(1) & self.mask;
            self.old_head = self.old_head.wrapping_sub(1) & self.mask;
            self.count += 1;
        }
        self.fade = (self.fade + 1.0 / fade).min(1.0);
        self.write = (self.write + 1) & self.mask;
    }

    /// The last segment, played out backwards.
    fn tail_seconds(&self) -> f32 {
        self.values[FLIP] * (1.0 + self.values[OVERLAP])
    }
}
