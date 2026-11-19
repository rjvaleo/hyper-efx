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
//! - **Moving controls.** While playing, changes to Length, Window, Ring or
//!   Brighten are coalesced and built incrementally into a second preallocated
//!   partition bank on a bounded per-64-sample budget (never colliding with a
//!   partition-boundary `block()`), then swapped in over a 15 ms crossfade.
//!   Still renders, `set_sound`, `reset`, and the first prepare stay bit-identical.
//!
//! With no impulse yet, it passes the sound through, delayed in step.

use crate::fft::{Fft, FftStepper};
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

/// Crossfade duration when swapping from an old impulse bank to a newly built one.
const XFADE_MS: f32 = 15.0;

/// Incremental rebuild work budgets per quantum (one quantum every 64 frames,
/// skipped on the 8th 64-frame slice where `block()` runs, so 7 quanta per 512
/// samples). Sized so every 64-frame slot stays well under 25% even at 4 s +
/// Brighten.
const WORK_STRIDE: usize = 64;
const WINDOW_STEP: usize = 4_096;
const COPY_STEP: usize = 32_768;
const MEASURE_STEP: usize = 32_768;
const BRIGHTEN_FFT_STEP: usize = 32_768;
const PART_FFT_STEP: usize = 4;

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

#[derive(Clone, Copy, PartialEq, Debug)]
struct IrParams {
    length_ms: f32,
    window: usize,
    ring: bool,
    brighten: bool,
}

impl Default for IrParams {
    fn default() -> Self {
        IrParams {
            length_ms: PARAMS[LENGTH].default,
            window: RECTANGLE as usize,
            ring: false,
            brighten: false,
        }
    }
}

/// One complete preallocated impulse-response bank (both channels).
///
/// Two banks are held so an updated impulse can be built incrementally into the
/// inactive bank without touching the active playback bank, then crossfaded in.
struct IrBank {
    /// Shaped and scaled impulse samples per side (valid in `0..seg_len`).
    segment: [Vec<f32>; 2],
    seg_len: usize,
    h_re: [Vec<f32>; 2],
    h_im: [Vec<f32>; 2],
    /// Current 512-sample convolved wet output block per side.
    wet: [Vec<f32>; 2],
    length: usize,
    parts: usize,
    params: IrParams,
}

impl IrBank {
    fn new(max_parts: usize) -> Self {
        let cap = max_parts * P;
        let h_cap = max_parts * N;
        IrBank {
            segment: [vec![0.0; cap], vec![0.0; cap]],
            seg_len: 0,
            h_re: [vec![0.0; h_cap], vec![0.0; h_cap]],
            h_im: [vec![0.0; h_cap], vec![0.0; h_cap]],
            wet: [vec![0.0; P], vec![0.0; P]],
            length: 0,
            parts: 0,
            params: IrParams::default(),
        }
    }

    fn clear_wet(&mut self) {
        self.wet[0].fill(0.0);
        self.wet[1].fill(0.0);
    }

    fn early(&self, has_sound: bool) -> usize {
        if has_sound && self.params.brighten && !self.params.ring { P } else { 0 }
    }
}

/// Per-channel input, FDL spectra, and dry delay line (shared by both banks).
struct Side {
    /// The impulse as handed over, at this rate.
    sound: Vec<f32>,
    /// The input's spectra, newest at `head`.
    fdl_re: Vec<f32>,
    fdl_im: Vec<f32>,
    /// The last two blocks of input, oldest first.
    input: Vec<f32>,
    /// The input, delayed with the wet path: one partition, and one more
    /// while a brightened impulse starts a partition early.
    dry: Vec<f32>,
}

impl Side {
    fn new(max_parts: usize) -> Self {
        Side {
            sound: Vec::new(),
            fdl_re: vec![0.0; max_parts * N],
            fdl_im: vec![0.0; max_parts * N],
            input: vec![0.0; N],
            dry: vec![0.0; 2 * P],
        }
    }

