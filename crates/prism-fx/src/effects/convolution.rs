//! Convolution: SoundHack's "Convolve with Impulse Response", in real time.
//!
//! The input's spectrum is multiplied by an impulse's, so what the two share is
//! reinforced and the rest falls away: a complex, tuned reverb when the impulse
//! is a room, cross-synthesis when it is anything else. The impulse is a sound
//! handed over with `set_sound`; its first `lengthMs` are used, SoundHack's
//! *Length Used*.
//!
//! - **The engine** is a uniformly partitioned convolver: the impulse in
//!   512-sample partitions, each transformed once, the input transformed a
//!   block at a time and multiplied against all of them. One partition is the
//!   delay, reported through `latency_frames`; the dry sound is delayed with
//!   it so the mix stays in step.
//! - **Scaling.** SoundHack scales the result "automatically" and warns the
//!   gain cannot be predicted, then offers 0, 24 or 42 dB on top. Here the
//!   impulse is scaled to unit energy, so noise in keeps its level out, and
//!   `gainDb` is the dial those three buttons were points on.
//! - **Brighten** is SoundHack's: each bin of the windowed impulse's spectrum
//!   times its frequency, DC to nothing — +6 dB an octave all the way to
//!   Nyquist — so two dull sounds do not convolve to something duller.
//! - **Window** shapes the impulse first, from SoundHack's list and drawn as
//!   its code draws them (Kaiser at β 6.8 with both ends zero); Rectangle is
//!   none at all.
//! - **Ring modulate** multiplies the input by the impulse in time instead,
//!   scaled to a peak of one and started again every length used: SoundHack's
//!   "ring modulation (or convolution in frequency)".
//! - **Moving** — the impulse window walking through the impulse file to end
//!   with the input — needs the whole input's length, so it is a render, not
//!   this.
//!
//! With no impulse yet, it passes the sound through, delayed in step.

use crate::fft::Fft;
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec, Smoothed, SoundInput};

const LENGTH: usize = 0;
const GAIN: usize = 1;
const WINDOW: usize = 2;
const RING: usize = 3;
const BRIGHTEN: usize = 4;
const MIX: usize = 5;

/// One partition, and the delay.
const P: usize = 512;
const N: usize = 2 * P;
/// The longest impulse used.
const MAX_SECONDS: f32 = 4.0;

const WINDOWS: &[&str] = &["Hamming", "Kaiser", "Ramp", "Rectangle", "Sinc", "Triangle", "von Hann"];
const RECTANGLE: f32 = 3.0;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("lengthMs", "Length used", 10.0, MAX_SECONDS * 1000.0, 100.0).log().unit("ms").smooth(0.0),
    ParamSpec::new("gainDb", "Gain", -24.0, 48.0, 0.0).unit("dB"),
    ParamSpec::new("window", "Window", 0.0, 6.0, RECTANGLE).kind(ParamKind::Choice(WINDOWS)).smooth(0.0),
    ParamSpec::new("ringModulate", "Ring modulate", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("brighten", "Brighten", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0),
    ParamSpec::new("mix", "Mix", 0.0, 1.0, 1.0),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "convolution",
    code: 7,
    version: 1,
    label: "Convolution",
    category: Category::Spectral,
    params: PARAMS,
    panel: Panel {
        // SoundHack's dialog, top to bottom: length and gain, the window,
        // then ring modulate and brighten side by side.
        groups: &[
            Group { label: "Impulse", keys: &["lengthMs", "gainDb"] },
            Group { label: "Shape", keys: &["window", "brighten"] },
            Group { label: "Process", keys: &["ringModulate", "mix"] },
        ],
        primary: &["lengthMs", "gainDb", "mix"],
        displays: &[Display::Curve],
        pad: Some(PadPair { x: "lengthMs", x_range: (10.0, 1_000.0), y: "gainDb", y_range: (-24.0, 24.0) }),
    },
    source: "SoundHack: Convolution",
    sound: Some(SoundInput { key: "impulse", label: "Impulse" }),
};

