//! The Network family: Valhalla Supermassive 1.3.0's twelve networks, as
//! decoded and measured.
//!
//! From `decoded/valhalla-supermassive.md` (the static decode, with its
//! errata) and `sources/reports/001-valhalla-measured.md` (Reaper renders of
//! the original against every claim). One engine of sixteen modulated delay
//! lines; each mode wires them into its own network (§10), and each is its own
//! kernel here so a document names the mode it is.
//!
//! As coded, including what a redesign would not do (SUITE-PLAN D10):
//! - **32-sample blocks.** The network runs once per 32 samples on 32-sample
//!   vectors, so the wet path lags the dry by 32 samples, uncompensated and
//!   reported as no latency (§2.1, §3).
//! - **Delay law** d₀ = max(D, M), dₖ = max(dₖ₋₁·r_mode, M), r = 1 −
//!   (0.007071·Spacing)³; lines read at d+m+1 when written first, |d+m|+33 when
//!   read first, and loops of +33 or +65 (§6–7).
//! - **First-order all-pass interpolation** from the table (1 − i/256)/(1 +
//!   i/256) clamped at 0.99, the read delay ramped across each block (§6).
//! - **Smoothing in float32** once a block: Delay, Spacing and ModDepth at
//!   300 ms; the LFO's sine and cosine at 0.25 Hz (§4–5).
//! - Feedback, Density, Width, Mix and the filters jump (§4). Low Cut is the
//!   unnormalised first-order high-pass with 6.28318 for 2π (§9).
//!
//! **What is ours.** Tempo sync is not here: a kernel is not given the tempo,
//! so Delay is in milliseconds only. The delay smoother starts at its target —
//! the original's start depends on the order its host set the parameters
//! (MEAS S2). The blow-up mute after the engine is left out: its threshold was
//! not traced (§2.2). The tiny alternating offset the original adds against
//! denormals is not added (§2.1 says to ignore it).

use crate::spec::Role;
use crate::{Category, Descriptor, Group, Kernel, PadPair, Panel, ParamSpec};

const BLOCK: usize = 32;
type V = [f32; BLOCK];

const MIX: usize = 0;
const DELAY: usize = 1;
const SPACING: usize = 2;
const FEEDBACK: usize = 3;
const DENSITY: usize = 4;
const WIDTH: usize = 5;
const LOW_CUT: usize = 6;
const HIGH_CUT: usize = 7;
const MOD_RATE: usize = 8;
const MOD_DEPTH: usize = 9;
const N_PARAMS: usize = 10;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("mix", "Mix", 0.0, 100.0, 50.0).unit("%").smooth(0.0).role(Role::Output),
    ParamSpec::new("delayMs", "Delay", 0.0, 2000.0, 300.0).unit("ms").smooth(0.0).role(Role::Time),
    ParamSpec::new("spacing", "Spacing", 0.0, 100.0, 0.0).unit("%").smooth(0.0).role(Role::Space),
    ParamSpec::new("feedback", "Feedback", 0.0, 100.0, 50.0).unit("%").smooth(0.0).role(Role::Time),
    ParamSpec::new("density", "Density", 0.0, 100.0, 0.0).unit("%").smooth(0.0).role(Role::Space),
    ParamSpec::new("width", "Width", -100.0, 100.0, 100.0).unit("%").smooth(0.0).role(Role::Stereo),
    ParamSpec::new("lowCutHz", "Low cut", 10.0, 2000.0, 10.0).log().unit("Hz").smooth(0.0).role(Role::Tone),
    ParamSpec::new("highCutHz", "High cut", 200.0, 20_000.0, 20_000.0).log().unit("Hz").smooth(0.0).role(Role::Tone),
    ParamSpec::new("modRateHz", "Mod rate", 0.01, 10.0, 0.5).log().unit("Hz").smooth(0.0).role(Role::Motion),
    ParamSpec::new("modDepth", "Mod depth", 0.0, 100.0, 50.0).unit("%").smooth(0.0).role(Role::Motion),
];

const PANEL: Panel = Panel {
    groups: &[
        Group { label: "Time", keys: &["delayMs", "feedback"] },
        Group { label: "Space", keys: &["density", "spacing", "width"] },
        Group { label: "Tone", keys: &["lowCutHz", "highCutHz"] },
        Group { label: "Motion", keys: &["modRateHz", "modDepth"] },
        Group { label: "Output", keys: &["mix"] },
    ],
    primary: &["delayMs", "feedback", "density"],
    displays: &[],
    pad: Some(PadPair { x: "density", x_range: (0.0, 100.0), y: "feedback", y_range: (0.0, 100.0) }),
};