    fn clear(&mut self) {
        self.fdl_re.fill(0.0);
        self.fdl_im.fill(0.0);
        self.input.fill(0.0);
        self.dry.fill(0.0);
    }
}

#[derive(Clone, Copy, Debug)]
enum BuildStage {
    Window { i: usize },
    BrightenCopy { i: usize, m: usize },
    BrightenFwd { m: usize, stepper: FftStepper },
    BrightenRamp { k: usize, m: usize },
    BrightenInv { m: usize, stepper: FftStepper },
    BrightenRotate { i: usize, m: usize, kept: usize },
    Measure { i: usize, acc: f32 },
    Scale { i: usize, scale: f32 },
    PartFft { k: usize },
}

#[derive(Clone, Copy, Debug)]
struct BuildState {
    params: IrParams,
    length: usize,
    parts: usize,
    seg_len: usize,
    side: usize,
    stage: BuildStage,
}

pub struct Convolution {
    sample_rate: f32,
    values: [f32; 6],
    fft: Fft,
    sides: [Side; 2],
    banks: [IrBank; 2],
    active: usize,
    max_parts: usize,
    head: usize,
    fill: usize,
    /// Samples since the start, for the ring modulator's place in the impulse.
    clock: u64,
    has_sound: bool,
    /// Whether the active bank has been prepared for the current sound.
    prepared: bool,
    /// Latest wanted impulse parameters waiting to be built into the staging bank.
    pending: Option<IrParams>,
    /// In-progress incremental build into `banks[1 - active]`.
    build: Option<BuildState>,
    /// True when the staging bank is finished and waiting for the next `block()`
    /// boundary so both old and new `wet` buffers are fresh before crossfading.
    xfade_armed: bool,
    xfade_len: usize,
    xfade_pos: usize,
    xfade_rem: usize,
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
    fn wanted_params(&self) -> IrParams {
        IrParams {
            length_ms: self.values[LENGTH],
            window: self.values[WINDOW] as usize,
            ring: self.values[RING] >= 0.5,
            brighten: self.values[BRIGHTEN] >= 0.5,
        }
    }

    fn ir_dims(&self, p: IrParams) -> (usize, usize) {
        let wanted = (p.length_ms * 0.001 * self.sample_rate).round() as usize;
        let longest = self.sides[0].sound.len();
        let length = wanted.clamp(1, self.max_parts * P).min(longest.max(1));
        // Brightened, the impulse is the whole transform block it was
        // brightened in, up to the longest kept.
        let used = if p.brighten && !p.ring {
            (2 * length).next_power_of_two().min(self.max_parts * P)
        } else {
            length
        };
        let parts = used.div_ceil(P);
        (length, parts)
    }

