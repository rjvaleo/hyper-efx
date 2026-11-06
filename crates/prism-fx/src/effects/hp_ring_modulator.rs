//! Hyperprism Ring Modulator: Hyperprism-PPC 1.2.1's Ring Modulator process
//! (EModulation, process 3), as its code does it.
//!
//! The RingMod routine over a 1024-entry sine table read with linear
//! interpolation: `y = x·(1 − d) + sin·(x·d)`, so full depth is a pure ring
//! modulator and less blends in the dry sound. One carrier phase serves both
//! sides; it starts at 0.
//!
//! - **Pad.** X is the carrier, 0–4000 Hz (a table step of `v·1024/SR`); Y is
//!   the depth, 0–100 % (`d = v·0.01`). Both linear, from the window's edit
//!   fields, starting at 2000 Hz and 0 %.
//! - **Smoothing.** The additional setting, 0–100 %, default 50 % (the
//!   window's slider): `a = int(10^(0.04·(100 − s)))·0.0001`, a step
//!   `v ← a·target + (1 − a)·v` of carrier and depth every 45th sample.
//!
//! Changed from the original: the stereo loop's dry term was `(d − 1)·x`,
//! which inverted stereo below full depth; both sides now take the mono law.
//! The phase wrapped at 1023 — the carrier 1024/1023 sharp, and a read one
//! past the table at the wrap; it wraps at 1024.
//!
//! Added (not in the original):
//!
//! - **Stereo phase**, 0–180°, default 0°: the right side reads the LFO that
//!   far ahead of the left. At 0° it is the original exactly.
//! - **LFO shape**, default Sine (the original's table), or a triangle or a
//!   square, each in the sine's phase and its ±1 range (the square +1 for
//!   the first half cycle, −1 for the second). The original's table is kept
//!   bit for bit.
//! - **Smoothing step**, 1–2048 samples at 44.1 kHz (scaled with the rate),
//!   default 45: how often the smoothing steps. At 45 it is the original
//!   exactly.

use super::hp_mod_common::{degrees, increment, shape_table, RingMod, Stepper};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const RATE: usize = 0;
const DEPTH: usize = 1;
const SMOOTHING: usize = 2;
const PHASE: usize = 3;
const SHAPE: usize = 4;
const STEP: usize = 5;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("rateHz", "Frequency", 0.0, 4000.0, 2000.0).unit("Hz").smooth(100.0),
    ParamSpec::new("depth", "Depth", 0.0, 100.0, 0.0).unit("%").smooth(100.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 50.0).unit("%").kind(ParamKind::Int).smooth(0.0).glides("rateHz"),
    ParamSpec::new("stereoPhaseDeg", "Stereo phase", 0.0, 180.0, 0.0).unit("°").smooth(0.0),
    ParamSpec::new("lfoShape", "LFO shape", 0.0, 2.0, 0.0)
        .kind(ParamKind::Choice(&["Sine", "Triangle", "Square"]))
        .smooth(0.0),
    ParamSpec::new("smoothingStep", "Smoothing step", 1.0, 2048.0, 45.0)
        .unit("samples")
        .kind(ParamKind::Int)
        .smooth(0.0)
        .glides("rateHz"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-ring-modulator",
    code: 50,
    version: 1,
    label: "Hyperprism Ring Modulator",
    category: Category::Modulation,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Pad", keys: &["rateHz", "depth"] },
            Group { label: "Additional settings", keys: &["smoothing"] },
            Group { label: "Added (not in the original)", keys: &["stereoPhaseDeg", "lfoShape", "smoothingStep"] },
        ],
        primary: &["rateHz", "depth"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "rateHz", x_range: (0.0, 4000.0), y: "depth", y_range: (0.0, 100.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Ring Modulator",
    sound: None,
};

pub struct HpRingModulator {
    sample_rate: f32,
    values: [f32; 6],
    ring: RingMod,
    steps: Stepper,
    /// The shape the ring's table was built for.
    shape: f32,
}

impl HpRingModulator {
    fn targets(&self) -> (f32, f32) {
        (increment(self.values[RATE], self.sample_rate), self.values[DEPTH] * 0.01)
    }

    fn apply(&mut self) {
        self.steps.set_percent(self.values[SMOOTHING]);
        self.steps.set_every(self.sample_rate, self.values[STEP]);
        self.ring.offset = degrees(self.values[PHASE]);
        if self.values[SHAPE] != self.shape {
            self.shape = self.values[SHAPE];
            self.ring.set_table(shape_table(self.shape, false));
        }
    }
}

impl Kernel for HpRingModulator {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 6];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpRingModulator {
            sample_rate,
            values,
            ring: RingMod::new(shape_table(values[SHAPE], false)),
            steps: Stepper::new(sample_rate, 44.0, 44.0, values[SMOOTHING]),
            shape: values[SHAPE],
        };
        k.apply();
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        self.values[index] = spec.clamp(value);
        if index >= SMOOTHING {
            self.apply();
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.steps.reset();
        self.ring.clear();
        (self.ring.rate, self.ring.depth) = self.targets();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.steps.due() {
            let (rate, depth) = self.targets();
            self.ring.rate = self.steps.step(self.ring.rate, rate);
            self.ring.depth = self.steps.step(self.ring.depth, depth);
        }
        self.ring.tick(frame);
    }
}

