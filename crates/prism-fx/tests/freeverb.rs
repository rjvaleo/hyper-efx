//! EX-S for Comb Hall: Jezar's Freeverb as decoded from Black Water Reverb
//! (`sources/reports/001-black-water-reverb.md`). The constants are pinned by
//! where an impulse first arrives and with what value, which depend on nothing
//! but the decode's tables; the laws and the order of every operation are
//! pinned by a second implementation written from the decode's own
//! pseudo-code (§2, §3, §6), which the kernel must match bit for bit.

use prism_fx::{make, DynKernel};

fn kernel(sr: f32, set: &[(&str, f32)]) -> Box<dyn DynKernel> {
    let mut k = make("freeverb", sr).unwrap();
    for (key, v) in set {
        let i = k.descriptor().params.iter().position(|p| p.key == *key).unwrap();
        k.set_param(i, *v);
    }
    k
}

fn impulse(k: &mut Box<dyn DynKernel>, n: usize) -> (Vec<f32>, Vec<f32>) {
    let mut l = vec![0.0f32; n];
    let mut r = vec![0.0f32; n];
    l[0] = 1.0;
    k.process(&mut l, &mut r);
    (l, r)
}

#[test]
fn the_first_arrivals_are_the_shortest_combs_at_the_fixed_gain() {
    // Defaults: Wet 1/3 → 1.0, Width 1 → wet1 1, wet2 0, Dry 0.
    let (l, r) = impulse(&mut kernel(44_100.0, &[]), 1400);
    assert!(l[..1116].iter().all(|&x| x == 0.0), "nothing before comb L1 (1116)");
    assert_eq!(l[1116], 0.015, "0.015 × 1 through four all-passes' direct terms (−1)^4");
    assert!(r[..1139].iter().all(|&x| x == 0.0), "nothing before comb R1 (1116 + 23)");
    assert_eq!(r[1139], 0.015);
    // The last all-pass (225, g 0.5, output b − x) returns it inverted.
    assert_eq!(l[1116 + 225], -0.015);
}

#[test]
fn the_lengths_are_samples_not_seconds_at_any_rate() {
    for sr in [44_100.0, 48_000.0, 96_000.0] {
        let (l, _) = impulse(&mut kernel(sr, &[]), 1200);
        assert_eq!(l.iter().position(|&x| x != 0.0), Some(1116), "{sr}");
    }
}

#[test]
fn width_wet_and_dry_follow_the_originals_laws() {
    // Width 0: wet1 = wet2 = wet/2, so each side hears the other's comb.
    let (l, r) = impulse(&mut kernel(44_100.0, &[("width", 0.0)]), 1200);
    assert_eq!(l[1116], 0.015 * 0.5);
    assert_eq!(r[1116], 0.015 * 0.5);
    // Wet v → 3·v.
    let (l, _) = impulse(&mut kernel(44_100.0, &[("wet", 1.0)]), 1200);
    assert_eq!(l[1116], 0.015 * 3.0);
    // Dry v → 2·v, straight through.
    let (l, r) = impulse(&mut kernel(44_100.0, &[("dry", 0.5)]), 4);
    assert_eq!((l[0], r[0]), (1.0, 0.0));
    let (l, _) = impulse(&mut kernel(44_100.0, &[("dry", 1.0)]), 4);
    assert_eq!(l[0], 2.0);
}

#[test]
fn freeze_mutes_the_input_and_holds_the_tail() {
    let mut k = kernel(44_100.0, &[("freeze", 1.0)]);
    let (l, r) = impulse(&mut k, 44_100);
    assert!(l.iter().chain(&r).all(|&x| x == 0.0), "a frozen reverb takes no input");
    // Fed, then frozen: the combs hold at feedback 1 with no damping.
    let mut k = kernel(44_100.0, &[]);
    let mut noise = 7u32;
    let mut l: Vec<f32> = (0..22_050).map(|_| { noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223); (noise >> 9) as f32 / (1u32 << 23) as f32 - 0.5 }).collect();
    let mut r = l.clone();
    k.process(&mut l, &mut r);
    k.set_param(0, 1.0);
    let rms = |k: &mut Box<dyn DynKernel>| {
        let (mut l, mut r) = (vec![0.0f32; 44_100], vec![0.0f32; 44_100]);
        k.process(&mut l, &mut r);
        (l.iter().map(|x| x * x).sum::<f32>() / l.len() as f32).sqrt()
    };
    let (a, b) = (rms(&mut k), { rms(&mut k); rms(&mut k) });
    assert!(a > 1e-4 && (b / a - 1.0).abs() < 0.02, "held: {a} then {b}");
}

