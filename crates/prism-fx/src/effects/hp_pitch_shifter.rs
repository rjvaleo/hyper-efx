//! Hyperprism Pitch Shifter: Hyperprism-PPC 1.2.1's PitchShift2 routine in
//! its Pitch Shifter mode (EPitchShift, process index 0 — process 32, not in
//! 1.2.1's menu), as its code does it.
//!
//! A two-tap rotating delay line. The read phase `φ` moves `r = pitch − 1` a
//! sample through `0 … 2G − X`, the grain `G` and crossfade `X` in frames:
//!
//! ```text
//! G = int(16384 · clamp(0.01·|r|·grain%, 0.001, 1))     16 … 16384
//! X = int(0.01 · G · xfade%)
//! tap 1: 2G − φ frames back;  tap 2: X − φ frames back
//! φ ≥ X:  wet = tap 1
//! φ < X:  wet = sin(π/2·φ/X)·tap 1 + cos(π/2·φ/X)·tap 2
//! y = (1 − mix)·x + mix·wet;   the line then takes  x + feedback·y
//! ```
//!
//! Each tap is read linearly between neighbouring frames; the crossfade is
//! an equal-power pair of 1024-entry quarter-sine and quarter-cosine tables,
//! indexed at `1023·φ/X` and read linearly. One grain — one trip of `φ` — is
//! `(2G − X)/|r|` samples: `163.84 × grain%` at a full crossfade, whatever
//! the ratio. Pitch below 100 % reads backwards through the same geometry.
//! Feedback takes the whole output, dry included.
//!
//! - **Grain changes.** When smoothing moves `|r|` up or down, or the grain
//!   or crossfade setting moves, the new geometry is taken in two steps at
//!   the next two wraps of `φ` (growing: G at the first, X at the second;
//!   shrinking: X at the first, G at the second), and smoothing holds still
//!   in between. A crossfade move always takes the shrinking path, as in the
//!   original.
//! - **Pad.** X is the pitch, 0–200 % (ratio `v/100`, linear; 0 % freezes the
//!   read); Y the mix, 0–100 %. Linear, starting at 100 % and 50 %.
//! - **Additional settings**, from the window's sliders: Feedback 0–100 %
//!   (0), Grain Size 0–100 % (15), X-Fade Size 0–100 % (100), Smoothing
//!   0–100 % (50).
//! - **Smoothing.** `a = int(10^(0.04·(100 − s)))·0.0001`; on the first
//!   sample and every 513th after, mix and ratio step `v ← a·target +
//!   (1 − a)·v` — about 1.2 s to settle at 50 %.
//! - **Sample rate.** The line (32 768 frames), the grain scale (16 384) and
//!   the smoothing interval are counts at 44.1 kHz, scaled elsewhere.
//!
//! In the original's Pitch Time Changer the same routine sat behind the file
//! reader: the reader varispeeds the source at Speed (time and pitch together)
//! and the shifter corrects by `r = pitch/speed − 1`, recomputed once per
//! block, so time goes as 1/speed and pitch as pitch. Here the host's engine
//! does the speed; this kernel is the shifter, at the ratio it is given.
//!
//! Changed from the original: the Pitch Time Changer took its mix before
//! setting it to 100 % and never smoothed it after, so it ran half dry. The
//! mix here is the control, smoothed, and 100 % is all wet.

use super::hp_mod_common::{samples, Stepper};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};
use core::f64::consts::FRAC_PI_2;

const PITCH: usize = 0;
const MIX: usize = 1;
const FEEDBACK: usize = 2;
const GRAIN: usize = 3;
const XFADE: usize = 4;
const SMOOTHING: usize = 5;

const TABLE: usize = 1024;
/// P7: the crossfade tables' scale.
const SCALE: f32 = 1023.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("pitch", "Pitch", 0.0, 200.0, 100.0).unit("%").smooth(1200.0),
    ParamSpec::new("mix", "Mix", 0.0, 100.0, 50.0).unit("%").smooth(1200.0),
    ParamSpec::new("feedback", "Feedback", 0.0, 100.0, 0.0).unit("%").kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("grain", "Grain size", 0.0, 100.0, 15.0).unit("%").kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("xfade", "X-fade size", 0.0, 100.0, 100.0).unit("%").kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 50.0).unit("%").kind(ParamKind::Int).smooth(0.0).glides("pitch"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-pitch-shifter",
    code: 55,
    version: 1,
    label: "Hyperprism Pitch Shifter",
    category: Category::Pitch,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Pad", keys: &["pitch", "mix"] },
            Group { label: "Additional settings", keys: &["feedback", "grain", "xfade", "smoothing"] },
        ],
        primary: &["pitch", "mix"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "pitch", x_range: (0.0, 200.0), y: "mix", y_range: (0.0, 100.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Pitch Shifter",
    sound: None,
};

/// Where a grain change stands: what the next wrap does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Change {
    None,
    /// |r| or the grain grew: new G at the next wrap.
    Grow,
    /// …and then the new X.
    GrowX,
    /// |r|, the grain or the crossfade shrank: new X at the next wrap.
    Shrink,
    /// …and then the new G.
    ShrinkG,
}

