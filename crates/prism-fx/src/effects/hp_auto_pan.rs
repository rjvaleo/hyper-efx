//! Hyperprism Auto-Pan: Hyperprism-PPC 1.2.1's process 28, "Sweeping Pan"
//! (class ESpatializer, routine `SweepingPan`), as its code does it.
//!
//! The input is summed to mono, ½(L + R), and panned by a sine LFO about a
//! fixed Position: pan = ½(lfo × depth + position) + ½, clamped to 0…1, with
//! position running −1…+1 over the slider's 0–100 %. The pan law is constant
//! power, read without interpolation from a 1024-entry quarter-sine table:
//! left = Q[int((1 − pan) × 1023)], right = Q[int(pan × 1023)]. So the centre
//! reads entry 511 on both sides, 0.706 (−3.02 dB), and at no depth it is a
//! static pan of the mono sum rather than unity.
//!
//! The LFO is a 1024-entry sine table read with linear interpolation. Its
//! phase starts at 0 — the centre, moving right first — and advances
//! Hz × 1024 / SR table steps a sample, but wraps at 1023, so the LFO runs
//! 1024/1023 fast (0.1 %). Depth and rate glide by Hyperprism's smoothing:
//! every 45th sample, `a·target + (1 − a)·value`, a from the Smoothing slider.
//! Position does not glide.
//!
//! Position starts at 25 % — a pan offset of −0.5, left of centre. The
//! constructor sets 50 %, but opening the window reads the settings dialog
//! back into the effect (`InitializeAdditionalSettings` calls
//! `GetAdditionalSettings`, which calls `ApplyAdditionalSettings`), so the
//! slider resource's 25 is what the process starts with. Smoothing starts at
//! the slider's 30 % (a = 0.063) the same way.
//!
//! Not reproduced: the original's first moments after choosing the process,
//! when the rate glides in from 1 Hz (a value slot nothing else writes)
//! because the effect is set up before the window sends its start values.
//! Here a reset lands every glide on its target. The routine's other three
//! siblings — Stereo Tremolo, Crossing Pan and Hyper Pan — are empty in the
//! original and have no algorithm to build. The window's rate is 0–20 Hz; the
//! original would take typed rates to ±32767 Hz.
//!
//! Added (not in the original):
//!
//! - **Start phase**, 0–360°, default 0°: where in its cycle the LFO starts,
//!   360° to the 1024-entry table (90° starts hard right). A move shifts the
//!   running phase by the change; a reset starts it there. At 0° it is the
//!   original exactly.
//! - **Pan law**, default Constant power (the original's quarter-sine table,
//!   read as above), or Linear 0 dB — each side at full level until the pan
//!   passes the centre, then falling in a straight line, so the centre is
//!   unity — or Linear −6 dB, `left = 1 − pan`, `right = pan`, the centre at
//!   half level. At Constant power it is the original exactly.
//! - **Smoothing step**, 1–2048 samples at 44.1 kHz (scaled with the rate),
//!   default 45: how often depth and rate glide. At 45 it is the original
//!   exactly.

use super::hp_stereo_shared::{quarter_sin_table, scaled, sin_table, Stepper};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const RATE: usize = 0;
const DEPTH: usize = 1;
const POSITION: usize = 2;
const SMOOTHING: usize = 3;
const START: usize = 4;
const LAW: usize = 5;
const STEP: usize = 6;

