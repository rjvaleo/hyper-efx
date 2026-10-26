//! Dattorro plate: MultiGrainLive's `Plate` (`ShapeKind::DattorroPlate`), the
//! figure-eight plate of Part 1, Fig. 1 of Jon Dattorro's "Effect Design",
//! sample for sample.
//!
//! - **Into the tank.** The two sides are summed to mono, band-limited by a
//!   one-pole (`bandwidth`), predelayed by at least one sample, and diffused
//!   by four lattice all-passes (two at input diffusion 1, two at 2).
//! - **The tank.** Two halves, each a modulated all-pass (the LFO's sine in
//!   f64, the halves moving opposite ways by up to `modDepth` samples), a
//!   delay, a one-pole damping filter, the decay, a second all-pass and a
//!   second delay, each half fed from the end of the other times the decay.
//!   Lengths are Dattorro's primes at 29 761 Hz scaled to the running rate,
//!   the modulated lines 64 samples longer for the swing.
//! - **Out.** Table 2's seven taps a side, each at 0.6, read mostly from the
//!   opposite half — the stereo image is made there, as the input is mono.
//!   Width crosses each side's sum into the other; then wet and dry.

use super::mgl_dattorro_common::{advance, ap, ap_mod, spec, store, whole_rate, Delay};
use crate::{Category, Descriptor, Display, Group, Kernel, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[
    spec("predelayMs", "Predelay", 0.0, 250.0, 0.0).unit("ms"),
    spec("bandwidth", "Bandwidth", 0.0, 1.0, 0.9995),
    spec("inputDiffusion1", "Input diffusion 1", 0.0, 0.999, 0.75),
    spec("inputDiffusion2", "Input diffusion 2", 0.0, 0.999, 0.625),
    spec("decay", "Decay", 0.0, 0.9999, 0.5),
    spec("decayDiffusion1", "Decay diffusion 1", 0.0, 0.999, 0.7),
    spec("decayDiffusion2", "Decay diffusion 2", 0.0, 0.999, 0.5),
    spec("damping", "Damping", 0.0, 1.0, 0.0005),
    spec("modRateHz", "Mod rate", 0.01, 5.0, 0.3).log().unit("Hz"),
    spec("modDepth", "Mod depth", 0.0, 32.0, 8.0).unit("smp"),
    spec("wet", "Wet", 0.0, 1.0, 0.35),
    spec("dry", "Dry", 0.0, 1.0, 1.0),
    spec("crossfeed", "Width", 0.0, 1.0, 0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "dattorro-plate",
    code: 86,
    version: 1,
    label: "Dattorro plate",
    category: Category::Reverb,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Input", keys: &["predelayMs", "bandwidth", "inputDiffusion1", "inputDiffusion2"] },
            Group { label: "Tank", keys: &["decay", "decayDiffusion1", "decayDiffusion2", "damping"] },
            Group { label: "Modulation", keys: &["modRateHz", "modDepth"] },
            Group { label: "Output", keys: &["wet", "dry", "crossfeed"] },
        ],
        primary: &["decay", "damping", "predelayMs", "wet"],
        displays: &[Display::Meter],
        pad: None,
    },
    source: "MultiGrainLive: Dattorro plate (dattorro.rs, Plate)",
    sound: None,
};

/// Table 2: `(tank line, tap at 29 761 Hz, sign)`, left then right.
const TAPS_L: [(usize, usize, f32); 7] =
    [(5, 266, 1.0), (5, 2974, 1.0), (6, 1913, -1.0), (7, 1996, 1.0), (1, 1990, -1.0), (2, 187, -1.0), (3, 1066, -1.0)];
const TAPS_R: [(usize, usize, f32); 7] =
    [(1, 353, 1.0), (1, 3627, 1.0), (2, 1228, -1.0), (3, 2673, 1.0), (5, 2111, -1.0), (6, 335, -1.0), (7, 121, -1.0)];
const TAP_GAIN: f32 = 0.6;

pub struct MglDattorroPlate {
    p: [f32; 13],
    taps: ([usize; 7], [usize; 7]),
    pred: Delay,
    input: [Delay; 4],
    tank: [Delay; 8],
    bw: f32,
    damp: [f32; 2],
    phase: f64,
    sr: u32,
}

