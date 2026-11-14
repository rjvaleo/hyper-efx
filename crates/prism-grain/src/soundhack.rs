//! SoundHack's phase vocoder time scale, rebuilt exactly.
//!
//! Tom Erbe's SoundHack, Hack menu → Phase Vocoder, Time Scale. Transcribed
//! from the decoded program rather than from a description of what a phase
//! vocoder does: report `effect-algorithms/sources/reports/003-soundhack.md`
//! (sections 2, 3, 14 and 16), re-read here against the 0.896 i386
//! disassembly itself wherever the report left an operation's order or
//! precision open. Every quirk is kept. Where this file departs from the
//! program it says so, and why.
//!
//! The process is F. R. Moore's CARL `pvoc`, as SoundHack runs it, once per
//! block (`PvocBlock`):
//!
//! ```text
//! in += D; out += I
//! ShiftIn D → WindowFold (wA, rotated by in) → RealFFT → CartToPolar
//!   → gate → PhaseInterpolate (× I/D) → PolarToCart → inverse RealFFT
//!   → OverlapAdd (wS, at out) → ShiftOut I, once out + I ≥ 0
//! ```
//!
//! **Two versions.** 0.888 and 0.896 differ in two places only (report §16):
//!
//! - `PhaseInterpolate`. 0.888 wraps the raw frame-to-frame phase difference
//!   into ±π and then scales it by I/D, and never wraps the sum it keeps.
//!   0.896 scales the raw difference and wraps the sum. Neither takes out a
//!   bin-centre advance first (see `WindowFold` below for why there is little
//!   to take out), and wherever the difference passes ±π the scaled phase is
//!   out by 2π·m·I/D — part of the sound, so kept. At a whole-number I/D the
//!   two are the same phase mod 2π. At any other, 0.896
//!   turns every pass of a partial's measured phase through ±π into a phase
//!   flip of 2π·s, so a partial off its bin's centre cancels itself (a steady
//!   sine at 1.5× comes out 30 dB and more down, against 0.888's 3 dB), while
//!   one on a centre is kept by both.
//! - `FindBestRatio`. 0.896 tries a tolerance of 1.004 before 1.01. The break
//!   order the report left open is pinned here from the disassembly
//!   (0x2cda3): test 1.004 → step the hop → test 1.01, so a 1.01 hit returns
//!   the hop *after* the step. See [`find_best_ratio`].
//!
//! `WindowFold` rotates each frame by its input time, so the measured phase
//! is referenced to absolute time and its frame-to-frame difference is a
//! partial's deviation from its bin's centre; the wrap only bites where that
//! deviation's advance over D passes ±π.
//!
//! **Level.** SoundHack's time pointers mark where a window starts, and the
//! output pointer is the input one times s, so scaling the accumulated phase
//! by s scales the window's own centre offset as well: each resynthesised
//! grain lands (s − 1)·(Nw − 1)/2 samples off the synthesis window, circularly
//! within the frame. A tone on a bin centre comes out at the window's
//! autocorrelation at that lag: with the defaults 0.73 of its level (−2.7 dB)
//! at 1.5× and 0.5×, 0.47 (−6.6 dB) at 2× and 4×, all of it at 1×; one off a
//! centre loses a little more, where the wrapped end of the grain disagrees
//! with the rest. That follows from the decoded pointer arithmetic and is
//! kept; it is pinned by `the_grain_lands_where_soundhacks_pointers_put_it`.
//!
//! 0.896's peak-locking branch in `PhaseInterpolate` is dead code there
//! (`SetPvocDefaults` forces its flag off and no dialog item sets it), so it is
//! not here either.
//!
//! **Fixed at SoundHack's defaults** (`SetPhaDefaults`, `SetPvocDefaults`):
//! 512 bands (N = 1024), overlap 1x (Nw = N), Hamming, and the resynthesis
//! gate on at −60 dB under the frame's peak and −80 dB absolute — the gating
//! box opens ticked because `SetPvocDefaults` ticks it whenever the minimum
//! amplitude is not zero, and the defaults make it 0.0001. The bands, overlap
//! and window are all implemented ([`Config`]); only the version is a control.
//!
//! **Float, as the program is.** Samples, spectra, windows and phases are
//! single precision, with the double-precision steps where the program has
//! them (the window cosines, `hypot`, the I/D scale). The maths library is the
//! platform's, as 0.896's is OS X libm's, so last bits can differ from either
//! original; 0.888's PowerPC library differs more.
//!
//! **Where this departs from the program**, each for the host rather than the
//! sound:
//!
//! - SoundHack runs one file at one scale. Here a ratio that moves re-runs
//!   `FindBestRatio` at the next block, as SoundHack's own scaling function
//!   does, and also rescales the windows for the new hop, which SoundHack
//!   only does once in `InitPvocProcess`: without that, every move of the
//!   slider would change the level until the next seek.
//! - A seek starts a fresh run, as SoundHack starts a file, but reads the
//!   material before the seek point rather than SoundHack's zeros, and runs
//!   at most [`LIVE_PREROLL`] of the blocks that come before the first one
//!   written (an offline render from the top runs them all, and is the
//!   program's output sample for sample).
//! - SoundHack stops when its input runs out; the host decides the length.
//! - A hop of zero (a ratio far outside what SoundHack's dialog allows) is
//!   held at one rather than dividing by it.

use crate::stream::{StretchParams, Streamer};

/// `_Pi` and `_twoPi` as SoundHack holds them: floats (0x40490fdb,
/// 0x40c90fdb).
const PI: f32 = std::f32::consts::PI;
const TWO_PI: f32 = std::f32::consts::TAU;

/// The most pre-roll blocks a live seek runs. Enough to cover every block
/// whose output reaches the first frame written at any hop up to Nw/8, and
/// for stretches up to 8× the whole of SoundHack's own pre-roll.
pub const LIVE_PREROLL: usize = 64;

/// Which SoundHack the time scale behaves like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Version {
    /// 0.888 (PowerPC, 2001): wrap the phase difference, then scale it.
    V0888,
    /// 0.896 (Universal, 2007): scale the phase difference, then wrap the
    /// sum. The default, as the later program.
    #[default]
    V0896,
}

impl Version {
    pub fn as_str(self) -> &'static str {
        match self {
            Version::V0888 => "0.888",
            Version::V0896 => "0.896",
        }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "0.888" => Some(Version::V0888),
            "0.896" => Some(Version::V0896),
            _ => None,
        }
    }
}

/// The time scale's controls. One, because everything else is SoundHack's
/// defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SoundHackParams {
    pub version: Version,
}

/// `GetWindow`'s seven windows, by their popup index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    /// Index 0: the buffer is left as it is.
    None = 0,
    Hamming = 1,
    Kaiser = 2,
    Ramp = 3,
    Rectangle = 4,
    Sinc = 5,
    Triangle = 6,
    /// 0.5 − 0.4·cos, which is not a Hann window. SoundHack's, so kept.
    VonHann = 7,
}

/// What SoundHack's dialog sets: the FFT size N (2 × bands), the window
/// length Nw (overlap × N), the window, and the resynthesis gate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    pub n: usize,
    pub nw: usize,
    pub window: Window,
    /// "Threshold under max", linear. Zero with `min_amp` zero is gating off.
    pub under_max: f64,
    /// "Minimum amplitude", linear.
    pub min_amp: f64,
}

/// `SetPhaDefaults` / `SetPvocDefaults`: 512 bands, overlap 1x, Hamming,
/// gating on at 0.001 and 0.0001.
pub const DEFAULT_CONFIG: Config =
    Config { n: 1024, nw: 1024, window: Window::Hamming, under_max: 0.001, min_amp: 0.0001 };

// ---------------------------------------------------------------- the FFT

/// `InitFFTTable`: per stage s, with m = 2^(s+1), sin(2π/m) and −2·sin²(π/m).
pub struct FftTables {
    sin: [f32; 31],
    cos1: [f32; 31],
}

