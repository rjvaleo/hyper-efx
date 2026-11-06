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
//!   right (23.1 and 22.2 ms). Each line takes its own fixed tap back in at
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
//!
//! Added (not in the original):
//!
//! - **Left tap** and **Right tap**, 1–50 ms, defaults 1020 and 980 frames at
//!   44.1 kHz (23.13 and 22.22 ms): the two fixed taps, in whole frames,
//!   scaled with the rate as the original's counts are. At the defaults it
//!   is the original exactly.
//! - **Stereo phase**, 0–180°, default 90°: how far the right LFO starts
//!   ahead of the left, the original's 256 table steps at 90°. A move
//!   re-phases the right LFO from the left at once. At 90° it is the
//!   original exactly.
//! - **Smoothing step**, 1–2048 samples at 44.1 kHz (scaled with the rate),
//!   default 45: how often the smoothing steps. At 45 it is the original
//!   exactly.

use super::hp_mod_common::{advance, degrees, increment, lookup, one_m_cos_table, samples, Line, Stepper, HP_RATE, LFO};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const RATE: usize = 0;
const DEPTH: usize = 1;
const SMOOTHING: usize = 2;
const INPUT: usize = 3;
const LEFT_TAP: usize = 4;
const RIGHT_TAP: usize = 5;
const PHASE: usize = 6;
const STEP: usize = 7;

const WET: f32 = 0.5;
const DRY: f32 = 0.5;
/// P4: feedback of the modulated tap. ChorusWide leaves it at 0.
const FEEDBACK: f32 = 0.0;
/// P20 and P21: each fixed tap back into its own line.
const TAP_FEEDBACK: f32 = 0.25;
/// The fixed taps, 1020 and 980 frames at 44.1 kHz, in ms.
const LEFT_TAP_MS: f32 = 1020.0 / 44.1;
const RIGHT_TAP_MS: f32 = 980.0 / 44.1;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("rateHz", "Frequency", 0.0, 4.0, 2.0).unit("Hz").smooth(100.0),
    ParamSpec::new("depth", "Depth", 0.0, 100.0, 0.0).unit("samples").smooth(100.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 50.0).unit("%").kind(ParamKind::Int).smooth(0.0).glides("rateHz"),
    ParamSpec::new("input", "Input", 0.0, 1.0, 0.0).kind(ParamKind::Choice(&["Stereo", "Mono"])).smooth(0.0),
    ParamSpec::new("leftTapMs", "Left tap", 1.0, 50.0, LEFT_TAP_MS).unit("ms").smooth(0.0),
    ParamSpec::new("rightTapMs", "Right tap", 1.0, 50.0, RIGHT_TAP_MS).unit("ms").smooth(0.0),
    ParamSpec::new("stereoPhaseDeg", "Stereo phase", 0.0, 180.0, 90.0).unit("°").smooth(0.0),
    ParamSpec::new("smoothingStep", "Smoothing step", 1.0, 2048.0, 45.0)
        .unit("samples")
        .kind(ParamKind::Int)
        .smooth(0.0)
        .glides("rateHz"),
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
            Group {
                label: "Added (not in the original)",
                keys: &["leftTapMs", "rightTapMs", "stereoPhaseDeg", "smoothingStep"],
            },
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
    values: [f32; 8],
    line: Line,
    table: [f32; LFO],
    steps: Stepper,
    rate: f32,
    depth: f32,
    phase: [f32; 2],
    last_wet: [f32; 2],
    left_tap: usize,
    right_tap: usize,
    /// The right LFO's lead, in table steps.
    offset: f32,
}

impl HpChorus {
    fn targets(&self) -> (f32, f32) {
        let depth = samples(self.values[DEPTH] as f64, self.sample_rate) as f32;
        (increment(self.values[RATE], self.sample_rate), depth)
    }

    /// A tap in ms as whole frames at this rate: the original's count at 44.1 kHz.
    fn tap(&self, ms: f32) -> usize {
        let frames = ms as f64 * (HP_RATE / 1000.0);
        (samples(frames, self.sample_rate).round() as usize).clamp(1, self.line.len() - 1)
    }

    fn apply(&mut self) {
        self.steps.set_percent(self.values[SMOOTHING]);
        self.steps.set_every(self.sample_rate, self.values[STEP]);
        self.left_tap = self.tap(self.values[LEFT_TAP]);
        self.right_tap = self.tap(self.values[RIGHT_TAP]);
        let offset = degrees(self.values[PHASE]);
        if offset != self.offset {
            self.offset = offset;
            self.phase[1] = advance(self.phase[0], offset);
        }
    }

    fn wet(&self, side: usize) -> f32 {
        let lfo = lookup(&self.table, self.phase[side]);
        self.line.read(side, lfo as f64 * self.depth as f64)
    }
}

