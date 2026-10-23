//! Spectral Dynamics: SoundHack's gate, ducker, expander and compressor, on
//! every band of a short-time spectrum separately.
//!
//! The algorithm is SoundHack 0.896's, as its code does it (docs/effects/
//! soundhack-app.md in MultiGrainLive, *The algorithms, from the code*):
//!
//! - **Analysis.** A Hamming window of `2 × bands`, a quarter-window hop,
//!   amplitudes scaled so a full-scale sine reads 1 in its band. Only
//!   amplitudes change; the overlap-add gives the window back as the delay.
//! - **State.** Each band keeps a state starting at 1: a gain for Gate-Duck,
//!   an exponent for Expand (the ratio N) and Compress (1/N). That target is
//!   the *amount*.
//! - **Thresholds.** One level for every band; or each band's level from a
//!   threshold sound, times the threshold as an offset; or, relative to peak,
//!   the threshold times the frame's loudest band, which overrides both.
//! - **Hard.** A band past its threshold (quieter, for Below; louder, for
//!   Above) steps its state toward the amount, and one not past steps back
//!   toward 1, by `(amount − 1) / frames` a frame, where *frames* is the
//!   attack/decay time in hops (at least one). The steps stop at the amount
//!   and at 1.
//! - **Smoothed.** The last four frames' amplitudes are averaged, and the
//!   state is a soft knee of `avg / (threshold + avg)` (Below) or
//!   `threshold / (avg + threshold)` (Above), with no attack or decay.
//! - **Third-octave grouping** sets every band in a third of an octave —
//!   counted down from Nyquist — to the group's mean state; the lowest leftover
//!   group keeps its own.
//! - **Applying,** within the band range: Gate-Duck multiplies the amplitude
//!   by the state; Expand and Compress set it to `T × (amp/T)^state`.
//! - **The threshold sound.** The first five window-length blocks of it are
//!   analysed and averaged band by band: that spectrum is the per-band
//!   threshold, each side its own.
//!
//! The one change from the code: the Hamming window here is the periodic form,
//! so a ratio of one gives the sound back exactly; SoundHack's symmetric form
//! ripples by a fraction of a decibel.

use crate::fft::Fft;
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec, SoundInput};

const MODE: usize = 0;
const BANDS: usize = 1;
const LOW: usize = 2;
const HIGH: usize = 3;
const THIRD_OCTAVE: usize = 4;
const RATIO: usize = 5;
const REDUCTION: usize = 6;
const SMOOTHED: usize = 7;
const TIME: usize = 8;
const AFFECT: usize = 9;
const RELATIVE: usize = 10;
const THRESHOLD: usize = 11;
const USE_SOUND: usize = 12;

const GATE: usize = 0;
const EXPAND: usize = 1;