/// The zeroth-order modified Bessel function, for the Kaiser window.
fn bessel_i0(x: f32) -> f32 {
    let (mut sum, mut term) = (1.0f32, 1.0f32);
    for k in 1..32 {
        term *= (x / (2.0 * k as f32)).powi(2);
        sum += term;
        if term < 1e-9 * sum {
            break;
        }
    }
    sum
}

/// SoundHack's windows over `len` samples, as its code draws them.
fn window_at(choice: usize, i: usize, len: usize) -> f32 {
    let n = len.max(1) as f32;
    let i_f = i as f32;
    // Hamming and von Hann are the symmetric forms.
    let x = if len > 1 { i_f / (n - 1.0) } else { 0.5 };
    let tau = core::f32::consts::TAU;
    match choice {
        0 => 0.54 - 0.46 * (tau * x).cos(),
        1 => {
            // β 6.8, and both end samples set to nothing.
            if i == 0 || i + 1 == len {
                return 0.0;
            }
            let beta = 6.8;
            let u = 2.0 * x - 1.0;
            bessel_i0(beta * (1.0 - u * u).max(0.0).sqrt()) / bessel_i0(beta)
        }
        2 => 1.0 - i_f / n,
        4 => {
            let half = n / 2.0;
            let u = core::f32::consts::PI * (i_f - half) / half;
            if u.abs() < 1e-6 { 1.0 } else { u.sin() / u }
        }
        5 => 2.0 * (i_f / n).min(1.0 - i_f / n),
        6 => 0.5 - 0.5 * (tau * x).cos(),
        _ => 1.0,
    }
}

struct Side {
    /// The impulse as handed over, at this rate.
    sound: Vec<f32>,
    /// The part used, shaped and scaled.
    segment: Vec<f32>,
    h_re: Vec<f32>,
    h_im: Vec<f32>,
    /// The input's spectra, newest at `head`.
    fdl_re: Vec<f32>,
    fdl_im: Vec<f32>,
    /// The last two blocks of input, oldest first.
    input: Vec<f32>,
    wet: Vec<f32>,
    /// The input, delayed with the wet path: one partition, and one more
    /// while a brightened impulse starts a partition early.
    dry: Vec<f32>,
}

impl Side {
    fn new(max_parts: usize) -> Self {
        Side {
            sound: Vec::new(),
            segment: Vec::new(),
            h_re: vec![0.0; max_parts * N],
            h_im: vec![0.0; max_parts * N],
            fdl_re: vec![0.0; max_parts * N],
            fdl_im: vec![0.0; max_parts * N],
            input: vec![0.0; N],
            wet: vec![0.0; P],
            dry: vec![0.0; 2 * P],
        }
    }

    fn clear(&mut self) {
        self.fdl_re.fill(0.0);
        self.fdl_im.fill(0.0);
        self.input.fill(0.0);
        self.wet.fill(0.0);
        self.dry.fill(0.0);
    }
}

pub struct Convolution {
    sample_rate: f32,
    values: [f32; 6],
    fft: Fft,
    sides: [Side; 2],
    max_parts: usize,
    parts: usize,
    length: usize,
    head: usize,
    fill: usize,
    /// Samples since the start, for the ring modulator's place in the impulse.
    clock: u64,
    has_sound: bool,
    dirty: bool,
    /// Where the dry delay line is written.
    dpos: usize,
    gain: Smoothed,
    mix: Smoothed,
    re: Vec<f32>,
    im: Vec<f32>,
    /// For Brighten: the whole used impulse transformed at once, as SoundHack
    /// does it, before the partitions are cut.
    whole: Fft,
    whole_re: Vec<f32>,
    whole_im: Vec<f32>,
}

