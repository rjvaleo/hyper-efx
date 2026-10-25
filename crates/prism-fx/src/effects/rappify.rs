//! Rappify: MultiGrainLive's take on Peak's Rappify (`fx::shape::Rappify`),
//! moved here unchanged.
//!
//! Each side has an envelope follower (a one-pole on the rectified signal at
//! the speed's corner, capped at 0.5) that steers a resonant band pass: eight
//! times the envelope, held to 0–1, swings the centre two octaves either way
//! of its resting frequency, scaled by the amount. The band pass is a cheap
//! state-variable form, recomputed every sample, with a damping that falls
//! from 0.95 to 0.05 as the amount rises; its band is boosted by 1 + 2·amount
//! and blended with the dry by the amount. At an amount of zero it does
//! nothing and its state waits.

use crate::{Category, Descriptor, Group, Kernel, Panel, ParamSpec};

const AMOUNT: usize = 0;
const HZ: usize = 1;
const SPEED: usize = 2;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("amount", "Amount", 0.0, 1.0, 0.6).smooth(0.0),
    ParamSpec::new("hz", "Centre", 60.0, 6000.0, 400.0).log().unit("Hz").smooth(0.0),
    ParamSpec::new("speed", "Speed", 1.0, 200.0, 40.0).log().unit("Hz").smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "rappify",
    code: 70,
    version: 1,
    label: "Rappify",
    category: Category::Filter,
    params: PARAMS,
    panel: Panel {
        groups: &[Group { label: "Filter", keys: &["amount", "hz", "speed"] }],
        primary: &["amount", "hz"],
        displays: &[],
        pad: None,
    },
    source: "MultiGrainLive: Rappify (fx::shape::Rappify)",
    sound: None,
};

pub struct Rappify {
    sample_rate: f32,
    values: [f32; 3],
    amount: f32,
    follow: f32,
    q: f32,
    env: [f32; 2],
    lp: [f32; 2],
    bp: [f32; 2],
}

impl Rappify {
    fn derive(&mut self) {
        let sr = self.sample_rate;
        self.amount = self.values[AMOUNT].clamp(0.0, 1.0);
        self.follow = (std::f32::consts::TAU * self.values[SPEED].clamp(1.0, 200.0) / sr).min(0.5);
        self.q = 0.05 + (1.0 - self.amount) * 0.9;
    }
}

impl Kernel for Rappify {
    fn new(sample_rate: f32) -> Self {
        let mut values = [0.0; 3];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = Rappify {
            sample_rate: sample_rate.max(1.0),
            values,
            amount: 0.0,
            follow: 0.0,
            q: 0.0,
            env: [0.0; 2],
            lp: [0.0; 2],
            bp: [0.0; 2],
        };
        k.derive();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(spec) = PARAMS.get(index) {
            self.values[index] = spec.clamp(value);
            self.derive();
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.env = [0.0; 2];
        self.lp = [0.0; 2];
        self.bp = [0.0; 2];
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let amount = self.amount;
        if amount <= 1e-6 {
            return;
        }
        let sr = self.sample_rate;
        let q = self.q;
        let centre = self.values[HZ];
        for (ch, s) in frame.iter_mut().enumerate() {
            let x = *s;
            self.env[ch] += self.follow * (x.abs() - self.env[ch]);
            let open = (self.env[ch] * 8.0).clamp(0.0, 1.0);
            let hz = (centre * 2f32.powf((open * 2.0 - 1.0) * 2.0 * amount)).clamp(20.0, sr * 0.45);
            let g = (std::f32::consts::PI * hz / sr).tan().clamp(1e-4, 1.0);
            let hp = (x - self.lp[ch] - self.bp[ch] * q * 2.0) / (1.0 + g * q * 2.0 + g * g);
            self.bp[ch] += g * hp;
            self.lp[ch] += g * self.bp[ch];
            let band = self.bp[ch] * (1.0 + amount * 2.0);
            *s = x * (1.0 - amount) + band * amount;
        }
    }
}