impl FftTables {
    pub fn new() -> Self {
        let mut t = FftTables { sin: [0.0; 31], cos1: [0.0; 31] };
        for s in 0..31 {
            let m = (2u64 << s) as f32;
            t.sin[s] = (TWO_PI / m).sin();
            let x = ((PI / m) as f64).sin();
            t.cos1[s] = ((x * -2.0) * x) as f32;
        }
        t
    }
}

impl Default for FftTables {
    fn default() -> Self {
        Self::new()
    }
}

/// `FFT(x, NC, dir)`: Numerical Recipes `four1` on NC interleaved complex
/// points, e^{+i} forward. The inverse divides by NC.
fn fft(x: &mut [f32], nc: usize, forward: bool, t: &FftTables) {
    let n = 2 * nc;
    // `bitreverse`, the NR swap loop on pairs.
    let (mut i, mut j) = (0usize, 0usize);
    while n > 0 {
        let mut m = nc;
        while m > 1 && j >= m {
            j -= m;
            m >>= 1;
        }
        j += m;
        i += 2;
        if i >= n {
            break;
        }
        if i < j {
            x.swap(i, j);
            x.swap(i + 1, j + 1);
        }
    }
    if n > 2 {
        let (mut mmax, mut s) = (2usize, 0usize);
        while n > mmax {
            let istep = 2 * mmax;
            let wpi = if forward { t.sin[s] } else { -t.sin[s] };
            let wpr = t.cos1[s];
            let (mut wr, mut wi) = (1.0f32, 0.0f32);
            let mut m = 0;
            while m < mmax {
                let mut i = m;
                while i < n {
                    let j = i + mmax;
                    let tempr = wr * x[j] - wi * x[j + 1];
                    let tempi = wr * x[j + 1] + wi * x[j];
                    x[j] = x[i] - tempr;
                    x[j + 1] = x[i + 1] - tempi;
                    x[i] = tempr + x[i];
                    x[i + 1] = tempi + x[i + 1];
                    i += istep;
                }
                let next = (wpr * wr - wpi * wi) + wr;
                wi = wi + (wpr * wi + wr * wpi);
                wr = next;
                m += 2;
            }
            mmax = istep;
            s += 1;
        }
    }
    if !forward {
        let scale = (1.0 / nc as f64) as f32;
        for v in x[..n].iter_mut() {
            *v = scale * *v;
        }
    }
}

/// `RealFFT(x, NC, dir)`: NR `realft` on 2·NC reals, packed with DC in x[0]
/// and Nyquist in x[1]. The forward runs `FFT` first, the inverse last.
///
/// Laid out as the program does it rather than as the book does: one loop
/// from k = 0 to NC/2 inclusive, with the k = 0 partner (the element past the
/// end, which is bin 0 again) held in two locals. The middle bin is its own
/// partner and comes out as it went in.
pub fn real_fft(x: &mut [f32], nc: usize, forward: bool, t: &FftTables) {
    let theta0 = PI / nc as f32;
    let (theta, c2, mut h0r, mut h0i);
    if forward {
        fft(x, nc, true, t);
        theta = theta0;
        c2 = -0.5f32;
        h0r = x[0];
        h0i = x[1];
    } else {
        theta = -theta0;
        c2 = 0.5f32;
        h0r = x[1];
        x[1] = 0.0;
        h0i = 0.0;
    }
    let c1 = 0.5f32;
    let nc2 = -c2;
    let half = ((theta * 0.5).sin()) as f64;
    let wpr = (half * (half * -2.0)) as f32;
    let wpi = theta.sin();
    let (mut wr, mut wi) = (1.0f32, 0.0f32);
    for k in 0..=nc / 2 {
        if k == 0 {
            let x0 = x[0];
            let x1 = x[1];
            let h1r = (h0r + x0) * c1;
            let h1i = (x1 - h0i) * c1;
            let h2r = (h0i + x1) * nc2;
            let h2i = (x0 - h0r) * c2;
            let a = wr * h2r;
            let b = wi * h2i;
            x[0] = (h1r + a) - b;
            x[1] = (h1i + h2i * wr) + h2r * wi;
            h0r = (h1r - a) + b;
            h0i = (h2i * wr - h1i) + h2r * wi;
        } else {
            let (i1, i2, i3, i4) = (2 * k, 2 * k + 1, 2 * nc - 2 * k, 2 * nc - 2 * k + 1);
            let (a1, a3, a2, a4) = (x[i1], x[i3], x[i2], x[i4]);
            let h1r = (a1 + a3) * c1;
            let h1i = (a2 - a4) * c1;
            let h2r = (a2 + a4) * nc2;
            let h2i = (a1 - a3) * c2;
            let a = wr * h2r;
            let b = wi * h2i;
            x[i1] = (h1r + a) - b;
            x[i2] = (h1i + h2i * wr) + h2r * wi;
            x[i3] = (h1r - a) + b;
            x[i4] = (h2i * wr - h1i) + h2r * wi;
        }
        let next = (wpr * wr - wi * wpi) + wr;
        wi = (wi * wpr + wr * wpi) + wi;
        wr = next;
    }
    if forward {
        x[1] = h0r;
    } else {
        fft(x, nc, false, t);
    }
}

// ---------------------------------------------------------------- the windows

/// `ino`: the Bessel I0 series, in float, stopping when a term's square falls
/// under 1e-8 of the sum, and after 25 terms in any case.
fn ino(x: f32) -> f32 {
    let half = 0.5f32 * x;
    let (mut t, mut sum) = (1.0f32, 1.0f32);
    for k in 1..26 {
        t = t * half;
        t = t / k as f32;
        let t2 = t * t;
        sum = sum + t2;
        if sum * 1e-8f32 > t2 {
            break;
        }
    }
    sum
}

/// `GetWindow(buf, n, type)`. Every one symmetric over n points.
pub fn get_window(buf: &mut [f32], n: usize, w: Window) {
    let buf = &mut buf[..n];
    if n == 0 {
        return;
    }
    // The two raised cosines share a shape: the angle in float, its cosine in
    // double, the constants floats widened to double.
    let raised = |buf: &mut [f32], a: f64, b: f64| {
        let den = (n as i64 - 1) as f32;
        for (i, v) in buf.iter_mut().enumerate() {
            let x = ((i as f32 * TWO_PI) / den) as f64;
            *v = (a - x.cos() * b) as f32;
        }
    };
    match w {
        Window::None => {}
        Window::Hamming => raised(buf, 0.54f32 as f64, 0.46f32 as f64),
        Window::VonHann => raised(buf, 0.5, 0.4f32 as f64),
        Window::Rectangle => buf.fill(1.0),
        Window::Ramp => {
            for (i, v) in buf.iter_mut().enumerate() {
                *v = (1.0 - ((i as f32 / n as f32) as f64)) as f32;
            }
        }
        Window::Triangle => {
            let nd = (n as f32) as f64;
            let (mut v, mut up) = (0.0f32, true);
            for o in buf.iter_mut() {
                *o = v + v;
                if up {
                    v = ((v as f64) + 1.0 / nd) as f32;
                    if v > 0.5 {
                        v = 1.0 - v;
                        up = false;
                    }
                } else {
                    v = ((v as f64) + -1.0 / nd) as f32;
                }
            }
        }
        Window::Sinc => {
            let half = n as f32 * 0.5;
            for (i, o) in buf.iter_mut().enumerate() {
                let xi = i as f32;
                if xi == half {
                    *o = 1.0;
                    continue;
                }
                let x = xi - half;
                let arg = (x * PI) / half;
                let den = (PI as f64 + PI as f64) * x as f64;
                *o = ((arg as f64).sin() / den * n as f64) as f32;
            }
        }
        Window::Kaiser => {
            let half = (n as i64 / 2) as usize;
            let i0b = ino(6.8f32) as f64;
            let den = {
                let m = (n as i64 - 1) as f32;
                (m * m) as f64
            };
            for i in 0..half {
                let r = ((((i as f64 * -4.0) * i as f64) / den + 1.0).sqrt() * 6.8) as f32;
                let v = (ino(r) as f64 / i0b) as f32;
                buf[half + i] = v;
                buf[half - i] = v;
            }
            buf[n - 1] = 0.0;
            buf[0] = 0.0;
        }
    }
}