pub struct HpPitchShifter {
    values: [f32; 6],
    frames: Vec<[f32; 2]>,
    write: usize,
    sin: [f32; TABLE],
    cos: [f32; TABLE],
    steps: Stepper,
    /// 16 384 frames at 44.1 kHz.
    grain_scale: f64,
    /// The smoothed ratio − 1 and mix.
    r: f32,
    mix: f32,
    /// The read phase and where it wraps (P2, P12).
    phase: f32,
    wrap: f32,
    /// Grain and crossfade in frames, the second tap's distance past the
    /// first, and the pending grain (C0, C1, P6, P13).
    grain: i64,
    xfade: i64,
    tap: i64,
    pending: f32,
    inv_xfade: f32,
    change: Change,
}

impl HpPitchShifter {
    fn targets(&self) -> (f32, f32) {
        let ratio = self.values[PITCH] * 0.01;
        (ratio - 1.0, self.values[MIX] * 0.01)
    }

    fn feedback(&self) -> f32 {
        (0.01 * self.values[FEEDBACK] as f64) as f32
    }

    /// `clamp(0.01·|r|·grain%, 0.001, 1)`, then scaled to frames.
    fn grain_frames(&self, r: f32) -> f64 {
        let size = (0.01 * (r.abs() * self.values[GRAIN]) as f64) as f32;
        let size = if size > 1.0 { 1.0 } else if (size as f64) < 0.001 { 0.001f32 } else { size };
        self.grain_scale * size as f64
    }

    fn xfade_for(&self, grain: f32) -> i64 {
        (0.01 * (grain * self.values[XFADE]) as f64) as i64
    }

    fn set_inv(&mut self) {
        self.inv_xfade = if self.xfade > 0 { (1.0 / self.xfade as f32 as f64) as f32 } else { 0.0 };
    }

    /// InitializeGrains.
    fn grains(&mut self) {
        self.grain = self.grain_frames(self.r) as i64;
        self.pending = self.grain as f32;
        self.xfade = self.xfade_for(self.grain as f32);
        self.tap = 2 * self.grain - self.xfade;
        self.wrap = self.tap as f32;
        self.set_inv();
    }

    /// Frame `back` frames behind the write head and the frame after it, read
    /// `f` of the way along.
    #[inline]
    fn read(&self, back: i64, f: f32) -> [f32; 2] {
        let n = self.frames.len() as i64;
        let p = (self.write as i64 - back).rem_euclid(n) as usize;
        let q = if p + 1 == self.frames.len() { 0 } else { p + 1 };
        let (a, b) = (self.frames[p], self.frames[q]);
        [a[0] + f * (b[0] - a[0]), a[1] + f * (b[1] - a[1])]
    }

    fn table(t: &[f32; TABLE], k: f32) -> f32 {
        let j = (k as usize).min(TABLE - 1);
        let f = k - j as f32;
        let next = if j + 1 < TABLE { t[j + 1] } else { t[j] };
        t[j] + f * (next - t[j])
    }

    /// What a wrap of the phase does to a pending grain change.
    fn on_wrap(&mut self, r: f32) {
        match self.change {
            Change::None => {}
            Change::ShrinkG => {
                self.grain = self.pending as i64;
                self.tap = 2 * self.grain - self.xfade;
                self.wrap = self.tap as f32;
                if r < 0.0 {
                    self.phase = self.wrap;
                }
                self.change = Change::None;
            }
            Change::Shrink => {
                // P13 keeps the new grain unrounded until the next wrap.
                self.pending = self.grain_frames(r) as f32;
                self.xfade = self.xfade_for(self.pending);
                self.set_inv();
                if r <= 0.0 {
                    self.tap = 2 * self.grain - self.xfade;
                } else {
                    self.wrap = (2 * self.grain - self.xfade) as f32;
                }
                self.change = Change::ShrinkG;
            }
            Change::GrowX => {
                self.xfade = self.xfade_for(self.grain as f32);
                self.set_inv();
                if r <= 0.0 {
                    self.wrap = (2 * self.grain - self.xfade) as f32;
                    self.phase = self.wrap;
                } else {
                    self.tap = 2 * self.grain - self.xfade;
                }
                self.change = Change::None;
            }
            Change::Grow => {
                self.grain = self.grain_frames(r) as i64;
                let xfade = self.xfade_for(self.grain as f32);
                if r <= 0.0 {
                    self.tap = 2 * self.grain - xfade;
                    self.wrap = (2 * self.grain - self.xfade) as f32;
                    self.phase = self.wrap;
                } else {
                    self.wrap = (2 * self.grain - xfade) as f32;
                    self.tap = 2 * self.grain - self.xfade;
                }
                self.change = Change::GrowX;
            }
        }
    }

