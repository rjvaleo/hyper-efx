//! Amp fit: MultiGrainLive's live version of Peak's Amplitude Fit
//! (`fx::shape::AmplitudeFit`), moved here unchanged.
//!
//! A peak follower on the louder side rises in about a millisecond and falls
//! over one grain. The gain that would put that peak at 0.9 (held to
//! 0.03–32, or unity while the peak is under the floor) is approached at the
//! fast rate, and the amount blends from unity gain to it; both sides take
//! the same gain. Quiet passages are lifted to meet loud ones, which is the
//! sound of it. At an amount of zero it does nothing and its follower waits.
//!
//! The same machine runs inside Utility.

use crate::{Category, Descriptor, Display, Group, Kernel, Panel, ParamSpec};

const GRAIN: usize = 0;
const AMOUNT: usize = 1;
const FLOOR: usize = 2;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("grainMs", "Grain", 5.0, 500.0, 30.0).log().unit("ms").smooth(0.0),
    ParamSpec::new("amount", "Amount", 0.0, 1.0, 0.7).smooth(0.0),
    ParamSpec::new("floorDb", "Floor", -80.0, -20.0, -50.0).unit("dB").smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "fit",
    code: 72,
    version: 1,
    label: "Amp fit",
    category: Category::Dynamics,
    params: PARAMS,
    panel: Panel {
        groups: &[Group { label: "Fit", keys: &["amount", "grainMs", "floorDb"] }],
        primary: &["amount", "grainMs"],
        displays: &[Display::Meter],
        pad: None,
    },
    source: "MultiGrainLive: Amp fit (fx::shape::AmplitudeFit)",
    sound: None,
};

/// The follower and gain, shared with Utility.
#[derive(Clone, Copy)]
pub(crate) struct FitCore {
    up: f32,
    down: f32,
    floor: f32,
    amount: f32,
    peak: f32,
    gain: f32,
}

impl FitCore {
    pub(crate) fn new(sample_rate: f32, grain_ms: f32, amount: f32, floor_db: f32) -> Self {
        let mut core = FitCore { up: 0.0, down: 0.0, floor: 0.0, amount: 0.0, peak: 0.0, gain: 1.0 };
        core.configure(sample_rate, grain_ms, amount, floor_db);
        core
    }

    pub(crate) fn configure(&mut self, sample_rate: f32, grain_ms: f32, amount: f32, floor_db: f32) {
        let sr = sample_rate;
        self.amount = amount.clamp(0.0, 1.0);
        let grain = ((grain_ms.clamp(5.0, 500.0) / 1000.0) * sr).max(8.0);
        self.up = (1.0 / (0.001 * sr)).min(0.5);
        self.down = (1.0 / grain).min(0.5);
        self.floor = 10f32.powf(floor_db.clamp(-80.0, -20.0) / 20.0);
    }

    pub(crate) fn reset(&mut self) {
        self.peak = 0.0;
        self.gain = 1.0;
    }

    #[inline]
    pub(crate) fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.amount <= 1e-6 {
            return;
        }
        let mut loud = 0f32;
        for s in frame.iter() {
            loud = loud.max(s.abs());
        }
        let k = if loud > self.peak { self.up } else { self.down };
        self.peak += k * (loud - self.peak);
        let want = if self.peak > self.floor { 0.9 / self.peak.max(1e-9) } else { 1.0 };
        self.gain += self.up * (want.clamp(0.03, 32.0) - self.gain);
        let g = 1.0 + (self.gain - 1.0) * self.amount;
        for s in frame.iter_mut() {
            *s *= g;
        }
    }
}

pub struct Fit {
    sample_rate: f32,
    values: [f32; 3],
    core: FitCore,
}

impl Kernel for Fit {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let values = [PARAMS[GRAIN].default, PARAMS[AMOUNT].default, PARAMS[FLOOR].default];
        Fit { sample_rate, values, core: FitCore::new(sample_rate, values[GRAIN], values[AMOUNT], values[FLOOR]) }
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(spec) = PARAMS.get(index) {
            self.values[index] = spec.clamp(value);
            let v = self.values;
            self.core.configure(self.sample_rate, v[GRAIN], v[AMOUNT], v[FLOOR]);
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.core.reset();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        self.core.tick(frame);
    }
}
