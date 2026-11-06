//! Hyperprism Doppler: the Doppler of Hyperprism-PPC's EDelay class (process
//! 4, routine `Doppler`), as its code does it. 1.05's menu has it; 1.2.1
//! still carries the code but neither a menu item nor a window.
//!
//! A sound source on a plane around the listener: the pad's X is metres to
//! the right, Y metres ahead. The two inputs are summed to one line at half
//! level, and the output is that line read `r / 340` seconds late (r the
//! distance, at least 1 m; 340 m/s for sound), by linear interpolation, and
//! panned by the bearing: `θ = ½·atan2(−x, y) + π/4`, left `sin θ`, right
//! `cos θ`, an equal-power pan from hard left at x < 0 to hard right at
//! x > 0. There is no dry signal and no fall-off with distance. A moving
//! source changes the delay, so its pitch rises as it comes closer and falls
//! as it goes away: the Doppler shift.
//!
//! On every second sample the position takes one step of CEffect's
//! "Smoothing (%)" law (`a = 1e-4 × int(10^(0.04 × (100 − s)))`), and the
//! delay's rate is set to cover 1/44 of the way to the new distance's delay
//! each sample until the next update, so the delay trails the position by
//! about 44 samples on top of the smoothing. Behind the listener the bearing
//! passes ±π: one side's gain turns negative, and directly behind the pan
//! flips from one side to the other; that is the original's, kept.
//!
//! The arithmetic is the original's, in single precision where it was, fused
//! multiply-adds included. At rates other than 44.1 kHz the updates still
//! run every second sample, with the smoothing pole and the 1/44 rate scaled
//! to keep their times.
//!
//! Not known from 1.2.1, which has no window for it: the pad's ranges and
//! starting position, and the smoothing's default. The ranges here are
//! ±100 m, the source starts 10 m ahead, and the smoothing at 50 % like the
//! Comb Filter's window.
//!
//! Changed from the original: a fresh or reset Doppler starts with its
//! delay at the source's distance; the original's initialisation seeded the
//! read from the x position instead and left the delay to sweep there.
//! Settings made before the first frame land at once.
//!
//! Added (not in the original): **Smoothing step**, 1–2048 samples, default
//! 2: how often the position takes its smoothing step and the delay its new
//! rate. Like the original's 2 it is a count of samples at any rate. Each
//! update the delay still covers the share of the way the original's two
//! samples at 1/44 cover, kept in time — `1 − (1 − 2/44)^(n/2)` over n
//! samples — so it trails the position by the same 44 samples and stays
//! stable at any step; longer steps move the position in coarser ramps and
//! slow the smoothing in proportion. The default is the original exactly.

use super::hp_delay_law::{smoothing, ORIGINAL_RATE};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const X: usize = 0;
const Y: usize = 1;
const SMOOTHING: usize = 2;
const STEP: usize = 3;

/// The original's update: every second sample.
const EVERY: f32 = 2.0;

const RANGE_M: f32 = 100.0;
/// The original's constant for 1/340, a double written to ten places.
const PER_METRE: f64 = 0.002_941_176_5;
/// The delay's share of the way per sample at 44.1 kHz.
const FOLLOW: f32 = 1.0 / 44.0;
/// The line: four seconds at 44.1 kHz, the original's 176 400 floats.
const LINE_S: f64 = 4.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("xM", "X (right)", -RANGE_M, RANGE_M, 0.0).unit("m").smooth(0.0),
    ParamSpec::new("yM", "Y (ahead)", -RANGE_M, RANGE_M, 10.0).unit("m").smooth(0.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 50.0)
        .unit("%")
        .kind(ParamKind::Int)
        .smooth(0.0)
        .glides("xM"),
    ParamSpec::new("smoothingStep", "Smoothing step", 1.0, 2048.0, EVERY)
        .unit("samples")
        .kind(ParamKind::Int)
        .smooth(0.0)
        .glides("xM"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-doppler",
    code: 45,
    version: 1,
    label: "Hyperprism Doppler",
    category: Category::Stereo,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Pad", keys: &["xM", "yM"] },
            Group { label: "Additional settings", keys: &["smoothing"] },
            Group { label: "Added (not in the original)", keys: &["smoothingStep"] },
        ],
        primary: &["xM", "yM"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "xM", x_range: (-RANGE_M, RANGE_M), y: "yM", y_range: (-RANGE_M, RANGE_M) }),
    },
    source: "Hyperprism-PPC 1.2.1: Doppler",
    sound: None,
};

