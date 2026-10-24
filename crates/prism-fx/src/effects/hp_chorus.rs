//! Hyperprism Chorus: Hyperprism-PPC 1.2.1's Chorus process, which runs
//! EModulation's ChorusWide setup (process index 1) through its Chorus
//! routine, as its code does it.
//!
//! A line per side (32 768 frames at 44.1 kHz) with three reads:
//!
//! - **The modulated tap.** `d = depth·0.5·(1 − cos)` frames back, from a
//!   1024-entry raised-cosine table, linear between the frames `int(d)` and
//!   `int(d) + 1` back: a sweep from 0 (the newest sample) to the depth. The
//!   right LFO starts 256 table steps — a quarter cycle — ahead of the left.
//! - **Two fixed taps.** 1020 frames back on the left line and 980 on the
//!   right (22.2 and 23.1 ms). Each line takes its own fixed tap back in at
//!   0.25; each output takes the other side's at wet level.
//! - **Out.** `yL = 0.5·wetL + 0.5·xL + 0.5·R[n−980]`,
//!   `yR = 0.5·wetR + 0.5·xR + 0.5·L[n−1020]`. No feedback on the modulated
//!   tap (P4 = 0); wet and dry are fixed at 0.5.
//! - **Pad.** X 0–4 Hz (`v·1024/SR` table steps a sample), Y 0–100 samples
//!   at 44.1 kHz, both linear, starting at 2 Hz and 0.
//! - **Smoothing.** The additional setting, 0–100 %, default 50 % (the
//!   window's slider): `a = int(10^(0.04·(100 − s)))·0.0001`, a step of rate
//!   and depth every 45th sample.
//! - **Input.** The original ran a separate loop for a mono file: one line,
//!   its 1020-frame tap fed back at 0.25 and added to the output at wet
//!   level, the left LFO only. *Mono* runs that loop on the left input and
//!   sends the result to both sides.
//!
//! Changed from the original:
//!
//! - The 980-frame tap started on a right slot of the interleaved buffer but
//!   wrapped to the buffer's first slot, a left one, so after 980 frames it
//!   read the left line: the right line was fed from the left and the left
//!   output echoed itself. The tap was placed on a right slot on purpose; it
//!   now stays there.
//! - InitializeChorus set the two fixed-tap coefficients to 0.25 as a pair,
//!   then wrote 0 over the left one among the state it was clearing. The
//!   left line feeds its tap back at 0.25 like the right.
//! - The stereo loop stepped the right LFO with the rate as it stood at the
//!   start of each host block, the left with every smoothing step, so the
//!   quarter-cycle offset drifted while the rate glided. Both step together,
//!   as in the Modulation routine.
//! - The output's fixed-tap gain was a copy of the wet gain taken at
//!   initialisation; the wet gain never changes, so it is simply 0.5.

use super::hp_mod_common::{increment, lookup, one_m_cos_table, samples, Line, Stepper, LFO};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const RATE: usize = 0;
const DEPTH: usize = 1;
const SMOOTHING: usize = 2;
const INPUT: usize = 3;

const WET: f32 = 0.5;
const DRY: f32 = 0.5;
/// P4: feedback of the modulated tap. ChorusWide leaves it at 0.
const FEEDBACK: f32 = 0.0;
/// P20 and P21: each fixed tap back into its own line.
const TAP_FEEDBACK: f32 = 0.25;
/// The fixed taps, in frames at 44.1 kHz.
const LEFT_TAP: f64 = 1020.0;
const RIGHT_TAP: f64 = 980.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("rateHz", "Frequency", 0.0, 4.0, 2.0).unit("Hz").smooth(100.0),
    ParamSpec::new("depth", "Depth", 0.0, 100.0, 0.0).unit("samples").smooth(100.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 50.0).unit("%").kind(ParamKind::Int).smooth(0.0).glides("rateHz"),
    ParamSpec::new("input", "Input", 0.0, 1.0, 0.0).kind(ParamKind::Choice(&["Stereo", "Mono"])).smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-chorus",
    code: 48,
    version: 1,
    label: "Hyperprism Chorus",
    category: Category::Modulation,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Pad", keys: &["rateHz", "depth"] },
            Group { label: "Additional settings", keys: &["smoothing", "input"] },
        ],
        primary: &["rateHz", "depth"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "rateHz", x_range: (0.0, 4.0), y: "depth", y_range: (0.0, 100.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Chorus",
    sound: None,
};

pub struct HpChorus {
    sample_rate: f32,
    values: [f32; 4],
    line: Line,
    table: [f32; LFO],
    steps: Stepper,
    rate: f32,
    depth: f32,
    phase: [f32; 2],
    last_wet: [f32; 2],
    left_tap: usize,
    right_tap: usize,
}

impl HpChorus {
    fn targets(&self) -> (f32, f32) {
        let depth = samples(self.values[DEPTH] as f64, self.sample_rate) as f32;
        (increment(self.values[RATE], self.sample_rate), depth)
    }

    fn wet(&self, side: usize) -> f32 {
        let lfo = lookup(&self.table, self.phase[side]);
        self.line.read(side, lfo as f64 * self.depth as f64)
    }
}

impl Kernel for HpChorus {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 4];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let line = Line::new(sample_rate);
        let tap = |frames: f64| (samples(frames, sample_rate).round() as usize).clamp(1, line.len() - 1);
        let mut k = HpChorus {
            sample_rate,
            values,
            left_tap: tap(LEFT_TAP),
            right_tap: tap(RIGHT_TAP),
            line,
            table: one_m_cos_table(),
            steps: Stepper::new(sample_rate, 44.0, 44.0, values[SMOOTHING]),
            rate: 0.0,
            depth: 0.0,
            phase: [0.0; 2],
            last_wet: [0.0; 2],
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
        if index == SMOOTHING {
            self.steps.set_percent(self.values[SMOOTHING]);
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.steps.reset();
        self.line.clear();
        // P16 and P17: the right LFO a quarter of the table ahead.
        self.phase = [0.0, (LFO / 4) as f32];
        self.last_wet = [0.0; 2];
        (self.rate, self.depth) = self.targets();
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.steps.due() {
            let (rate, depth) = self.targets();
            self.rate = self.steps.step(self.rate, rate);
            self.depth = self.steps.step(self.depth, depth);
        }
        if self.values[INPUT] >= 0.5 {
            let x = frame[0];
            let tap = self.line.back(0, self.left_tap);
            self.line.write(0, x + FEEDBACK * self.last_wet[0] + TAP_FEEDBACK * tap);
            let wet = self.wet(0);
            let y = wet * WET + x * DRY + WET * tap;
            self.last_wet[0] = wet;
            self.phase[0] = super::hp_mod_common::advance(self.phase[0], self.rate);
            *frame = [y, y];
        } else {
            let x = *frame;
            let taps = [self.line.back(0, self.left_tap), self.line.back(1, self.right_tap)];
            for c in 0..2 {
                self.line.write(c, x[c] + FEEDBACK * self.last_wet[c] + TAP_FEEDBACK * taps[c]);
            }
            for c in 0..2 {
                let wet = self.wet(c);
                // Each output takes the other side's fixed tap.
                frame[c] = wet * WET + x[c] * DRY + WET * taps[1 - c];
                self.last_wet[c] = wet;
                self.phase[c] = super::hp_mod_common::advance(self.phase[c], self.rate);
            }
        }
        self.line.step();
    }

    fn tail_seconds(&self) -> f32 {
        // The fixed taps recirculate at 0.25 every 23 ms.
        0.25
    }
}
