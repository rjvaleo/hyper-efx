//! JC Reverb: Pluggo's `cverb~`, the Schroeder/Chowning "JCRev", as coded.
//!
//! From the static decode in `sources/reports/003-pluggo-externals.md`: four
//! feedback combs side by side (29.7, 37.1, 41.1 and 43.7 ms), their plain sum
//! through two Schroeder all-passes in series (5 and 1.7 ms). Every gain —
//! the all-passes' too — is `expf(−6.9078·τ/T)`, so each loop falls 60 dB in
//! the reverb time T. Line lengths are `(int)(τ·sr)` at the running rate. A
//! reverb time of nothing or less is taken as 1 ms, as `cverb~` takes it.
//!
//! `cverb~` is mono in and mono out, and wholly wet. The stereo here is ours:
//! two of them, one per channel, independent, as the Rough Reverb that used it
//! describes itself. The slot's own Wet/dry sets the mix.
//!
//! The order of each operation follows the decode's perform loop; where the
//! decode writes `−6.9078·τ/T` it is evaluated left to right in f32.

use crate::spec::Role;
use crate::{Descriptor, Group, Kernel, Panel, ParamSpec};

/// `_revtimes`: the combs' and then the all-passes' loop times, in seconds.
pub const COMBS_S: [f32; 4] = [0.0297, 0.0371, 0.0411, 0.0437];
pub const ALLPASSES_S: [f32; 2] = [0.005, 0.0017];
/// ln 1000: a loop that falls by this many nepers in T is down 60 dB.
const LN_1000: f32 = 6.9078;

static PARAMS: &[ParamSpec] = &[ParamSpec::new("revtimeMs", "Reverb time", 1.0, 20_000.0, 2_000.0)
    .log()
    .unit("ms")
    .smooth(0.0)
    .role(Role::Time)];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "jcrev",
    code: 103,
    version: 1,
    label: "JC Reverb",
    category: crate::Category::Reverb,
    params: PARAMS,
    panel: Panel {
        groups: &[Group { label: "Time", keys: &["revtimeMs"] }],
        primary: &["revtimeMs"],
        displays: &[],
        pad: None,
    },
    source: "Pluggo cverb~ (Cycling '74): the Schroeder/Chowning JCRev, sources/reports/003-pluggo-externals.md. Mono; the stereo pair is ours",
    sound: None,
};

struct Line {
    buf: Vec<f32>,
    p: usize,
}

impl Line {
    fn new(tau: f32, sr: f32) -> Line {
        Line { buf: vec![0.0; ((tau * sr) as usize).max(1)], p: 0 }
    }

    #[inline]
    fn advance(&mut self) {
        self.p += 1;
        if self.p >= self.buf.len() {
            self.p = 0;
        }
    }
}

/// One `cverb~`.
struct Cverb {
    combs: [Line; 4],
    allpasses: [Line; 2],
}

impl Cverb {
    fn new(sr: f32) -> Cverb {
        Cverb {
            combs: core::array::from_fn(|i| Line::new(COMBS_S[i], sr)),
            allpasses: core::array::from_fn(|i| Line::new(ALLPASSES_S[i], sr)),
        }
    }

    #[inline]
    fn sample(&mut self, x: f32, g: &[f32; 6]) -> f32 {
        let mut d = [0.0f32; 4];
        for (i, c) in self.combs.iter_mut().enumerate() {
            d[i] = c.buf[c.p];
            c.buf[c.p] = d[i] * g[i] + x;
            c.advance();
        }
        let mut y = d[0] + d[1] + d[2] + d[3];
        for (j, a) in self.allpasses.iter_mut().enumerate() {
            let gj = g[4 + j];
            let dd = a.buf[a.p];
            let w = y + gj * dd;
            a.buf[a.p] = w;
            a.advance();
            y = dd - gj * w;
        }
        y
    }

    fn clear(&mut self) {
        for l in self.combs.iter_mut().chain(self.allpasses.iter_mut()) {
            l.buf.fill(0.0);
            l.p = 0;
        }
    }
}

pub struct Jcrev {
    revtime_ms: f32,
    g: [f32; 6],
    sides: [Cverb; 2],
}

impl Jcrev {
    fn gains(&mut self) {
        let t = self.revtime_ms * 0.001;
        let t = if t <= 0.0 { 0.001 } else { t };
        for (g, tau) in self.g.iter_mut().zip(COMBS_S.iter().chain(&ALLPASSES_S)) {
            *g = (-LN_1000 * tau / t).exp();
        }
    }
}

impl Kernel for Jcrev {
    fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let mut k = Jcrev { revtime_ms: PARAMS[0].default, g: [0.0; 6], sides: [Cverb::new(sr), Cverb::new(sr)] };
        k.gains();
        k
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if index == 0 {
            self.revtime_ms = PARAMS[0].clamp(value);
            self.gains();
        }
    }

    fn param(&self, index: usize) -> f32 {
        if index == 0 { self.revtime_ms } else { 0.0 }
    }

    /// `clear`.
    fn reset(&mut self) {
        for s in &mut self.sides {
            s.clear();
        }
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        for (x, side) in frame.iter_mut().zip(self.sides.iter_mut()) {
            *x = side.sample(*x, &self.g);
        }
    }

    fn tail_seconds(&self) -> f32 {
        (self.revtime_ms * 0.001 + COMBS_S[3]).min(60.0)
    }
}