/// `revmodel`, written again from the decode's pseudo-code alone.
struct Reference {
    combs: Vec<(Vec<f32>, usize, f32)>,
    allpasses: Vec<(Vec<f32>, usize)>,
    gain: f32, roomsize1: f32, damp1: f32, damp2: f32, wet1: f32, wet2: f32, dry: f32,
}

fn undenormalise(x: f32) -> f32 { if x.to_bits() & 0x7f80_0000 == 0 { 0.0 } else { x } }

impl Reference {
    fn new() -> Self {
        let mut combs = Vec::new();
        for n in [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617] { combs.push((vec![0.0; n], 0, 0.0)); }
        for n in [1139, 1211, 1300, 1379, 1445, 1514, 1580, 1640] { combs.push((vec![0.0; n], 0, 0.0)); }
        let mut allpasses = Vec::new();
        for n in [556, 441, 341, 225, 579, 464, 364, 248] { allpasses.push((vec![0.0; n], 0)); }
        Reference { combs, allpasses, gain: 0.0, roomsize1: 0.0, damp1: 0.0, damp2: 0.0, wet1: 0.0, wet2: 0.0, dry: 0.0 }
    }
    /// §3's laws and §2.3's update, from the normalised values.
    fn update(&mut self, v: [f32; 6]) {
        let (mode, room, damp, width, wet, dry) = (v[0], v[1] * 0.28 + 0.7, v[2] * 0.4, v[3], v[4] * 3.0, v[5] * 2.0);
        self.wet1 = wet * (width / 2.0 + 0.5);
        self.wet2 = wet * ((1.0 - width) / 2.0);
        if mode >= 0.5 { self.roomsize1 = 1.0; self.damp1 = 0.0; self.gain = 0.0; }
        else { self.roomsize1 = room; self.damp1 = damp; self.gain = 0.015; }
        self.damp2 = 1.0 - self.damp1;
        self.dry = dry;
    }
    fn frame(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        let input = (in_l + in_r) * self.gain;
        let (mut out_l, mut out_r) = (0.0f32, 0.0f32);
        for i in 0..8 {
            for (side, out) in [(0, &mut out_l), (8, &mut out_r)] {
                let (buf, idx, store) = &mut self.combs[side + i];
                let output = undenormalise(buf[*idx]);
                *store = undenormalise(output * self.damp2 + *store * self.damp1);
                buf[*idx] = input + *store * self.roomsize1;
                *idx = (*idx + 1) % buf.len();
                *out += output;
            }
        }
        for i in 0..4 {
            for (side, out) in [(0, &mut out_l), (4, &mut out_r)] {
                let (buf, idx) = &mut self.allpasses[side + i];
                let bufout = undenormalise(buf[*idx]);
                buf[*idx] = *out + bufout * 0.5;
                *idx = (*idx + 1) % buf.len();
                *out = bufout - *out;
            }
        }
        (out_l * self.wet1 + out_r * self.wet2 + in_l * self.dry, out_r * self.wet1 + out_l * self.wet2 + in_r * self.dry)
    }
}

#[test]
fn every_sample_is_the_decodes_revmodel_through_moves_and_freeze() {
    // Two seconds of noise in stereo, with every control moved partway.
    let mut k = kernel(48_000.0, &[]);
    let mut reference = Reference::new();
    let mut v = [0.0, 0.5, 0.5, 1.0, 1.0 / 3.0, 0.0];
    reference.update(v);
    let moves: [(usize, usize, f32); 7] = [(9_000, 1, 0.93), (20_000, 2, 0.1), (31_000, 3, 0.25), (42_000, 4, 0.8), (53_000, 5, 0.6), (64_000, 0, 1.0), (80_000, 0, 0.0)];
    let mut x = 12_345u32;
    let mut next = || { x ^= x << 13; x ^= x >> 17; x ^= x << 5; x as f32 / u32::MAX as f32 - 0.5 };
    let mut m = 0;
    for n in 0..96_000 {
        while m < moves.len() && moves[m].0 == n {
            let (_, i, val) = moves[m];
            k.set_param(i, val);
            v[i] = val;
            reference.update(v);
            m += 1;
        }
        let (a, b) = (next(), next());
        let (mut l, mut r) = ([a], [b]);
        k.process(&mut l, &mut r);
        let (wl, wr) = reference.frame(a, b);
        assert_eq!((l[0].to_bits(), r[0].to_bits()), (wl.to_bits(), wr.to_bits()), "frame {n}");
    }
}
