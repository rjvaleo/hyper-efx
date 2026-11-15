//! Ambient FDN: Stone Voices' Ambient Reverb 4.3, as coded.
//!
//! From the static decode `sources/reports/001-ambientreverb.md` (§2–4):
//! input gain written back onto the dry path; an 18-tap early-reflection FIR
//! per side, cross-fed 0.5/0.3, from the input and not the predelay; a
//! predelay per side into an 8-line FDN — each line an integer delay then a
//! Schroeder all-pass at ±0.6, the Sylvester Hadamard times √0.125 as the
//! feedback matrix, a crossover shelf and a one-pole damping low-pass solved
//! so the RT60 halves at High Damping, times 0.001^(t/Te); late taps
//! L = h0 + h4, R = h1 − h2; the width cross-mix and M/S; two Regalia–Mitra
//! bells on the wet; dry added last.
//!
//! As coded (D10): Size reallocates — here, re-lengths — and clears every
//! line; Freeze is not a true freeze (the input keeps feeding the network);
//! the lengths are the decode's times at the running rate; no modulation.
//!
//! **What is ours.** The original ramps its levels over "the next host block"
//! and steps its EQ once per host block, so its sound depends on the block
//! size the host happens to use. A kernel here must sound the same whatever
//! block it is handed, so those block-rate steps run on a fixed internal block
//! of [`BLOCK`] frames. Controls are taken at the start of the next internal
//! block, as the original takes them at the start of the next host block. The
//! EQ's state before its first move is not in the decode; here it starts at
//! its targets. Decay is shown in seconds: the original's knob is log10 s,
//! linear, so a log knob in seconds is the same knob.

use crate::spec::Role;
use crate::{Category, Descriptor, Group, Kernel, Panel, ParamKind, ParamSpec};

/// The block the ramps and the EQ's steps run on (ours; see the module notes).
pub const BLOCK: usize = 256;

/// §2.2: all-pass and loop times (s), early-reflection times and gains.
pub const TA: [f32; 8] = [0.020346, 0.024421, 0.031604, 0.027333, 0.022904, 0.029291, 0.013458, 0.019123];
pub const TN: [f32; 8] = [0.153129, 0.210389, 0.127837, 0.256891, 0.174713, 0.192303, 0.125000, 0.219991];
pub const TL: [f32; 18] = [
    0.0133, 0.0305, 0.0315, 0.0358, 0.0360, 0.0388, 0.0548, 0.0575, 0.0662, 0.0677, 0.0685, 0.0702, 0.0797, 0.0798, 0.0816,
    0.0831, 0.0843, 0.0887,
];
pub const TR: [f32; 18] = [
    0.0043, 0.0215, 0.0225, 0.0268, 0.0270, 0.0298, 0.0458, 0.0485, 0.0572, 0.0587, 0.0595, 0.0612, 0.0707, 0.0708, 0.0726,
    0.0741, 0.0753, 0.0797,
];
pub const G: [f32; 18] = [
    0.841, 0.504, 0.491, 0.379, 0.380, 0.346, 0.289, 0.272, 0.192, 0.193, 0.217, 0.181, 0.180, 0.181, 0.176, 0.142, 0.167,
    0.134,
];
const ER_LEN_L: f32 = 0.0887;
const ER_LEN_R: f32 = 0.0797;
const PREDELAY_BUFFER_S: f32 = 0.251;
const AP_G: f32 = 0.6;
const PRE_SCALE: f32 = 0.3;
const HADAMARD: f32 = 0.35355339;
const FREEZE_TE: f32 = 1e6;

const IN: usize = 0;
const OUT: usize = 1;
const DRY: usize = 2;
const ER: usize = 3;
const LATE: usize = 4;
const PD_L: usize = 5;
const PD_R: usize = 6;
const DECAY: usize = 7;
const FREEZE: usize = 8;
const SIZE: usize = 9;
const WIDE: usize = 10;
const BASS: usize = 11;
const XOVER: usize = 12;
const HIDAMP: usize = 13;
const EQ1_DB: usize = 14;
const EQ1_HZ: usize = 15;
const EQ2_DB: usize = 16;
const EQ2_HZ: usize = 17;
const N_PARAMS: usize = 18;