/// `ScaleWindows(wA, wS, info)`, Moore's: the analysis window to sum to two,
/// the synthesis window to its squares one hop apart summing to one (Nw ≤ N);
/// or, for Nw > N, both windows multiplied by a sinc first and the synthesis
/// one by the inverse factor.
///
/// `csound` is the spectral-file format flag, which scales the analysis by
/// D/2. It is never set for a sound file (the field is not written by the
/// PV's defaults), and it cancels out of the output either way.
pub fn scale_windows(wa: &mut [f32], ws: &mut [f32], n: usize, nw: usize, d: usize, i: usize, csound: bool) {
    if nw > n {
        let mut x = ((((nw as f32) as f64) - 1.0) * -0.5) as f32;
        for k in 0..nw {
            if x != 0.0 {
                let xp = x * PI;
                wa[k] = (((n as f32 * wa[k]) as f64 * ((xp / n as f32) as f64).sin()) / xp as f64) as f32;
                if i != 0 {
                    ws[k] = (((i as f32 * ws[k]) as f64 * ((xp / i as f32) as f64).sin()) / xp as f64) as f32;
                }
            }
            x = 1.0 + x;
        }
    }
    let mut sum = 0.0f32;
    for v in &wa[..nw] {
        sum = sum + *v;
    }
    let mut afac = 2.0f32 / sum;
    if csound {
        afac = (afac as f64 * (d as f64 * 0.5)) as f32;
    }
    let sfac = if nw > n { (1.0 / afac as f64) as f32 } else { afac };
    for k in 0..nw {
        wa[k] = afac * wa[k];
        ws[k] = sfac * ws[k];
    }
    if nw <= n && nw > 0 {
        let mut s2 = 0.0f32;
        let mut k = 0;
        while k < nw {
            s2 = s2 + ws[k] * ws[k];
            k += i.max(1);
        }
        let sc = 1.0f32 / s2;
        for v in ws[..nw].iter_mut() {
            *v = sc * *v;
        }
    }
}

// ---------------------------------------------------------------- one frame

/// `WindowFold`: the window, folded mod N and rotated by the input time, so
/// phases stay referenced to absolute time.
fn window_fold(input: &[f32], wa: &[f32], out: &mut [f32], time: i64, n: usize, nw: usize) {
    out[..n].fill(0.0);
    let mut k = time.rem_euclid(n as i64) as usize;
    for i in 0..nw {
        out[k] = input[i] * wa[i] + out[k];
        k += 1;
        if k == n {
            k = 0;
        }
    }
}

/// SoundHack's own `hypot`, in double.
fn hypot_sh(a: f64, b: f64) -> f64 {
    let s = a * a + b * b;
    if s == 0.0 {
        0.0
    } else {
        s.sqrt()
    }
}

/// `CartToPolar`: interleaved {amp, phase} for bins 0..=N2, phase −atan2.
pub fn cart_to_polar(x: &[f32], spec: &mut [f32], n2: usize) {
    for k in 0..=n2 {
        let (re, im) = if k == 0 {
            (x[0], 0.0)
        } else if k == n2 {
            (x[1], 0.0)
        } else {
            (x[2 * k], x[2 * k + 1])
        };
        let amp = hypot_sh(re as f64, im as f64) as f32;
        spec[2 * k] = amp;
        spec[2 * k + 1] = if amp == 0.0 { 0.0 } else { -(im.atan2(re)) };
    }
}

/// `PolarToCart`: re = A·cos φ, im = −A·sin φ; bins 0 and N2 real only.
fn polar_to_cart(spec: &[f32], x: &mut [f32], n2: usize) {
    for k in 0..=n2 {
        let amp = spec[2 * k];
        let ph = spec[2 * k + 1];
        if amp == 0.0 {
            if k == n2 {
                x[1] = 0.0;
            } else if k == 0 {
                x[0] = 0.0;
            } else {
                x[2 * k] = 0.0;
                x[2 * k + 1] = 0.0;
            }
        } else if k == 0 {
            x[0] = ph.cos() * amp;
        } else if k == n2 {
            x[1] = ph.cos() * amp;
        } else {
            x[2 * k] = ph.cos() * amp;
            x[2 * k + 1] = ph.sin() * (-amp);
        }
    }
}

/// `OverlapAdd`: y[i] += x[(out + i) mod N]·wS[i].
fn overlap_add(x: &[f32], ws: &[f32], y: &mut [f32], time: i64, n: usize, nw: usize) {
    let mut k = time.rem_euclid(n as i64) as usize;
    for i in 0..nw {
        y[i] = x[k] * ws[i] + y[i];
        k += 1;
        if k == n {
            k = 0;
        }
    }
}

/// `SimpleSpectralGate`, as `PvocBlock` has it inlined: a bin under
/// `under_max` of the frame's peak, or under `min_amp`, is silenced.
fn gate(spec: &mut [f32], n2: usize, under_max: f64, min_amp: f64) {
    let mut peak = 0.0f32;
    for k in 0..=n2 {
        let a = spec[2 * k];
        peak = if a > peak { a } else { peak };
    }
    let thresh = (peak as f64 * under_max) as f32;
    for k in 0..=n2 {
        let a = spec[2 * k];
        if thresh > a || min_amp > a as f64 {
            spec[2 * k] = 0.0;
        }
    }
}

/// `PhaseInterpolate`, the time scale proper. `scale` is I/D as
/// `FindBestRatio` returns it: a float quotient, held as a double.
///
/// A silent bin takes the last synthesis phase and leaves the analysis phase
/// where it was, so the next difference it sees spans the silence.
pub fn phase_interpolate(
    v: Version,
    spec: &mut [f32],
    last_anal: &mut [f32],
    last_synth: &mut [f32],
    n2: usize,
    scale: f64,
) {
    for k in 0..=n2 {
        if spec[2 * k] == 0.0 {
            spec[2 * k + 1] = last_synth[k];
            continue;
        }
        let ph = spec[2 * k + 1];
        let mut dphi = ph - last_anal[k];
        last_anal[k] = ph;
        let p = match v {
            Version::V0888 => {
                while dphi > PI {
                    dphi -= TWO_PI;
                }
                while dphi < -PI {
                    dphi += TWO_PI;
                }
                ((dphi as f64 * scale) as f32) + last_synth[k]
            }
            Version::V0896 => {
                let mut p = ((dphi as f64 * scale) as f32) + last_synth[k];
                while p > PI {
                    p -= TWO_PI;
                }
                while p < -PI {
                    p += TWO_PI;
                }
                p
            }
        };
        spec[2 * k + 1] = p;
        last_synth[k] = p;
    }
}

// ---------------------------------------------------------------- the hops

/// How far I/D is from s, as SoundHack measures it: the quotient in float,
/// the comparison's ratio rounded to float.
fn miss(i: usize, d: usize, s: f64) -> f64 {
    let r = (i as f32 / d as f32) as f64;
    if r > s {
        (r / s) as f32 as f64
    } else {
        (s / r) as f32 as f64
    }
}

