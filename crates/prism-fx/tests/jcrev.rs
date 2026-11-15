//! EX-S for JC Reverb: Pluggo's `cverb~`, the Schroeder/Chowning "JCRev", as
//! decoded in `sources/reports/003-pluggo-externals.md`. The tables are pinned
//! by where an impulse first comes out and with what value; the laws and the
//! order of every operation by a second `cverb~` written from the decode's
//! perform loop, which each channel must match bit for bit.

use prism_fx::{make, DynKernel};

/// `_revtimes`, from the decode.
const COMBS_S: [f32; 4] = [0.0297, 0.0371, 0.0411, 0.0437];
const ALLPASSES_S: [f32; 2] = [0.005, 0.0017];

fn kernel(sr: f32, revtime_ms: f32) -> Box<dyn DynKernel> {
    let mut k = make("jcrev", sr).unwrap();
    k.set_param(0, revtime_ms);
    k
}

fn gain(tau: f32, t: f32) -> f32 {
    (-6.9078f32 * tau / t).exp()
}

#[test]
fn an_impulse_first_comes_out_after_the_shortest_comb_through_both_allpasses() {
    let sr = 48_000.0f32;
    let len = (COMBS_S[0] * sr) as usize;
    assert_eq!(len, 1425, "(int)(0.0297 × 48000)");
    let mut k = kernel(sr, 1000.0);
    let (mut l, mut r) = (vec![0.0f32; 1500], vec![0.0f32; 1500]);
    l[0] = 1.0;
    k.process(&mut l, &mut r);
    assert!(l[..len].iter().all(|&x| x == 0.0));
    // Into the all-passes as 1: the first gives −g5, the second −g6·(−g5).
    let (g5, g6) = (gain(ALLPASSES_S[0], 1.0), gain(ALLPASSES_S[1], 1.0));
    let w2 = -g5;
    assert_eq!(l[len], 0.0 - g6 * w2);
    // The channels are two independent cverb~s.
    assert!(r.iter().all(|&x| x == 0.0));
}

#[test]
fn each_comb_falls_sixty_db_in_the_reverb_time() {
    for t in [0.5f32, 2.0, 7.5] {
        for tau in COMBS_S {
            let after = gain(tau, t).powf(t / tau);
            assert!((after - 0.001).abs() < 1e-5, "{tau} s at {t} s: {after}");
        }
    }
}

/// `cverb~`'s perform loop, written again from the decode alone.
struct Reference {
    combs: Vec<(Vec<f32>, usize)>,
    allpasses: Vec<(Vec<f32>, usize)>,
    g: [f32; 6],
}

impl Reference {
    fn new(sr: f32) -> Self {
        let line = |tau: f32| (vec![0.0; (tau * sr) as usize], 0usize);
        Reference { combs: COMBS_S.iter().map(|&t| line(t)).collect(), allpasses: ALLPASSES_S.iter().map(|&t| line(t)).collect(), g: [0.0; 6] }
    }
    fn revtime(&mut self, ms: f32) {
        let t = if ms * 0.001 <= 0.0 { 0.001 } else { ms * 0.001 };
        for (i, tau) in COMBS_S.iter().chain(&ALLPASSES_S).enumerate() {
            self.g[i] = (-6.9078f32 * tau / t).exp();
        }
    }
    fn sample(&mut self, x: f32) -> f32 {
        let mut d = [0.0f32; 4];
        for (i, (line, p)) in self.combs.iter_mut().enumerate() {
            d[i] = line[*p];
            line[*p] = d[i] * self.g[i] + x;
            *p = (*p + 1) % line.len();
        }
        let mut y = d[0] + d[1] + d[2] + d[3];
        for (j, (line, p)) in self.allpasses.iter_mut().enumerate() {
            let g = self.g[4 + j];
            let dd = line[*p];
            let w = y + g * dd;
            line[*p] = w;
            *p = (*p + 1) % line.len();
            y = dd - g * w;
        }
        y
    }
}

#[test]
fn every_sample_is_the_decodes_cverb_on_each_channel() {
    for sr in [44_100.0f32, 48_000.0] {
        let mut k = kernel(sr, 1800.0);
        let (mut rl, mut rr) = (Reference::new(sr), Reference::new(sr));
        rl.revtime(1800.0);
        rr.revtime(1800.0);
        let mut x = 99u32;
        let mut next = || { x ^= x << 13; x ^= x >> 17; x ^= x << 5; x as f32 / u32::MAX as f32 - 0.5 };
        for n in 0..96_000 {
            if n == 30_000 {
                k.set_param(0, 9_000.0);
                rl.revtime(9_000.0);
                rr.revtime(9_000.0);
            }
            if n == 70_000 {
                k.set_param(0, 1.0);
                rl.revtime(1.0);
                rr.revtime(1.0);
            }
            let (a, b) = (next(), next());
            let (mut l, mut r) = ([a], [b]);
            k.process(&mut l, &mut r);
            assert_eq!((l[0].to_bits(), r[0].to_bits()), (rl.sample(a).to_bits(), rr.sample(b).to_bits()), "{sr} frame {n}");
        }
    }
}