pub struct HpDoppler {
    sample_rate: f32,
    values: [f32; 4],
    line: Vec<f32>,
    write: usize,
    /// The smoothed position (the original's +0x68, +0x70).
    x: f32,
    y: f32,
    /// The delay in samples and its change per sample (`p[5]`, `p[6]`).
    delay: f32,
    rate: f32,
    /// The read: whole age and fraction (`p[3]`).
    age: usize,
    fraction: f32,
    left: f32,
    right: f32,
    countdown: i32,
    a: f32,
    b: f32,
    follow: f32,
    /// The rate's share of the way per sample for the step, and the
    /// countdown's reload: `follow` and 1 at the original's step of 2.
    share: f32,
    reload: i32,
    started: bool,
}

impl HpDoppler {
    fn apply(&mut self) {
        (self.a, self.b) = smoothing(self.values[SMOOTHING], self.sample_rate);
        let n = self.values[STEP];
        self.reload = n as i32 - 1;
        self.countdown = self.countdown.min(self.reload);
        self.share = if n == EVERY {
            self.follow
        } else {
            let per_two = 2.0 * self.follow as f64;
            ((1.0 - (1.0 - per_two).powf(f64::from(n) / 2.0)) / f64::from(n)) as f32
        };
        if !self.started {
            self.land();
        }
    }

    /// The position on its target, the delay at its distance, still.
    fn land(&mut self) {
        self.x = self.values[X];
        self.y = self.values[Y];
        self.delay = self.aim();
        self.rate = 0.0;
        self.read_at();
    }

    /// The pan for the smoothed position; returns the delay it asks for.
    fn aim(&mut self) -> f32 {
        let (x, y) = (self.x, self.y);
        let mut r = (x.mul_add(x, y * y) as f64).sqrt() as f32;
        if (r as f64) < 1.0 {
            r = 1.0;
        }
        let theta = (0.5 * (-x as f64).atan2(y as f64)) as f32 + core::f32::consts::FRAC_PI_4;
        self.left = (theta as f64).sin() as f32;
        self.right = (theta as f64).cos() as f32;
        (self.sample_rate as f64 * (PER_METRE * r as f64)) as f32
    }

    fn read_at(&mut self) {
        let whole = self.delay as i32;
        self.age = whole.max(0) as usize;
        self.fraction = self.delay - whole as f32;
    }
}

