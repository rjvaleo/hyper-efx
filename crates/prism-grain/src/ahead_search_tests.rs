//! Reproducible standalone measurements; run through the benchmark lock.
use crate::{
    hstream::{HybridStream, Parts},
    hybrid::HybridParams,
    noise::Morph,
    nstream::NoiseStream,
    stream::{Streamer, StretchParams, WsolaStream},
    transient::TimeMap,
};
fn params() -> StretchParams {
    StretchParams {
        ratio: 2.0,
        window_ms: 40.0,
        sample_rate: 48000,
        wsola: Default::default(),
        vocoder: Default::default(),
        grain: Default::default(),
        soundhack: Default::default(),
    }
}
fn source() -> Vec<f32> {
    (0..192000)
        .map(|i| ((i as f32 * 0.017).sin() * 0.4) + (i as f32 * 0.113).sin() * 0.07)
        .collect()
}
fn hash(h: &mut u64, x: f32) {
    *h = (*h ^ x.to_bits() as u64).wrapping_mul(1099511628211);
}
#[cfg(target_os = "macos")]
fn cpu() -> f64 {
    unsafe extern "C" {
        fn clock_gettime(id: i32, t: *mut [i64; 2]) -> i32;
    }
    let mut t = [0; 2];
    assert_eq!(unsafe { clock_gettime(16, &mut t) }, 0);
    t[0] as f64 + t[1] as f64 * 1e-9
}
#[cfg(not(target_os = "macos"))]
fn cpu() -> f64 {
    panic!("measurement uses macOS thread CPU clock")
}
fn bench(name: &str, mut f: impl FnMut(usize) -> u64) {
    let mut times = Vec::new();
    let mut h = 0;
    for i in 0..1200 {
        let t = cpu();
        h ^= f(i);
        let dt = (cpu() - t) * 1000.0;
        if i >= 100 {
            times.push(dt);
        }
    }
    times.sort_by(f64::total_cmp);
    println!(
        "FOCUS name={name} median_ms={:.9} p99_ms={:.9} worst_ms={:.9} hash={h:016x}",
        times[times.len() / 2],
        times[times.len() * 99 / 100],
        times[times.len() - 1]
    );
}
#[test]
#[ignore = "standalone task 045 measurements"]
fn focused_cost() {
    let src = source();
    let mut p = params();
    let mut out = [0.0; 128];
    let mut noise = NoiseStream::new(128, 1);
    bench("noise", |_| {
        noise.render(&mut out, 1, &src, &p, Morph::default());
        let mut h = 0;
        for &x in &out {
            hash(&mut h, x);
        }
        h
    });
    let parts = Parts {
        harmonic: src.clone(),
        percussive: src.clone(),
        residual: src.clone(),
        channels: 1,
        split: None,
    };
    for (name, h) in [
        (
            "hybrid-zero",
            HybridParams {
                harmonic_level: 0.0,
                percussive_level: 0.0,
                residual_level: 0.0,
                ..Default::default()
            },
        ),
        ("hybrid-one", HybridParams::default()),
    ] {
        let mut deck = HybridStream::new(128, 1, 48000);
        bench(name, |_| {
            deck.render(&mut out, 1, &parts, &p, h);
            let mut hashv = 0;
            for &x in &out {
                hash(&mut hashv, x);
            }
            hashv
        });
    }
    let anchors = (0..1000)
        .map(|i| (i as f64 * 192.0, i as f64 * 384.0))
        .collect();
    let map = TimeMap::from_anchors(anchors);
    bench("transient", |i| {
        let mut h = 0;
        for k in 0..128 {
            let x = map.input_at(((i * 128 + k) % 380000) as f64);
            h ^= x.to_bits();
        }
        h
    });
    let mut deck = WsolaStream::new(128, 1, 48000);
    bench("window-moving", |i| {
        p.grain.envelope = (i % 71) as f32 / 70.0;
        deck.render(&mut out, 1, &src, &p);
        let mut h = 0;
        for &x in &out {
            hash(&mut h, x);
        }
        h
    });
}
#[test]
#[ignore = "standalone task 045 render fingerprint"]
fn moving_fingerprint() {
    let src = source();
    let mut total = 0xcbf29ce484222325;
    for ch in [1, 2] {
        let src: Vec<_> = src
            .iter()
            .flat_map(|&x| std::iter::repeat(x).take(ch))
            .collect();
        for block in [1, 31, 128, 511] {
            let mut p = params();
            let mut deck = WsolaStream::new(512, ch, 48000);
            let mut out = vec![0.0; block * ch];
            for i in 0..2000 {
                if i % 43 == 0 {
                    p.ratio = 0.7 + (i % 7) as f32 * 0.31;
                    p.window_ms = 10.0 + (i % 5) as f32 * 13.0;
                    p.wsola.stride = 1 + (i % 9) as u32;
                    p.wsola.search_ms = (i % 31) as f32;
                    p.wsola.splice = match i % 3 {
                        0 => crate::stretch::Splice::Similar,
                        1 => crate::stretch::Splice::Different,
                        _ => crate::stretch::Splice::Loudest,
                    };
                    p.grain.envelope = (i % 11) as f32 / 10.0;
                }
                if i % 137 == 0 {
                    deck.seek((i * 59) as u64, src.len() / ch, &p);
                }
                deck.render(&mut out, ch, &src, &p);
                for &x in &out {
                    hash(&mut total, x);
                }
            }
        }
    }
    println!("MOVING_HASH {total:016x}");
}