static PARAMS: &[ParamSpec] = &[
    ParamSpec::new("inputDb", "Input", -200.0, 0.0, 0.0).unit("dB").smooth(0.0).role(Role::Output),
    ParamSpec::new("outputDb", "Output", -200.0, 0.0, 0.0).unit("dB").smooth(0.0).role(Role::Output),
    ParamSpec::new("dryDb", "Dry", -200.0, 0.0, 0.0).unit("dB").smooth(0.0).role(Role::Output),
    ParamSpec::new("erDb", "ER", -200.0, 0.0, -9.0).unit("dB").smooth(0.0).role(Role::Output),
    ParamSpec::new("lateDb", "Late", -200.0, 0.0, -4.0).unit("dB").smooth(0.0).role(Role::Output),
    ParamSpec::new("predelayLMs", "Pre-delay L", 0.0, 200.0, 0.0).unit("ms").smooth(0.0).role(Role::Time),
    ParamSpec::new("predelayRMs", "Pre-delay R", 0.0, 200.0, 0.0).unit("ms").smooth(0.0).role(Role::Time),
    ParamSpec::new("decayS", "Decay", 0.1, 100.0, 1.4996844).log().unit("s").smooth(0.0).role(Role::Time),
    ParamSpec::new("freeze", "Freeze", 0.0, 1.0, 0.0).kind(ParamKind::Toggle).smooth(0.0).role(Role::Time),
    ParamSpec::new("size", "Size", 0.0, 100.0, 50.0).unit("%").smooth(0.0).role(Role::Space),
    ParamSpec::new("wide", "Wide", 0.0, 200.0, 150.0).unit("%").smooth(0.0).role(Role::Space),
    ParamSpec::new("bassRatio", "Bass ratio", 0.0, 3.0, 1.0).unit("×").smooth(0.0).role(Role::Tone),
    ParamSpec::new("crossoverHz", "Crossover", 50.0, 2000.0, 200.0).log().unit("Hz").smooth(0.0).role(Role::Tone),
    ParamSpec::new("highDampHz", "High damping", 500.0, 20_000.0, 10_000.0).log().unit("Hz").smooth(0.0).role(Role::Tone),
    ParamSpec::new("eqLowDb", "EQ low", -15.0, 15.0, 0.0).unit("dB").smooth(0.0).role(Role::Tone),
    ParamSpec::new("eqLowHz", "EQ low freq", 30.0, 3000.0, 100.0).log().unit("Hz").smooth(0.0).role(Role::Tone),
    ParamSpec::new("eqHighDb", "EQ high", -15.0, 15.0, 0.0).unit("dB").smooth(0.0).role(Role::Tone),
    ParamSpec::new("eqHighHz", "EQ high freq", 160.0, 10_000.0, 2000.0).log().unit("Hz").smooth(0.0).role(Role::Tone),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "ambient-fdn",
    code: 104,
    version: 1,
    label: "Ambient FDN",
    category: Category::Reverb,
    params: PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Time", keys: &["decayS", "predelayLMs", "predelayRMs", "freeze"] },
            Group { label: "Space", keys: &["size", "wide"] },
            Group { label: "Tone", keys: &["bassRatio", "crossoverHz", "highDampHz"] },
            Group { label: "EQ", keys: &["eqLowDb", "eqLowHz", "eqHighDb", "eqHighHz"] },
            Group { label: "Output", keys: &["inputDb", "dryDb", "erDb", "lateDb", "outputDb"] },
        ],
        primary: &["decayS", "size", "lateDb"],
        displays: &[],
        pad: None,
    },
    source: "Stone Voices Ambient Reverb 4.3, sources/reports/001-ambientreverb.md §2–4 (static decode, not yet measured)",
    sound: None,
};

