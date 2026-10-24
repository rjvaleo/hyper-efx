//! Hyperprism Single Delay: Hyperprism-PPC 1.2.1's Single Delay, as its code
//! does it (class EDelay2, process 0, routine `Delay2`).
//!
//! Echo's routine with the feedback fixed at 0 and the pad's Y axis as the
//! mix: one whole-sample tap, `out = in·(1 − mix) + mix·y`, and a time change
//! walked in equal-power crossfades of max(4096, distance) samples, each
//! covering "Param. Speed Ch." % of the remaining way, so the pitch never
//! moves. See [`super::hp_echo`] for the walk in full.
//!
//! The original's window also has a "Mix (% Effect)" slider, which 1.2.1
//! stores and never reads for this process (the pad's Y is the mix); it is
//! left out here, as it does nothing.
//!
//! Param. Speed Ch. defaults to 50 %, the window's slider value, which the
//! original reads as its default; the edit field beside it shows 100.
//!
//! Changed from the original: setting the delay no longer also sets the mix
//! to 0.01 × the delay in ms (a missing `break`), and the time-walk fixes
//! listed for Echo: 0 ms is reached, a tap is never crossfaded with itself, a
//! walk cannot stall, and a jump at speed 100 leaves no stale tap. Settings
//! made before the first frame land at once.

use super::hp_echo::{fraction, Delay2, LONGEST_MS};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const DELAY: usize = 0;
const MIX: usize = 1;
const SPEED: usize = 2;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("delayMs", "Delay", 0.0, LONGEST_MS, 1000.0).unit("ms").smooth(0.0),
    ParamSpec::new("mix", "Mix", 0.0, 100.0, 50.0).unit("%").smooth(0.0),
    ParamSpec::new("speed", "Param. Speed Ch.", 1.0, 100.0, 50.0)
        .unit("%")
        .kind(ParamKind::Int)
        .smooth(0.0)
        .glides("delayMs"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-single-delay",
    code: 40,
    version: 1,
    label: "Hyperprism Single Delay",
    category: Category::Delay,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Pad", keys: &["delayMs", "mix"] },
            Group { label: "Additional settings", keys: &["speed"] },
        ],
        primary: &["delayMs", "mix"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "delayMs", x_range: (0.0, LONGEST_MS), y: "mix", y_range: (0.0, 100.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Single Delay",
    sound: None,
};

pub struct HpSingleDelay {
    values: [f32; 3],
    delay: Delay2,
    mix: f32,
    dry: f32,
    step: f32,
}

impl HpSingleDelay {
    fn apply(&mut self) {
        self.mix = fraction(self.values[MIX]);
        self.dry = (1.0 - self.mix as f64) as f32;
        self.step = fraction(self.values[SPEED]);
    }
}

impl Kernel for HpSingleDelay {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 3];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpSingleDelay { values, delay: Delay2::new(sample_rate, values[DELAY]), mix: 0.0, dry: 0.0, step: 0.0 };
        k.apply();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        let v = spec.clamp(value);
        self.values[index] = v;
        if index == DELAY {
            self.delay.set_delay(v, self.values[SPEED]);
        }
        self.apply();
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.delay.reset(self.values[DELAY]);
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        self.delay.tick(frame, self.mix, self.dry, 0.0, self.step);
    }

    fn tail_seconds(&self) -> f32 {
        self.delay.seconds()
    }
}
