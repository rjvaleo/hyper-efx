//! ++decimate: SoundHack Freesound's decimator (Tom Erbe, v9.10), as its code
//! does it.
//!
//! It works in frames of 256 samples, so the sound comes out exactly one frame
//! late. When a frame fills, it is cut into groups of N samples (N snaps to a
//! power of two, 1–256); each group's mean, driven by the input gain, becomes
//! a 32-bit integer and loses its low bits by masking — which rounds toward
//! minus infinity, so one bit is −1 or 0 — and is held for the group.
//!
//! A fractional depth blends between the `bits` and `bits + 1` floors and cuts
//! the result toward zero. The input gain is the drive: it is read once per
//! frame and acts before the quantiser, so it moves the sound against the
//! steps. The output gain follows, and the result is clipped at ±1.5. No
//! filter, no dither: the steps and the aliasing are the sound.
//!
//! Fixed from the original: it reports its frame as latency (the plug-in did
//! not tell the host), a full-scale mean stays at full scale (the plug-in's
//! integer conversion wrapped it to −1), and both sides are clipped (the
//! plug-in clipped only the right).

use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec, Smoothed};

const DEPTH: usize = 0;
const AVERAGING: usize = 1;
const IN_DB: usize = 2;
const OUT_DB: usize = 3;

const FRAME: usize = 256;
const CLIP: f32 = 1.5;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("depth", "Depth", 1.0, 32.0, 32.0).log().unit("bits").smooth(0.0),
    ParamSpec::new("averaging", "Averaging", 1.0, 256.0, 1.0).log().kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("inDb", "In", -20.0, 20.0, 0.0).unit("dB"),
    ParamSpec::new("outDb", "Out", -20.0, 20.0, 0.0).unit("dB"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "sh-decimate",
    code: 8,
    version: 1,
    label: "++decimate",
    category: Category::Distortion,
    params: PARAMS,
    panel: Panel {
        // The original: two long sliders, depth over averaging, and the in and
        // out dials at the top right.
        groups: &[
            Group { label: "Decimate", keys: &["depth", "averaging"] },
            Group { label: "Level", keys: &["inDb", "outDb"] },
        ],
        primary: &["depth", "averaging"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "averaging", x_range: (1.0, 256.0), y: "depth", y_range: (1.0, 32.0) }),
    },
    source: "SoundHack: ++decimate (Freesound 9.10)",
    sound: None,
};

/// SoundHack's averaging table: the nearest power of two, by midpoints.
fn group_size(averaging: f32) -> usize {
    const STEPS: [(f32, usize); 8] = [(1.5, 1), (3.0, 2), (6.0, 4), (12.0, 8), (24.0, 16), (48.0, 32), (96.0, 64), (192.0, 128)];
    STEPS.iter().find(|(below, _)| averaging < *below).map_or(256, |(_, n)| *n)
}

/// The top `bits` bits of a 32-bit word.
fn top_bits(bits: i32) -> i32 {
    if bits >= 32 {
        -1
    } else {
        (!0u32 << (32 - bits.max(0)) as u32) as i32
    }
}

pub struct ShDecimate {
    values: [f32; 4],
    in_db: Smoothed,
    out_db: Smoothed,
    input: [[f32; FRAME]; 2],
    output: [[f32; FRAME]; 2],
    pos: usize,
}

impl ShDecimate {
    fn frame(&mut self, gain: f32) {
        let depth = self.values[DEPTH];
        let bits = depth as i32;
        let frac = depth - bits as f32;
        let (lo_mask, hi_mask) = (top_bits(bits), top_bits(bits + 1));
        let n = group_size(self.values[AVERAGING]);
        let scale = 2_147_483_648.0 / n as f32;
        for side in 0..2 {
            for g in (0..FRAME).step_by(n) {
                let sum: f32 = self.input[side][g..g + n].iter().map(|x| x * gain).sum();
                // Saturating, where the original's conversion wrapped.
                let q = (sum * scale) as i32;
                let (lo, hi) = (q & lo_mask, q & hi_mask);
                let blended = (lo as f32 + (hi.wrapping_sub(lo)) as f32 * frac) as i32;
                let y = blended as f32 * 4.656_613e-10;
                self.output[side][g..g + n].fill(y);
            }
        }
    }
}

impl Kernel for ShDecimate {
    fn new(sample_rate: f32) -> Self {
        let mut values = [0.0; 4];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = ShDecimate {
            values,
            in_db: Smoothed::new(sample_rate, PARAMS[IN_DB].smooth_ms),
            out_db: Smoothed::new(sample_rate, PARAMS[OUT_DB].smooth_ms),
            input: [[0.0; FRAME]; 2],
            output: [[0.0; FRAME]; 2],
            pos: 0,
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
        match index {
            IN_DB => self.in_db.set(v),
            OUT_DB => self.out_db.set(v),
            _ => {}
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.in_db.jump(self.values[IN_DB]);
        self.out_db.jump(self.values[OUT_DB]);
        self.input = [[0.0; FRAME]; 2];
        self.output = [[0.0; FRAME]; 2];
        self.pos = 0;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let gain_in = 10f32.powf(self.in_db.next() * 0.05);
        let gain_out = 10f32.powf(self.out_db.next() * 0.05);
        let pos = self.pos;
        for (side, s) in frame.iter_mut().enumerate() {
            self.input[side][pos] = *s;
            *s = (self.output[side][pos] * gain_out).clamp(-CLIP, CLIP);
        }
        self.pos += 1;
        if self.pos == FRAME {
            self.pos = 0;
            // The drive is the input gain as the frame completes.
            self.frame(gain_in);
        }
    }

    fn latency_frames(&self) -> u32 {
        FRAME as u32
    }
}