/// §3.1's factory programs, in parameter order (In, Out, Dry, ER, Late, PdL,
/// PdR, Decay s, Freeze, Size, Wide, Bass, Crossover, High damping, EQ low
/// dB, Hz, EQ high dB, Hz). Program 0 is the defaults.
pub const PROGRAMS: [(&str, [f32; N_PARAMS]); 16] = [
    ("Default", [0.0, 0.0, 0.0, -9.0, -4.0, 0.0, 0.0, 1.4996844, 0.0, 50.0, 150.0, 1.0, 200.0, 10_000.0, 0.0, 100.0, 0.0, 2000.0]),
    ("Cathedral", [0.0, 0.0, 0.0, -5.97, -7.09, 76.0, 65.0, 4.62, 0.0, 90.5, 152.0, 1.51, 316.0, 3162.0, 0.0, 490.0, -4.8, 2172.0]),
    ("Station", [0.0, 0.0, -4.21, -5.97, -2.94, 75.0, 78.0, 2.02, 0.0, 58.0, 121.0, 1.405, 347.0, 8252.0, -5.55, 263.0, -4.8, 2000.0]),
    ("Small Hall", [0.0, 0.0, 0.0, -4.84, -4.94, 42.0, 38.0, 1.43, 0.0, 33.5, 122.0, 0.505, 511.0, 4094.0, -4.65, 102.0, -5.4, 5177.0]),
    ("Medium Hall", [0.0, 0.0, 0.0, -10.21, -9.39, 39.0, 72.0, 2.66, 0.0, 33.5, 149.0, 0.505, 511.0, 4573.0, -3.3, 102.0, -3.9, 5177.0]),
    ("Large Hall", [0.0, 0.0, 0.0, -10.21, -9.95, 42.0, 38.0, 4.62, 0.0, 55.0, 122.0, 1.03, 511.0, 5203.0, 0.0, 490.0, 0.0, 2172.0]),
    ("Stadium", [0.0, 0.0, 0.0, -10.78, -9.03, 120.0, 137.0, 4.62, 0.0, 59.5, 176.0, 0.795, 328.0, 2831.0, -5.55, 417.0, -4.8, 6635.0]),
    ("Far Space", [0.0, 0.0, -4.21, -6.81, -7.43, 16.0, 61.0, 10.2, 0.0, 100.0, 176.0, 3.0, 699.0, 5015.0, -5.55, 417.0, -4.8, 2000.0]),
    ("Long Ambience", [0.0, 0.0, -5.48, -17.73, -15.29, 174.0, 31.0, 48.4, 0.0, 100.0, 200.0, 2.115, 699.0, 20_000.0, 2.7, 209.0, -2.4, 3354.0]),
    ("Spring Reverb", [0.0, 0.0, 0.0, -10.59, -7.43, 20.0, 13.0, 2.09, 0.0, 8.0, 163.0, 1.605, 699.0, 5015.0, -6.45, 417.0, -4.8, 2000.0]),
    ("Metal And Vacuum", [0.0, 0.0, 0.0, -200.0, -11.89, 0.0, 0.0, 3.16, 0.0, 0.0, 190.0, 1.605, 699.0, 5015.0, 0.0, 417.0, 0.0, 2000.0]),
    ("Mellow", [0.0, 0.0, 0.0, -8.73, -6.24, 18.0, 15.0, 6.31, 0.0, 11.5, 162.0, 0.54, 520.0, 6492.0, -6.6, 100.0, -6.0, 3957.0]),
    ("Garage", [0.0, 0.0, 0.0, -9.65, -7.09, 14.0, 14.0, 1.95, 0.0, 7.0, 190.0, 1.605, 699.0, 5015.0, -3.6, 417.0, -2.7, 2902.0]),
    ("Input In A Cave", [0.0, 0.0, 0.0, -7.32, -7.95, 200.0, 150.0, 15.5, 0.0, 65.5, 42.0, 2.535, 73.6, 1598.0, 0.0, 417.0, 0.0, 2000.0]),
    ("Dark Ambience", [0.0, 0.0, -3.44, -22.52, -10.91, 0.0, 0.0, 33.1, 0.0, 70.5, 200.0, 3.0, 2000.0, 6492.0, -15.0, 3000.0, 0.0, 3957.0]),
    ("Deep Underground", [0.0, 0.0, -4.37, -200.0, -15.29, 184.0, 106.0, 100.0, 0.0, 100.0, 200.0, 3.0, 699.0, 20_000.0, 3.6, 389.0, -11.7, 6499.0]),
];

/// §3: dB to gain, `exp(v·0.115129)`, nothing below −90 dB.
pub fn level(db: f32) -> f32 {
    if db < -90.0 { 0.0 } else { (db * 0.115129).exp() }
}

/// §2.2: the Size factor.
pub fn size_factor(size: f32) -> f32 {
    size * 0.01 * 0.95 + 0.05
}

/// §2.2: a length in samples, `floor(sf·sr·τ + 0.5)`.
pub fn samples(sf: f32, sr: f32, tau: f32) -> usize {
    (sf * sr * tau + 0.5).floor() as usize
}