#[test]
fn split_search_keeps_scores_order_and_ties() {
    use crate::stretch::{best_offset, OffsetSearch, Splice};
    let src = source();
    let expect = &src[311..1335];
    for channels in [1, 2] {
        for centre in [0, 31, 800, 191999] {
            for len in [1, 16, 512] {
                for radius in [0, 1, 480, 1000] {
                    for stride in [1, 4, 256] {
                        for splice in [Splice::Similar, Splice::Different, Splice::Loudest] {
                            let p = crate::stretch::WsolaParams {
                                stride,
                                splice,
                                ..Default::default()
                            };
                            let expected =
                                best_offset(&src, channels, centre, radius, expect, len, p);
                            let mut job =
                                OffsetSearch::new(src.len(), channels, centre, radius, len, p);
                            while job.remaining() > 0 {
                                job.advance(&src, channels, expect, len, p, 7);
                            }
                            assert_eq!(job.result(), expected);
                        }
                    }
                }
            }
        }
    }
    let zero = [0.0; 200];
    let p = Default::default();
    let mut job = OffsetSearch::new(200, 1, 90, 80, 16, p);
    job.advance(&zero, 1, &zero, 16, p, usize::MAX);
    assert_eq!(job.result(), 10);
}
#[test]
fn cached_map_matches_linear_walk_at_seeks_and_ties() {
    let anchors = vec![
        (0.0, 0.0),
        (5.0, 10.0),
        (7.0, 10.0),
        (31.0, 53.0),
        (100.0, 200.0),
    ];
    let m = TimeMap::from_anchors(anchors.clone());
    for out in (0..300)
        .map(|i| i as f64)
        .chain((0..300).rev().map(|i| i as f64))
        .chain([f64::NAN, -1.0, 10.0])
    {
        let expected = if out <= anchors[0].1 {
            anchors[0].0
        } else if out >= anchors.last().unwrap().1 {
            anchors.last().unwrap().0
        } else {
            anchors
                .windows(2)
                .find_map(|w| {
                    let [(i0, o0), (i1, o1)] = [w[0], w[1]];
                    if out <= o1 {
                        Some(i0 + (out - o0) / (o1 - o0) * (i1 - i0))
                    } else {
                        None
                    }
                })
                .unwrap_or(anchors.last().unwrap().0)
        };
        assert_eq!(m.input_at(out).to_bits(), expected.to_bits());
    }
}
#[test]
fn running_sum_moves_a_noise_bin() {
    let mag: Vec<f32> = (0..1025)
        .map(|i| ((i as f32 * 0.017).sin() * 0.4).abs())
        .collect();
    let half = 4;
    let mut sum = 0f32;
    for &x in &mag[..5] {
        sum += x * x;
    }
    let mut first = None;
    for idx in 0usize..mag.len() {
        let lo = idx.saturating_sub(half);
        let hi = (idx + half + 1).min(mag.len());
        if idx > 0 {
            let old_lo = (idx - 1).saturating_sub(half);
            let old_hi = (idx + half).min(mag.len());
            if lo > old_lo {
                sum -= mag[old_lo] * mag[old_lo];
            }
            if hi > old_hi {
                sum += mag[hi - 1] * mag[hi - 1];
            }
        }
        let mut reference = 0.0;
        for &x in &mag[lo..hi] {
            reference += x * x;
        }
        let a = (reference / (hi - lo) as f32).sqrt();
        let b = (sum / (hi - lo) as f32).sqrt();
        if a.to_bits() != b.to_bits() && first.is_none() {
            first = Some((idx, a.to_bits(), b.to_bits()));
        }
    }
    println!("RUNNING_SUM_FIRST {first:?}");
    assert!(first.is_some());
}
#[test]
fn skipping_muted_parts_changes_their_future() {
    let src = source();
    let parts = Parts {
        harmonic: src.clone(),
        percussive: src.clone(),
        residual: src,
        channels: 1,
        split: None,
    };
    let p = params();
    let mut a = HybridStream::new(128, 1, 48000);
    let mut b = HybridStream::new(128, 1, 48000);
    let mut x = [0.0; 128];
    let mut y = [0.0; 128];
    let muted = HybridParams {
        harmonic_level: 0.0,
        percussive_level: 0.0,
        residual_level: 0.0,
        ..Default::default()
    };
    for _ in 0..100 {
        a.render(&mut x, 1, &parts, &p, muted);
    }
    a.render(&mut x, 1, &parts, &p, Default::default());
    b.render(&mut y, 1, &parts, &p, Default::default());
    let first = x
        .iter()
        .zip(y)
        .position(|(a, b)| a.to_bits() != b.to_bits());
    println!("SKIP_FIRST {first:?}");
    assert!(first.is_some());
    assert_eq!((-1.0f32 * 0.0).to_bits(), 0x80000000);
    assert!((f32::NAN * 0.0).is_nan());
}