/// SoundHack's band counts, smallest first.
const BAND_CHOICES: &[&str] = &["8", "16", "32", "64", "128", "256", "512", "1024", "2048", "4096"];
const MAX_N: usize = 8192;
const MAX_BINS: usize = MAX_N / 2 + 1;
/// Frames averaged for a smoothed amplitude.
const HISTORY: usize = 4;
/// Window-length blocks of a threshold sound that are analysed.
const PRINT_BLOCKS: usize = 5;
/// At these, the band range reaches DC and Nyquist.
const LOWEST_HZ: f32 = 20.0;
const HIGHEST_HZ: f32 = 20_000.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("mode", "Type", 0.0, 2.0, 1.0).kind(ParamKind::Choice(&["Gate-Duck", "Expand", "Compress"])).smooth(0.0),
    ParamSpec::new("bands", "Bands", 0.0, 9.0, 6.0).kind(ParamKind::Choice(BAND_CHOICES)).smooth(0.0),
    ParamSpec::new("lowHz", "Lowest band", LOWEST_HZ, HIGHEST_HZ, LOWEST_HZ).log().unit("Hz").smooth(0.0),
    ParamSpec::new("highHz", "Highest band", LOWEST_HZ, HIGHEST_HZ, HIGHEST_HZ).log().unit("Hz").smooth(0.0),
    ParamSpec::new("thirdOctave", "3rd octave grouping", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("ratio", "Ratio", 1.0, 20.0, 2.0).unit(":1").smooth(0.0),
    ParamSpec::new("reductionDb", "Gain/Reduction", -96.0, 24.0, -60.0).unit("dB").smooth(0.0).when("mode", 0.0),
    ParamSpec::new("smoothed", "Smoothed amplitude", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("timeMs", "Attack/decay", 0.0, 2_000.0, 0.0).unit("ms").smooth(0.0),
    ParamSpec::new("affect", "Affect", 0.0, 1.0, 0.0).kind(ParamKind::Choice(&["Below", "Above"])).smooth(0.0),
    ParamSpec::new("relativeToPeak", "Relative to peak", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
    // With a threshold sound in use, the level above that sound's spectrum:
    // SoundHack's "Threshold Above File (dB)".
    ParamSpec::new("thresholdDb", "Threshold", -120.0, 24.0, -60.0).unit("dB").smooth(0.0),
    ParamSpec::new("useSound", "Use threshold sound", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "spectral-dynamics",
    code: 6,
    version: 1,
    label: "Spectral Dynamics",
    category: Category::Spectral,
    params: PARAMS,
    panel: Panel {
        // SoundHack's dialog, top to bottom.
        groups: &[
            Group { label: "Process", keys: &["mode", "bands"] },
            Group { label: "Bands", keys: &["lowHz", "highHz", "thirdOctave"] },
            Group { label: "Amount", keys: &["ratio", "reductionDb", "smoothed", "timeMs"] },
            Group { label: "Threshold", keys: &["affect", "relativeToPeak", "thresholdDb", "useSound"] },
        ],
        primary: &["mode", "thresholdDb", "ratio"],
        displays: &[Display::Spectrum],
        pad: Some(PadPair { x: "thresholdDb", x_range: (-90.0, 0.0), y: "ratio", y_range: (1.0, 10.0) }),
    },
    source: "SoundHack: Spectral Dynamics",
    sound: Some(SoundInput { key: "thresholds", label: "Threshold sound" }),
};

struct Channel {
    input: Vec<f32>,
    output: Vec<f32>,
    state: Vec<f32>,
    history: Vec<f32>,
    /// The threshold sound's averaged spectrum, at this band count.
    print: Vec<f32>,
    /// The threshold sound, at this rate.
    sound: Vec<f32>,
}

impl Channel {
    fn new() -> Self {
        Channel {
            input: vec![0.0; MAX_N],
            output: vec![0.0; MAX_N],
            state: vec![1.0; MAX_BINS],
            history: vec![0.0; HISTORY * MAX_BINS],
            print: vec![0.0; MAX_BINS],
            sound: Vec::new(),
        }
    }

    fn clear(&mut self) {
        self.input.fill(0.0);
        self.output.fill(0.0);
        self.state.fill(1.0);
        self.history.fill(0.0);
    }
}

pub struct SpectralDynamics {
    sample_rate: f32,
    values: [f32; 13],
    fft: Fft,
    window: Vec<f32>,
    /// What the window's sum makes a band's amplitude: 2 / Σw.
    amp_scale: f32,
    /// What the overlap-add is divided by to give back unity.
    ola: f32,
    n: usize,
    pos: usize,
    since: usize,
    frame: usize,
    channels: [Channel; 2],
    has_sound: bool,
    re: Vec<f32>,
    im: Vec<f32>,
    amp: Vec<f32>,
    threshold: Vec<f32>,
}

fn n_for(choice: f32) -> usize {
    2 * (8usize << (choice.round().clamp(0.0, 9.0) as usize))
}

impl SpectralDynamics {
    fn start(&mut self, n: usize) {
        self.n = n;
        for (i, w) in self.window[..n].iter_mut().enumerate() {
            *w = 0.54 - 0.46 * (core::f32::consts::TAU * i as f32 / n as f32).cos();
        }
        let sum: f32 = self.window[..n].iter().sum();
        self.amp_scale = 2.0 / sum;
        self.ola = self.window[..n].iter().map(|w| w * w).sum::<f32>() / (n / 4) as f32;
        for c in &mut self.channels {
            c.clear();
        }
        self.pos = 0;
        self.since = 0;
        self.frame = 0;
        self.analyse_print();
    }

    /// The threshold sound's spectrum: the first five window-length blocks,
    /// windowed, transformed and averaged band by band, as SoundHack reads its
    /// threshold file.
    fn analyse_print(&mut self) {
        let n = self.n;
        let bins = n / 2 + 1;
        for side in 0..2 {
            self.channels[side].print[..bins].fill(0.0);
            if !self.has_sound {
                continue;
            }
            for block in 0..PRINT_BLOCKS {
                for i in 0..n {
                    let x = self.channels[side].sound.get(block * n + i).copied().unwrap_or(0.0);
                    self.re[i] = x * self.window[i];
                    self.im[i] = 0.0;
                }
                self.fft.forward(&mut self.re[..n], &mut self.im[..n]);
                let c = &mut self.channels[side];
                for k in 0..bins {
                    c.print[k] += (self.re[k] * self.re[k] + self.im[k] * self.im[k]).sqrt() * self.amp_scale;
                }
            }
            for v in &mut self.channels[side].print[..bins] {
                *v /= PRINT_BLOCKS as f32;
            }
        }
    }

    /// The amount a past band's state goes to.
    fn amount(&self) -> f32 {
        match self.values[MODE] as usize {
            GATE => 10f32.powf(self.values[REDUCTION] / 20.0),
            EXPAND => self.values[RATIO],
            _ => 1.0 / self.values[RATIO],
        }
    }

    fn frame(&mut self, side: usize) {
        let n = self.n;
        let bins = n / 2 + 1;
        let start = self.pos;
        {
            let c = &self.channels[side];
            for i in 0..n {
                self.re[i] = c.input[(start + i) % n] * self.window[i];
                self.im[i] = 0.0;
            }
        }
        self.fft.forward(&mut self.re[..n], &mut self.im[..n]);
        let mut peak = 0.0f32;
        for k in 0..bins {
            self.amp[k] = (self.re[k] * self.re[k] + self.im[k] * self.im[k]).sqrt() * self.amp_scale;
            peak = peak.max(self.amp[k]);
        }

        let mode = self.values[MODE] as usize;
        let below = self.values[AFFECT] < 0.5;
        let smoothed = self.values[SMOOTHED] >= 0.5;
        let level = 10f32.powf(self.values[THRESHOLD] / 20.0);
        let relative = self.values[RELATIVE] >= 0.5;
        let use_sound = self.values[USE_SOUND] >= 0.5 && self.has_sound;
        let amount = self.amount();
        let frames = (self.values[TIME] * 0.001 * self.sample_rate / (n / 4) as f32).max(1.0);
        let step = (amount - 1.0) / frames;
        let per_bin = self.sample_rate / n as f32;
        let low = if self.values[LOW] <= LOWEST_HZ { 0 } else { ((self.values[LOW] / per_bin).ceil() as usize).min(bins) };
        let high = if self.values[HIGH] >= HIGHEST_HZ { bins - 1 } else { (self.values[HIGH] / per_bin).floor() as usize };
        let high = high.min(bins - 1);
        let slot = self.frame % HISTORY;

        let c = &mut self.channels[side];
        for k in low..=high {
            let t = if relative {
                level * peak
            } else if use_sound {
                level * c.print[k]
            } else {
                level
            };
            self.threshold[k] = t;
            if smoothed {
                c.history[slot * MAX_BINS + k] = self.amp[k];
                let avg = 0.25 * (0..HISTORY).map(|h| c.history[h * MAX_BINS + k]).sum::<f32>();
                let x = if avg + t <= 0.0 {
                    1.0
                } else if below {
                    avg / (t + avg)
                } else {
                    t / (avg + t)
                };
                c.state[k] = if mode != GATE && amount > 1.0 {
                    1.0 / (x * (1.0 - 1.0 / amount) + 1.0 / amount)
                } else {
                    amount + x * (1.0 - amount)
                };
                continue;
            }
            let past = if below { self.amp[k] < t } else { t < self.amp[k] };
            let s = &mut c.state[k];
            if past {
                *s += step;
                *s = if amount <= 1.0 { s.max(amount) } else { s.min(amount) };
            } else {
                *s -= step;
                *s = if amount <= 1.0 { s.min(1.0) } else { s.max(1.0) };
            }
        }

        if self.values[THIRD_OCTAVE] >= 0.5 {
            // Down from Nyquist, a third of an octave at a time; the bands of
            // each group take its mean. The lowest leftover keeps its own.
            let top_band = bins - 1;
            let mut exponent = -1.0f32 / 3.0;
            let mut bound = (top_band as f32 * exponent.exp2()) as usize;
            let mut top = top_band;
            let (mut sum, mut count) = (0.0f32, 0usize);
            for k in (0..=top_band).rev() {
                if bound < k {
                    count += 1;
                } else {
                    let mean = if count > 0 { sum / count as f32 } else { 0.0 };
                    for j in bound + 1..=top {
                        c.state[j] = mean;
                    }
                    exponent -= 1.0 / 3.0;
                    top = bound;
                    bound = (top_band as f32 * exponent.exp2()) as usize;
                    sum = 0.0;
                    count = 1;
                }
                sum += c.state[k];
            }
        }

        for k in low..=high {
            let a = self.amp[k];
            if a <= 0.0 {
                continue;
            }
            let wanted = if mode == GATE {
                a * c.state[k]
            } else {
                let t = self.threshold[k];
                if t <= 0.0 {
                    continue;
                }
                t * (a / t).powf(c.state[k])
            };
            let scale = wanted / a;
            if scale != 1.0 && scale.is_finite() {
                self.re[k] *= scale;
                self.im[k] *= scale;
                if k != 0 && k != n / 2 {
                    self.re[n - k] *= scale;
                    self.im[n - k] *= scale;
                }
            }
        }

        self.fft.inverse(&mut self.re[..n], &mut self.im[..n]);
        for i in 0..n {
            c.output[(start + i) % n] += self.re[i] * self.window[i] / self.ola;
        }
    }
}

impl Kernel for SpectralDynamics {
    fn new(sample_rate: f32) -> Self {
        let mut values = [0.0; 13];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = SpectralDynamics {
            sample_rate: sample_rate.max(1.0),
            values,
            fft: Fft::new(MAX_N),
            window: vec![0.0; MAX_N],
            amp_scale: 1.0,
            ola: 1.0,
            n: 0,
            pos: 0,
            since: 0,
            frame: 0,
            channels: [Channel::new(), Channel::new()],
            has_sound: false,
            re: vec![0.0; MAX_N],
            im: vec![0.0; MAX_N],
            amp: vec![0.0; MAX_BINS],
            threshold: vec![0.0; MAX_BINS],
        };
        k.start(n_for(values[BANDS]));
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(spec) = PARAMS.get(index) {
            self.values[index] = spec.clamp(value);
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    /// The threshold sound: kept at this rate, and analysed again whenever
    /// the band count changes.
    fn set_sound(&mut self, samples: &[f32], channels: usize, sample_rate: f32) {
        let channels = channels.max(1);
        let frames = samples.len() / channels;
        let step = sample_rate.max(1.0) / self.sample_rate;
        // Five blocks of the longest window is all that is ever read.
        let out = ((frames as f32 / step) as usize).min(PRINT_BLOCKS * MAX_N);
        for side in 0..2 {
            let ch = side.min(channels - 1);
            let at = |f: usize| samples.get(f * channels + ch).copied().unwrap_or(0.0);
            self.channels[side].sound = (0..out)
                .map(|i| {
                    let p = i as f32 * step;
                    let f = p.floor() as usize;
                    at(f) + (at(f + 1) - at(f)) * (p - f as f32)
                })
                .collect();
        }
        self.has_sound = out > 0;
        self.analyse_print();
    }

    fn reset(&mut self) {
        self.start(n_for(self.values[BANDS]));
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        let wanted = n_for(self.values[BANDS]);
        if wanted != self.n {
            // A new band count is a new window: start it clean.
            self.start(wanted);
        }
        let pos = self.pos;
        for (side, s) in frame.iter_mut().enumerate() {
            let c = &mut self.channels[side];
            c.input[pos] = *s;
            *s = c.output[pos];
            c.output[pos] = 0.0;
        }
        self.pos = (pos + 1) % self.n;
        self.since += 1;
        if self.since == self.n / 4 {
            self.since = 0;
            self.frame(0);
            self.frame(1);
            self.frame += 1;
        }
    }

    fn latency_frames(&self) -> u32 {
        n_for(self.values[BANDS]) as u32
    }

    /// One window: what is in the transform when the input stops.
    fn tail_seconds(&self) -> f32 {
        n_for(self.values[BANDS]) as f32 / self.sample_rate
    }
}