/// `FindBestRatio`: the analysis and synthesis hops (D, I) whose ratio comes
/// nearest the scale s, with neither above Nw/8.
///
/// The hop that is not pinned at Nw/8 is stepped down from it until the
/// ratio is within tolerance. If it reaches one first, the pinned hop's
/// partner is computed outright and taken.
///
/// **0.888** (report §3.4): one tolerance, 1.01, tested before the step.
///
/// **0.896** (disassembly at 0x2cda3, which settles report §15 item 6a):
/// ```text
/// do { other = pinned·s (or pinned/s); err = …
///      if err < 1.004: break
///      if --count == 0: { stepped = Nw/8; other = …; err = 1.0 }
///      stepped -= 1
/// } while err > 1.01
/// ```
/// so a hit on the looser tolerance keeps the stepped hop: 3.0 comes out as
/// 126/42, which is exactly 3, and 1.003 as 127/127, which is no stretch.
pub fn find_best_ratio(v: Version, s: f64, nw: usize) -> (usize, usize) {
    let top = (nw as i64 / 8) as i64;
    let (d, i) = if s > 1.0 {
        // I is pinned at the top and stepped; D follows it.
        let d_of = |i: i64| ((i as f32) as f64 / s).trunc() as i64;
        match v {
            Version::V0888 => {
                let mut i = top;
                loop {
                    let d = d_of(i);
                    if miss(i as usize, d.max(0) as usize, s) < 1.01 {
                        break (d, i);
                    }
                    if i == 1 {
                        break (d_of(top), top);
                    }
                    i -= 1;
                }
            }
            Version::V0896 => {
                let (mut i, mut count) = (top, top);
                loop {
                    let mut d = d_of(i);
                    let mut err = miss(count as usize, d.max(0) as usize, s);
                    if err < 1.004 {
                        break (d, i);
                    }
                    count -= 1;
                    if count == 0 {
                        i = top;
                        d = d_of(top);
                        err = 1.0;
                    }
                    i -= 1;
                    if err <= 1.01 {
                        break (d, i);
                    }
                    count = i;
                }
            }
        }
    } else {
        let i_of = |d: i64| (d as f64 * s).trunc() as i64;
        match v {
            Version::V0888 => {
                let mut d = top;
                loop {
                    let i = i_of(d);
                    if miss(i.max(0) as usize, d as usize, s) < 1.01 {
                        break (d, i);
                    }
                    if d == 1 {
                        break (top, i_of(top));
                    }
                    d -= 1;
                }
            }
            Version::V0896 => {
                let (mut d, mut count) = (top, top);
                loop {
                    let mut i = i_of(d);
                    let mut err = miss(i.max(0) as usize, count as usize, s);
                    if err < 1.004 {
                        break (d, i);
                    }
                    count -= 1;
                    if count == 0 {
                        d = top;
                        i = i_of(top);
                        err = 1.0;
                    }
                    d -= 1;
                    if err <= 1.01 {
                        break (d, i);
                    }
                    count = d;
                }
            }
        }
    };
    (d.max(1) as usize, i.max(1) as usize)
}

/// The scale `PhaseInterpolate` is given: `FindBestRatio`'s float I/D.
fn scale_of(d: usize, i: usize) -> f64 {
    (i as f32 / d as f32) as f64
}

// ---------------------------------------------------------------- streaming

/// SoundHack's time scale, a block at a time.
///
/// All of its state is fixed in size: per channel, the window being
/// overlap-added and the two phase arrays; and the frames written but not yet
/// handed out. Nothing allocates once it is built.
pub struct SoundHackStream {
    channels: usize,
    cfg: Config,
    version: Version,
    tables: FftTables,
    wa: Vec<f32>,
    ws: Vec<f32>,
    /// The hops the windows were last scaled for.
    win_for: Option<(usize, usize)>,
    /// One channel's Nw input samples, read fresh for each block.
    inwin: Vec<f32>,
    frame: Vec<f32>,
    spec: Vec<f32>,
    /// Per channel, Nw long: the overlap-add, `ShiftOut`'s buffer.
    outbuf: Vec<f32>,
    /// Per channel, N2 + 1 long.
    last_anal: Vec<f32>,
    last_synth: Vec<f32>,
    /// Frames `ShiftOut` has written and `render` has not handed out yet.
    fifo: Vec<f32>,
    fifo_cap: usize,
    fifo_at: usize,
    fifo_len: usize,
    /// The input frame the run's time zero sits on.
    origin: i64,
    /// SoundHack's two time pointers, run-relative.
    in_t: i64,
    out_t: i64,
    d: usize,
    i: usize,
    scale: f64,
    ratio_for: f32,
    position: u64,
    max_block: usize,
}

impl SoundHackStream {
    pub fn new(max_block: usize, channels: usize) -> Self {
        Self::with_config(max_block, channels, DEFAULT_CONFIG)
    }

    pub fn with_config(max_block: usize, channels: usize, cfg: Config) -> Self {
        let channels = channels.max(1);
        let max_block = max_block.max(1);
        let (n, nw) = (cfg.n, cfg.nw);
        let bins = n / 2 + 1;
        let fifo_cap = max_block + nw / 8 + 1;
        let mut s = SoundHackStream {
            channels,
            cfg,
            version: Version::default(),
            tables: FftTables::new(),
            wa: vec![0.0; nw],
            ws: vec![0.0; nw],
            win_for: None,
            inwin: vec![0.0; nw],
            frame: vec![0.0; n],
            spec: vec![0.0; n + 2],
            outbuf: vec![0.0; channels * nw],
            last_anal: vec![0.0; channels * bins],
            last_synth: vec![0.0; channels * bins],
            fifo: vec![0.0; fifo_cap * channels],
            fifo_cap,
            fifo_at: 0,
            fifo_len: 0,
            origin: 0,
            in_t: 0,
            out_t: 0,
            d: 1,
            i: 1,
            scale: 1.0,
            ratio_for: f32::NAN,
            position: 0,
            max_block,
        };
        s.start(0, 1.0, Version::default(), None);
        s
    }

    /// The hops in force, (D, I).
    pub fn hops(&self) -> (usize, usize) {
        (self.d, self.i)
    }

    /// Choose the hops for `ratio`, and rescale the windows if they moved.
    fn retune(&mut self, ratio: f32, version: Version) {
        if ratio.to_bits() == self.ratio_for.to_bits() && version == self.version {
            return;
        }
        self.ratio_for = ratio;
        self.version = version;
        let (d, i) = find_best_ratio(version, ratio.clamp(0.01, 100.0) as f64, self.cfg.nw);
        self.d = d;
        self.i = i;
        self.scale = scale_of(d, i);
        if self.win_for != Some((d, i)) {
            let nw = self.cfg.nw;
            get_window(&mut self.wa, nw, self.cfg.window);
            get_window(&mut self.ws, nw, self.cfg.window);
            scale_windows(&mut self.wa, &mut self.ws, self.cfg.n, nw, d, i, false);
            self.win_for = Some((d, i));
        }
    }

    /// Begin a run at output frame `out_frame`, as `InitPvocProcess` begins a
    /// file. `preroll` caps the blocks run before the first one written.
    fn start(&mut self, out_frame: u64, ratio: f32, version: Version, preroll: Option<usize>) {
        self.ratio_for = f32::NAN;
        self.retune(ratio, version);
        let (d, i, nw) = (self.d as i64, self.i as i64, self.cfg.nw as i64);
        self.outbuf.fill(0.0);
        self.last_anal.fill(0.0);
        self.last_synth.fill(0.0);
        self.fifo_at = 0;
        self.fifo_len = 0;
        self.position = out_frame;
        // Output frame f of the run is SoundHack's output time f + t0, and
        // that is input time (f + t0)·D/I: so the run reads from here.
        self.origin = ((out_frame as f64) * d as f64 / i as f64).round() as i64;
        self.in_t = -nw;
        self.out_t = -((nw * i) / d);
        // Blocks before the first that `ShiftOut` writes (out + I ≥ 0 after
        // the block's own step).
        let first = (-self.out_t - 2 * i).max(0) / i + if (-self.out_t - 2 * i).max(0) % i != 0 { 1 } else { 0 };
        if let Some(cap) = preroll {
            let skip = first.saturating_sub(cap as i64);
            self.in_t += skip * d;
            self.out_t += skip * i;
        }
    }