    fn held(&self) -> bool {
        matches!(self.change, Change::GrowX | Change::ShrinkG)
    }
}

impl Kernel for HpPitchShifter {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 6];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let grain_scale = samples(16_384.0, sample_rate);
        let len = (samples(32_768.0, sample_rate).round() as usize).max(2 * grain_scale as usize + 2).max(64);
        let mut sin = [0.0; TABLE];
        let mut cos = [0.0; TABLE];
        for i in 0..TABLE {
            let a = i as f64 * FRAC_PI_2 / TABLE as f64;
            sin[i] = a.sin().powf(1.0) as f32;
            cos[i] = a.cos().powf(1.0) as f32;
        }
        let mut k = HpPitchShifter {
            values,
            frames: vec![[0.0; 2]; len],
            write: 0,
            sin,
            cos,
            steps: Stepper::new(sample_rate, 512.0, 0.0, values[SMOOTHING]),
            grain_scale,
            r: 0.0,
            mix: 0.0,
            phase: 0.0,
            wrap: 0.0,
            grain: 0,
            xfade: 0,
            tap: 0,
            pending: 0.0,
            inv_xfade: 0.0,
            change: Change::None,
        };
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        let old = self.values[index];
        self.values[index] = spec.clamp(value);
        match index {
            SMOOTHING => self.steps.set_percent(self.values[SMOOTHING]),
            // The sliders' live path: a grain move flags growth or shrinkage
            // for the next wrap; a crossfade move always flags shrinkage.
            GRAIN if self.change == Change::None => {
                self.change = if old < self.values[GRAIN] { Change::Grow } else { Change::Shrink };
            }
            XFADE if self.change == Change::None => self.change = Change::Shrink,
            _ => {}
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.steps.reset();
        (self.r, self.mix) = self.targets();
        self.change = Change::None;
        self.grains();
        self.frames.fill([0.0; 2]);
        self.write = 0;
        self.phase = 0.0;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let mut r = self.r;
        if self.steps.due_unless(self.held()) {
            let (r_target, mix_target) = self.targets();
            self.mix = self.steps.step(self.mix, mix_target);
            let next = self.steps.step(r, r_target);
            if self.change == Change::None {
                let mut moved = next - r;
                if next < 0.0 {
                    moved = -moved;
                }
                if moved > 0.0 {
                    self.change = Change::Grow;
                } else if moved < 0.0 {
                    self.change = Change::Shrink;
                }
            }
            r = next;
        }
        let x = *frame;
        self.frames[self.write] = x;
        let mix = self.mix;
        let dry = 1.0 - mix;
        let i = self.phase as i64;
        let f = self.phase - i as f32;
        let back = 2 * self.grain - i;
        let y = if self.grain < 2 || self.xfade <= i {
            let wet = if self.grain < 2 { x } else { self.read(back, f) };
            [x[0] * dry + wet[0] * mix, x[1] * dry + wet[1] * mix]
        } else {
            let one = self.read(back, f);
            let two = self.read(back - self.tap, f);
            let k = SCALE * (self.phase * self.inv_xfade);
            let (s, c) = (Self::table(&self.sin, k), Self::table(&self.cos, k));
            [x[0] * dry + mix * (one[0] * s + two[0] * c), x[1] * dry + mix * (one[1] * s + two[1] * c)]
        };
        let fb = self.feedback();
        let w = &mut self.frames[self.write];
        w[0] += y[0] * fb;
        w[1] += y[1] * fb;
        self.write += 1;
        if self.write == self.frames.len() {
            self.write = 0;
        }
        *frame = y;

        self.phase += r;
        let high = self.wrap < self.phase;
        if high {
            self.phase -= self.wrap;
        }
        let low = self.phase < 0.0;
        if low {
            self.phase += self.wrap;
        }
        if !self.phase.is_finite() {
            self.phase = 0.0;
        }
        if high || low {
            self.on_wrap(r);
        }
        self.r = r;
    }

    fn tail_seconds(&self) -> f32 {
        // Two grains of the longest line, more with feedback.
        1.0
    }
}