const TABLE: usize = 1024;
/// Where the phase wraps: one entry short of the table.
const WRAP: f32 = (TABLE - 1) as f32;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("rateHz", "Frequency", 0.0, 20.0, 5.0).unit("Hz").smooth(0.0),
    ParamSpec::new("depth", "Depth", 0.0, 100.0, 50.0).unit("%").smooth(0.0),
    ParamSpec::new("position", "Position", 0.0, 100.0, 25.0).unit("%").kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 30.0).unit("%").kind(ParamKind::Int).smooth(0.0),
    ParamSpec::new("startPhaseDeg", "Start phase", 0.0, 360.0, 0.0).unit("°").smooth(0.0),
    ParamSpec::new("panLaw", "Pan law", 0.0, 2.0, 0.0)
        .kind(ParamKind::Choice(&["Constant power", "Linear 0 dB", "Linear −6 dB"]))
        .smooth(0.0),
    ParamSpec::new("smoothingStep", "Smoothing step", 1.0, 2048.0, 45.0)
        .unit("samples")
        .kind(ParamKind::Int)
        .smooth(0.0)
        .glides("rateHz"),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-auto-pan",
    code: 57,
    version: 1,
    label: "Hyperprism Auto-Pan",
    category: Category::Stereo,
    params: PARAMS,
    panel: Panel {
        // The original: the X/Y pad, frequency across and depth up, and the
        // position and smoothing sliders in its settings.
        groups: &[
            Group { label: "Sweep", keys: &["rateHz", "depth"] },
            Group { label: "Settings", keys: &["position", "smoothing"] },
            Group { label: "Added (not in the original)", keys: &["startPhaseDeg", "panLaw", "smoothingStep"] },
        ],
        primary: &["rateHz", "depth"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "rateHz", x_range: (0.0, 20.0), y: "depth", y_range: (0.0, 100.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Auto-Pan",
    sound: None,
};

pub struct HpAutoPan {
    sample_rate: f32,
    values: [f32; 7],
    sine: [f32; TABLE],
    quarter: [f32; TABLE],
    /// Targets: table steps a sample, depth as a fraction.
    inc: f32,
    depth: f32,
    position: f32,
    inc_s: f32,
    depth_s: f32,
    phase: f32,
    /// The smoothing's coefficients; its countdown is kept here, so its
    /// period can move.
    step: Stepper,
    period: u32,
    count: u32,
    /// The start phase in table steps, and the pan law.
    start: f32,
    law: u8,
}

impl HpAutoPan {
    fn targets(&mut self) {
        self.inc = (self.values[RATE] * TABLE as f32) / self.sample_rate;
        self.depth = (0.01 * self.values[DEPTH] as f64) as f32;
        self.position = (0.02 * self.values[POSITION] as f64 - 1.0) as f32;
        self.law = self.values[LAW].round() as u8;
        self.period = scaled(self.values[STEP], self.sample_rate) as u32;
        self.count = self.count.min(self.period - 1);
        let start = self.values[START] / 360.0 * TABLE as f32;
        if start != self.start {
            self.phase = wrap(self.phase + start - self.start);
            self.start = start;
        }
    }

    /// The countdown as `Stepper::fires` runs it: true on every period-th sample.
    #[inline]
    fn fires(&mut self) -> bool {
        if self.count == 0 {
            self.count = self.period - 1;
            true
        } else {
            self.count -= 1;
            false
        }
    }
}

/// A phase brought back into 0–1023, where the original wraps it.
fn wrap(mut phase: f32) -> f32 {
    while phase >= WRAP {
        phase -= WRAP;
    }
    while phase < 0.0 {
        phase += WRAP;
    }
    phase
}

impl Kernel for HpAutoPan {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 7];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpAutoPan {
            sample_rate,
            values,
            sine: sin_table(),
            quarter: quarter_sin_table(),
            inc: 0.0,
            depth: 0.0,
            position: 0.0,
            inc_s: 0.0,
            depth_s: 0.0,
            phase: 0.0,
            step: Stepper::new(sample_rate, values[SMOOTHING]),
            period: 1,
            count: 0,
            start: 0.0,
            law: 0,
        };
        k.targets();
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
            SMOOTHING => self.step.set_smoothing(self.values[SMOOTHING]),
            _ => self.targets(),
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.inc_s = self.inc;
        self.depth_s = self.depth;
        // The original starts half a cycle on for a negative rate, which the
        // window's range never gives.
        self.phase = wrap(self.start + if self.inc < 0.0 { 512.0 } else { 0.0 });
        self.step.restart();
        self.count = self.period - 1;
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.fires() {
            self.depth_s = self.step.toward(self.depth_s, self.depth);
            self.inc_s = self.step.toward(self.inc_s, self.inc);
        }
        let i = self.phase as usize;
        let f = self.phase - i as f32;
        let lfo = if i + 1 < TABLE {
            f * (self.sine[i + 1] - self.sine[i]) + self.sine[i]
        } else {
            // The last entry wraps to the first. The original added the value
            // one past the end of its table here; its phase never gets here
            // (it wraps at 1023), and neither does this one.
            f * (self.sine[0] - self.sine[TABLE - 1]) + self.sine[TABLE - 1]
        };
        let pan = ((0.5 * ((lfo * self.depth_s + self.position) as f64) + 0.5) as f32).clamp(0.0, 1.0);
        let m = (0.5 * (frame[0] + frame[1]) as f64) as f32;
        let (left, right) = match self.law {
            0 => {
                let span = WRAP as f64;
                (self.quarter[((1.0 - pan as f64) * span) as usize], self.quarter[(pan as f64 * span) as usize])
            }
            1 => ((2.0 * (1.0 - pan)).min(1.0), (2.0 * pan).min(1.0)),
            _ => (1.0 - pan, pan),
        };
        frame[0] = left * m;
        frame[1] = right * m;
        self.phase = wrap(self.phase + self.inc_s);
    }
}