impl Convolution {
    /// Cut, shape, scale and transform the impulse.
    fn prepare(&mut self) {
        self.dirty = false;
        let wanted = (self.values[LENGTH] * 0.001 * self.sample_rate).round() as usize;
        let ring = self.values[RING] >= 0.5;
        let brighten = self.values[BRIGHTEN] >= 0.5;
        let window = self.values[WINDOW] as usize;
        let longest = self.sides[0].sound.len();
        self.length = wanted.clamp(1, self.max_parts * P).min(longest.max(1));
        let length = self.length;
        // Brightened, the impulse is the whole transform block it was
        // brightened in, up to the longest kept.
        let used = if brighten && !ring {
            (2 * length).next_power_of_two().min(self.max_parts * P)
        } else {
            length
        };
        self.parts = used.div_ceil(P);
        for side in 0..2 {
            let s = &mut self.sides[side];
            s.segment.clear();
            for i in 0..length {
                let x = s.sound.get(i).copied().unwrap_or(0.0);
                s.segment.push(x * window_at(window, i, length));
            }
            if brighten {
                // SoundHack's +6 dB an octave: each bin times its frequency,
                // DC to nothing, on the windowed impulse's spectrum.
                let m = (2 * length).next_power_of_two().min(self.whole.max());
                let half = m / 2;
                for i in 0..m {
                    self.whole_re[i] = s.segment.get(i).copied().unwrap_or(0.0);
                    self.whole_im[i] = 0.0;
                }
                self.whole.forward(&mut self.whole_re[..m], &mut self.whole_im[..m]);
                for k in 0..=half {
                    let g = k as f32 / half as f32;
                    self.whole_re[k] *= g;
                    self.whole_im[k] *= g;
                    if k != 0 && k != half {
                        self.whole_re[m - k] *= g;
                        self.whole_im[m - k] *= g;
                    }
                }
                self.whole.inverse(&mut self.whole_re[..m], &mut self.whole_im[..m]);
                // The ramp is a zero-phase filter: it spreads the impulse a
                // little before time zero as well as after. SoundHack's
                // blocks wrap that early part round to the end, where it
                // spoils the low end — the manual's "probably too simple".
                // Here it is kept where it belongs, a partition early, and
                // the dry sound is delayed to match.
                let kept = m.min(self.max_parts * P);
                s.segment.clear();
                s.segment.extend((0..kept).map(|i| self.whole_re[(i + m - P) % m]));
            }
            let measure = if ring {
                s.segment.iter().fold(0.0f32, |m, v| m.max(v.abs()))
            } else {
                s.segment.iter().map(|v| v * v).sum::<f32>().sqrt()
            };
            let scale = if measure > 0.0 { 1.0 / measure } else { 0.0 };
            for v in &mut s.segment {
                *v *= scale;
            }
            for k in 0..self.parts {
                for i in 0..N {
                    self.re[i] = if i < P { s.segment.get(k * P + i).copied().unwrap_or(0.0) } else { 0.0 };
                    self.im[i] = 0.0;
                }
                self.fft.forward(&mut self.re, &mut self.im);
                s.h_re[k * N..(k + 1) * N].copy_from_slice(&self.re);
                s.h_im[k * N..(k + 1) * N].copy_from_slice(&self.im);
            }
        }
    }

    /// How much earlier than its own start the impulse begins: a partition,
    /// while brightened (see `prepare`).
    fn early(&self) -> usize {
        if self.has_sound && self.values[BRIGHTEN] >= 0.5 && self.values[RING] < 0.5 { P } else { 0 }
    }

    /// A block of input is in: transform it, and multiply against every
    /// partition of the impulse.
    fn block(&mut self) {
        self.head = (self.head + 1) % self.max_parts;
        for side in 0..2 {
            let s = &mut self.sides[side];
            self.re.copy_from_slice(&s.input);
            self.im.fill(0.0);
            self.fft.forward(&mut self.re, &mut self.im);
            let at = self.head * N;
            s.fdl_re[at..at + N].copy_from_slice(&self.re);
            s.fdl_im[at..at + N].copy_from_slice(&self.im);
            self.re.fill(0.0);
            self.im.fill(0.0);
            for k in 0..self.parts {
                let x = ((self.head + self.max_parts - k) % self.max_parts) * N;
                let h = k * N;
                for i in 0..N {
                    let (a, b) = (s.fdl_re[x + i], s.fdl_im[x + i]);
                    let (c, d) = (s.h_re[h + i], s.h_im[h + i]);
                    self.re[i] += a * c - b * d;
                    self.im[i] += a * d + b * c;
                }
            }
            self.fft.inverse(&mut self.re, &mut self.im);
            s.wet.copy_from_slice(&self.re[P..]);
            s.input.copy_within(P.., 0);
        }
    }
}

