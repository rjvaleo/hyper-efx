//! Hyperprism Phaser: Hyperprism-PPC 1.2.1's Phaser process (EPhase,
//! process 5), as its code does it.
//!
//! Four first-order allpasses in a chain on each side, all on one
//! coefficient `g = (1 − w)/(1 + w)` with `w = π·f/SR` (no tan prewarp). Each
//! stage is `s = g·(in + s′) − in′` — `(g − z⁻¹)/(1 − g·z⁻¹)`, the negative of
//! the usual first-order allpass, so four of them are four ordinary ones.
//!
//! - **Feedback.** The chain's input is `x + fb·s4′`, the fourth stage's
//!   previous output.
//! - **Out.** `0.5·s4 − 0.5·x`: the dry sound is subtracted, so the notches
//!   fall where the chain is at 0 or −360° — DC, the sweep frequency and
//!   Nyquist — and the peaks at −180° and −540°. Both sides share `g`.
//! - **Sweep.** `w` moves geometrically between `wmin = π·base/SR` and
//!   `wmax = π·base·depth²/SR`, multiplied each sample by
//!   `k = (depth²)^(P0/(2·SR))` upward or `1/k` downward: a triangle in log
//!   frequency that turns when it passes a bound. `P0 = 3.896 × X`, so one
//!   cycle is `4·SR/P0` samples, 0.974 × X Hz. A rate change reaches the
//!   multiplier at the next turn, as in the original. `g` is taken before
//!   each sample's multiply. The sweep starts at the bottom going up.
//! - **Pad.** X is the frequency, 0–4 Hz; Y the depth, 0–10. Both linear,
//!   starting at 2 Hz and 5.
//! - **Additional settings**, from the window's sliders: Feedback 0–100 %
//!   (20 %), Base Freq 0–10 000 Hz (440), Smoothing 0–100 % (50 %). The
//!   constructor's 0.85 and 100 Hz are replaced by these as soon as the
//!   dialog's values are applied, and those are what are used here. A new
//!   Base Freq puts the sweep back at the bottom, as applying it did.
//! - **Smoothing.** `a = int(10^(0.04·(100 − s)))·0.0001`; every 45th sample
//!   depth and rate step `v ← a·target + (1 − a)·v` and the bounds and
//!   multipliers are worked out again from them.
//!
//! Added (not in the original):
//!
//! - **Stages**, 2–12, default 4: how many allpasses are in the chain, the
//!   feedback taken from the last of them as the original takes it from the
//!   fourth. Each pair adds a notch; an odd count turns the chain over, moving
//!   the notches to where an even one has its peaks. A new count starts the
//!   stages it adds from silence. Default 4 is the original exactly.
//! - **Notch depth**, 0–100 %, default 50: the dry part subtracted,
//!   `out = (1 − n)·s − n·x`. 50 % is the original's 0.5/0.5 and the deepest
//!   notches; 0 % is the chain alone, an allpass with no notches; above 50 %
//!   the dry takes over. Default 50 % is the original exactly.
//! - **Smoothing step**, 1–2048 samples at 44.1 kHz, default 45: how often
//!   depth and rate step, every 45th sample in the original; scaled with the
//!   rate as that count is. A new step starts its count afresh. Default 45
//!   is the original exactly.
//!
//! Changed from the original:
//!
//! - Its initialisers set the sweep's range up with `2^depth` while the
//!   routine uses `depth²` from its first smoothing step on; both use depth².
//! - Below depth 1 the top bound fell under the bottom one, so `w` climbed
//!   without end and went to NaN, and depth 0 divided by zero. The range is
//!   taken as at least 1: such a depth stops the sweep where it is, as depth
//!   1 did in the original (at the base, from a fresh start).

use super::hp_mod_common::Stepper;
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};
use core::f64::consts::PI;

const RATE: usize = 0;
const DEPTH: usize = 1;
const FEEDBACK: usize = 2;
const BASE: usize = 3;
const SMOOTHING: usize = 4;
const STAGES: usize = 5;
const NOTCH: usize = 6;
const STEP: usize = 7;

/// The most stages the chain can run.
const MOST_STAGES: usize = 12;