#[test]
fn spread_stream_matches_deadline_stream_with_controls_and_seeks() {
    let src = source();
    let mut p = params();
    let mut ahead = WsolaStream::new(512, 1, 48000);
    let mut reference = WsolaStream::new(512, 1, 48000);
    reference.search_at_deadline();
    for i in 0..3000 {
        if i % 17 == 0 {
            p.ratio = 0.5 + (i % 9) as f32 * 0.25;
            p.wsola.stride = 1 + (i % 8) as u32;
            p.wsola.search_ms = (i % 29) as f32;
            p.window_ms = 10.0 + (i % 5) as f32 * 11.0;
            p.grain.envelope = (i % 13) as f32 / 12.0;
        }
        if i % 113 == 0 {
            ahead.seek((i * 97) as u64, src.len(), &p);
            reference.seek((i * 97) as u64, src.len(), &p);
        }
        let n = [1, 31, 64, 128, 511][i % 5];
        let mut a = [0.0; 512];
        let mut b = [0.0; 512];
        ahead.render(&mut a[..n], 1, &src, &p);
        reference.render(&mut b[..n], 1, &src, &p);
        assert!(
            a[..n]
                .iter()
                .zip(&b[..n])
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "block {i}"
        );
    }
}
#[test]
fn hoisted_power_is_the_original_window() {
    use crate::stretch::{shape_at, WinShape};
    for n in [1, 64, 1920] {
        for skew in [0.0, 0.49999, 0.5, 0.8, 1.0] {
            for shape in [WinShape::Hann, WinShape::Rect, WinShape::Triangle] {
                for i in 0..n {
                    let expected = if n <= 1 {
                        1.0
                    } else {
                        let t = i as f32 / (n - 1) as f32;
                        let t = if (skew - 0.5f32).abs() < 1e-4 {
                            t
                        } else {
                            t.powf(4f32.powf(skew * 2.0 - 1.0))
                        };
                        match shape {
                            WinShape::Hann => 0.5 - 0.5 * (2.0 * std::f32::consts::PI * t).cos(),
                            WinShape::Rect => 1.0,
                            WinShape::Triangle => 1.0 - (t * 2.0 - 1.0).abs(),
                        }
                    };
                    assert_eq!(shape_at(i, n, shape, skew).to_bits(), expected.to_bits());
                }
            }
        }
    }
}