    /// One `PvocBlock`, time-scale branch, every channel.
    fn block(&mut self, channels: usize, input: &[f32], in_frames: usize) {
        let (n, nw, n2) = (self.cfg.n, self.cfg.nw, self.cfg.n / 2);
        let (d, i) = (self.d, self.i);
        let bins = n2 + 1;
        self.in_t += d as i64;
        self.out_t += i as i64;
        for ch in 0..channels {
            // `ShiftIn`: the window of input that ends at in + Nw, zeros
            // outside the file.
            let base = self.origin + self.in_t;
            for (j, v) in self.inwin.iter_mut().enumerate() {
                let at = base + j as i64;
                *v = if at >= 0 && (at as usize) < in_frames {
                    input[at as usize * channels + ch]
                } else {
                    0.0
                };
            }
            window_fold(&self.inwin, &self.wa, &mut self.frame, self.in_t, n, nw);
            real_fft(&mut self.frame, n2, true, &self.tables);
            cart_to_polar(&self.frame, &mut self.spec, n2);
            gate(&mut self.spec, n2, self.cfg.under_max, self.cfg.min_amp);
            phase_interpolate(
                self.version,
                &mut self.spec,
                &mut self.last_anal[ch * bins..(ch + 1) * bins],
                &mut self.last_synth[ch * bins..(ch + 1) * bins],
                n2,
                self.scale,
            );
            polar_to_cart(&self.spec, &mut self.frame, n2);
            real_fft(&mut self.frame, n2, false, &self.tables);
            overlap_add(&self.frame, &self.ws, &mut self.outbuf[ch * nw..(ch + 1) * nw], self.out_t, n, nw);
        }
        // `ShiftOut`, handed the output time plus one hop.
        if self.out_t + i as i64 >= 0 {
            for f in 0..i {
                let slot = (self.fifo_at + self.fifo_len + f) % self.fifo_cap;
                for ch in 0..channels {
                    self.fifo[slot * self.channels + ch] = self.outbuf[ch * nw + f];
                }
            }
            self.fifo_len += i;
        }
        for ch in 0..channels {
            let y = &mut self.outbuf[ch * nw..(ch + 1) * nw];
            y.copy_within(i.., 0);
            y[nw - i..].fill(0.0);
        }
    }

    fn render_some(&mut self, out: &mut [f32], channels: usize, input: &[f32], p: &StretchParams) {
        let frames = out.len() / channels;
        self.retune(p.ratio, p.soundhack.version);
        let in_frames = input.len() / channels;
        while self.fifo_len < frames {
            self.block(channels, input, in_frames);
        }
        for f in 0..frames {
            let slot = (self.fifo_at + f) % self.fifo_cap;
            for ch in 0..channels {
                out[f * channels + ch] = self.fifo[slot * self.channels + ch];
            }
        }
        self.fifo_at = (self.fifo_at + frames) % self.fifo_cap;
        self.fifo_len -= frames;
        self.position += frames as u64;
    }

    /// Start from output frame `out_frame` exactly as SoundHack starts a
    /// file: every pre-roll block run. A run from frame zero is then the
    /// program's output file, sample for sample.
    pub fn seek_exact(&mut self, out_frame: u64, p: &StretchParams) {
        self.start(out_frame, p.ratio, p.soundhack.version, None);
    }
}

impl Streamer for SoundHackStream {
    fn render(&mut self, out: &mut [f32], channels: usize, input: &[f32], p: &StretchParams) {
        let channels = channels.clamp(1, self.channels);
        // In pieces no longer than the queue was sized for.
        let step = self.max_block * channels;
        for piece in out.chunks_mut(step) {
            self.render_some(piece, channels, input, p);
        }
    }

    fn seek(&mut self, out_frame: u64, _input_frames: usize, p: &StretchParams) {
        self.start(out_frame, p.ratio, p.soundhack.version, Some(LIVE_PREROLL));
    }

    fn position(&self) -> u64 {
        self.position
    }
}

// ---------------------------------------------------------------- offline

/// Stretch `input` (interleaved) by `ratio`, as SoundHack would: its output
/// file from the top, held to the length the ratio promises.
pub fn stretch_with(
    input: &[f32],
    channels: usize,
    ratio: f32,
    params: SoundHackParams,
    prog: crate::Progress,
) -> Vec<f32> {
    render_cfg(input, channels, ratio, params.version, DEFAULT_CONFIG, prog)
}

/// The same with the dialog's other settings named, for the tests.
#[cfg(test)]
pub(crate) fn render_with(input: &[f32], channels: usize, ratio: f32, v: Version, cfg: Config) -> Vec<f32> {
    render_cfg(input, channels, ratio, v, cfg, None)
}