    /// Cut, shape, scale and transform the impulse synchronously into `banks[bank_idx]`.
    /// Used on the initial prepare and on `reset()` so still renders stay bit-identical.
    fn prepare_bank_sync(&mut self, bank_idx: usize, p: IrParams) {
        let (length, parts) = self.ir_dims(p);
        self.banks[bank_idx].params = p;
        self.banks[bank_idx].length = length;
        self.banks[bank_idx].parts = parts;

        for side in 0..2 {
            let sound = &self.sides[side].sound;
            let seg = &mut self.banks[bank_idx].segment[side];
            for i in 0..length {
                let x = sound.get(i).copied().unwrap_or(0.0);
                seg[i] = x * window_at(p.window, i, length);
            }
            let mut seg_len = length;

            if p.brighten {
                // SoundHack's +6 dB an octave: each bin times its frequency,
                // DC to nothing, on the windowed impulse's spectrum.
                let m = (2 * length).next_power_of_two().min(self.whole.max());
                let half = m / 2;
                for i in 0..m {
                    self.whole_re[i] = if i < seg_len { seg[i] } else { 0.0 };
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
                for i in 0..kept {
                    seg[i] = self.whole_re[(i + m - P) % m];
                }
                seg_len = kept;
            }

            let measure = if p.ring {
                seg[..seg_len].iter().fold(0.0f32, |m, v| m.max(v.abs()))
            } else {
                seg[..seg_len].iter().map(|v| v * v).sum::<f32>().sqrt()
            };
            let scale = if measure > 0.0 { 1.0 / measure } else { 0.0 };
            for v in &mut seg[..seg_len] {
                *v *= scale;
            }
            self.banks[bank_idx].seg_len = seg_len;

            for k in 0..parts {
                for i in 0..N {
                    self.re[i] = if i < P && k * P + i < seg_len { seg[k * P + i] } else { 0.0 };
                    self.im[i] = 0.0;
                }
                self.fft.forward(&mut self.re, &mut self.im);
                self.banks[bank_idx].h_re[side][k * N..(k + 1) * N].copy_from_slice(&self.re);
                self.banks[bank_idx].h_im[side][k * N..(k + 1) * N].copy_from_slice(&self.im);
            }
        }
    }

    fn prepare_sync(&mut self) {
        let p = self.wanted_params();
        self.prepare_bank_sync(self.active, p);
        self.prepared = true;
        self.pending = None;
        self.build = None;
        self.xfade_armed = false;
        self.xfade_rem = 0;
        self.xfade_pos = 0;
    }

    /// Begin or advance the incremental rebuild into `banks[1 - active]` by one
    /// bounded work quantum. Never allocates.
    fn step_build(&mut self) {
        if !self.has_sound || self.xfade_armed || self.xfade_rem > 0 {
            return;
        }
        let staging = 1 - self.active;

        if self.build.is_none() {
            let Some(wanted) = self.pending.take() else { return };
            if wanted == self.banks[self.active].params {
                return;
            }
            let (length, parts) = self.ir_dims(wanted);
            self.banks[staging].params = wanted;
            self.banks[staging].length = length;
            self.banks[staging].parts = parts;
            self.banks[staging].seg_len = length;
            self.build = Some(BuildState {
                params: wanted,
                length,
                parts,
                seg_len: length,
                side: 0,
                stage: BuildStage::Window { i: 0 },
            });
        }

        let Some(mut st) = self.build.take() else { return };
        let side = st.side;
        let p = st.params;
        let finished;

        match st.stage {
            BuildStage::Window { i } => {
                let end = (i + WINDOW_STEP).min(st.length);
                let sound = &self.sides[side].sound;
                let seg = &mut self.banks[staging].segment[side];
                for idx in i..end {
                    let x = sound.get(idx).copied().unwrap_or(0.0);
                    seg[idx] = x * window_at(p.window, idx, st.length);
                }
                if end < st.length {
                    st.stage = BuildStage::Window { i: end };
                } else {
                    st.seg_len = st.length;
                    if p.brighten {
                        let m = (2 * st.length).next_power_of_two().min(self.whole.max());
                        st.stage = BuildStage::BrightenCopy { i: 0, m };
                    } else {
                        st.stage = BuildStage::Measure { i: 0, acc: 0.0 };
                    }
                }
                finished = false;
            }
            BuildStage::BrightenCopy { i, m } => {
                let end = (i + COPY_STEP).min(m);
                let seg = &self.banks[staging].segment[side];
                for idx in i..end {
                    self.whole_re[idx] = if idx < st.seg_len { seg[idx] } else { 0.0 };
                    self.whole_im[idx] = 0.0;
                }
                if end < m {
                    st.stage = BuildStage::BrightenCopy { i: end, m };
                } else {
                    st.stage = BuildStage::BrightenFwd { m, stepper: FftStepper::new(m, false) };
                }
                finished = false;
            }
            BuildStage::BrightenFwd { m, mut stepper } => {
                let done = stepper.step(
                    &self.whole,
                    &mut self.whole_re[..m],
                    &mut self.whole_im[..m],
                    BRIGHTEN_FFT_STEP,
                );
                if done {
                    st.stage = BuildStage::BrightenRamp { k: 0, m };
                } else {
                    st.stage = BuildStage::BrightenFwd { m, stepper };
                }
                finished = false;
            }
            BuildStage::BrightenRamp { k, m } => {
                let half = m / 2;
                let end = (k + COPY_STEP).min(half + 1);
                for bin in k..end {
                    let g = bin as f32 / half as f32;
                    self.whole_re[bin] *= g;
                    self.whole_im[bin] *= g;
                    if bin != 0 && bin != half {
                        self.whole_re[m - bin] *= g;
                        self.whole_im[m - bin] *= g;
                    }
                }
                if end <= half {
                    st.stage = BuildStage::BrightenRamp { k: end, m };
                } else {
                    st.stage = BuildStage::BrightenInv { m, stepper: FftStepper::new(m, true) };
                }
                finished = false;
            }
            BuildStage::BrightenInv { m, mut stepper } => {
                let done = stepper.step(
                    &self.whole,
                    &mut self.whole_re[..m],
                    &mut self.whole_im[..m],
                    BRIGHTEN_FFT_STEP,
                );
                if done {
                    let kept = m.min(self.max_parts * P);
                    st.stage = BuildStage::BrightenRotate { i: 0, m, kept };
                } else {
                    st.stage = BuildStage::BrightenInv { m, stepper };
                }
                finished = false;
            }
            BuildStage::BrightenRotate { i, m, kept } => {
                let end = (i + COPY_STEP).min(kept);
                let seg = &mut self.banks[staging].segment[side];
                for idx in i..end {
                    seg[idx] = self.whole_re[(idx + m - P) % m];
                }
                if end < kept {
                    st.stage = BuildStage::BrightenRotate { i: end, m, kept };
                } else {
                    st.seg_len = kept;
                    self.banks[staging].seg_len = kept;
                    st.stage = BuildStage::Measure { i: 0, acc: 0.0 };
                }
                finished = false;
            }
            BuildStage::Measure { i, mut acc } => {
                let end = (i + MEASURE_STEP).min(st.seg_len);
                let seg = &self.banks[staging].segment[side];
                if p.ring {
                    for &v in &seg[i..end] {
                        acc = acc.max(v.abs());
                    }
                } else {
                    // Strict left-to-right f32 accumulation matches `iter().map(|v| v*v).sum::<f32>()`.
                    for &v in &seg[i..end] {
                        acc += v * v;
                    }
                }
                if end < st.seg_len {
                    st.stage = BuildStage::Measure { i: end, acc };
                } else {
                    let measure = if p.ring { acc } else { acc.sqrt() };
                    let scale = if measure > 0.0 { 1.0 / measure } else { 0.0 };
                    st.stage = BuildStage::Scale { i: 0, scale };
                }
                finished = false;
            }
            BuildStage::Scale { i, scale } => {
                let end = (i + COPY_STEP).min(st.seg_len);
                let seg = &mut self.banks[staging].segment[side];
                for v in &mut seg[i..end] {
                    *v *= scale;
                }
                if end < st.seg_len {
                    st.stage = BuildStage::Scale { i: end, scale };
                } else {
                    self.banks[staging].seg_len = st.seg_len;
                    st.stage = BuildStage::PartFft { k: 0 };
                }
                finished = false;
            }
            BuildStage::PartFft { k } => {
                let end = (k + PART_FFT_STEP).min(st.parts);
                let seg_len = st.seg_len;
                for part in k..end {
                    let seg = &self.banks[staging].segment[side];
                    for i in 0..N {
                        self.re[i] = if i < P && part * P + i < seg_len { seg[part * P + i] } else { 0.0 };
                        self.im[i] = 0.0;
                    }
                    self.fft.forward(&mut self.re, &mut self.im);
                    self.banks[staging].h_re[side][part * N..(part + 1) * N].copy_from_slice(&self.re);
                    self.banks[staging].h_im[side][part * N..(part + 1) * N].copy_from_slice(&self.im);
                }
                if end < st.parts {
                    st.stage = BuildStage::PartFft { k: end };
                    finished = false;
                } else if side == 0 {
                    st.side = 1;
                    st.seg_len = st.length;
                    st.stage = BuildStage::Window { i: 0 };
                    finished = false;
                } else {
                    finished = true;
                }
            }
        }

        if finished {
            if p.ring {
                // Ring mode reads `segment` directly; no wet block boundary wait needed.
                self.xfade_armed = false;
                self.xfade_pos = 0;
                self.xfade_rem = self.xfade_len;
            } else {
                // Wait for the next `block()` boundary so `banks[staging].wet` is freshly populated.
                self.xfade_armed = true;
            }
        } else {
            self.build = Some(st);
        }
    }

    /// How much earlier than its own start the impulse begins: a partition,
    /// while brightened (see `prepare_bank_sync`).
    fn early(&self) -> usize {
        if !self.has_sound {
            return 0;
        }
        if self.prepared {
            self.banks[self.active].early(true)
        } else if self.values[BRIGHTEN] >= 0.5 && self.values[RING] < 0.5 {
            P
        } else {
            0
        }
    }

    /// Convolve the current FDL spectra with `banks[bank_idx]` into `banks[bank_idx].wet[side]`.
    fn convolve_bank_side(&mut self, bank_idx: usize, side: usize) {
        let s = &self.sides[side];
        let bank = &mut self.banks[bank_idx];
        let parts = bank.parts;
        let max_parts = self.max_parts;
        let head = self.head;
        let h_re = &bank.h_re[side];
        let h_im = &bank.h_im[side];
        self.re.fill(0.0);
        self.im.fill(0.0);
        for k in 0..parts {
            let x = ((head + max_parts - k) % max_parts) * N;
            let h = k * N;
            let fdl_r = &s.fdl_re[x..x + N];
            let fdl_i = &s.fdl_im[x..x + N];
            let hr = &h_re[h..h + N];
            let hi = &h_im[h..h + N];
            for i in 0..N {
                let (a, b) = (fdl_r[i], fdl_i[i]);
                let (c, d) = (hr[i], hi[i]);
                self.re[i] += a * c - b * d;
                self.im[i] += a * d + b * c;
            }
        }
        self.fft.inverse(&mut self.re, &mut self.im);
        bank.wet[side].copy_from_slice(&self.re[P..]);
    }

    /// A block of input is in: transform it once into the shared FDL, and
    /// multiply against the active bank (plus the staging bank when crossfading).
    fn block(&mut self) {
        let old = self.active;
        let new = 1 - old;
        let want_new = self.xfade_armed || self.xfade_rem > 0;
        let run_old = !self.banks[old].params.ring;
        let run_new = want_new && !self.banks[new].params.ring;

        if !run_old && !run_new {
            for s in &mut self.sides {
                s.input.copy_within(P.., 0);
            }
            return;
        }

        self.head = (self.head + 1) % self.max_parts;
        for side in 0..2 {
            {
                let s = &mut self.sides[side];
                self.re.copy_from_slice(&s.input);
                self.im.fill(0.0);
                self.fft.forward(&mut self.re, &mut self.im);
                let at = self.head * N;
                s.fdl_re[at..at + N].copy_from_slice(&self.re);
                s.fdl_im[at..at + N].copy_from_slice(&self.im);
            }
            if run_old {
                self.convolve_bank_side(old, side);
            }
            if run_new {
                self.convolve_bank_side(new, side);
            }
            self.sides[side].input.copy_within(P.., 0);
        }

        if self.xfade_armed {
            self.xfade_armed = false;
            self.xfade_pos = 0;
            self.xfade_rem = self.xfade_len;
        }
    }
}

impl Kernel for Convolution {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        let max_parts = ((MAX_SECONDS * sample_rate) as usize).div_ceil(P);
        let xfade_len = ((XFADE_MS * 0.001 * sample_rate).round() as usize).max(1);
        let mut values = [0.0; 6];
        for (v, spec) in values.iter_mut().zip(PARAMS) {
            *v = spec.default;
        }
        let mut k = Convolution {
            sample_rate,
            values,
            fft: Fft::new(N),
            sides: [Side::new(max_parts), Side::new(max_parts)],
            banks: [IrBank::new(max_parts), IrBank::new(max_parts)],
            active: 0,
            max_parts,
            head: 0,
            fill: 0,
            clock: 0,
            has_sound: false,
            prepared: false,
            pending: None,
            build: None,
            xfade_armed: false,
            xfade_len,
            xfade_pos: 0,
            xfade_rem: 0,
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
            _ => {
                if changed && self.has_sound && self.prepared {
                    self.pending = Some(self.wanted_params());
                }
            }
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
        }
        self.has_sound = out > 0;
        self.prepared = false;
        self.pending = None;
        self.build = None;
        self.xfade_armed = false;
        self.xfade_rem = 0;
        self.xfade_pos = 0;
    }