impl Kernel for HpChorus {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let mut values = [0.0; 8];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = HpChorus {
            sample_rate,
            values,
            left_tap: 1,
            right_tap: 1,
            offset: 0.0,
            line: Line::new(sample_rate),
            table: one_m_cos_table(),
            steps: Stepper::new(sample_rate, 44.0, 44.0, values[SMOOTHING]),
            rate: 0.0,
            depth: 0.0,
            phase: [0.0; 2],
            last_wet: [0.0; 2],
        };
        k.apply();
        k.reset();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        self.values[index] = spec.clamp(value);
        if index != RATE && index != DEPTH && index != INPUT {
            self.apply();
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        self.steps.reset();
        self.line.clear();
        // P16 and P17: the right LFO a quarter of the table ahead, or as far
        // as the stereo phase says.
        self.phase = [0.0, self.offset];
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
            self.phase[0] = advance(self.phase[0], self.rate);
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
                self.phase[c] = advance(self.phase[c], self.rate);
            }
        }
        self.line.step();
    }

    fn tail_seconds(&self) -> f32 {
        // The fixed taps recirculate at 0.25 every 23 ms.
        0.25
    }
}

#[cfg(test)]
mod tests {
    use super::super::hp_mod_common::check;
    use super::*;

    /// The renders as they were before any control was added, at both rates.
    #[test]
    fn defaults_render_as_the_original() {
        let pad = [(RATE, 0.7), (DEPTH, 80.0)];
        let moved = [(RATE, 3.0), (DEPTH, 100.0)];
        let got = [
            check::checksums::<HpChorus>(44_100.0, 3.0, &pad, &moved),
            check::checksums::<HpChorus>(48_000.0, 3.0, &pad, &moved),
            check::checksums::<HpChorus>(44_100.0, 3.0, &[(INPUT, 1.0), (DEPTH, 50.0)], &[]),
        ];
        assert_eq!(
            got,
            [[5274422542064589373, 12924673055759259285, 6665645135068432288],
             [16030692852700301375, 409714257744167990, 11418620731014146314],
             [5274422542064589373, 4739825747537058233, 5274422542064589373]]
        );
    }

    fn chorus(settings: &[(usize, f32)]) -> HpChorus {
        let mut k = HpChorus::new(44_100.0);
        for &(i, v) in settings {
            k.set_param(i, v);
        }
        k.reset();
        k
    }

    /// Where a click on the left first reaches the right output.
    fn crossing(settings: &[(usize, f32)]) -> usize {
        let mut x = vec![0.0f32; 4_000];
        x[0] = 1.0;
        let (_, r) = check::render(&mut chorus(settings), &(x, vec![0.0; 4_000]), &[]);
        r.iter().position(|&v| v != 0.0).unwrap()
    }

    #[test]
    fn the_tap_times_move_the_cross_taps() {
        assert_eq!(crossing(&[]), 1020);
        assert_eq!(crossing(&[(LEFT_TAP, 10.0)]), 441);
        assert_eq!(crossing(&[(LEFT_TAP, 50.0)]), 2205);
        let k = chorus(&[(RIGHT_TAP, 1.0)]);
        assert_eq!((k.left_tap, k.right_tap), (1020, 44));
        let k = HpChorus::new(88_200.0);
        assert_eq!((k.left_tap, k.right_tap), (2040, 1960), "the same times at twice the rate");
    }

    #[test]
    fn the_stereo_phase_sets_how_far_the_sides_sweep_apart() {
        let (l, _) = check::input(2.0, 44_100.0);
        let same = (l.clone(), l);
        let taps = [(LEFT_TAP, 20.0), (RIGHT_TAP, 20.0), (RATE, 2.0), (DEPTH, 100.0)];
        let gap = |deg: f32| {
            let (yl, yr) = check::render(&mut chorus(&[&taps[..], &[(PHASE, deg)]].concat()), &same, &[]);
            check::power(&yl.iter().zip(&yr).map(|(a, b)| a - b).collect::<Vec<_>>())
        };
        assert_eq!(gap(0.0), 0.0, "in phase, the same input gives the same sides");
        let (quarter, half) = (gap(90.0), gap(180.0));
        assert!(quarter > 0.0 && half > quarter, "{quarter} {half}");
        assert_eq!(chorus(&[]).phase, [0.0, 256.0]);
        assert_eq!(chorus(&[(PHASE, 180.0)]).phase, [0.0, 512.0]);
    }

    #[test]
    fn the_smoothing_step_sets_how_often_the_glides_move() {
        let fires = |every: f32| {
            let mut k = chorus(&[(STEP, every)]);
            (0..1_000).filter(|_| k.steps.due()).count()
        };
        assert_eq!((fires(45.0), fires(10.0), fires(1.0)), (22, 100, 1_000));
    }
}