/// The all-pass and loop lengths at a rate and Size (§2.2's worked values).
pub fn lengths(sr: f32, size: f32) -> ([usize; 8], [usize; 8]) {
    let sf = size_factor(size);
    (core::array::from_fn(|k| samples(sf, sr, TA[k])), core::array::from_fn(|k| samples(sf, sr, TN[k])))
}

/// Each line's loop gain, shelf, crossover and damping coefficients (§2.3).
pub struct Coefficients {
    pub g: [f32; 8],
    pub sh: [f32; 8],
    pub cx: f32,
    pub cd: [f32; 8],
}

/// §2.3, flag 1: from Decay (s), Size, Freeze, Bass ratio, Crossover and
/// High damping. `t` is each line's nominal loop time, unscaled.
pub fn coefficients(sr: f32, decay_s: f32, size: f32, freeze: bool, bass: f32, xover: f32, hidamp: f32) -> Coefficients {
    let sf = size_factor(size);
    let te = if freeze { FREEZE_TE } else { decay_s / sf };
    let cx = (std::f64::consts::TAU * xover as f64 / sr as f64) as f32;
    let c = if hidamp as f64 <= 0.49 * sr as f64 {
        1.0 - ((std::f64::consts::TAU * hidamp as f64 / sr as f64) as f32).cos()
    } else {
        2.0
    };
    let mut out = Coefficients { g: [0.0; 8], sh: [0.0; 8], cx, cd: [0.0; 8] };
    for k in 0..8 {
        let t = TN[k];
        let g = 0.001f32.powf(t / te);
        let g_lo = 0.001f32.powf(t / (te * bass));
        out.g[k] = g;
        out.sh[k] = g_lo / g - 1.0;
        let h = 0.001f32.powf(t / (te / 2.0)) / g;
        let a = (1.0 - h * h) / (2.0 * h * h * c);
        out.cd[k] = ((4.0 * a + 1.0).sqrt() - 1.0) / (2.0 * a);
    }
    out
}

/// §4: one bell's coefficients c0, c1, c2 for a gain in dB and a centre.
pub fn bell(gain_db: f32, freq: f32, sr: f32) -> [f32; 3] {
    Bell::coefficients(10f32.powf(gain_db * 0.05), freq, sr)
}

/// A ring of fixed room whose used length can change.
struct Ring {
    buf: Vec<f32>,
    len: usize,
    idx: usize,
}

impl Ring {
    fn new(room: usize) -> Ring {
        Ring { buf: vec![0.0; room.max(1)], len: room.max(1), idx: 0 }
    }

    fn set_len(&mut self, len: usize) {
        self.len = len.clamp(1, self.buf.len());
        self.buf.fill(0.0);
        self.idx = 0;
    }
}

/// A gain that moves linearly across one block when it is told to (§3).
#[derive(Clone, Copy, Default)]
struct Ramp {
    now: f32,
    step: f32,
}

/// One Regalia–Mitra bell (§4), on both channels.
#[derive(Clone, Copy, Default)]
struct Bell {
    target_g: f32,
    target_f: f32,
    g: f32,
    f: f32,
    c: [f32; 3],
    /// The coefficients the left channel keeps through a moving block (§4's quirk).
    c_left: [f32; 3],
    mode: u8,
    s0: [f32; 2],
    s2: [f32; 2],
}

impl Bell {
    fn coefficients(g: f32, f: f32, sr: f32) -> [f32; 3] {
        let w = ((f as f64 * std::f64::consts::PI / sr as f64) as f32) * 2.0;
        let r = w / g.sqrt();
        [-w.cos(), (1.0 - r) / (1.0 + r), (g - 1.0) / 2.0]
    }

    /// `FUN_100080c0`, once per block.
    fn step(&mut self, sr: f32) {
        let toward = |c: f32, t: f32| if t > c { t.min(c * 2.0) } else { t.max(c * 0.5) };
        if self.g != self.target_g || self.f != self.target_f {
            self.g = toward(self.g, self.target_g);
            self.f = toward(self.f, self.target_f);
            self.c_left = self.c;
            self.c = Bell::coefficients(self.g, self.f, sr);
            self.mode = 2;
        } else if (self.g - 1.0).abs() >= 0.001 {
            self.mode = 1;
        } else {
            if self.mode != 0 {
                self.s0 = [0.0; 2];
                self.s2 = [0.0; 2];
            }
            self.mode = 0;
        }
    }