fn render_cfg(
    input: &[f32],
    channels: usize,
    ratio: f32,
    v: Version,
    cfg: Config,
    prog: crate::Progress,
) -> Vec<f32> {
    let channels = channels.max(1);
    let in_frames = input.len() / channels;
    let ratio = ratio.clamp(0.01, 100.0);
    let want = ((in_frames as f64) * ratio as f64).round() as usize;
    if in_frames == 0 || want == 0 {
        return vec![0.0; want * channels];
    }
    let p = StretchParams {
        ratio,
        window_ms: 40.0,
        sample_rate: 44_100,
        wsola: Default::default(),
        vocoder: Default::default(),
        grain: Default::default(),
        soundhack: SoundHackParams { version: v },
    };
    const CHUNK: usize = 1 << 14;
    let mut s = SoundHackStream::with_config(CHUNK, channels, cfg);
    s.seek_exact(0, &p);
    let mut out = vec![0.0; want * channels];
    let mut at = 0usize;
    while at < want {
        let take = CHUNK.min(want - at);
        s.render(&mut out[at * channels..(at + take) * channels], channels, input, &p);
        at += take;
        if !crate::tick(prog, take as u64) {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PI32: f32 = std::f32::consts::PI;

    /// A deterministic test signal: three partials that sit between bins and
    /// a little noise, so every bin has something in it.
    fn busy(frames: usize, channels: usize) -> Vec<f32> {
        let mut seed = 0x1234_5678u32;
        let mut out = Vec::with_capacity(frames * channels);
        for f in 0..frames {
            for ch in 0..channels {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = ((seed >> 9) as f32 / (1u32 << 23) as f32 - 0.5) * 0.02;
                let t = f as f32 / 44_100.0;
                let v = 0.30 * (2.0 * PI32 * 220.7 * t).sin()
                    + 0.20 * (2.0 * PI32 * 1_317.3 * t + ch as f32).sin()
                    + 0.10 * (2.0 * PI32 * 5_003.9 * t).sin()
                    + noise;
                out.push(v);
            }
        }
        out
    }

    fn peak(v: &[f32]) -> f32 {
        v.iter().fold(0.0f32, |m, x| m.max(x.abs()))
    }

    fn max_diff(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).fold(0.0f32, |m, (x, y)| m.max((x - y).abs()))
    }

    // ------------------------------------------------------------- the windows

    /// Every window is the decoded formula, value for value, including von
    /// Hann's 0.5 − 0.4·cos: not a true Hann, and kept that way.
    #[test]
    fn the_windows_are_soundhacks_own() {
        let n = 1024usize;
        let mut w = vec![0.0f32; n];
        let x = |i: usize| ((i as f32 * std::f32::consts::TAU) / (n - 1) as f32) as f64;

        get_window(&mut w, n, Window::Hamming);
        for i in [0, 1, 100, 511, 512, 1023] {
            let want = (0.540_000_021_457_672_1 - x(i).cos() * 0.460_000_008_344_650_27) as f32;
            assert_eq!(w[i], want, "Hamming {i}");
        }
        assert!((w[0] - 0.08).abs() < 1e-6 && (w[1023] - 0.08).abs() < 1e-6);

        get_window(&mut w, n, Window::VonHann);
        assert!((w[0] - 0.1).abs() < 1e-6, "von Hann starts at 0.1, not 0: {}", w[0]);
        assert!((w[1023] - 0.1).abs() < 1e-6);
        let top = w.iter().cloned().fold(0.0f32, f32::max);
        assert!((top - 0.9).abs() < 1e-5, "von Hann peaks at 0.9, not 1: {top}");
        for i in [0, 37, 511, 900] {
            let want = (0.5 - x(i).cos() * 0.400_000_005_960_464_5) as f32;
            assert_eq!(w[i], want, "von Hann {i}");
        }

        get_window(&mut w, n, Window::Rectangle);
        assert!(w.iter().all(|v| *v == 1.0));

        get_window(&mut w, n, Window::Ramp);
        assert_eq!(w[0], 1.0);
        assert_eq!(w[512], 0.5);
        assert_eq!(w[1023], (1.0 - (1023.0f32 / 1024.0) as f64) as f32);

        get_window(&mut w, n, Window::Triangle);
        assert_eq!(w[0], 0.0);
        assert!((w[256] - 0.5).abs() < 1e-4, "{}", w[256]);
        assert!((w[512] - 1.0).abs() < 1e-2, "{}", w[512]);
        assert!(w[1023] < 0.01);

        get_window(&mut w, n, Window::Sinc);
        assert_eq!(w[512], 1.0, "the centre is set to one outright");
        // n·sin(πx/(n/2))/(2πx): at x = 256 that is n/(2π·256) = 2/π, and
        // the zeros fall on the ends.
        assert!((w[768] - 2.0 / PI32).abs() < 1e-5, "{}", w[768]);
        assert!(w[0].abs() < 1e-5, "{}", w[0]);

        get_window(&mut w, n, Window::Kaiser);
        assert_eq!(w[0], 0.0, "both ends forced to zero");
        assert_eq!(w[1023], 0.0);
        assert!((w[512] - 1.0).abs() < 1e-6, "{}", w[512]);
        assert_eq!(w[512 - 100], w[512 + 100], "built outward from the centre");
    }

    /// `ScaleWindows`: the analysis window sums to two, and the synthesis
    /// window's squares, taken one hop apart, sum to one.
    #[test]
    fn the_windows_are_scaled_as_moores_pvoc_scales_them() {
        let (n, nw, d, i) = (1024, 1024, 128, 128);
        let mut wa = vec![0.0f32; nw];
        let mut ws = vec![0.0f32; nw];
        get_window(&mut wa, nw, Window::Hamming);
        get_window(&mut ws, nw, Window::Hamming);
        scale_windows(&mut wa, &mut ws, n, nw, d, i, false);
        let sum: f32 = wa.iter().sum();
        assert!((sum - 2.0).abs() < 1e-4, "{sum}");
        // wS is divided by the sum of its own squares one hop apart (not by
        // that sum's root), which with wA the same shape makes the
        // overlap-add of wA·wS exactly one at the hop's own phase.
        let ola: f32 = (0..nw).step_by(i).map(|k| wa[k] * ws[k]).sum();
        assert!((ola - 1.0).abs() < 1e-5, "{ola}");
    }

    // ------------------------------------------------------------- the FFT

    /// NR `four1`/`realft`, e^{+i} forward, packed with Nyquist in x[1]; the
    /// round trip is the identity, and `CartToPolar`'s −atan2 turns the
    /// e^{+i} phases back into the usual ones.
    #[test]
    fn the_real_fft_is_numerical_recipes() {
        let t = FftTables::new();
        let n = 64usize;
        let x: Vec<f32> = (0..n).map(|i| ((i * 7 + 3) % 11) as f32 / 11.0 - 0.4).collect();
        let mut y = x.clone();
        real_fft(&mut y, n / 2, true, &t);
        for k in 0..=n / 2 {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (j, v) in x.iter().enumerate() {
                let a = 2.0 * std::f64::consts::PI * (j * k) as f64 / n as f64;
                re += *v as f64 * a.cos();
                im += *v as f64 * a.sin();
            }
            let (gr, gi) = match k {
                0 => (y[0], 0.0),
                k if k == n / 2 => (y[1], 0.0),
                k => (y[2 * k], y[2 * k + 1]),
            };
            assert!((gr as f64 - re).abs() < 1e-4, "re {k}: {gr} vs {re}");
            assert!((gi as f64 - im).abs() < 1e-4, "im {k}: {gi} vs {im}");
        }
        real_fft(&mut y, n / 2, false, &t);
        assert!(max_diff(&x, &y) < 1e-5);

        // A cosine at bin 5 with phase 0.7 reads back as 0.7.
        let z: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * 5.0 * i as f64 / n as f64 + 0.7).cos() as f32)
            .collect();
        let mut zz = z.clone();
        real_fft(&mut zz, n / 2, true, &t);
        let mut spec = vec![0.0f32; n + 2];
        cart_to_polar(&zz, &mut spec, n / 2);
        assert!((spec[11] - 0.7).abs() < 1e-4, "{}", spec[11]);
        assert!((spec[10] - n as f32 / 2.0).abs() < 1e-3, "{}", spec[10]);
    }

    // ------------------------------------------------------------- the hop choice

    /// `FindBestRatio`, both versions. The pairs are (D, I), worked out from
    /// the 0.896 disassembly and the 0.888 decode by an independent model
    /// (`soundhack-pv/re/scripts/fbr.py`).
    ///
    /// 0.896 tests 1.004 first; on a miss it steps the hop and *then* tests
    /// 1.01, so a 1.01 hit keeps the stepped hop. That is why 3.0 lands on
    /// 126/42 (exactly 3) there and on 127/42 in 0.888, and why 1.003 is no
    /// stretch at all in 0.896.
    #[test]
    fn the_hops_are_the_ones_find_best_ratio_picks() {
        let nw = 1024;
        let cases: [(f64, (usize, usize), (usize, usize)); 14] = [
            (1.0, (128, 128), (128, 128)),
            (2.0, (64, 128), (64, 128)),
            (4.0, (32, 128), (32, 128)),
            (8.0, (16, 128), (16, 128)),
            (3.0, (42, 127), (42, 126)),
            (1.5, (85, 128), (85, 128)),
            (2.37, (54, 128), (54, 128)),
            (0.5, (128, 64), (128, 64)),
            (0.25, (128, 32), (128, 32)),
            (0.7, (128, 89), (127, 89)),
            (0.333, (127, 42), (126, 42)),
            (1.003, (127, 128), (127, 127)),
            (0.01, (101, 1), (100, 1)),
            (100.0, (1, 101), (1, 100)),
        ];
        for (s, old, new) in cases {
            assert_eq!(find_best_ratio(Version::V0888, s, nw), old, "0.888 at {s}");
            assert_eq!(find_best_ratio(Version::V0896, s, nw), new, "0.896 at {s}");
        }
        // The largest hop is Nw/8, whatever Nw is.
        assert_eq!(find_best_ratio(Version::V0896, 1.0, 4096), (512, 512));
    }

    // ------------------------------------------------------------- the phase

    /// One synthetic bin, three frames. 0.888 wraps the raw difference and
    /// then scales it, and never wraps what it accumulates; 0.896 scales the
    /// raw difference and wraps the sum. Neither takes out the bin-centre
    /// advance first. A silent bin holds its phase and does not move the
    /// analysis phase on.
    #[test]
    fn the_phase_wraps_where_each_version_wraps_it() {
        let run = |v: Version| {
            let mut last_anal = [3.0f32];
            let mut last_synth = [0.0f32];
            let mut out = Vec::new();
            for ph in [-3.0f32, 3.0, -3.0] {
                let mut spec = [1.0f32, ph];
                phase_interpolate(v, &mut spec, &mut last_anal, &mut last_synth, 0, 1.5);
                out.push(spec[1]);
            }
            // A silent frame: phase held, analysis phase untouched.
            let mut spec = [0.0f32, 2.0];
            phase_interpolate(v, &mut spec, &mut last_anal, &mut last_synth, 0, 1.5);
            out.push(spec[1]);
            (out, last_anal[0])
        };
        let tau = std::f32::consts::TAU;
        let (old, old_anal) = run(Version::V0888);
        // dφ = −6 wraps to −6 + 2π = 0.2832, ×1.5, and keeps adding.
        let step = (-6.0f32 + tau) as f64 * 1.5;
        assert!((old[0] as f64 - step).abs() < 1e-5, "{old:?}");
        let back = (6.0f32 - tau) as f64 * 1.5;
        assert!((old[1] as f64 - (step + back)).abs() < 1e-5, "{old:?}");
        assert!((old[2] as f64 - (2.0 * step + back)).abs() < 1e-5, "{old:?}");
        assert_eq!(old[3], old[2], "a silent bin holds its phase");
        assert_eq!(old_anal, -3.0, "and does not update the analysis phase");

        let (new, new_anal) = run(Version::V0896);
        // −6 × 1.5 = −9, then wrapped: −9 + 2π = −2.7168.
        assert!((new[0] - (-9.0 + tau)).abs() < 1e-5, "{new:?}");
        for p in &new {
            assert!(p.abs() <= PI32 + 1e-6, "0.896 keeps the sum inside ±π: {new:?}");
        }
        assert_eq!(new[3], new[2]);
        assert_eq!(new_anal, -3.0);

        // The two differ by 2π·0.5 = π mod 2π on the first frame.
        let gap = (old[0] - new[0]).rem_euclid(tau);
        assert!((gap - PI32).abs() < 1e-4, "{gap}");

        // 0.888's sum leaves ±π and is never brought back.
        let mut last_anal = [0.0f32];
        let mut last_synth = [0.0f32];
        for _ in 0..10 {
            let mut spec = [1.0f32, 3.0];
            last_anal[0] = 0.0;
            phase_interpolate(Version::V0888, &mut spec, &mut last_anal, &mut last_synth, 0, 1.5);
        }
        assert!((last_synth[0] - 45.0).abs() < 1e-3, "{}", last_synth[0]);
    }

    // ------------------------------------------------------------- the whole

    /// At a ratio of one SoundHack is the input again, one synthesis hop late
    /// (the first block it writes starts I samples before time zero), times
    /// the overlap's ripple. Without the gate the ripple is all there is.
    #[test]
    fn unity_is_the_input_one_hop_late() {
        let x = busy(30_000, 1);
        let ungated = Config { under_max: 0.0, min_amp: 0.0, ..DEFAULT_CONFIG };
        for v in [Version::V0888, Version::V0896] {
            let y = render_with(&x, 1, 1.0, v, ungated);
            assert_eq!(y.len(), x.len());
            let lag = 128;
            // Skip the first window, where the output is still fading in.
            let (a, b) = (&y[2048..28_000], &x[2048 - lag..28_000 - lag]);
            let err = max_diff(a, b) / peak(b);
            assert!(err < 2e-3, "{v:?}: {err}");
            // And nothing before the lag but the fade-in of a zero history.
            assert!(peak(&y[..lag]) < 1e-6, "{}", peak(&y[..lag]));
        }
        // The gate at its SoundHack defaults takes only what is 60 dB down.
        let y = render_with(&x, 1, 1.0, Version::V0896, DEFAULT_CONFIG);
        let (a, b) = (&y[2048..28_000], &x[2048 - 128..28_000 - 128]);
        assert!(max_diff(a, b) / peak(b) < 5e-2, "{}", max_diff(a, b) / peak(b));
    }

    /// The two versions agree where the ratio is a whole number and both pick
    /// the same hops: wrapping before or after a whole-number scale is the
    /// same phase mod 2π, and only float rounding of 0.888's ever-growing sum
    /// tells them apart. Elsewhere they are two different sounds.
    #[test]
    fn the_versions_agree_at_whole_ratios_and_part_elsewhere() {
        let x = busy(20_000, 2);
        for r in [1.0f32, 2.0, 4.0] {
            let a = stretch_with(&x, 2, r, SoundHackParams { version: Version::V0888 }, None);
            let b = stretch_with(&x, 2, r, SoundHackParams { version: Version::V0896 }, None);
            let d = max_diff(&a, &b) / peak(&b);
            assert!(d < 3e-3, "{r}: {d}");
        }
        for r in [1.5f32, 2.37, 0.7] {
            let a = stretch_with(&x, 2, r, SoundHackParams { version: Version::V0888 }, None);
            let b = stretch_with(&x, 2, r, SoundHackParams { version: Version::V0896 }, None);
            let d = max_diff(&a, &b) / peak(&b);
            assert!(d > 0.05, "{r}: {d}");
        }
    }

    /// The length is the ratio's, and a stretch is a stretch.
    #[test]
    fn the_output_is_as_long_as_the_ratio_says() {
        let x = busy(10_000, 2);
        let y = stretch_with(&x, 2, 2.0, SoundHackParams::default(), None);
        assert_eq!(y.len(), 2 * 20_000);
        assert!(peak(&y[10_000..30_000]) > 0.1);
    }

    /// SoundHack's time pointers mark where a window *starts*, and the
    /// output pointer is the input one times s. Scaling the accumulated phase
    /// by s therefore scales the window's own centre offset too, and each
    /// resynthesised grain lands K = (s − 1)·(Nw − 1)/2 samples off the
    /// synthesis window, circularly within the frame.
    ///
    /// For a tone on a bin centre (where the circular shift costs nothing
    /// else) that predicts its level outright: the Hamming window's
    /// autocorrelation at lag K over its value at zero,
    /// (a² + b²/2·cos 2πK/N) / (a² + b²/2) with a = 0.54, b = 0.46. So 0.73 at
    /// 1.5× and 0.5× (K ≈ ±256), 0.47 at 2× and 4× (K ≈ 512 and 1534 ≡ 510),
    /// and 1 at 1×. Measured with 0.888, which a fractional scale does not
    /// otherwise disturb.
    #[test]
    fn the_grain_lands_where_soundhacks_pointers_put_it() {
        let sr = 48_000.0f64;
        let f = 5.0 * sr / 1024.0;
        let x: Vec<f32> =
            (0..96_000).map(|i| ((2.0 * std::f64::consts::PI * f * i as f64 / sr).sin() * 0.5) as f32).collect();
        let cfg = Config { under_max: 0.0, min_amp: 0.0, ..DEFAULT_CONFIG };
        let level = |r: f32| {
            let y = render_with(&x, 1, r, Version::V0888, cfg);
            let mid = &y[y.len() / 4..y.len() * 3 / 4];
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, s) in mid.iter().enumerate() {
                let p = 2.0 * std::f64::consts::PI * f * i as f64 / sr;
                re += *s as f64 * p.cos();
                im += *s as f64 * p.sin();
            }
            2.0 * (re * re + im * im).sqrt() / mid.len() as f64 / 0.5
        };
        let predicted = |r: f64| {
            let (d, i) = find_best_ratio(Version::V0888, r, 1024);
            let s = i as f64 / d as f64;
            let k = (s - 1.0) * 1023.0 / 2.0;
            let (a2, b2) = (0.54f64 * 0.54, 0.46f64 * 0.46 / 2.0);
            (a2 + b2 * (2.0 * std::f64::consts::PI * k / 1024.0).cos()) / (a2 + b2)
        };
        for r in [1.0f32, 1.5, 0.5, 2.0, 4.0] {
            let (got, want) = (level(r), predicted(r as f64));
            assert!((got - want).abs() < 0.02, "{r}: {got} against {want}");
        }
    }

    /// Streaming in blocks of any size is the offline render, sample for
    /// sample.
    #[test]
    fn the_block_size_does_not_change_the_sound() {
        let x = busy(12_000, 2);
        let p = StretchParams { ratio: 1.7, ..params() };
        let whole = stretch_with(&x, 2, 1.7, SoundHackParams::default(), None);
        for block in [64usize, 333, 1024] {
            let mut s = SoundHackStream::new(1024, 2);
            s.seek(0, 12_000, &p);
            let mut got = vec![0.0f32; whole.len()];
            let mut at = 0;
            while at < whole.len() / 2 {
                let n = block.min(whole.len() / 2 - at);
                s.render(&mut got[at * 2..(at + n) * 2], 2, &x, &p);
                at += n;
            }
            assert_eq!(got, whole, "block {block}");
            assert_eq!(s.position(), (whole.len() / 2) as u64);
        }
    }

    /// A seek mid-file lands on the material a straight run reaches there.
    #[test]
    fn a_seek_lands_where_a_straight_run_is() {
        let x = busy(40_000, 1);
        let p = StretchParams { ratio: 2.0, ..params() };
        let whole = stretch_with(&x, 1, 2.0, SoundHackParams::default(), None);
        let mut s = SoundHackStream::new(512, 1);
        s.seek(40_000, 40_000, &p);
        let mut got = vec![0.0f32; 4096];
        for c in got.chunks_mut(512) {
            s.render(c, 1, &x, &p);
        }
        // Past the first window the seeked run is the straight one's sound;
        // only its phases started somewhere else, so compare envelopes.
        let env = |v: &[f32]| v.chunks(256).map(|c| c.iter().map(|s| s * s).sum::<f32>().sqrt()).collect::<Vec<_>>();
        let (a, b) = (env(&got[1024..]), env(&whole[41_024..44_096]));
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 0.15 * y.max(0.1), "{a:?} vs {b:?}");
        }
    }

    /// Silence in, silence out, and nothing allocated or panicking at the
    /// edges of the ratio's range.
    #[test]
    fn the_edges_of_the_range_hold() {
        let x = vec![0.0f32; 8_000];
        let y = stretch_with(&x, 1, 3.0, SoundHackParams::default(), None);
        assert!(peak(&y) == 0.0);
        let x = busy(4_000, 1);
        for r in [0.01f32, 100.0] {
            let y = stretch_with(&x, 1, r, SoundHackParams::default(), None);
            assert_eq!(y.len(), (4_000.0 * r as f64).round() as usize);
            assert!(y.iter().all(|v| v.is_finite()));
        }
    }

    #[test]
    fn the_version_has_a_name() {
        assert_eq!(Version::V0888.as_str(), "0.888");
        assert_eq!(Version::V0896.as_str(), "0.896");
        assert_eq!(Version::from_str("0.888"), Some(Version::V0888));
        assert_eq!(Version::from_str("0.896"), Some(Version::V0896));
        assert_eq!(Version::from_str("1.0"), None);
        assert_eq!(Version::default(), Version::V0896);
    }

    /// The two versions part company on any partial off its bin's centre at
    /// a scale that is not a whole number. Such a partial's measured phase
    /// drifts through ±π; 0.888 wraps the difference first, so the drift is
    /// all it scales, while 0.896 scales the raw difference, so every pass
    /// through ±π scales a 2π jump into 2π·s — π at 1.5× — and the partial
    /// flips sign and cancels itself. A tone on a bin centre never drifts, and
    /// both keep it. Measured on a steady sine at 1.5× (Goertzel, at the
    /// tone): 0.888 about 0.35 of 0.5 on and off the centre; 0.896 the same on
    /// it and 30 dB and more down off it.
    #[test]
    fn at_a_fractional_ratio_0896_loses_a_partial_off_its_bin_centre() {
        let sr = 48_000.0f64;
        let level = |bin: f64, v: Version| {
            let f = bin * sr / 1024.0;
            let x: Vec<f32> =
                (0..96_000).map(|i| ((2.0 * std::f64::consts::PI * f * i as f64 / sr).sin() * 0.5) as f32).collect();
            let y = stretch_with(&x, 1, 1.5, SoundHackParams { version: v }, None);
            let mid = &y[y.len() / 4..y.len() * 3 / 4];
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, s) in mid.iter().enumerate() {
                let p = 2.0 * std::f64::consts::PI * f * i as f64 / sr;
                re += *s as f64 * p.cos();
                im += *s as f64 * p.sin();
            }
            (2.0 * (re * re + im * im).sqrt() / mid.len() as f64) as f32
        };
        for bin in [5.0, 20.0] {
            assert!(level(bin, Version::V0888) > 0.3, "{bin}");
            assert!(level(bin, Version::V0896) > 0.3, "{bin}");
        }
        for bin in [4.5, 4.693, 20.5] {
            assert!(level(bin, Version::V0888) > 0.3, "{bin}");
            assert!(level(bin, Version::V0896) < 0.02, "{bin}: {}", level(bin, Version::V0896));
        }
    }

    fn params() -> StretchParams {
        StretchParams {
            ratio: 1.0,
            window_ms: 40.0,
            sample_rate: 44_100,
            wsola: Default::default(),
            vocoder: Default::default(),
            grain: Default::default(),
            soundhack: SoundHackParams::default(),
        }
    }
}