#[cfg(test)]
mod tests {
    use super::super::hp_mod_common::check;
    use super::*;

    /// The renders as they were before any control was added, at both rates.
    #[test]
    fn defaults_render_as_the_original() {
        let pad = [(RATE, 700.0), (DEPTH, 100.0)];
        let moved = [(RATE, 3000.0), (DEPTH, 60.0)];
        let got = [
            check::checksums::<HpRingModulator>(44_100.0, 3.0, &pad, &moved),
            check::checksums::<HpRingModulator>(48_000.0, 3.0, &pad, &moved),
        ];
        assert_eq!(
            got,
            [[7279736341450684295, 14072212929570951658, 1042564165276027081],
             [5611665164035847497, 1995788435230916388, 11199038155840047218]]
        );
    }

    fn kernel(settings: &[(usize, f32)]) -> HpRingModulator {
        let mut k = HpRingModulator::new(44_100.0);
        for &(i, v) in settings {
            k.set_param(i, v);
        }
        k.reset();
        k
    }

    /// A second of 0.5 on both sides through the kernel at 2 Hz and full depth.
    fn steady(settings: &[(usize, f32)]) -> (Vec<f32>, Vec<f32>) {
        let x = vec![0.5f32; 44_100];
        check::render(&mut kernel(&[&[(RATE, 20.0), (DEPTH, 100.0)], settings].concat()), &(x.clone(), x), &[])
    }

    /// The LFO's phase in table steps at sample `n`.
    fn phase(n: usize) -> f32 {
        n as f32 * 20.0 * 1024.0 / 44_100.0
    }

    #[test]
    fn the_smoothing_step_sets_how_often_the_glides_move() {
        let fires = |every: f32| {
            let mut k = kernel(&[(STEP, every)]);
            (0..1_000).filter(|_| k.steps.due()).count()
        };
        assert_eq!((fires(45.0), fires(10.0), fires(1.0)), (22, 100, 1_000));
        let mut k = HpRingModulator::new(96_000.0);
        k.set_param(STEP, 10.0);
        k.reset();
        // 9 at 44.1 kHz is 20 at 96 kHz: a step every 21 samples.
        assert_eq!((0..1_050).filter(|_| k.steps.due()).count(), 50);
    }

    #[test]
    fn the_stereo_phase_moves_the_right_carrier_ahead() {
        let (l, r) = steady(&[]);
        assert_eq!(l, r, "the original: one carrier for both sides");
        let (l, r) = steady(&[(PHASE, 180.0)]);
        assert!(l != r);
        assert!(l.iter().zip(&r).all(|(a, b)| (a + b).abs() < 1e-4), "half a cycle on, the carrier inverts");
        let (_, r) = steady(&[(PHASE, 90.0)]);
        assert!((r[0] - 0.5).abs() < 1e-6, "a quarter ahead, the right starts at the top: {}", r[0]);
    }

    #[test]
    fn the_lfo_shape_changes_the_carrier() {
        let (sine, _) = steady(&[]);
        let n = 300;
        assert!((sine[n] - 0.5 * (phase(n) as f64 * std::f64::consts::TAU / 1024.0).sin() as f32).abs() < 1e-4);
        // The triangle rises in a straight line to 1 at a quarter cycle.
        let (tri, _) = steady(&[(SHAPE, 1.0)]);
        assert!((tri[n] - 0.5 * phase(n) / 256.0).abs() < 1e-4, "{}", tri[n]);
        // The square is +1 for the first half and −1 for the second.
        let (sq, _) = steady(&[(SHAPE, 2.0)]);
        assert!(sq[0..1_090].iter().all(|&v| v == 0.5));
        assert!(sq[1_115..2_190].iter().all(|&v| v == -0.5));
        assert!(check::power(&sq) > check::power(&tri) && check::power(&tri) < check::power(&sine));
    }
}