    #[inline]
    fn process(&mut self, x: f32, ch: usize) -> f32 {
        if self.mode == 0 {
            return x;
        }
        let [c0, c1, c2] = if self.mode == 2 && ch == 0 { self.c_left } else { self.c };
        let (s0, s2) = (self.s0[ch], self.s2[ch]);
        let v = x - c1 * s2;
        let y = x - ((c1 * v + s2) - x) * c2;
        let t = v - c0 * s0;
        self.s2[ch] = c0 * t + s0;
        self.s0[ch] = t + 1e-20;
        y
    }
}

pub struct AmbientFdn {
    sr: f32,
    values: [f32; N_PARAMS],
    dirty: [bool; N_PARAMS],
    pos: usize,
    // FDN
    delay: [Ring; 8],
    allpass: [Ring; 8],
    lp1: [f32; 8],
    lp2: [f32; 8],
    g: [f32; 8],
    sh: [f32; 8],
    cx: f32,
    cd: [f32; 8],
    // predelay
    pre: [Ring; 2],
    pre_d: [usize; 2],
    // early reflections
    er: [Ring; 2],
    taps: [[usize; 18]; 2],
    // ramped gains
    g_in: Ramp,
    g_dry: Ramp,
    g_late: Ramp,
    g_er: Ramp,
    c_a: Ramp,
    c_b: Ramp,
    w: Ramp,
    bells: [Bell; 2],
}

impl AmbientFdn {
    /// Flag 2: lengths and taps, from Size. Clears every FDN line.
    fn lengths(&mut self) {
        let sf = size_factor(self.values[SIZE]);
        for k in 0..8 {
            let a = samples(sf, self.sr, TA[k]);
            let n = samples(sf, self.sr, TN[k]);
            self.allpass[k].set_len(a);
            self.delay[k].set_len(n.saturating_sub(a));
        }
        for i in 0..18 {
            self.taps[0][i] = samples(sf, self.sr, TL[i]).min(self.er[0].len);
            self.taps[1][i] = samples(sf, self.sr, TR[i]).min(self.er[1].len);
        }
    }

    /// Flag 1: decay and damping (§2.3).
    fn decay(&mut self) {
        let v = &self.values;
        let c = coefficients(self.sr, v[DECAY], v[SIZE], v[FREEZE] >= 0.5, v[BASS], v[XOVER], v[HIDAMP]);
        (self.g, self.sh, self.cx, self.cd) = (c.g, c.sh, c.cx, c.cd);
    }

    /// The start of a block: whatever moved since the last one takes effect.
    fn block(&mut self) {
        let d = self.dirty;
        self.dirty = [false; N_PARAMS];
        let n = BLOCK as f32;
        let ramp = |r: &mut Ramp, target: f32, moved: bool| r.step = if moved { (target - r.now) / n } else { 0.0 };
        if d[SIZE] {
            self.lengths();
        }
        if d[SIZE] || d[DECAY] || d[FREEZE] || d[BASS] || d[XOVER] || d[HIDAMP] {
            self.decay();
        }
        for (s, i) in [(0usize, PD_L), (1, PD_R)] {
            if d[i] {
                self.pre_d[s] = ((self.sr * (self.values[i] * 0.001) + 0.5).floor() as usize).min(self.pre[s].len - 1);
            }
        }
        let out = level(self.values[OUT]);
        ramp(&mut self.g_in, level(self.values[IN]), d[IN]);
        ramp(&mut self.g_dry, out * level(self.values[DRY]), d[OUT] || d[DRY]);
        ramp(&mut self.g_er, out * level(self.values[ER]), d[OUT] || d[ER]);
        let late = (0.3 + 0.7 * (self.values[SIZE] * 0.01)) * out * level(self.values[LATE]);
        ramp(&mut self.g_late, late, d[OUT] || d[LATE] || d[SIZE]);
        let v = self.values[WIDE];
        let (ca, cb, w) = if v <= 100.0 { (0.5 + 0.5 * (v * 0.01), 0.5 - 0.5 * (v * 0.01), 0.0) } else { (1.0, 0.0, (v - 100.0) * 0.02) };
        ramp(&mut self.c_a, ca, d[WIDE]);
        ramp(&mut self.c_b, cb, d[WIDE]);
        ramp(&mut self.w, w, d[WIDE]);
        for (b, (gi, fi)) in self.bells.iter_mut().zip([(EQ1_DB, EQ1_HZ), (EQ2_DB, EQ2_HZ)]) {
            b.target_g = 10f32.powf(self.values[gi] * 0.05);
            b.target_f = self.values[fi];
            b.step(self.sr);
        }
    }
}