impl Kernel for HpDoppler {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 4];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let follow = if sample_rate as f64 == ORIGINAL_RATE {
            FOLLOW
        } else {
            // Two samples at 2/44 a sample, as a pole, kept in time.
            let pole = (1.0 - 2.0 * FOLLOW as f64).powf(ORIGINAL_RATE / sample_rate as f64);
            ((1.0 - pole) / 2.0) as f32
        };
        let length = (LINE_S * sample_rate as f64) as usize;
        let mut k = HpDoppler {
            sample_rate,
            values,
            line: vec![0.0; length],
            write: 0,
            x: 0.0,
            y: 0.0,
            delay: 0.0,
            rate: 0.0,
            age: 0,
            fraction: 0.0,
            left: 0.0,
            right: 0.0,
            countdown: 1,
            a: 1.0,
            b: 0.0,
            follow,
            share: follow,
            reload: 1,
            started: false,
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
        self.apply();
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.line.fill(0.0);
        self.write = 0;
        self.countdown = self.values[STEP] as i32 - 1;
        self.started = false;
        self.apply();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        self.started = true;
        let update = self.countdown < 1;
        self.countdown -= 1;
        if update {
            self.y = self.a.mul_add(self.values[Y], self.b * self.y);
            self.x = self.a.mul_add(self.values[X], self.b * self.x);
            let target = self.aim();
            self.rate = self.share * (target - self.delay);
            self.countdown = self.reload;
        }
        let len = self.line.len();
        let at = |age: usize| self.line[(self.write + len - age % len) % len];
        let (now, before) = (at(self.age), at(self.age + 1));
        let v = self.fraction.mul_add(before - now, now);
        let input = (0.5 * (frame[0] + frame[1]) as f64) as f32;
        frame[0] = v * self.left;
        frame[1] = v * self.right;
        self.line[self.write] = input;
        self.write = (self.write + 1) % len;
        self.delay += self.rate;
        self.read_at();
    }

    fn tail_seconds(&self) -> f32 {
        let far = (2.0f64.sqrt() * RANGE_M as f64 * PER_METRE) as f32;
        (self.delay / self.sample_rate).max(far)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::hp_fr::render::checksum;

    /// The renders before any control was added, at the defaults and at two
    /// other settings, one at 48 kHz: the added controls at their defaults
    /// leave every bit as it was.
    #[test]
    fn defaults_render_as_the_original() {
        let got = [
            checksum::<HpDoppler>(44_100.0, 3.0, &[], &[]),
            checksum::<HpDoppler>(44_100.0, 3.0, &[("xM", 30.0), ("yM", 5.0)], &[("xM", -40.0), ("yM", -20.0)]),
            checksum::<HpDoppler>(48_000.0, 3.0, &[("smoothing", 30.0), ("yM", 80.0)], &[("yM", 2.0)]),
        ];
        assert_eq!(got, [86303782164117845, 4967918048877032271, 5957184625838451813]);
    }

    fn doppler(step: f32) -> HpDoppler {
        let mut k = HpDoppler::new(44_100.0);
        k.set_param(STEP, step);
        k.reset();
        k
    }

    /// Samples until the position is within 1 % of a move 50 m right.
    fn settles_in(step: f32) -> usize {
        let mut k = doppler(step);
        k.tick(&mut [0.0, 0.0]);
        k.set_param(X, 50.0);
        (1..60 * 44_100)
            .find(|_| {
                k.tick(&mut [0.0, 0.0]);
                (k.x - 50.0).abs() < 0.5
            })
            .unwrap()
    }

    #[test]
    fn the_smoothing_step_paces_the_glide() {
        // At 50 % (a = 0.01) the position takes 459 updates to come within 1 %.
        let at = |n: usize| settles_in(n as f32).abs_diff(459 * n);
        assert!(at(2) <= 2 && at(1) <= 1 && at(64) <= 64, "{} {} {}", at(1), at(2), at(64));
    }

    #[test]
    fn the_delay_trails_the_position_alike_and_stays_put_at_any_step() {
        // Each update covers the original's share of the way in its time,
        // so the delay lands on the distance's delay at long steps too.
        for n in [1.0, 2.0, 100.0, 2_048.0] {
            let mut k = doppler(n);
            k.set_param(SMOOTHING, 0.0);
            k.tick(&mut [0.0, 0.0]);
            k.set_param(Y, 50.0);
            let mut worst = 0f32;
            for _ in 0..10 * 44_100 {
                let mut frame = [0.5, 0.5];
                k.tick(&mut frame);
                worst = worst.max(frame[0].abs()).max(frame[1].abs());
            }
            let want = (44_100.0 * (PER_METRE * 50.0)) as f32;
            assert!((k.delay - want).abs() < 0.5, "step {n}: delay {} for {want}", k.delay);
            assert!(worst <= 0.5, "step {n}: {worst}");
        }
    }
}
