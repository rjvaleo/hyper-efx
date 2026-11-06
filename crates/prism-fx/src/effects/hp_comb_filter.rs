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
//!
//! Added (not in the original):
//! - Feedback, 0–95 %, default 0: the routine's own feedback term, which the
//!   line takes as `in + feedback·y`, so the comb recirculates and its peaks
//!   ring and sharpen. Default 0 is the original exactly. Where it sits is
//!   the code's: `CombFilter`'s stereo path writes `fb·y + in` to the line
//!   (`fb` its parameter 1, `y` the interpolated read before depth); its
//!   mono path writes the input alone, so there the term never acted at all.
//! - Smoothing step, 1–2048 samples, default 2: how often the smoothers
//!   update, on every second sample in the original. Like the original's
//!   count it is not scaled with the rate. Default 2 is the original exactly.

use super::hp_delay_law::{samples, smoothing, Line};
use super::hp_echo::fraction;
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec};

const DELAY: usize = 0;
const DEPTH: usize = 1;
const SMOOTHING: usize = 2;
const FEEDBACK: usize = 3;
const STEP: usize = 4;

const LONGEST_MS: f32 = 100.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("delayMs", "Delay", 0.0, LONGEST_MS, 50.0).unit("ms").smooth(0.0),
    ParamSpec::new("depth", "Depth", 0.0, 100.0, 0.0).unit("%").smooth(0.0),
    ParamSpec::new("smoothing", "Smoothing", 0.0, 100.0, 50.0)
        .unit("%")
        .kind(ParamKind::Int)
        .smooth(0.0)
        .glides("depth"),
    ParamSpec::new("feedback", "Feedback", 0.0, 95.0, 0.0).unit("%").smooth(0.0),
    ParamSpec::new("smoothingStep", "Smoothing step", 1.0, 2048.0, 2.0)
        .unit("samples")
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
            Group { label: "Additional settings", keys: &["feedback", "smoothing", "smoothingStep"] },
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
    values: [f32; 5],
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
    /// The countdown's reload: the smoothing step less one.
    reload: i32,
    feedback: f32,
    a: f32,
    b: f32,
    started: bool,
}

impl HpCombFilter {
    fn apply(&mut self) {
        self.delay = samples(self.values[DELAY], self.sample_rate);
        self.depth = fraction(self.values[DEPTH]);
        (self.a, self.b) = smoothing(self.values[SMOOTHING], self.sample_rate);
        self.feedback = fraction(self.values[FEEDBACK]);
        self.reload = self.values[STEP] as i32 - 1;
        self.countdown = self.countdown.min(self.reload);
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
        let mut values = [0.0; 5];
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
            reload: 1,
            feedback: 0.0,
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
        self.countdown = self.reload;
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
            self.countdown = self.reload;
        }
        let p = self.line.at(self.age);
        let older = self.line.at(self.age + 1);
        let mut written = *frame;
        for (ch, s) in frame.iter_mut().enumerate() {
            let y = self.fraction.mul_add(older[ch] - p[ch], p[ch]);
            if self.feedback > 0.0 {
                written[ch] = self.feedback.mul_add(y, *s);
            }
            *s = 0.5 * y.mul_add(self.gain, *s);
        }
        // At zero feedback the line takes the input, as the original's does.
        self.line.push(written);
    }

    /// Until the recirculation is 60 dB down.
    fn tail_seconds(&self) -> f32 {
        let longest = self.time.max(self.delay) / self.sample_rate;
        if self.feedback <= 0.0 {
            return longest;
        }
        (longest * (0.001f32.ln() / self.feedback.ln()).ceil()).min(60.0)
    }
}

#[cfg(test)]
mod tests {
    use super::super::hp_delay_law::golden::hash_of;
    use super::*;

    fn hashes() -> [u64; 4] {
        let h = hash_of::<HpCombFilter>;
        [
            h(44_100.0, 3.0, &[], &[]),
            h(44_100.0, 3.0, &[(DEPTH, 100.0), (DELAY, 3.0)], &[(66_150, DELAY, 20.0), (66_150, DEPTH, 40.0)]),
            h(44_100.0, 3.0, &[(DEPTH, 70.0), (SMOOTHING, 80.0)], &[(50_000, DELAY, 7.5)]),
            h(48_000.0, 3.0, &[(DEPTH, 100.0), (DELAY, 1.0)], &[(72_000, DELAY, 9.0)]),
        ]
    }

    fn run(k: &mut HpCombFilter, x: &[f32]) -> Vec<f32> {
        x.iter()
            .map(|&v| {
                let mut f = [v, v];
                k.tick(&mut f);
                f[0]
            })
            .collect()
    }

    fn comb(settings: &[(usize, f32)]) -> HpCombFilter {
        let mut k = HpCombFilter::new(44_100.0);
        for &(i, v) in settings {
            k.set_param(i, v);
        }
        k
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    fn tone(hz: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| 0.1 * (core::f32::consts::TAU * hz * i as f32 / 44_100.0).sin()).collect()
    }

    #[test]
    fn feedback_rings_and_deepens_the_peaks() {
        // 1 ms: a peak at 1 kHz. Without feedback it is the input's level; at
        // 90 % the loop lifts it to 0.5·(1 + 1/(1 − 0.9)) = 5.5 times.
        let x = tone(1_000.0, 20_000);
        let level = rms(&x[10_000..]);
        let plain = rms(&run(&mut comb(&[(DELAY, 1.0), (DEPTH, 100.0)]), &x)[10_000..]) / level;
        let fed = rms(&run(&mut comb(&[(DELAY, 1.0), (DEPTH, 100.0), (FEEDBACK, 90.0)]), &x)[10_000..]) / level;
        assert!((plain - 1.0).abs() < 0.02, "{plain}");
        assert!((fed - 5.5).abs() < 0.3, "{fed}");
        // And a click comes back again and again.
        let mut click = vec![0.0; 2_000];
        click[0] = 1.0;
        let y = run(&mut comb(&[(DELAY, 1.0), (DEPTH, 100.0), (FEEDBACK, 50.0)]), &click);
        assert!(y.iter().filter(|v| v.abs() > 1e-6).count() > 10);
        let k = comb(&[(DELAY, 10.0), (FEEDBACK, 50.0)]);
        assert!(k.tail_seconds() > 0.09, "{}", k.tail_seconds());
    }

    #[test]
    fn the_smoothing_step_sets_how_often_the_glide_moves() {
        // Depth 0 → 100 at 50 % smoothing (a = 0.01 per update) on a steady
        // input: the output is 0.5·(1 + gain), gain = 1 − 0.99^updates.
        let x = vec![1.0; 4_000];
        let at = |step: f32| {
            let mut k = comb(&[(DELAY, 1.0), (STEP, step)]);
            for _ in 0..100 {
                k.tick(&mut [1.0, 1.0]);
            }
            k.set_param(DEPTH, 100.0);
            run(&mut k, &x)[2_999] * 2.0 - 1.0
        };
        let expect = |updates: i32| 1.0 - 0.99f32.powi(updates);
        assert!((at(2.0) - expect(1_500)).abs() < 1e-3, "{}", at(2.0));
        assert!((at(64.0) - expect(3_000 / 64)).abs() < 0.02, "{}", at(64.0));
        assert!((at(1.0) - expect(3_000)).abs() < 1e-3, "{}", at(1.0));
        assert!(at(64.0) < 0.5 * at(2.0));
    }

    #[test]
    fn defaults_render_as_the_original() {
        assert_eq!(hashes(), [7070672829978493104, 9673374163221694478, 8886443455048002011, 2015076870949773658]);
    }
}
