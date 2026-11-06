//! Hyperprism Tremolo: Hyperprism-PPC 1.2.1's Tremolo process (EModulation,
//! process 5), as its code does it.
//!
//! The RingMod routine over a 1024-entry raised-cosine table (`0.5·(1 − cos)`,
//! read with linear interpolation): the gain is `1 − d + d·lfo`, from `1 − d`
//! up to 1, starting at the bottom of the cycle. One phase serves both sides,
//! so there is no auto-pan.
//!
//! - **Pad.** X is the frequency, 0–4 Hz (a table step of `v·1024/SR` a
//!   sample); Y is the depth, 0–100 % (`d = v·0.01`). Both linear, from the
//!   window's edit fields, starting at 2 Hz and 0 %.
//! - **Smoothing.** The additional setting, 0–100 %, default 50 % (the
//!   window's slider): `a = int(10^(0.04·(100 − s)))·0.0001`. Every 45th
//!   sample rate and depth each step `v ← a·target + (1 − a)·v`, and hold
//!   between steps. The count runs on the kernel's own samples.
//!
//! Changed from the original: the stereo loop's dry term was `(d − 1)·x`, so
//! stereo below full depth was inverted and quieter than mono; both sides now
//! take the mono law. The phase wrapped at 1023, not 1024, running 1024/1023
//! fast and reading one entry past the table at the wrap; it wraps at 1024
//! and the last step leads back to the first entry.
//!
//! Added (not in the original):
//!
//! - **Stereo phase**, 0–180°, default 0°: the right side reads the LFO that
//!   far ahead of the left. At 0° it is the original exactly.
//! - **LFO shape**, default Raised cosine (the original's table), or a
//!   triangle or a square, each from 0 up to 1 and back as the raised cosine
//!   is: the gain at `1 − d` at the start and at 1 halfway. The original's
//!   table is kept bit for bit.
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
    ParamSpec::new("rateHz", "Frequency", 0.0, 4.0, 2.0).unit("Hz").smooth(100.0),
    ParamSpec::new("depth", "Depth", 0.0, 100.0, 0.0).unit("%").smooth(100.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 50.0).unit("%").kind(ParamKind::Int).smooth(0.0).glides("rateHz"),
    ParamSpec::new("stereoPhaseDeg", "Stereo phase", 0.0, 180.0, 0.0).unit("°").smooth(0.0),
    ParamSpec::new("lfoShape", "LFO shape", 0.0, 2.0, 0.0)
        .kind(ParamKind::Choice(&["Raised cosine", "Triangle", "Square"]))
        .smooth(0.0),
    ParamSpec::new("smoothingStep", "Smoothing step", 1.0, 2048.0, 45.0)
        .unit("samples")
        .kind(ParamKind::Int)
        .smooth(0.0)
        .glides("rateHz"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-tremolo",
    code: 52,
    version: 1,
    label: "Hyperprism Tremolo",
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
        pad: Some(PadPair { x: "rateHz", x_range: (0.0, 4.0), y: "depth", y_range: (0.0, 100.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Tremolo",
    sound: None,
};

pub struct HpTremolo {
    sample_rate: f32,
    values: [f32; 6],
    ring: RingMod,
    steps: Stepper,
    /// The shape the ring's table was built for.
    shape: f32,
}

impl HpTremolo {
    fn targets(&self) -> (f32, f32) {
        (increment(self.values[RATE], self.sample_rate), self.values[DEPTH] * 0.01)
    }

    fn apply(&mut self) {
        self.steps.set_percent(self.values[SMOOTHING]);
        self.steps.set_every(self.sample_rate, self.values[STEP]);
        self.ring.offset = degrees(self.values[PHASE]);
        if self.values[SHAPE] != self.shape {
            self.shape = self.values[SHAPE];
            self.ring.set_table(shape_table(self.shape, true));
        }
    }
}

impl Kernel for HpTremolo {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 6];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpTremolo {
            sample_rate,
            values,
            ring: RingMod::new(shape_table(values[SHAPE], true)),
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
        let pad = [(RATE, 3.0), (DEPTH, 80.0)];
        let moved = [(RATE, 1.0), (DEPTH, 100.0)];
        let got = [
            check::checksums::<HpTremolo>(44_100.0, 3.0, &pad, &moved),
            check::checksums::<HpTremolo>(48_000.0, 3.0, &pad, &moved),
        ];
        assert_eq!(
            got,
            [[7279736341450684295, 12424528988881469688, 14405482511249544469],
             [5611665164035847497, 4316677606220586227, 2401658124785871806]]
        );
    }

    fn kernel(settings: &[(usize, f32)]) -> HpTremolo {
        let mut k = HpTremolo::new(44_100.0);
        for &(i, v) in settings {
            k.set_param(i, v);
        }
        k.reset();
        k
    }

    /// A second of 0.5 on both sides through the kernel at 2 Hz and full depth.
    fn steady(settings: &[(usize, f32)]) -> (Vec<f32>, Vec<f32>) {
        let x = vec![0.5f32; 44_100];
        check::render(&mut kernel(&[&[(RATE, 2.0), (DEPTH, 100.0)], settings].concat()), &(x.clone(), x), &[])
    }

    /// The LFO's phase in table steps at sample `n`.
    fn phase(n: usize) -> f32 {
        n as f32 * 2.0 * 1024.0 / 44_100.0
    }

    #[test]
    fn the_smoothing_step_sets_how_often_the_glides_move() {
        let fires = |every: f32| {
            let mut k = kernel(&[(STEP, every)]);
            (0..1_000).filter(|_| k.steps.due()).count()
        };
        assert_eq!((fires(45.0), fires(10.0), fires(1.0)), (22, 100, 1_000));
        let mut k = HpTremolo::new(96_000.0);
        k.set_param(STEP, 10.0);
        k.reset();
        // 9 at 44.1 kHz is 20 at 96 kHz: a step every 21 samples.
        assert_eq!((0..1_050).filter(|_| k.steps.due()).count(), 50);
    }

    #[test]
    fn the_stereo_phase_moves_the_right_gain_ahead() {
        let (l, r) = steady(&[]);
        assert_eq!(l, r, "the original: one phase for both sides");
        // Half a cycle apart the raised cosines sum to one: the sides cross-fade.
        let (l, r) = steady(&[(PHASE, 180.0)]);
        assert!(l != r);
        assert!(l.iter().zip(&r).all(|(a, b)| (a + b - 0.5).abs() < 1e-4));
        // A quarter ahead, the right starts halfway up.
        let (_, r) = steady(&[(PHASE, 90.0)]);
        assert!((r[0] - 0.25).abs() < 1e-3, "{}", r[0]);
    }

    #[test]
    fn the_lfo_shape_changes_the_gain_curve() {
        // Each starts at the bottom and is at the top halfway, as the original.
        for shape in [0.0, 1.0, 2.0] {
            let (l, _) = steady(&[(SHAPE, shape)]);
            assert_eq!(l[0], 0.0, "shape {shape}");
            assert!((l[11_025] - 0.5).abs() < 1e-3, "shape {shape}: {}", l[11_025]);
        }
        // The triangle rises in a straight line: p/512 over the first quarter.
        let (l, _) = steady(&[(SHAPE, 1.0)]);
        for n in [1_000, 2_000, 5_000] {
            assert!((l[n] - 0.5 * phase(n) / 512.0).abs() < 1e-4, "{n}: {}", l[n]);
        }
        // The square is off for the first quarter and fully on for the middle half.
        let (l, _) = steady(&[(SHAPE, 2.0)]);
        assert!(l[100..5_400].iter().all(|&v| v == 0.0));
        assert!(l[5_600..16_400].iter().all(|&v| v == 0.5));
    }
}