impl Kernel for AmbientFdn {
    fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let mut values = [0.0; N_PARAMS];
        for (v, p) in values.iter_mut().zip(PARAMS) {
            *v = p.default;
        }
        let mut k = AmbientFdn {
            sr,
            values,
            dirty: [true; N_PARAMS],
            pos: 0,
            delay: core::array::from_fn(|k| Ring::new(samples(1.0, sr, TN[k]) + 2)),
            allpass: core::array::from_fn(|k| Ring::new(samples(1.0, sr, TA[k]) + 2)),
            lp1: [0.0; 8],
            lp2: [0.0; 8],
            g: [0.0; 8],
            sh: [0.0; 8],
            cx: 0.0,
            cd: [0.0; 8],
            pre: core::array::from_fn(|_| Ring::new((sr * PREDELAY_BUFFER_S) as usize)),
            pre_d: [0; 2],
            er: [Ring::new(samples(1.0, sr, ER_LEN_L)), Ring::new(samples(1.0, sr, ER_LEN_R))],
            taps: [[0; 18]; 2],
            g_in: Ramp::default(),
            g_dry: Ramp::default(),
            g_late: Ramp::default(),
            g_er: Ramp::default(),
            c_a: Ramp::default(),
            c_b: Ramp::default(),
            w: Ramp::default(),
            bells: [Bell::default(); 2],
        };
        // The EQ starts at its targets (not in the decode; see the notes).
        for (b, (gi, fi)) in k.bells.iter_mut().zip([(EQ1_DB, EQ1_HZ), (EQ2_DB, EQ2_HZ)]) {
            b.g = 10f32.powf(k.values[gi] * 0.05);
            b.f = k.values[fi];
            b.c = Bell::coefficients(b.g, b.f, sr);
            b.c_left = b.c;
        }
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(p) = PARAMS.get(index) else { return };
        self.values[index] = p.clamp(value);
        self.dirty[index] = true;
    }

    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    /// As just made, keeping the controls: every line, filter and ramp from
    /// nothing, and every control taken again at the next block.
    fn reset(&mut self) {
        for r in self.delay.iter_mut().chain(self.allpass.iter_mut()).chain(self.pre.iter_mut()).chain(self.er.iter_mut()) {
            r.buf.fill(0.0);
            r.idx = 0;
        }
        self.lp1 = [0.0; 8];
        self.lp2 = [0.0; 8];
        for r in [&mut self.g_in, &mut self.g_dry, &mut self.g_late, &mut self.g_er, &mut self.c_a, &mut self.c_b, &mut self.w] {
            *r = Ramp::default();
        }
        self.dirty = [true; N_PARAMS];
        self.pos = 0;
        let (sr, v) = (self.sr, self.values);
        for (b, (gi, fi)) in self.bells.iter_mut().zip([(EQ1_DB, EQ1_HZ), (EQ2_DB, EQ2_HZ)]) {
            *b = Bell::default();
            b.g = 10f32.powf(v[gi] * 0.05);
            b.f = v[fi];
            b.c = Bell::coefficients(b.g, b.f, sr);
            b.c_left = b.c;
        }
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        if self.pos == 0 {
            self.block();
        }
        self.pos = (self.pos + 1) % BLOCK;

        // 1. Input gain, onto the dry path too.
        self.g_in.now += self.g_in.step;
        let in_l = frame[0] * self.g_in.now;
        let in_r = frame[1] * self.g_in.now;

        // 3. Predelay: write, then read round(sr·Pd) back.
        let mut p = [0.0f32; 2];
        for (s, x) in [in_l, in_r].into_iter().enumerate() {
            let r = &mut self.pre[s];
            r.buf[r.idx] = x;
            let read = (r.idx + r.len - self.pre_d[s]) % r.len;
            p[s] = PRE_SCALE * r.buf[read];
            r.idx = (r.idx + 1) % r.len;
        }
        let inj = [p[0], p[0], -p[0], -p[0], p[1], p[1], -p[1], -p[1]];

        // 4. Each line: its delay out, plus its injection, through its all-pass.
        let mut a = [0.0f32; 8];
        for k in 0..8 {
            let d = &self.delay[k];
            let u = d.buf[d.idx] + inj[k];
            let ap = &mut self.allpass[k];
            let c = if k % 2 == 0 { AP_G } else { -AP_G };
            let z = ap.buf[ap.idx];
            let v = u - c * z;
            ap.buf[ap.idx] = v;
            ap.idx = (ap.idx + 1) % ap.len;
            a[k] = c * v + z;
        }

        // 5. Hadamard, Sylvester order.
        let h = [
            a[0] + a[1] + a[2] + a[3] + a[4] + a[5] + a[6] + a[7],
            a[0] - a[1] + a[2] - a[3] + a[4] - a[5] + a[6] - a[7],
            a[0] + a[1] - a[2] - a[3] + a[4] + a[5] - a[6] - a[7],
            a[0] - a[1] - a[2] + a[3] + a[4] - a[5] - a[6] + a[7],
            (a[0] + a[1] + a[2] + a[3]) - (a[4] + a[5] + a[6] + a[7]),
            (a[0] - a[1] + a[2] - a[3]) - (a[4] - a[5] + a[6] - a[7]),
            (a[0] + a[1] - a[2] - a[3]) - (a[4] + a[5] - a[6] - a[7]),
            (a[0] - a[1] - a[2] + a[3]) - (a[4] - a[5] - a[6] + a[7]),
        ];

        // 6. Early reflections, from the input after its gain.
        let mut e = [0.0f32; 2];
        for (s, x) in [0.5 * in_l + 0.3 * in_r, 0.5 * in_r + 0.3 * in_l].into_iter().enumerate() {
            let r = &mut self.er[s];
            r.idx = (r.idx + 1) % r.len;
            r.buf[r.idx] = x;
            let mut sum = 0.0f32;
            for i in 0..18 {
                sum += G[i] * r.buf[(r.idx + r.len - self.taps[s][i]) % r.len];
            }
            e[s] = sum;
        }

        // 7. The ramps, once a sample.
        for r in [&mut self.g_late, &mut self.g_er, &mut self.c_a, &mut self.c_b, &mut self.w] {
            r.now += r.step;
        }

        // 8. Mix and width.
        let lp = (h[0] + h[4]) * self.g_late.now + e[0] * self.g_er.now;
        let rp = (h[1] - h[2]) * self.g_late.now + e[1] * self.g_er.now;
        let (ca, cb, w) = (self.c_a.now, self.c_b.now, self.w.now);
        let l2 = ca * lp + cb * rp;
        let r2 = ca * rp + cb * lp;
        let m = 0.5 * (l2 + r2);
        let wet_l = 0.5 * ((l2 - m) * w + m + l2);
        let wet_r = 0.5 * ((r2 - m) * w + m + r2);

        // 9. Feedback filters, then into each line's delay.
        for k in 0..8 {
            let x = h[k] * HADAMARD;
            self.lp1[k] = (x - self.lp1[k]) * self.cx + 1e-10 + self.lp1[k];
            self.lp2[k] = ((x + self.lp1[k] * self.sh[k]) - self.lp2[k]) * self.cd[k] + self.lp2[k];
            let d = &mut self.delay[k];
            d.buf[d.idx] = self.lp2[k] * self.g[k];
            d.idx = (d.idx + 1) % d.len;
        }

        // 10–11. The two bells on the wet, then the dry added.
        let wet_l = self.bells[0].process(wet_l, 0);
        let wet_l = self.bells[1].process(wet_l, 0);
        let wet_r = self.bells[0].process(wet_r, 1);
        let wet_r = self.bells[1].process(wet_r, 1);
        self.g_dry.now += self.g_dry.step;
        frame[0] = in_l * self.g_dry.now + wet_l;
        frame[1] = in_r * self.g_dry.now + wet_r;
    }

    fn tail_seconds(&self) -> f32 {
        if self.values[FREEZE] >= 0.5 {
            return 60.0;
        }
        let longest = self.values[DECAY] * self.values[BASS].max(1.0);
        (longest + 0.2 + 0.251).min(60.0)
    }
}
