//! ++chebyshev: SoundHack Freesound's Chebyshev waveshaper (Tom Erbe, v9.10),
//! as its code does it.
//!
//! The driven input goes through the Chebyshev polynomials T1–T5 and T7, and
//! the results are mixed by their gains over the sum of the gains:
//! `y = Σ gₖ·Tₖ(x) / Σ gₖ × out`, clipped at ±1.5. Tₖ turns a full-scale
//! sine into its k-th harmonic; quieter sounds bring out lower ones.
//!
//! As in the original, the input is not clamped first, so drive past full
//! scale runs the polynomials away until the clip squares them off, and the
//! even ones offset silence (T2(0) is −1, T4(0) is 1). The gains glide in dB;
//! the original ramped them in dB across each host block.

use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamSpec, Smoothed};

const PARTIALS: usize = 6;
const IN_DB: usize = 6;
const OUT_DB: usize = 7;

const CLIP: f32 = 1.5;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("t1Db", "cp 1", -60.0, 0.0, 0.0).unit("dB"),
    ParamSpec::new("t2Db", "cp 2", -60.0, 0.0, -60.0).unit("dB"),
    ParamSpec::new("t3Db", "cp 3", -60.0, 0.0, -60.0).unit("dB"),
    ParamSpec::new("t4Db", "cp 4", -60.0, 0.0, -60.0).unit("dB"),
    ParamSpec::new("t5Db", "cp 5", -60.0, 0.0, -60.0).unit("dB"),
    ParamSpec::new("t7Db", "cp 7", -60.0, 0.0, -60.0).unit("dB"),
    ParamSpec::new("inDb", "In", -20.0, 20.0, 0.0).unit("dB"),
    ParamSpec::new("outDb", "Out", -20.0, 20.0, 0.0).unit("dB"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "sh-chebyshev",
    code: 15,
    version: 1,
    label: "++chebyshev",
    category: Category::Distortion,
    params: PARAMS,
    panel: Panel {
        // The original: six partial dials in a row, the levels above.
        groups: &[
            Group { label: "Partials", keys: &["t1Db", "t2Db", "t3Db", "t4Db", "t5Db", "t7Db"] },
            Group { label: "Level", keys: &["inDb", "outDb"] },
        ],
        primary: &["t1Db", "t2Db", "t3Db", "inDb"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "inDb", x_range: (-20.0, 20.0), y: "t3Db", y_range: (-60.0, 0.0) }),
    },
    source: "SoundHack: ++chebyshev (Freesound 9.10)",
    sound: None,
};

pub struct ShChebyshev {
    values: [f32; 8],
    glides: [Smoothed; 8],
}

impl Kernel for ShChebyshev {
    fn new(sample_rate: f32) -> Self {
        let mut values = [0.0; 8];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let glides = core::array::from_fn(|i| Smoothed::new(sample_rate, PARAMS[i].smooth_ms));
        let mut k = ShChebyshev { values, glides };
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
        self.glides[index].set(v);
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        for (glide, &v) in self.glides.iter_mut().zip(&self.values) {
            glide.jump(v);
        }
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let db = |d: f32| 10f32.powf(d * 0.05);
        let mut g = [0.0f32; PARTIALS];
        for (g, glide) in g.iter_mut().zip(&mut self.glides) {
            *g = db(glide.next());
        }
        let gain_in = db(self.glides[IN_DB].next());
        let gain_out = db(self.glides[OUT_DB].next());
        let sum: f32 = g.iter().sum();
        let norm = if sum != 0.0 { 1.0 / sum } else { 0.0 };
        for s in frame.iter_mut() {
            let x = *s * gain_in;
            let x2 = x * x;
            let t1 = x;
            let t2 = 2.0 * x2 - 1.0;
            let t3 = (4.0 * x2 - 3.0) * x;
            let t4 = (8.0 * x2 - 8.0) * x2 + 1.0;
            let t5 = ((16.0 * x2 - 20.0) * x2 + 5.0) * x;
            let t7 = (((64.0 * x2 - 112.0) * x2 + 56.0) * x2 - 7.0) * x;
            let y = (g[5] * t7 + g[4] * t5 + g[3] * t4 + g[2] * t3 + g[1] * t2 + g[0] * t1) * norm * gain_out;
            // Far past full scale the polynomials overflow; the clip still holds.
            *s = if y.is_nan() { 0.0 } else { y.clamp(-CLIP, CLIP) };
        }
    }
}