    fn reset(&mut self) {
        for s in &mut self.sides {
            s.clear();
        }
        self.banks[0].clear_wet();
        self.banks[1].clear_wet();
        self.active = 0;
        self.head = 0;
        self.fill = 0;
        self.clock = 0;
        self.dpos = 0;
        self.gain.jump(self.values[GAIN]);
        self.mix.jump(self.values[MIX]);
        self.pending = None;
        self.build = None;
        self.xfade_armed = false;
        self.xfade_rem = 0;
        self.xfade_pos = 0;
        self.prepared = false;
        if self.has_sound {
            self.prepare_sync();
        }
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.has_sound && !self.prepared {
            self.prepare_sync();
        } else if (self.pending.is_some() || self.build.is_some())
            && self.fill % WORK_STRIDE == 0
            && self.fill != P - WORK_STRIDE
        {
            // Advance at most one bounded rebuild quantum every 64 frames,
            // skipping the final 64-frame slice before `block()` so rebuild
            // work and partition convolution never land on the same 64-frame slot.
            self.step_build();
        }

        let gain = 10f32.powf(self.gain.next() / 20.0);
        let mix = self.mix.next();
        let fill = self.fill;
        let old = self.active;

        if self.xfade_rem == 0 {
            let bank = &self.banks[old];
            let ring = bank.params.ring;
            let place = self.clock.checked_sub(P as u64).map(|t| (t % bank.length.max(1) as u64) as usize);
            let read = (self.dpos + 2 * P - P - bank.early(self.has_sound)) % (2 * P);
            for (side, s) in frame.iter_mut().enumerate() {
                let sd = &mut self.sides[side];
                let dry = sd.dry[read];
                let wet = if !self.has_sound {
                    dry
                } else if ring {
                    place.map_or(0.0, |p| dry * bank.segment[side][p]) * gain
                } else {
                    bank.wet[side][fill] * gain
                };
                sd.dry[self.dpos] = *s;
                sd.input[P + fill] = *s;
                *s = if self.has_sound { dry + (wet - dry) * mix } else { dry };
            }
        } else {
            let new = 1 - old;
            let b_old = &self.banks[old];
            let b_new = &self.banks[new];
            let place_old = self.clock.checked_sub(P as u64).map(|t| (t % b_old.length.max(1) as u64) as usize);
            let place_new = self.clock.checked_sub(P as u64).map(|t| (t % b_new.length.max(1) as u64) as usize);
            let read_old = (self.dpos + 2 * P - P - b_old.early(self.has_sound)) % (2 * P);
            let read_new = (self.dpos + 2 * P - P - b_new.early(self.has_sound)) % (2 * P);
            let t = (self.xfade_pos + 1) as f32 / self.xfade_len as f32;
            for (side, s) in frame.iter_mut().enumerate() {
                let sd = &mut self.sides[side];
                let dry_old = sd.dry[read_old];
                let dry_new = sd.dry[read_new];
                let wet_old = if b_old.params.ring {
                    place_old.map_or(0.0, |p| dry_old * b_old.segment[side][p]) * gain
                } else {
                    b_old.wet[side][fill] * gain
                };
                let wet_new = if b_new.params.ring {
                    place_new.map_or(0.0, |p| dry_new * b_new.segment[side][p]) * gain
                } else {
                    b_new.wet[side][fill] * gain
                };
                let out_old = dry_old + (wet_old - dry_old) * mix;
                let out_new = dry_new + (wet_new - dry_new) * mix;
                sd.dry[self.dpos] = *s;
                sd.input[P + fill] = *s;
                *s = out_old + (out_new - out_old) * t;
            }
            self.xfade_pos += 1;
            self.xfade_rem -= 1;
            if self.xfade_rem == 0 {
                self.active = new;
                self.xfade_pos = 0;
            }
        }

        self.clock += 1;
        self.dpos = (self.dpos + 1) % (2 * P);
        self.fill += 1;
        if self.fill == P {
            self.fill = 0;
            if self.has_sound {
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
        let len = if self.prepared { self.banks[self.active].length } else { 0 };
        (len + P) as f32 / self.sample_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_impulse(secs: f32, sr: f32) -> Vec<f32> {
        let n = (secs * sr) as usize;
        let mut x = 0x1234_5678u32;
        let mut out = Vec::with_capacity(n * 2);
        for i in 0..n {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let env = (-3.0 * i as f32 / n as f32).exp();
            let l = ((x as f32 / u32::MAX as f32) * 2.0 - 1.0) * env;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let r = ((x as f32 / u32::MAX as f32) * 2.0 - 1.0) * env;
            out.push(l);
            out.push(r);
        }
        out
    }

    #[test]
    fn incremental_build_matches_sync_prepare_bit_for_bit() {
        let sr = 48_000.0;
        let imp = test_impulse(4.0, sr);
        let cases = [
            IrParams { length_ms: 10.0, window: 3, ring: false, brighten: false },
            IrParams { length_ms: 100.0, window: 1, ring: false, brighten: false },
            IrParams { length_ms: 500.0, window: 6, ring: false, brighten: true },
            IrParams { length_ms: 250.0, window: 4, ring: true, brighten: false },
            IrParams { length_ms: 200.0, window: 5, ring: true, brighten: true },
            IrParams { length_ms: 4000.0, window: 1, ring: false, brighten: true },
        ];

        for p in cases {
            let mut sync_k = Convolution::new(sr);
            sync_k.set_sound(&imp, 2, sr);
            sync_k.prepare_bank_sync(0, p);

            let mut inc_k = Convolution::new(sr);
            inc_k.set_sound(&imp, 2, sr);
            // Prepare initial bank with different settings so `p` is guaranteed to rebuild.
            let init = IrParams {
                length_ms: if (p.length_ms - 10.0).abs() < 1.0 { 100.0 } else { 10.0 },
                window: (p.window + 1) % WINDOWS.len(),
                ring: !p.ring,
                brighten: !p.brighten,
            };
            inc_k.prepare_bank_sync(0, init);
            inc_k.prepared = true;
            inc_k.pending = Some(p);

            let mut steps = 0usize;
            while inc_k.pending.is_some() || inc_k.build.is_some() {
                inc_k.step_build();
                steps += 1;
                assert!(steps < 5_000, "incremental build did not finish for {p:?}");
            }

            let want = &sync_k.banks[0];
            let got = &inc_k.banks[1];
            assert_eq!(got.length, want.length, "length {p:?}");
            assert_eq!(got.parts, want.parts, "parts {p:?}");
            assert_eq!(got.seg_len, want.seg_len, "seg_len {p:?}");
            for side in 0..2 {
                for i in 0..want.seg_len {
                    assert_eq!(
                        got.segment[side][i].to_bits(),
                        want.segment[side][i].to_bits(),
                        "segment mismatch side={side} i={i} {p:?}"
                    );
                }
                for i in 0..want.parts * N {
                    assert_eq!(
                        got.h_re[side][i].to_bits(),
                        want.h_re[side][i].to_bits(),
                        "h_re mismatch side={side} i={i} {p:?}"
                    );
                    assert_eq!(
                        got.h_im[side][i].to_bits(),
                        want.h_im[side][i].to_bits(),
                        "h_im mismatch side={side} i={i} {p:?}"
                    );
                }
            }
        }
    }
}