macro_rules! descriptor {
    ($id:expr, $code:expr, $label:expr, $orig:expr, $sec:expr) => {
        Descriptor {
            id: $id,
            code: $code,
            version: 1,
            label: $label,
            category: Category::Reverb,
            params: PARAMS,
            panel: PANEL,
            source: concat!(
                "Valhalla Supermassive 1.3.0, mode \"",
                $orig,
                "\": decoded/valhalla-supermassive.md ",
                $sec,
                "; measured against the original (sources/reports/001-valhalla-measured.md S3, S5)"
            ),
            sound: None,
        }
    };
}

/// The twelve, in the original's index order (§10, MEAS S4).
pub static DESCRIPTORS: [Descriptor; 12] = [
    descriptor!("sm-gemini", 105, "Mesh 16", "Gemini", "§10.1"),
    descriptor!("sm-hydra", 106, "Mesh 8 + Diffuser", "Hydra", "§10.2"),
    descriptor!("sm-centaurus", 107, "Quad Cascade", "Centaurus", "§10.3"),
    descriptor!("sm-sagittarius", 108, "Stereo Cascade", "Sagittarius", "§10.4"),
    descriptor!("sm-great-annihilator", 109, "Quad Loop + Diffusers", "Great Annihilator", "§10.5"),
    descriptor!("sm-andromeda", 110, "Echo Loop + Diffusers", "Andromeda", "§10.6"),
    descriptor!("sm-lyra", 111, "Mesh 4", "Lyra", "§10.7"),
    descriptor!("sm-capricorn", 112, "Mesh 8", "Capricorn", "§10.8"),
    descriptor!("sm-triangulum", 113, "Ping-Pong Diffuse", "Triangulum", "§10.9"),
    descriptor!("sm-lmc", 114, "Householder Loop", "Large Magellanic Cloud", "§10.10"),
    descriptor!("sm-cirrus-major", 115, "Filtered Mesh + Diffuser", "Cirrus Major", "§10.11"),
    descriptor!("sm-cirrus-minor", 116, "Filtered Mesh 8", "Cirrus Minor", "§10.12"),
];

const GEMINI: usize = 0;
const HYDRA: usize = 1;
const CENTAURUS: usize = 2;
const SAGITTARIUS: usize = 3;
const GREAT_ANNIHILATOR: usize = 4;
const ANDROMEDA: usize = 5;
const LYRA: usize = 6;
const CAPRICORN: usize = 7;
const TRIANGULUM: usize = 8;
const LMC: usize = 9;
const CIRRUS_MAJOR: usize = 10;
const CIRRUS_MINOR: usize = 11;

/// How many line objects a mode uses (§11).
const fn lines(mode: usize) -> usize {
    match mode {
        LYRA => 4,
        CAPRICORN | CIRRUS_MINOR => 8,
        _ => 16,
    }
}

/// The Cirrus permutation π (§10.11–12).
const PI: [usize; 8] = [7, 6, 1, 0, 3, 2, 5, 4];

/// §6: the interpolator's coefficient table, as the line constructor computes it.
fn interp_table() -> [f32; 256] {
    core::array::from_fn(|i| {
        let x = i as f32 / 256.0;
        let t = (1.0 - x) / (1.0 + x);
        if 0.99f32 <= t { 0.99 } else { t }
    })
}

/// §5: the LFO's sine table, 257 entries with a guard.
fn lfo_table() -> [f32; 257] {
    core::array::from_fn(|j| (std::f64::consts::TAU * j as f64 / 256.0).sin() as f32)
}

/// One modulated delay line (§6): a power-of-two ring, written at absolute
/// sample times, read through the first-order all-pass interpolator with the
/// read delay ramped across the block.
struct Line {
    buf: Vec<f32>,
    mask: usize,
    prev: f32,
    started: bool,
    y1: f32,
}

impl Line {
    fn new(room: usize) -> Line {
        let n = room.next_power_of_two();
        Line { buf: vec![0.0; n], mask: n - 1, prev: 0.0, started: false, y1: 0.0 }
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
        self.prev = 0.0;
        self.started = false;
        self.y1 = 0.0;
    }

