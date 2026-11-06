//! Hyperprism Vibrato: Hyperprism-PPC 1.2.1's Vibrato process (EModulation,
//! process 4), as its code does it.
//!
//! The Modulation routine wet only (wet 1, dry 0, no feedback): a line per
//! side read `d = D·0.5·(1 − cos)` frames back from a 1024-entry raised-cosine
//! table, linearly interpolated, both sides in phase, the LFO starting at the
//! bottom (no delay).
//!
//! - **Depth.** Y sets `D = v / P0`, where `P0 = f·1024/SR` is the rate's
//!   table step: `D = v·SR/(1024·f)` samples, at most 16 384 (371 ms). The
//!   steepest slope of the delay is `D·π·f/SR = π·v/1024`, so Y is a pitch
//!   deviation the rate does not change: ±3.07 % at v = 10. The original's
//!   pad sends X before Y, so D is taken against the new rate; a move of
//!   either control here does the same. Rate and D are smoothed separately,
//!   so a rate move briefly bends the deviation, as in the original.
//! - **Pad.** X 0–4 Hz, Y 0–10 "%", both linear, starting at 2 Hz and 0.
//! - **Smoothing.** The additional setting, 0–100 %, default 50 % (the
//!   window's slider): `a = int(10^(0.04·(100 − s)))·0.0001`, a step of rate
//!   and D every 45th sample.
//!
//! Changed from the original: at rate 0 and depth 0, `v / P0` was 0/0 — a NaN
//! that passed the 16 384 cap, stuck in the smoother and sent the read
//! outside the line. Depth 0 is no delay at any rate.
//!
//! Added (not in the original):
//!
//! - **Stereo phase**, 0–180°, default 0°: the right side reads the LFO that
//!   far ahead of the left. At 0° it is the original exactly.
//! - **LFO shape**, default Raised cosine (the original's table), or a
//!   triangle or a square, each from 0 up to 1 and back as the raised cosine
//!   is, no delay at the start and the full swing halfway. The depth law is
//!   the raised cosine's: a triangle's steepest slope is 2/π of it, and a
//!   square jumps between no delay and the full swing. The original's table
//!   is kept bit for bit.
//! - **Smoothing step**, 1–2048 samples at 44.1 kHz (scaled with the rate),
//!   default 45: how often the smoothing steps. At 45 it is the original
//!   exactly.

use super::hp_mod_common::{degrees, increment, samples, shape_table, Modulation, Stepper};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const RATE: usize = 0;
const DEPTH: usize = 1;
const SMOOTHING: usize = 2;
const PHASE: usize = 3;
const SHAPE: usize = 4;
const STEP: usize = 5;