impl Kernel for Convolution {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let max_parts = ((MAX_SECONDS * sample_rate) as usize).div_ceil(P);
        let mut values = [0.0; 6];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = Convolution {
            sample_rate,
            values,
            fft: Fft::new(N),
            sides: [Side::new(max_parts), Side::new(max_parts)],
            max_parts,
            parts: 0,
            length: 0,
            head: 0,
            fill: 0,
            clock: 0,
            has_sound: false,
            dirty: false,
            dpos: 0,
            gain: Smoothed::new(sample_rate, PARAMS[GAIN].smooth_ms),
            mix: Smoothed::new(sample_rate, PARAMS[MIX].smooth_ms),
            re: vec![0.0; N],
            im: vec![0.0; N],
            whole: Fft::new(2 * max_parts * P),
            whole_re: vec![0.0; (2 * max_parts * P).next_power_of_two()],
            whole_im: vec![0.0; (2 * max_parts * P).next_power_of_two()],
        };
        k.gain.jump(values[GAIN]);
        k.mix.jump(values[MIX]);
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(spec) = PARAMS.get(index) else { return };
        let v = spec.clamp(value);
        let changed = self.values[index] != v;
        self.values[index] = v;
        match index {
            GAIN => self.gain.set(v),
            MIX => self.mix.set(v),
            _ => self.dirty |= changed && self.has_sound,
        }
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn set_sound(&mut self, samples: &[f32], channels: usize, sample_rate: f32) {
        let channels = channels.max(1);
        let frames = samples.len() / channels;
        let step = sample_rate.max(1.0) / self.sample_rate;
        let out = ((frames as f32 / step) as usize).min(self.max_parts * P);
        for side in 0..2 {
            let ch = side.min(channels - 1);
            let at = |f: usize| samples.get(f * channels + ch).copied().unwrap_or(0.0);
            let s = &mut self.sides[side];
            s.sound = (0..out)
                .map(|i| {
                    let p = i as f32 * step;
                    let f = p.floor() as usize;
                    let t = p - f as f32;
                    at(f) + (at(f + 1) - at(f)) * t
                })
                .collect();
            s.segment = Vec::with_capacity(self.max_parts * P);
        }
        self.has_sound = out > 0;
        self.dirty = self.has_sound;
    }

    fn reset(&mut self) {
        for s in &mut self.sides {
            s.clear();
        }
        self.head = 0;
        self.fill = 0;
        self.clock = 0;
        self.dpos = 0;
        self.gain.jump(self.values[GAIN]);
        self.mix.jump(self.values[MIX]);
        if self.has_sound {
            self.prepare();
        }
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.dirty {
            self.prepare();
        }
        let gain = 10f32.powf(self.gain.next() / 20.0);
        let mix = self.mix.next();
        let ring = self.values[RING] >= 0.5;
        let fill = self.fill;
        // Where the delayed input is in the impulse, for ring modulation.
        let place = self.clock.checked_sub(P as u64).map(|t| (t % self.length.max(1) as u64) as usize);
        let read = (self.dpos + 2 * P - P - self.early()) % (2 * P);
        for (side, s) in frame.iter_mut().enumerate() {
            let sd = &mut self.sides[side];
            let dry = sd.dry[read];
            let wet = if !self.has_sound {
                dry
            } else if ring {
                place.map_or(0.0, |p| dry * sd.segment[p]) * gain
            } else {
                sd.wet[fill] * gain
            };
            sd.dry[self.dpos] = *s;
            sd.input[P + fill] = *s;
            *s = if self.has_sound { dry + (wet - dry) * mix } else { dry };
        }
        self.clock += 1;
        self.dpos = (self.dpos + 1) % (2 * P);
        self.fill += 1;
        if self.fill == P {
            self.fill = 0;
            if self.has_sound && !ring {
                self.block();
            } else {
                for s in &mut self.sides {
                    s.input.copy_within(P.., 0);
                }
            }
        }
    }

    fn latency_frames(&self) -> u32 {
        (P + self.early()) as u32
    }

    fn tail_seconds(&self) -> f32 {
        (self.length + P) as f32 / self.sample_rate
    }
}