    #[inline]
    fn write(&mut self, t0: usize, w: &V) {
        for (k, &x) in w.iter().enumerate() {
            self.buf[(t0 + k) & self.mask] = x;
        }
    }

    /// The block's 32 reads, ending at delay `arg` (samples back from each
    /// sample's own time).
    #[inline]
    fn read(&mut self, t0: usize, arg: f32, table: &[f32; 256]) -> V {
        let mut cur = if self.started { self.prev } else { arg };
        let inc = (arg - cur) * (1.0 / 32.0);
        let mut out = [0.0; BLOCK];
        for (k, o) in out.iter_mut().enumerate() {
            cur += inc;
            let d = cur.max(0.0);
            let di = d as usize;
            let f = d - di as f32;
            let c = table[((f * 256.0) as usize).min(255)];
            let t = t0 + k;
            let x0 = self.buf[t.wrapping_sub(di) & self.mask];
            let x1 = self.buf[t.wrapping_sub(di + 1) & self.mask];
            let y = c * x0 + x1 - c * self.y1;
            self.y1 = y;
            *o = y;
        }
        self.prev = arg;
        self.started = true;
        out
    }
}

/// §9's unnormalised first-order high-pass, `y = x − x₁ − a·y₁`.
#[derive(Clone, Copy, Default)]
struct LowCut {
    x1: f32,
    y1: f32,
}

