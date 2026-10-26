//! Doppler: MultiGrainLive's `Doppler` (`ShapeKind::Doppler`), the moving
//! source of Part 2, sections 4.1 and 4.4 of Jon Dattorro's "Effect Design",
//! sample for sample.
//!
//! A two-second line per side read `current` samples back. The read starts at
//! the distance and then travels `velocity / speed of sound` samples per
//! sample — away for positive velocity, so the pitch falls — held between one
//! sample and the end of the line. Setting the distance puts the source back
//! there. The output is `dry·x + wet·delayed`.

use super::mgl_dattorro_common::{spec, store, whole_rate, Delay};
use crate::{Category, Descriptor, Display, Group, Kernel, Panel, ParamSpec};

const DISTANCE: usize = 0;

static PARAMS: &[ParamSpec] = &[
    spec("distanceMs", "Distance", 0.1, 1500.0, 40.0).log().unit("ms"),
    spec("velocityMps", "Velocity", -150.0, 150.0, 0.0).unit("m/s"),
    spec("wet", "Wet", 0.0, 1.0, 1.0),
    spec("dry", "Dry", 0.0, 1.0, 0.0),
    spec("speedMps", "Speed of sound", 20.0, 2000.0, 343.0).log().unit("m/s"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "doppler",
    code: 94,
    version: 1,
    label: "Doppler",
    category: Category::Delay,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Source", keys: &["distanceMs", "velocityMps", "speedMps"] },
            Group { label: "Mix", keys: &["wet", "dry"] },
        ],
        primary: &["distanceMs", "velocityMps"],
        displays: &[Display::Meter],
        pad: None,
    },
    source: "MultiGrainLive: Doppler (dattorro.rs, Doppler)",
    sound: None,
};

pub struct MglDoppler {
    p: [f32; 5],
    delay: [Delay; 2],
    current: f32,
    sr: u32,
}

impl Kernel for MglDoppler {
    fn new(sample_rate: f32) -> Self {
        let sr = whole_rate(sample_rate);
        let n = (sr as usize * 2).max(8);
        MglDoppler {
            p: [40.0, 0.0, 1.0, 0.0, 343.0],
            delay: [Delay::new(n), Delay::new(n)],
            current: 40.0 * sr as f32 / 1000.0,
            sr,
        }
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if store(PARAMS, &mut self.p, index, value) && index == DISTANCE {
            self.current = self.p[DISTANCE] * self.sr as f32 / 1000.0;
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.p.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        for d in &mut self.delay {
            d.clear();
        }
        self.current = self.p[DISTANCE] * self.sr as f32 / 1000.0;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        for (ch, out) in frame.iter_mut().enumerate() {
            let dry = *out;
            let wet = self.delay[ch].tap(self.current.max(1.0));
            self.delay[ch].push(dry);
            *out = dry * self.p[3] + wet * self.p[2];
        }
        self.current =
            (self.current + self.p[1] / self.p[4].max(1.0)).clamp(1.0, (self.delay[0].b.len() - 2) as f32);
    }

    fn tail_seconds(&self) -> f32 {
        2.0
    }
}