/// The engine as the host sees it: by name, in an export, and on the audio
/// thread.
#[cfg(test)]
mod host {
    use super::*;
    use crate::stretch::{Algorithm, Stretch};
    use crate::{Source, StreamParams, Stretcher};

    fn busy(frames: usize) -> Vec<f32> {
        (0..frames * 2)
            .map(|i| {
                let t = (i / 2) as f32 / 44_100.0;
                let c = (i % 2) as f32;
                0.4 * (2.0 * std::f32::consts::PI * 311.1 * t + c).sin()
                    + 0.2 * (2.0 * std::f32::consts::PI * 2_017.0 * t).sin()
            })
            .collect()
    }

    fn spec(ratio: f32, version: Version) -> Stretch {
        Stretch {
            ratio,
            algorithm: Algorithm::SoundHack,
            soundhack: SoundHackParams { version },
            ..Default::default()
        }
    }

    #[test]
    fn the_engine_has_a_name() {
        assert_eq!(Algorithm::SoundHack.as_str(), "soundhack");
        assert_eq!(Algorithm::from_str("soundhack"), Some(Algorithm::SoundHack));
    }

    /// Choosing it is an edit, as choosing the grain cloud is: even at 1×
    /// SoundHack gates, lags a hop and ripples, so an export must run it.
    #[test]
    fn it_is_never_an_identity() {
        let s = spec(1.0, Version::V0896);
        assert!(!s.is_identity());
        let x = busy(8_000);
        assert_ne!(s.process(&x, 2, 44_100), x);
    }