impl LowCut {
    #[inline]
    fn tick(&mut self, x: f32, a: f32) -> f32 {
        let y = x - self.x1 - a * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

/// §9's one-pole low-pass, `y = k·x + (1−k)·y₁`.
#[derive(Clone, Copy, Default)]
struct HighCut {
    y1: f32,
}

impl HighCut {
    #[inline]
    fn tick(&mut self, x: f32, k: f32) -> f32 {
        let y = k * x + (1.0 - k) * self.y1;
        self.y1 = y;
        y
    }
}

/// A rotation by the Density angle on a pair of vectors (§8):
/// p' = cos a·p − sin a·q, q' = sin a·p + cos a·q.
#[inline]
fn rot(p: &mut V, q: &mut V, sa: f32, ca: f32) {
    for n in 0..BLOCK {
        let (a, b) = (p[n], q[n]);
        p[n] = ca * a - sa * b;
        q[n] = sa * a + ca * b;
    }
}

/// A butterfly over `v.len()` channels: one stage per stride, pairs (j, j+s).
#[inline]
fn butterfly(v: &mut [V], strides: &[usize], sa: f32, ca: f32) {
    for &s in strides {
        for j in 0..v.len() {
            if j & s == 0 {
                let (mut p, mut q) = (v[j], v[j + s]);
                rot(&mut p, &mut q, sa, ca);
                v[j] = p;
                v[j + s] = q;
            }
        }
    }
}

pub struct Supermassive<const MODE: usize> {
    sr: f32,
    values: [f32; N_PARAMS],
    table: [f32; 256],
    lfo: [f32; 257],
    /// sin and cos of j·π/8, j = 0..15, as the phase tables give them (§5).
    phase_sin: [f32; 16],
    phase_cos: [f32; 16],
    lines: Vec<Line>,
    t: usize,
    pos: usize,
    inbuf: [V; 2],
    wetbuf: [V; 2],
    /// The previous block's feedback state, per lane.
    fb: [V; 16],
    // smoothers (§4)
    c_smooth: f32,
    c_lfo: f32,
    d_s: f32,
    r_s: f32,
    m_s: f32,
    s_s: f32,
    co_s: f32,
    phase: u32,
    inc: u32,
    // derived from the controls (§3)
    f_lin: f32,
    f_sqrt: f32,
    g: f32,
    g_d: f32,
    sa: f32,
    ca: f32,
    w_sin: f32,
    w_cos: f32,
    lc_a: f32,
    hc_k: f32,
    wet: f32,
    dry: f32,
    // filters: post (0, 1) or in the loop (per lane)
    lc: [LowCut; 8],
    hc: [HighCut; 8],
}

impl<const MODE: usize> Supermassive<MODE> {
    fn d_target(&self) -> f32 {
        self.values[DELAY] * self.sr / 1000.0
    }

    fn r_target(&self) -> f32 {
        let x = 0.007071 * self.values[SPACING];
        1.0 - x * x * x
    }

    fn m_target(&self) -> f32 {
        let v = self.values[MOD_DEPTH] / 100.0;
        (self.sr / 48_000.0) * (v * v) * 500.0
    }

    /// The control laws of §3 that are not smoothed.
    fn laws(&mut self) {
        let v = &self.values;
        let f = v[FEEDBACK] / 100.0;
        self.f_lin = f * 0.99999988 + 1e-7;
        self.f_sqrt = f.sqrt() * 0.99999988 + 1e-7;
        self.g = 0.0091 * v[FEEDBACK] + 1e-7;
        self.g_d = 0.0091 * v[DENSITY] + 1e-7;
        let a = v[DENSITY] * std::f32::consts::PI / 400.0;
        (self.sa, self.ca) = (a.sin(), a.cos());
        let phi = (v[WIDTH] / 100.0 * 0.5 + 0.5) * std::f32::consts::FRAC_PI_2;
        (self.w_sin, self.w_cos) = (phi.sin(), phi.cos());
        let fc = v[LOW_CUT].max(5.0);
        let k = (2.0 * self.sr) * ((1.0 / (fc as f64 * 6.28318)) as f32);
        self.lc_a = (1.0 - k) / (1.0 + k);
        self.hc_k = 1.0 - (-(std::f64::consts::TAU * v[HIGH_CUT] as f64 / self.sr as f64)).exp() as f32;
        let theta = v[MIX] / 100.0 * std::f32::consts::FRAC_PI_2;
        (self.wet, self.dry) = (theta.sin(), theta.cos());
        self.inc = (v[MOD_RATE] as f64 * 256.0 / (self.sr as f64 / 32.0) * 16_777_216.0) as u32;
    }

    /// The modulation for phase j·22.5° (§5): M·sin(θ + j·π/8).
    #[inline]
    fn m_at(&self, ms: f32, mc: f32, j: usize) -> f32 {
        ms * self.phase_cos[j] + mc * self.phase_sin[j]
    }

    /// One 32-sample block of the network (§10), from `inbuf` into `wetbuf`.
    fn block(&mut self) {
        // §4: the smoothers, once a block.
        self.d_s += self.c_smooth * (self.d_target() - self.d_s);
        self.r_s += self.c_smooth * (self.r_target() - self.r_s);
        self.m_s += self.c_smooth * (self.m_target() - self.m_s);
        // §5: the LFO, read, smoothed at 0.25 Hz, then advanced.
        let idx = (self.phase >> 24) as usize;
        let frac = (self.phase & 0x00ff_ffff) as f32 * (1.0 / 16_777_216.0);
        let s = self.lfo[idx] + (self.lfo[idx + 1] - self.lfo[idx]) * frac;
        let j = (idx + 64) & 255;
        let c = self.lfo[j] + (self.lfo[j + 1] - self.lfo[j]) * frac;
        self.s_s += self.c_lfo * (s - self.s_s);
        self.co_s += self.c_lfo * (c - self.co_s);
        self.phase = self.phase.wrapping_add(self.inc);

        // §7: the length series.
        let (big_d, m) = (self.d_s, self.m_s);
        let r = match MODE {
            LYRA => {
                let r2 = self.r_s * self.r_s;
                r2 * r2
            }
            CAPRICORN | CIRRUS_MINOR => self.r_s * self.r_s,
            _ => self.r_s,
        };
        let mut d = [0.0f32; 16];
        d[0] = big_d.max(m);
        for k in 1..16 {
            d[k] = (d[k - 1] * r).max(m);
        }
        let (ms, mc) = (m * self.s_s, m * self.co_s);

        let t0 = self.t;
        let (sa, ca) = (self.sa, self.ca);
        let [in_l, in_r] = self.inbuf;
        let mut out = [[0.0f32; BLOCK]; 2];
        let gout = (1.0 / (5.0 * self.f_sqrt).max(1.0)).sqrt();
        let fdn = |d: f32, m: f32| d + m + 1.0;
        let ap = |d: f32, m: f32| (d + m).abs() + 33.0;
        let table = self.table;

        match MODE {
            GEMINI => {
                let mut y = [[0.0f32; BLOCK]; 16];
                for jl in 0..16 {
                    let x = if jl % 2 == 0 { &in_l } else { &in_r };
                    let mut u = [0.0; BLOCK];
                    for n in 0..BLOCK {
                        u[n] = x[n] + self.f_sqrt * self.fb[jl][n];
                    }
                    let arg = fdn(d[jl], self.m_at(ms, mc, jl));
                    self.lines[jl].write(t0, &u);
                    y[jl] = self.lines[jl].read(t0, arg, &table);
                }
                for n in 0..BLOCK {
                    let (mut sl, mut sr) = (0.0f32, 0.0f32);
                    for k in 0..8 {
                        sl += y[2 * k][n];
                        sr += y[2 * k + 1][n];
                    }
                    out[0][n] = 0.35355339 * sl * gout;
                    out[1][n] = 0.35355339 * sr * gout;
                }
                butterfly(&mut y, &[1, 2, 4, 8], sa, ca);
                self.fb = y;
            }
            HYDRA | CIRRUS_MAJOR => {
                let mf = [ms, mc, mc, ms, -ms, -mc, -mc, -ms];
                let cirrus = MODE == CIRRUS_MAJOR;
                let mut x = [[0.0f32; BLOCK]; 8];
                for k in 0..8 {
                    let src = if cirrus {
                        if k % 2 == 1 { &in_l } else { &in_r }
                    } else if k % 2 == 0 {
                        &in_l
                    } else {
                        &in_r
                    };
                    let state = if cirrus { self.fb[PI[k]] } else { self.fb[k] };
                    let mut u = [0.0; BLOCK];
                    for n in 0..BLOCK {
                        u[n] = src[n] + self.f_sqrt * state[n];
                        if cirrus {
                            u[n] = self.hc[k].tick(self.lc[k].tick(u[n], self.lc_a), self.hc_k);
                        }
                    }
                    self.lines[2 * k].write(t0, &u);
                    x[k] = self.lines[2 * k].read(t0, fdn(d[2 * k], mf[k]), &table);
                }
                if !cirrus {
                    butterfly(&mut x, &[1, 2, 4], sa, ca);
                }
                let mut o = [[0.0f32; BLOCK]; 8];
                for k in 0..8 {
                    o[k] = self.lines[2 * k + 1].read(t0, ap(d[2 * k + 1], -mf[k]), &table);
                }
                butterfly(&mut o, &[1, 2, 4], sa, ca);
                let g = if cirrus { self.g_d } else { self.g };
                let mut y = [[0.0f32; BLOCK]; 8];
                for k in 0..8 {
                    let mut w = [0.0; BLOCK];
                    for n in 0..BLOCK {
                        y[k][n] = o[k][n] - g * x[k][n];
                        w[n] = x[k][n] + g * y[k][n];
                    }
                    self.lines[2 * k + 1].write(t0, &w);
                }
                let gain = if cirrus { gout } else { 1.0 };
                for n in 0..BLOCK {
                    out[0][n] = y[0][n] * gain;
                    out[1][n] = y[1][n] * gain;
                }
                self.fb[..8].copy_from_slice(&y);
            }
            CENTAURUS | GREAT_ANNIHILATOR | LMC => {
                let mt = [ms, -ms, mc, -mc, mc, -mc, ms, -ms, -ms, ms, -mc, mc, -mc, mc, -ms, ms];
                let g = if MODE == LMC { self.g_d } else { self.g };
                let mut ch = [in_l, in_r, in_l, in_r];
                let first_ap;
                if MODE == GREAT_ANNIHILATOR {
                    // Stage 0 is an FDN: written, then read at |d+m|+33, fed back
                    // with linear f from the last stage's previous block.
                    let mut o = [[0.0f32; BLOCK]; 4];
                    for c in 0..4 {
                        let mut u = [0.0; BLOCK];
                        for n in 0..BLOCK {
                            u[n] = ch[c][n] + self.f_lin * self.fb[c][n];
                        }
                        let j = 4 * c;
                        self.lines[j].write(t0, &u);
                        o[c] = self.lines[j].read(t0, ap(d[j], mt[j]), &table);
                    }
                    butterfly(&mut o, &[1, 2], sa, ca);
                    ch = o;
                    first_ap = 1;
                } else if MODE == LMC {
                    for c in 0..4 {
                        for n in 0..BLOCK {
                            ch[c][n] += self.f_lin * self.fb[c][n];
                        }
                    }
                    first_ap = 0;
                } else {
                    first_ap = 0;
                }
                let last_ap = if MODE == LMC { 3 } else { 4 };
                for k in first_ap..last_ap {
                    let mut o = [[0.0f32; BLOCK]; 4];
                    for c in 0..4 {
                        let j = k + 4 * c;
                        o[c] = self.lines[j].read(t0, ap(d[j], mt[j]), &table);
                    }
                    butterfly(&mut o, &[1, 2], sa, ca);
                    let mut y = [[0.0f32; BLOCK]; 4];
                    for c in 0..4 {
                        let mut w = [0.0; BLOCK];
                        for n in 0..BLOCK {
                            y[c][n] = o[c][n] - g * ch[c][n];
                            w[n] = ch[c][n] + g * y[c][n];
                        }
                        self.lines[k + 4 * c].write(t0, &w);
                    }
                    ch = y;
                }
                match MODE {
                    CENTAURUS => {
                        let g4 = -(g * g * g * g);
                        for n in 0..BLOCK {
                            out[0][n] = ch[0][n] + g4 * in_l[n];
                            out[1][n] = ch[1][n] + g4 * in_r[n];
                        }
                    }
                    GREAT_ANNIHILATOR => {
                        out = [ch[0], ch[1]];
                        self.fb[..4].copy_from_slice(&ch);
                    }
                    _ => {
                        // LMC: the main delays, written then read; filters in the
                        // loop; Householder I − ½J for the feedback.
                        let mut y = [[0.0f32; BLOCK]; 4];
                        for c in 0..4 {
                            let j = 3 + 4 * c;
                            self.lines[j].write(t0, &ch[c]);
                            y[c] = self.lines[j].read(t0, ap(d[j], mt[j]), &table);
                            for n in 0..BLOCK {
                                y[c][n] = self.hc[c].tick(self.lc[c].tick(y[c][n], self.lc_a), self.hc_k);
                            }
                        }
                        for n in 0..BLOCK {
                            let s = y[0][n] + y[1][n] + y[2][n] + y[3][n];
                            for c in 0..4 {
                                self.fb[c][n] = y[c][n] - 0.5 * s;
                            }
                        }
                        out = [y[0], y[1]];
                    }
                }
            }
            SAGITTARIUS | ANDROMEDA | TRIANGULUM => {
                // m_i = M·sin(θ + i·45°), i = 0..7.
                let mi: [f32; 8] = core::array::from_fn(|i| self.m_at(ms, mc, 2 * i));
                let (mut xl, mut xr) = (in_l, in_r);
                let (stage0, stages, g) = match MODE {
                    SAGITTARIUS => (0, 8, self.g),
                    ANDROMEDA => (1, 8, self.g),
                    _ => (0, 7, self.g_d),
                };
                if MODE == ANDROMEDA {
                    for n in 0..BLOCK {
                        xl[n] += self.g * self.fb[0][n];
                        xr[n] += self.g * self.fb[1][n];
                    }
                    self.lines[0].write(t0, &xl);
                    self.lines[1].write(t0, &xr);
                    let mut a = self.lines[0].read(t0, ap(d[0], mi[0]), &table);
                    let mut b = self.lines[1].read(t0, ap(d[1], -mi[0]), &table);
                    rot(&mut a, &mut b, sa, ca);
                    (xl, xr) = (a, b);
                } else if MODE == TRIANGULUM {
                    for n in 0..BLOCK {
                        xl[n] += self.f_lin * self.fb[1][n];
                        xr[n] += self.f_lin * self.fb[0][n];
                    }
                }
                for i in stage0..stages {
                    let mut a = self.lines[2 * i].read(t0, ap(d[2 * i], mi[i]), &table);
                    let mut b = self.lines[2 * i + 1].read(t0, ap(d[2 * i + 1], -mi[i]), &table);
                    rot(&mut a, &mut b, sa, ca);
                    let (mut yl, mut yr, mut wl, mut wr) = ([0.0; BLOCK], [0.0; BLOCK], [0.0; BLOCK], [0.0; BLOCK]);
                    for n in 0..BLOCK {
                        yl[n] = a[n] - g * xl[n];
                        yr[n] = b[n] - g * xr[n];
                        wl[n] = xl[n] + g * yl[n];
                        wr[n] = xr[n] + g * yr[n];
                    }
                    self.lines[2 * i].write(t0, &wl);
                    self.lines[2 * i + 1].write(t0, &wr);
                    (xl, xr) = (yl, yr);
                }
                match MODE {
                    SAGITTARIUS => {
                        let g8 = -((g * g) * (g * g) * ((g * g) * (g * g)));
                        for n in 0..BLOCK {
                            out[0][n] = xl[n] + g8 * in_l[n];
                            out[1][n] = xr[n] + g8 * in_r[n];
                        }
                    }
                    ANDROMEDA => {
                        out = [xl, xr];
                        self.fb[0] = xl;
                        self.fb[1] = xr;
                    }
                    _ => {
                        // Triangulum: the main delays, written then read, then
                        // the filters, in the loop; the feedback crosses over.
                        self.lines[14].write(t0, &xl);
                        self.lines[15].write(t0, &xr);
                        let mut yl = self.lines[14].read(t0, ap(d[14], mi[7]), &table);
                        let mut yr = self.lines[15].read(t0, ap(d[15], -mi[7]), &table);
                        for n in 0..BLOCK {
                            yl[n] = self.hc[0].tick(self.lc[0].tick(yl[n], self.lc_a), self.hc_k);
                            yr[n] = self.hc[1].tick(self.lc[1].tick(yr[n], self.lc_a), self.hc_k);
                        }
                        self.fb[0] = yl;
                        self.fb[1] = yr;
                        out = [yl, yr];
                    }
                }
            }
            LYRA => {
                let mk = [ms, mc, -ms, -mc];
                let mut y = [[0.0f32; BLOCK]; 4];
                for k in 0..4 {
                    let x = if k % 2 == 0 { &in_l } else { &in_r };
                    let mut u = [0.0; BLOCK];
                    for n in 0..BLOCK {
                        u[n] = x[n] + self.f_sqrt * self.fb[k][n];
                    }
                    self.lines[k].write(t0, &u);
                    y[k] = self.lines[k].read(t0, fdn(d[k], mk[k]), &table);
                }
                for n in 0..BLOCK {
                    out[0][n] = 0.70710677 * (y[0][n] + y[2][n]) * gout;
                    out[1][n] = 0.70710677 * (y[1][n] + y[3][n]) * gout;
                }
                butterfly(&mut y, &[1, 2], sa, ca);
                self.fb[..4].copy_from_slice(&y);
            }
            CAPRICORN | CIRRUS_MINOR => {
                let h = 0.70710677;
                let mk = if MODE == CAPRICORN {
                    [ms, h * (ms + mc), mc, h * (ms - mc), -ms, -h * (ms + mc), -mc, -h * (ms - mc)]
                } else {
                    [ms, -ms, h * (ms + mc), -h * (ms + mc), mc, -mc, h * (ms - mc), -h * (ms - mc)]
                };
                let mut y = [[0.0f32; BLOCK]; 8];
                for k in 0..8 {
                    let (x, state) = if MODE == CAPRICORN {
                        (if k % 2 == 0 { &in_l } else { &in_r }, self.fb[k])
                    } else {
                        (if k % 2 == 1 { &in_l } else { &in_r }, self.fb[PI[k]])
                    };
                    let mut u = [0.0; BLOCK];
                    for n in 0..BLOCK {
                        u[n] = x[n] + self.f_sqrt * state[n];
                    }
                    self.lines[k].write(t0, &u);
                    y[k] = self.lines[k].read(t0, fdn(d[k], mk[k]), &table);
                    if MODE == CIRRUS_MINOR {
                        for n in 0..BLOCK {
                            y[k][n] = self.hc[k].tick(self.lc[k].tick(y[k][n], self.lc_a), self.hc_k);
                        }
                    }
                }
                if MODE == CAPRICORN {
                    for n in 0..BLOCK {
                        let (mut sl, mut sr) = (0.0f32, 0.0f32);
                        for k in 0..4 {
                            sl += y[2 * k][n];
                            sr += y[2 * k + 1][n];
                        }
                        out[0][n] = 0.5 * sl * gout;
                        out[1][n] = 0.5 * sr * gout;
                    }
                } else {
                    for n in 0..BLOCK {
                        out[0][n] = y[0][n] * gout;
                        out[1][n] = y[1][n] * gout;
                    }
                }
                butterfly(&mut y, &[1, 2, 4], sa, ca);
                self.fb[..8].copy_from_slice(&y);
            }
            _ => unreachable!(),
        }

        // §9: post filters where the mode has them, then Width.
        let post = !matches!(MODE, TRIANGULUM | LMC | CIRRUS_MAJOR | CIRRUS_MINOR);
        for n in 0..BLOCK {
            let (mut l, mut r) = (out[0][n], out[1][n]);
            if post {
                l = self.hc[0].tick(self.lc[0].tick(l, self.lc_a), self.hc_k);
                r = self.hc[1].tick(self.lc[1].tick(r, self.lc_a), self.hc_k);
            }
            self.wetbuf[0][n] = self.w_sin * l + self.w_cos * r;
            self.wetbuf[1][n] = self.w_sin * r + self.w_cos * l;
        }
        self.t = self.t.wrapping_add(BLOCK);
    }
}

impl<const MODE: usize> Kernel for Supermassive<MODE> {
    fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let mut values = [0.0; N_PARAMS];
        for (v, p) in values.iter_mut().zip(PARAMS) {
            *v = p.default;
        }
        // Room for the longest delay, the deepest modulation and the offsets.
        let room = (2.0 * sr + 2.0 * 500.0 * sr / 48_000.0) as usize + 128;
        let blocks_per_s = sr as f64 / 32.0;
        let mut k = Supermassive {
            sr,
            values,
            table: interp_table(),
            lfo: lfo_table(),
            phase_sin: core::array::from_fn(|j| (j as f64 * std::f64::consts::PI / 8.0).sin() as f32),
            phase_cos: core::array::from_fn(|j| (j as f64 * std::f64::consts::PI / 8.0).cos() as f32),
            lines: (0..lines(MODE)).map(|_| Line::new(room)).collect(),
            t: 0,
            pos: 0,
            inbuf: [[0.0; BLOCK]; 2],
            wetbuf: [[0.0; BLOCK]; 2],
            fb: [[0.0; BLOCK]; 16],
            c_smooth: (1.0 - (-1000.0 / (blocks_per_s * 300.0)).exp()) as f32,
            c_lfo: (1.0 - (-std::f64::consts::FRAC_PI_2 / blocks_per_s).exp()) as f32,
            d_s: 0.0,
            r_s: 0.0,
            m_s: 0.0,
            s_s: 0.0,
            co_s: 0.0,
            phase: 0,
            inc: 0,
            f_lin: 0.0,
            f_sqrt: 0.0,
            g: 0.0,
            g_d: 0.0,
            sa: 0.0,
            ca: 1.0,
            w_sin: 1.0,
            w_cos: 0.0,
            lc_a: 0.0,
            hc_k: 1.0,
            wet: 0.0,
            dry: 1.0,
            lc: [LowCut::default(); 8],
            hc: [HighCut::default(); 8],
        };
        k.laws();
        k.d_s = k.d_target();
        k.r_s = k.r_target();
        k.m_s = k.m_target();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTORS[MODE]
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(p) = PARAMS.get(index) else { return };
        self.values[index] = p.clamp(value);
        self.laws();
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn reset(&mut self) {
        for l in &mut self.lines {
            l.clear();
        }
        self.t = 0;
        self.pos = 0;
        self.inbuf = [[0.0; BLOCK]; 2];
        self.wetbuf = [[0.0; BLOCK]; 2];
        self.fb = [[0.0; BLOCK]; 16];
        self.lc = [LowCut::default(); 8];
        self.hc = [HighCut::default(); 8];
        self.s_s = 0.0;
        self.co_s = 0.0;
        self.phase = 0;
        self.d_s = self.d_target();
        self.r_s = self.r_target();
        self.m_s = self.m_target();
    }

    /// §2.1: out = wetbuf·wet + in·dry, the input kept for the block.
    fn tick(&mut self, frame: &mut [f32; 2]) {
        let k = self.pos;
        let (l, r) = (frame[0], frame[1]);
        frame[0] = self.wetbuf[0][k] * self.wet + l * self.dry;
        frame[1] = self.wetbuf[1][k] * self.wet + r * self.dry;
        self.inbuf[0][k] = l;
        self.inbuf[1][k] = r;
        self.pos += 1;
        if self.pos == BLOCK {
            self.pos = 0;
            self.block();
        }
    }

    fn tail_seconds(&self) -> f32 {
        // The longest path once through, and a decay that grows with Feedback.
        let pass = (16.0 * (self.d_target() + 65.0)) / self.sr;
        (pass + 30.0 * self.values[FEEDBACK] / 100.0).min(60.0)
    }
}