#[cfg(test)]
mod tests {
    use super::super::hp_mod_common::check;
    use super::*;

    /// The renders as they were before any control was added, at both rates.
    #[test]
    fn defaults_render_as_the_original() {
        let pad = [(RATE, 2.0), (DEPTH, 100.0), (POSITION, 60.0)];
        let moved = [(RATE, 11.0), (DEPTH, 20.0)];
        let got = [
            check::checksums::<HpAutoPan>(44_100.0, 3.0, &pad, &moved),
            check::checksums::<HpAutoPan>(48_000.0, 3.0, &pad, &moved),
        ];
        assert_eq!(
            got,
            [[17557946306205525313, 6129279026959145644, 14431653288947737162],
             [9756249412647383586, 2359799646077667358, 2254260026791631866]]
        );
    }

    fn pan(settings: &[(usize, f32)]) -> HpAutoPan {
        let mut k = HpAutoPan::new(44_100.0);
        for &(i, v) in settings {
            k.set_param(i, v);
        }
        k.reset();
        k
    }

    /// The first frame of a steady 0.5 on both sides, at full depth about the centre.
    fn first(settings: &[(usize, f32)]) -> [f32; 2] {
        let mut k = pan(&[&[(DEPTH, 100.0), (POSITION, 50.0)], settings].concat());
        let mut frame = [0.5, 0.5];
        k.tick(&mut frame);
        frame
    }

    #[test]
    fn the_start_phase_sets_where_the_sweep_begins() {
        let [l, r] = first(&[]);
        assert_eq!(l, r, "the original starts at the centre");
        let [l, r] = first(&[(START, 90.0)]);
        assert!(l == 0.0 && r > 0.49, "a quarter on is hard right: {l} {r}");
        let [l, r] = first(&[(START, 270.0)]);
        assert!(r == 0.0 && l > 0.49, "three quarters on is hard left: {l} {r}");
        // A move while running shifts the sweep by the change.
        let x = check::input(1.0, 44_100.0);
        let still = check::render(&mut pan(&[]), &x, &[]);
        let moved = check::render(&mut pan(&[]), &x, &[(START, 180.0)]);
        let half = x.0.len() / 2;
        assert_eq!(still.0[..half], moved.0[..half]);
        assert_ne!(still.0[half..], moved.0[half..]);
    }

    #[test]
    fn the_pan_law_sets_the_centre_gain() {
        let centre = |law: f32| {
            let mut k = pan(&[(DEPTH, 0.0), (POSITION, 50.0), (LAW, law)]);
            let mut frame = [0.5, 0.5];
            k.tick(&mut frame);
            assert_eq!(frame[0], frame[1]);
            20.0 * (frame[0] as f64 / 0.5).log10()
        };
        assert!((centre(0.0) + 3.02).abs() < 0.01, "{}", centre(0.0));
        assert_eq!(centre(1.0), 0.0);
        assert!((centre(2.0) + 6.02).abs() < 0.01, "{}", centre(2.0));
        // Hard right, every law gives the left nothing and the right all.
        for law in [0.0, 1.0, 2.0] {
            let mut k = pan(&[(DEPTH, 0.0), (POSITION, 100.0), (LAW, law)]);
            let mut frame = [0.5, 0.5];
            k.tick(&mut frame);
            assert!(frame[0] == 0.0 && (frame[1] - 0.5).abs() < 1e-3, "{law}: {frame:?}");
        }
    }

    #[test]
    fn the_smoothing_step_sets_how_often_the_glides_move() {
        let fires = |sr: f32, every: f32| {
            let mut k = HpAutoPan::new(sr);
            k.set_param(STEP, every);
            k.reset();
            (0..1_000).filter(|_| k.fires()).count()
        };
        assert_eq!((fires(44_100.0, 45.0), fires(44_100.0, 10.0), fires(44_100.0, 1.0)), (22, 100, 1_000));
        assert_eq!(fires(88_200.0, 10.0), 50);
    }
}