    /// An export is this engine's render, and the version reaches it.
    #[test]
    fn the_export_is_the_engine() {
        let x = busy(12_000);
        for v in [Version::V0888, Version::V0896] {
            let s = spec(1.5, v);
            assert_eq!(s.process(&x, 2, 44_100), stretch_with(&x, 2, 1.5, s.soundhack, None));
        }
        assert_ne!(
            spec(1.5, Version::V0888).process(&x, 2, 44_100),
            spec(1.5, Version::V0896).process(&x, 2, 44_100)
        );
    }

    /// What the transport plays is what the export writes, both versions,
    /// and the version can be changed under a running transport.
    #[test]
    fn playback_is_the_export() {
        let x = busy(16_000);
        let src = Source { samples: x.clone(), channels: 2 };
        for v in [Version::V0888, Version::V0896] {
            let want = spec(2.0, v).process(&x, 2, 44_100);
            let mut sp = StreamParams::new(16_000, 44_100);
            sp.algorithm = Algorithm::SoundHack;
            sp.ratio = 2.0;
            sp.soundhack.version = v;
            let mut st = Stretcher::new(512, 2, 44_100);
            st.seek(0, &sp);
            let mut got = vec![0.0f32; want.len()];
            for c in got.chunks_mut(512 * 2) {
                st.render(c, 2, &src, &sp, &mut []);
            }
            assert_eq!(got, want, "{v:?}");
        }
    }

    /// A voice for the Grain page's keys can be built on it, and pitched.
    #[test]
    fn a_voice_plays_it_pitched() {
        let x = busy(16_000);
        let src = Source { samples: x, channels: 2 };
        let mut sp = StreamParams::new(16_000, 44_100);
        sp.algorithm = Algorithm::SoundHack;
        sp.semitones = 7.0;
        let mut v = Stretcher::voice(Algorithm::SoundHack, 1, 512, 2, 44_100);
        v.seek(0, &sp);
        let mut out = vec![0.0f32; 512 * 2 * 20];
        for c in out.chunks_mut(512 * 2) {
            v.render(c, 2, &src, &sp, &mut []);
        }
        assert!(out.iter().all(|s| s.is_finite()));
        assert!(out[4096..].iter().fold(0.0f32, |m, s| m.max(s.abs())) > 0.1);
    }
}