/// The longest swing, in samples at 44.1 kHz.
const MOST: f64 = 16_384.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("rateHz", "Frequency", 0.0, 4.0, 2.0).unit("Hz").smooth(100.0),
    ParamSpec::new("depth", "Depth", 0.0, 10.0, 0.0).unit("%").smooth(100.0),
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
    id: "hp-vibrato",
    code: 51,
    version: 1,
    label: "Hyperprism Vibrato",
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
        pad: Some(PadPair { x: "rateHz", x_range: (0.0, 4.0), y: "depth", y_range: (0.0, 10.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Vibrato",
    sound: None,
};

pub struct HpVibrato {
    sample_rate: f32,
    values: [f32; 6],
    modulation: Modulation,
    steps: Stepper,
    /// The shape the table was built for.
    shape: f32,
}

impl HpVibrato {
    fn targets(&self) -> (f32, f32) {
        let rate = increment(self.values[RATE], self.sample_rate);
        let v = self.values[DEPTH];
        let most = samples(MOST, self.sample_rate) as f32;
        let swing = if v == 0.0 { 0.0 } else { ((v as f64 / rate as f64) as f32).min(most) };
        (rate, swing)
    }

    fn apply(&mut self) {
        self.steps.set_percent(self.values[SMOOTHING]);
        self.steps.set_every(self.sample_rate, self.values[STEP]);
        self.modulation.offset = degrees(self.values[PHASE]);
        if self.values[SHAPE] != self.shape {
            self.shape = self.values[SHAPE];
            self.modulation.set_table(shape_table(self.shape, true));
        }
    }
}

impl Kernel for HpVibrato {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 6];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpVibrato {
            sample_rate,
            values,
            modulation: Modulation::new(sample_rate, 1.0, 0.0, 0.0),
            steps: Stepper::new(sample_rate, 44.0, 44.0, values[SMOOTHING]),
            // Modulation starts on the original's raised cosine.
            shape: 0.0,
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
        self.modulation.clear();
        (self.modulation.rate, self.modulation.depth) = self.targets();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.steps.due() {
            let (rate, depth) = self.targets();
            self.modulation.rate = self.steps.step(self.modulation.rate, rate);
            self.modulation.depth = self.steps.step(self.modulation.depth, depth);
        }
        self.modulation.tick(frame);
    }
}

#[cfg(test)]
mod tests {
    use super::super::hp_mod_common::check;
    use super::*;

    /// The renders as they were before any control was added, at both rates.
    #[test]
    fn defaults_render_as_the_original() {
        let pad = [(RATE, 1.5), (DEPTH, 6.0)];
        let moved = [(RATE, 3.5), (DEPTH, 10.0)];
        let got = [
            check::checksums::<HpVibrato>(44_100.0, 3.0, &pad, &moved),
            check::checksums::<HpVibrato>(48_000.0, 3.0, &pad, &moved),
        ];
        assert_eq!(
            got,
            [[7279736341450684295, 10941500383039635523, 16916293575998611204],
             [5611665164035847497, 16631628054793865162, 13018240781135063970]]
        );
    }

    fn vibrato(settings: &[(usize, f32)]) -> HpVibrato {
        let mut k = HpVibrato::new(44_100.0);
        for &(i, v) in settings {
            k.set_param(i, v);
        }
        k.reset();
        k
    }

    /// The delay each side reads at each sample, from a ramp through the
    /// line: a ramp `n` read `d` back is `n − d`.
    fn delays(settings: &[(usize, f32)]) -> (Vec<f32>, Vec<f32>) {
        let x: Vec<f32> = (0..44_100).map(|n| n as f32).collect();
        let mut k = vibrato(&[&[(RATE, 2.0), (DEPTH, 5.0)], settings].concat());
        let (l, r) = check::render(&mut k, &(x.clone(), x.clone()), &[]);
        let back = |y: Vec<f32>| y.iter().zip(&x).map(|(y, x)| x - y).collect();
        (back(l), back(r))
    }

    #[test]
    fn the_stereo_phase_moves_the_right_sweep_ahead() {
        let (l, r) = delays(&[]);
        assert_eq!(l, r, "the original: both sides in phase");
        let (l, r) = delays(&[(PHASE, 180.0)]);
        // 2 Hz at v = 5: a swing of 5·44100/2048 = 107.7 samples. Half a cycle
        // apart, one side is at the top while the other is at the bottom.
        let swing = 5.0 * 44_100.0 / 2_048.0;
        assert!(r[11_025].abs() < 0.1 && (l[11_025] - swing).abs() < 0.1, "{} {}", l[11_025], r[11_025]);
        assert!(l.iter().zip(&r).skip(500).all(|(a, b)| (a + b - swing).abs() < 0.1));
    }

    #[test]
    fn the_lfo_shape_changes_the_sweep() {
        let swing = 5.0 * 44_100.0 / 2_048.0;
        let (cos, _) = delays(&[]);
        let (tri, _) = delays(&[(SHAPE, 1.0)]);
        let (sq, _) = delays(&[(SHAPE, 2.0)]);
        // All three start with no delay and reach the full swing halfway.
        for d in [&cos, &tri, &sq] {
            assert!((d[11_025] - swing).abs() < 0.1, "{}", d[11_025]);
        }
        // An eighth of the way in, the triangle is a quarter up, the raised
        // cosine 0.146 of the way; the square has not moved.
        let n = 2_756;
        assert!((tri[n] / swing - 0.25).abs() < 1e-3, "{}", tri[n] / swing);
        assert!((cos[n] / swing - 0.146).abs() < 1e-3, "{}", cos[n] / swing);
        assert!(sq[100..5_400].iter().all(|&d| d == 0.0));
        assert!(sq[5_600..16_400].iter().all(|&d| (d - swing).abs() < 0.05));
    }

    #[test]
    fn the_smoothing_step_sets_how_often_the_glides_move() {
        let fires = |every: f32| {
            let mut k = vibrato(&[(STEP, every)]);
            (0..1_000).filter(|_| k.steps.due()).count()
        };
        assert_eq!((fires(45.0), fires(10.0), fires(1.0)), (22, 100, 1_000));
    }
}
