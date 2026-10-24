//! Hyperprism Comb Filter: the Comb Filter of Hyperprism-PPC's EDelay class
//! (process 3, routine `CombFilter`), as its code does it. 1.05's menu has
//! it; 1.2.1 still carries the code and its window but no menu item.
//!
//! A feed-forward comb: `out = 0.5·(in + depth·y)`, where `y` is the input
//! `delay` ago, read by linear interpolation. No feedback (the routine has a
//! feedback term, which nothing ever sets from 0). The notches fall at odd
//! multiples of 1 / (2 × delay); at full depth they are complete, and the
//! peaks come back to the input's own level.
//!
//! Delay and depth are smoothed by CEffect's "Smoothing (%)" law,
//! `a = 1e-4 × int(10^(0.04 × (100 − s)))` (1 at 0 %, 0.01 at the default
//! 50 %, 1e-4 at 100 %), in single precision, on every second sample; the
//! read position and its fraction change only then. The delay is held to at
//! least a sample. At rates other than 44.1 kHz the update still runs every
//! second sample and its pole is scaled to keep the time.
//!
//! Changed from the original: settings made before the first frame land at
//! once, as the original's initialisation copies the targets into the
//! smoothers. (Its delay field's maximum was computed from the line's float
//! count, twice the real length; the range here is the window's 0–100 ms.)

use super::hp_delay_law::{samples, smoothing, Line};
use super::hp_echo::fraction;
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const DELAY: usize = 0;
const DEPTH: usize = 1;
const SMOOTHING: usize = 2;

const LONGEST_MS: f32 = 100.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("delayMs", "Delay", 0.0, LONGEST_MS, 50.0).unit("ms").smooth(0.0),
    ParamSpec::new("depth", "Depth", 0.0, 100.0, 0.0).unit("%").smooth(0.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 50.0)
        .unit("%")
        .kind(ParamKind::Int)
        .smooth(0.0)
        .glides("depth"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-comb-filter",
    code: 44,
    version: 1,
    label: "Hyperprism Comb Filter",
    category: Category::Filter,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Pad", keys: &["delayMs", "depth"] },
            Group { label: "Additional settings", keys: &["smoothing"] },
        ],
        primary: &["delayMs", "depth"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "delayMs", x_range: (0.0, LONGEST_MS), y: "depth", y_range: (0.0, 100.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Comb Filter",
    sound: None,
};

pub struct HpCombFilter {
    sample_rate: f32,
    values: [f32; 3],
    line: Line,
    /// The targets (`p[0]` in samples, `p[2]` as a fraction).
    delay: f32,
    depth: f32,
    /// The smoothed values (the original's +0x68, +0x70).
    time: f32,
    gain: f32,
    /// The read: its whole age and fraction (`p[3]`), set on each update.
    age: usize,
    fraction: f32,
    countdown: i32,
    a: f32,
    b: f32,
    started: bool,
}

impl HpCombFilter {
    fn apply(&mut self) {
        self.delay = samples(self.values[DELAY], self.sample_rate);
        self.depth = fraction(self.values[DEPTH]);
        (self.a, self.b) = smoothing(self.values[SMOOTHING], self.sample_rate);
        if !self.started {
            self.land();
        }
    }

    /// The smoothers on their targets and the read where InitializeCombFilter
    /// puts it.
    fn land(&mut self) {
        self.time = self.delay;
        self.gain = self.depth;
        let whole = self.time as i32;
        self.age = whole.max(0) as usize;
        self.fraction = self.time - whole as f32;
    }
}

impl Kernel for HpCombFilter {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 3];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let longest = (samples(LONGEST_MS, sample_rate) as usize).max(1) + 1;
        let mut k = HpCombFilter {
            sample_rate,
            values,
            line: Line::new(longest),
            delay: 0.0,
            depth: 0.0,
            time: 0.0,
            gain: 0.0,
            age: 0,
            fraction: 0.0,
            countdown: 1,
            a: 1.0,
            b: 0.0,
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
        self.line.clear();
        self.countdown = 1;
        self.started = false;
        self.apply();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        self.started = true;
        let update = self.countdown < 1;
        self.countdown -= 1;
        if update {
            self.time = self.a.mul_add(self.delay, self.b * self.time);
            self.gain = self.a.mul_add(self.depth, self.b * self.gain);
            if self.time < 1.0 {
                self.time = 1.0;
            }
            let whole = self.time as i32;
            self.age = whole as usize;
            self.fraction = self.time - whole as f32;
            self.countdown = 1;
        }
        let p = self.line.at(self.age);
        let older = self.line.at(self.age + 1);
        self.line.push(*frame);
        for (ch, s) in frame.iter_mut().enumerate() {
            let y = self.fraction.mul_add(older[ch] - p[ch], p[ch]);
            *s = 0.5 * y.mul_add(self.gain, *s);
        }
    }

    fn tail_seconds(&self) -> f32 {
        self.time.max(self.delay) / self.sample_rate
    }
}