/// The routine's and initialiser's π: the constant in its data, 3.1415927,
/// not the library's (which Apply uses for the bottom bound).
#[allow(clippy::approx_constant)]
const PI_HP: f64 = 3.141_592_7;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("rateHz", "Frequency", 0.0, 4.0, 2.0).unit("Hz").smooth(100.0),
    ParamSpec::new("depth", "Depth", 0.0, 10.0, 5.0).smooth(100.0),
    ParamSpec::new("feedback", "Feedback", 0.0, 100.0, 20.0).unit("%").kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("baseHz", "Base freq", 0.0, 10_000.0, 440.0).unit("Hz").kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 50.0).unit("%").kind(ParamKind::Int).smooth(0.0).glides("rateHz"),
    ParamSpec::new("stages", "Stages", 2.0, MOST_STAGES as f32, 4.0).kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("notchDepth", "Notch depth", 0.0, 100.0, 50.0).unit("%").smooth(0.0),
    ParamSpec::new("smoothingStep", "Smoothing step", 1.0, 2048.0, 45.0)
        .unit("samples")
        .kind(ParamKind::Int)
        .smooth(0.0)
        .glides("rateHz"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-phaser",
    code: 54,
    version: 1,
    label: "Hyperprism Phaser",
    category: Category::Modulation,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Pad", keys: &["rateHz", "depth"] },
            Group { label: "Additional settings", keys: &["feedback", "baseHz", "smoothing", "stages", "notchDepth", "smoothingStep"] },
        ],
        primary: &["rateHz", "depth"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "rateHz", x_range: (0.0, 4.0), y: "depth", y_range: (0.0, 10.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Phaser",
    sound: None,
};

/// One side's chain: the previous input and each stage's previous output.
#[derive(Clone, Copy, Default)]
struct Chain {
    input: f32,
    stages: [f32; MOST_STAGES],
}

impl Chain {
    /// The first `n` stages, fed back from the last of them.
    #[inline]
    fn run(&mut self, x: f32, g: f32, fb: f32, n: usize) -> f32 {
        let mut v = self.stages[n - 1] * fb + x;
        let mut previous_in = self.input;
        self.input = v;
        for s in self.stages[..n].iter_mut() {
            let out = g * (*s + v) - previous_in;
            previous_in = *s;
            *s = out;
            v = out;
        }
        v
    }
}

pub struct HpPhaser {
    sample_rate: f32,
    values: [f32; 8],
    steps: Stepper,
    /// Stages in use, and the output's wet and dry weights.
    stages: usize,
    wet: f32,
    dry: f32,
    /// The smoothed rate (P0 units) and depth.
    rate: f32,
    depth: f32,
    /// The sweep: `w` now, its bounds, the two multipliers and the current one.
    w: f32,
    low: f32,
    high: f32,
    up: f32,
    down: f32,
    multiplier: f32,
    chains: [Chain; 2],
}

impl HpPhaser {
    /// The sweep's control now: `w = π·f/SR`, where `f` is the frequency the
    /// four stages sit at. For tests and displays.
    pub fn sweep(&self) -> f32 {
        self.w
    }

    fn targets(&self) -> (f32, f32) {
        (self.values[RATE] * 3.896, self.values[DEPTH])
    }

    /// The stepper at the Smoothing step's count: steps every `step`
    /// samples, the first `step` samples in, as the original's 45.
    fn stepper(&self) -> Stepper {
        let reload = self.values[STEP] as f64 - 1.0;
        Stepper::new(self.sample_rate, reload, reload, self.values[SMOOTHING])
    }

    fn mix(&mut self) {
        let n = (0.01 * self.values[NOTCH] as f64) as f32;
        self.wet = 1.0 - n;
        self.dry = -n;
    }

    fn feedback(&self) -> f32 {
        (0.01 * self.values[FEEDBACK] as f64) as f32
    }

    /// The top bound and multipliers from a depth and rate, as the routine
    /// works them out at each smoothing step.
    fn bounds(&mut self) {
        let sr = self.sample_rate as f64;
        let range = ((self.depth as f64 * self.depth as f64) as f32).max(1.0) as f64;
        self.high = (PI_HP * self.values[BASE] as f64 * range / sr) as f32;
        self.up = range.powf((self.rate as f64 / sr) as f32 as f64 * 0.5) as f32;
        self.down = 1.0 / self.up;
    }
}

impl Kernel for HpPhaser {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 8];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpPhaser {
            sample_rate,
            values,
            steps: Stepper::new(sample_rate, 44.0, 44.0, values[SMOOTHING]),
            stages: 4,
            wet: 0.5,
            dry: -0.5,
            rate: 0.0,
            depth: 0.0,
            w: 0.0,
            low: 0.0,
            high: 0.0,
            up: 1.0,
            down: 1.0,
            multiplier: 1.0,
            chains: [Chain::default(); 2],
        };
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        self.values[index] = spec.clamp(value);
        match index {
            SMOOTHING => self.steps.set_percent(self.values[SMOOTHING]),
            STEP => self.steps = self.stepper(),
            NOTCH => self.mix(),
            STAGES => {
                let n = self.values[STAGES] as usize;
                if n > self.stages {
                    for chain in self.chains.iter_mut() {
                        chain.stages[self.stages..n].fill(0.0);
                    }
                }
                self.stages = n;
            }
            // Applying Base Freq set the bottom bound and put the sweep there.
            BASE => {
                self.low = (PI * self.values[BASE] as f64 / self.sample_rate as f64) as f32;
                self.w = self.low;
                self.bounds();
            }
            _ => {}
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.steps.reset();
        (self.rate, self.depth) = self.targets();
        self.low = (PI * self.values[BASE] as f64 / self.sample_rate as f64) as f32;
        self.w = self.low;
        self.bounds();
        self.multiplier = self.up;
        self.chains = [Chain::default(); 2];
        self.stages = self.values[STAGES] as usize;
        self.mix();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.steps.due() {
            let (rate, depth) = self.targets();
            self.depth = self.steps.step(self.depth, depth);
            self.rate = self.steps.step(self.rate, rate);
            self.bounds();
        }
        let w = self.w as f64;
        let g = ((1.0 - w) / (1.0 + w)) as f32;
        self.w = (w * self.multiplier as f64) as f32;
        let fb = self.feedback();
        for (chain, x) in self.chains.iter_mut().zip(frame.iter_mut()) {
            let s = chain.run(*x, g, fb, self.stages);
            *x = s * self.wet + *x * self.dry;
        }
        if self.w > self.high {
            self.multiplier = self.down;
        } else if self.w < self.low {
            self.multiplier = self.up;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::hp_delay_law::golden::hash_of;
    use super::*;

    fn hashes() -> [u64; 4] {
        let h = hash_of::<HpPhaser>;
        [
            h(44_100.0, 3.0, &[], &[]),
            h(44_100.0, 3.0, &[(FEEDBACK, 70.0), (BASE, 200.0), (DEPTH, 8.0)], &[(66_150, RATE, 0.5), (66_150, DEPTH, 3.0)]),
            h(44_100.0, 3.0, &[(SMOOTHING, 10.0), (RATE, 4.0)], &[(50_000, BASE, 1_000.0)]),
            h(48_000.0, 3.0, &[(FEEDBACK, 50.0)], &[(72_000, RATE, 1.0)]),
        ]
    }

    /// The still chain's magnitude response at log-spaced frequencies from
    /// 20 Hz to 20 kHz: depth 0 stops the sweep at the base, 1 kHz here.
    fn response(settings: &[(usize, f32)]) -> Vec<f32> {
        let mut k = HpPhaser::new(44_100.0);
        for &(i, v) in [(DEPTH, 0.0), (FEEDBACK, 0.0), (BASE, 1_000.0)].iter().chain(settings) {
            k.set_param(i, v);
        }
        // Land the smoothed depth, so the sweep is still from the start.
        k.reset();
        let n = 16_384;
        let h: Vec<f32> = (0..n)
            .map(|i| {
                let mut f = [if i == 0 { 1.0 } else { 0.0 }; 2];
                k.tick(&mut f);
                f[0]
            })
            .collect();
        (0..400)
            .map(|j| {
                let hz = 20.0 * 1000f64.powf(j as f64 / 399.0);
                let w = core::f64::consts::TAU * hz / 44_100.0;
                let (mut re, mut im) = (0.0, 0.0);
                for (i, &v) in h.iter().enumerate() {
                    re += v as f64 * (w * i as f64).cos();
                    im -= v as f64 * (w * i as f64).sin();
                }
                (re * re + im * im).sqrt() as f32
            })
            .collect()
    }

    /// Dips of the response under a tenth of its level.
    fn notches(r: &[f32]) -> usize {
        r.windows(3).filter(|w| w[1] < w[0] && w[1] <= w[2] && w[1] < 0.1).count()
    }

    #[test]
    fn more_stages_add_notches() {
        // Four stages: one notch between DC and Nyquist, at the base. Each
        // further pair adds one.
        assert_eq!(notches(&response(&[])), 1);
        assert_eq!(notches(&response(&[(STAGES, 8.0)])), 3);
        assert_eq!(notches(&response(&[(STAGES, 12.0)])), 5);
        assert_eq!(notches(&response(&[(STAGES, 2.0)])), 0);
    }

    #[test]
    fn notch_depth_zero_leaves_the_allpass_alone() {
        let r = response(&[(NOTCH, 0.0)]);
        assert!(r.iter().all(|m| (m - 1.0).abs() < 0.01), "{:?}", r.iter().cloned().fold(f32::MAX, f32::min));
        assert_eq!(notches(&r), 0);
        // In between, the notches are partial: at 25 %, |0.75·A − 0.25| ≥ 0.5.
        let r = response(&[(NOTCH, 25.0)]);
        assert!(r.iter().all(|m| *m > 0.49));
        assert!(r.iter().any(|m| *m < 0.6));
    }

    #[test]
    fn the_smoothing_step_sets_how_often_depth_and_rate_move() {
        // Depth 5 → 8 at 50 % smoothing, a = 0.01 per step.
        let depth_after = |step: f32, samples: usize| {
            let mut k = HpPhaser::new(44_100.0);
            k.set_param(STEP, step);
            k.set_param(DEPTH, 8.0);
            for _ in 0..samples {
                k.tick(&mut [0.0, 0.0]);
            }
            k.depth
        };
        let expect = |steps: i32| 8.0 - 3.0 * 0.99f32.powi(steps);
        assert!((depth_after(45.0, 4_500) - expect(100)).abs() < 1e-3, "{}", depth_after(45.0, 4_500));
        assert!((depth_after(1.0, 450) - expect(450)).abs() < 1e-3, "{}", depth_after(1.0, 450));
        assert!((depth_after(450.0, 4_500) - expect(10)).abs() < 1e-3, "{}", depth_after(450.0, 4_500));
    }

    #[test]
    fn defaults_render_as_the_original() {
        assert_eq!(hashes(), [5065355288778672960, 4480620976796668407, 6857446728273201912, 3554667731195362828]);
    }
}
