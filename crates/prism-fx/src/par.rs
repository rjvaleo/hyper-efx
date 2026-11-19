//! Parity renders: a fingerprint of exactly what a kernel outputs.
//!
//! The suite layer (`suite`) wraps existing kernels as modes, and that must not
//! change a single bit of their sound. Before it was built, every kernel in the
//! catalogue was rendered here under a fixed set of scenarios and the hashes
//! were recorded (`tests/golden/par.tsv`). After every phase the same renders,
//! through the kernel directly and through the module path, must hash the same.
//!
//! The scenarios are the plan's (SUITE-PLAN §5, "PAR"): defaults; each control
//! at both ends; controls moved while running; mono and stereo; blocks of 512
//! and 61. A hash covers the raw bits of every output sample, so "the same"
//! means bit-identical, not close.

use crate::contract::{test_sound, SAMPLE_RATE};
use crate::DynKernel;

/// Frames rendered per scenario: a third of a second at 48 kHz, enough for
/// every kernel's smoothing to land and its first echoes to come back.
pub const FRAMES: usize = 16_000;

/// FNV-1a over the bits of a render, left then right.
pub fn hash(left: &[f32], right: &[f32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for x in left.iter().chain(right.iter()) {
        for b in x.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// The scenario input: a tone, a burst of noise, a click and silence, different
/// per side. Fixed for ever: changing it invalidates every golden.
pub fn input() -> (Vec<f32>, Vec<f32>) {
    // 16,000 stereo frames, interleaved little-endian f32 (128,000 bytes).
    // Captured from this function's original MacBook input, before replacing
    // system sin: every machine must feed the goldens the same sample bits.
    let bytes = include_bytes!("../tests/fixtures/input.f32");
    assert_eq!(bytes.len(), FRAMES * 2 * std::mem::size_of::<f32>());
    let mut l = Vec::with_capacity(FRAMES);
    let mut r = Vec::with_capacity(FRAMES);
    for frame in bytes.chunks_exact(8) {
        l.push(f32::from_le_bytes(frame[..4].try_into().unwrap()));
        r.push(f32::from_le_bytes(frame[4..].try_into().unwrap()));
    }
    (l, r)
}

/// How a scenario drives the kernel.
#[derive(Clone, Copy)]
enum Feed {
    Stereo,
    /// The host's mono path: `tick_mono` where the kernel has one, else the
    /// left fed to both sides and the left kept.
    Mono,
}

/// Run one scenario on a fresh kernel. `moves` are (frame, index, value) set
/// while running, at block starts.
fn run(k: &mut dyn DynKernel, feed: Feed, block: usize, moves: &[(usize, usize, f32)]) -> u64 {
    let (mut l, mut r) = input();
    let mut at = 0;
    while at < FRAMES {
        let end = (at + block).min(FRAMES);
        for &(f, i, v) in moves {
            if f >= at && f < end {
                k.set_param(i, v);
            }
        }
        match feed {
            Feed::Stereo => k.process(&mut l[at..end], &mut r[at..end]),
            Feed::Mono => {
                for n in at..end {
                    if k.has_mono() {
                        k.tick_mono(&mut l[n]);
                    } else {
                        let mut fr = [l[n], l[n]];
                        k.tick(&mut fr);
                        l[n] = fr[0];
                    }
                    r[n] = 0.0;
                }
            }
        }
        at = end;
    }
    hash(&l, &r)
}

/// Build a kernel the way a host does: made, handed its sound if it takes one.
fn fresh(make: &dyn Fn() -> Box<dyn DynKernel>) -> Box<dyn DynKernel> {
    let mut k = make();
    if k.descriptor().sound.is_some() {
        let (s, ch, sr) = test_sound();
        k.set_sound(&s, ch, sr);
    }
    k
}

/// Every scenario for one kernel, as (name, hash), in a fixed order.
pub fn scenarios(make: &dyn Fn() -> Box<dyn DynKernel>) -> Vec<(String, u64)> {
    let d = fresh(make).descriptor();
    let mut out = Vec::new();
    let mut one = |name: String, feed: Feed, block: usize, setup: &[(usize, f32)], moves: &[(usize, usize, f32)]| {
        let mut k = fresh(make);
        for &(i, v) in setup {
            k.set_param(i, v);
        }
        k.reset();
        out.push((name, run(k.as_mut(), feed, block, moves)));
    };
    one("default/512".into(), Feed::Stereo, 512, &[], &[]);
    one("default/61".into(), Feed::Stereo, 61, &[], &[]);
    one("default/mono".into(), Feed::Mono, 512, &[], &[]);
    for (i, p) in d.params.iter().enumerate() {
        if p.inert {
            continue;
        }
        let (lo, hi) = (p.min, p.max);
        one(format!("{}=min", p.key), Feed::Stereo, 512, &[(i, lo)], &[]);
        one(format!("{}=max", p.key), Feed::Stereo, 512, &[(i, hi)], &[]);
        // moved while running: to the far end a third of the way in, back at two thirds
        let far = if (p.default - lo).abs() > (p.default - hi).abs() { lo } else { hi };
        one(format!("{}~moved", p.key), Feed::Stereo, 61, &[], &[(FRAMES / 3, i, far), (2 * FRAMES / 3, i, p.default)]);
    }
    out
}