impl MglDattorroPlate {
    fn tap_sum(&self, which: &[(usize, usize, f32); 7], scaled: &[usize; 7]) -> f32 {
        let mut acc = 0.0;
        for (i, (line, _, sign)) in which.iter().enumerate() {
            let d = &self.tank[*line];
            let at = scaled[i].min(d.b.len().saturating_sub(2)).max(1);
            acc += sign * TAP_GAIN * d.tap(at as f32);
        }
        acc
    }
}

impl Kernel for MglDattorroPlate {
    fn new(sample_rate: f32) -> Self {
        let sr = whole_rate(sample_rate);
        let sc = sr.max(1) as f32 / 29761.0;
        let n = |x: usize| ((x as f32 * sc).round() as usize).max(2);
        MglDattorroPlate {
            p: [0.0, 0.9995, 0.75, 0.625, 0.5, 0.7, 0.5, 0.0005, 0.3, 8.0, 0.35, 1.0, 0.0],
            taps: (core::array::from_fn(|i| n(TAPS_L[i].1)), core::array::from_fn(|i| n(TAPS_R[i].1))),
            pred: Delay::new((sr as usize / 4).max(2)),
            input: [Delay::new(n(142)), Delay::new(n(107)), Delay::new(n(379)), Delay::new(n(277))],
            tank: [
                Delay::new(n(672) + 64),
                Delay::new(n(4453)),
                Delay::new(n(1800)),
                Delay::new(n(3720)),
                Delay::new(n(908) + 64),
                Delay::new(n(4217)),
                Delay::new(n(2656)),
                Delay::new(n(3163)),
            ],
            bw: 0.0,
            damp: [0.0; 2],
            phase: 0.0,
            sr,
        }
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        store(PARAMS, &mut self.p, index, value);
    }

    fn param(&self, index: usize) -> f32 {
        self.p.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.pred.clear();
        for d in self.input.iter_mut().chain(self.tank.iter_mut()) {
            d.clear();
        }
        self.bw = 0.0;
        self.damp = [0.0; 2];
        self.phase = 0.0;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let p = self.p;
        let [dry_l, dry_r] = *frame;
        let mono = (dry_l + dry_r) * 0.5;
        self.bw += p[1] * (mono - self.bw);
        let pd = (p[0] * self.sr as f32 / 1000.0).clamp(1.0, (self.pred.b.len() - 1) as f32);
        let mut x = self.pred.tap(pd);
        self.pred.push(self.bw);
        x = ap(&mut self.input[0], x, p[2]);
        x = ap(&mut self.input[1], x, p[2]);
        x = ap(&mut self.input[2], x, p[3]);
        x = ap(&mut self.input[3], x, p[3]);
        let fb_l = self.tank[7].tap((self.tank[7].b.len() - 1) as f32) * p[4];
        let fb_r = self.tank[3].tap((self.tank[3].b.len() - 1) as f32) * p[4];
        let md = p[9] * (self.phase as f32).sin();
        let dl = (self.tank[0].b.len() - 64) as f32 + md;
        let dr = (self.tank[4].b.len() - 64) as f32 - md;
        let mut l = ap_mod(&mut self.tank[0], x + fb_l, p[5], dl);
        l = self.tank[1].step(l);
        self.damp[0] += (1.0 - p[7]) * (l - self.damp[0]);
        l = ap(&mut self.tank[2], self.damp[0] * p[4], p[6]);
        self.tank[3].step(l);
        let mut r = ap_mod(&mut self.tank[4], x + fb_r, p[5], dr);
        r = self.tank[5].step(r);
        self.damp[1] += (1.0 - p[7]) * (r - self.damp[1]);
        r = ap(&mut self.tank[6], self.damp[1] * p[4], p[6]);
        self.tank[7].step(r);
        self.phase = advance(self.phase, p[8], self.sr);
        let wl = self.tap_sum(&TAPS_L, &self.taps.0);
        let wr = self.tap_sum(&TAPS_R, &self.taps.1);
        frame[0] = dry_l * p[11] + (wl + wr * p[12]) * p[10];
        frame[1] = dry_r * p[11] + (wr + wl * p[12]) * p[10];
    }

    /// The tank's loop is about 0.3 s; the decay sets how many trips to 60 dB.
    fn tail_seconds(&self) -> f32 {
        let decay = self.p[4];
        let trips = if decay <= 1e-3 { 1.0 } else { 0.001f32.ln() / decay.ln() };
        (self.p[0] / 1000.0 + 0.3 * trips).min(60.0)
    }
}
