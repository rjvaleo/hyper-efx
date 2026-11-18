//! Place: our stage around a reverb mode (SUITE-PLAN P4, the Network face).
//! It puts the sound in a room before the mode: the direct path's delay,
//! level, air and pan, a height cue, and first reflections from every wall,
//! the floor and the ceiling; after the mode, Spread sets the width. Off, it is
//! nothing at all: the mode stays exact.

use prism_fx::place::{taps, Place, PlaceSettings, SPEED_OF_SOUND};

const SR: f32 = 48_000.0;

fn noise(n: usize, seed: u32) -> Vec<f32> {
    let mut x = seed;
    (0..n).map(|_| { x ^= x << 13; x ^= x >> 17; x ^= x << 5; x as f32 / u32::MAX as f32 - 0.5 }).collect()
}

#[test]
fn off_is_nothing_at_all() {
    let mut p = Place::new(SR);
    p.set(PlaceSettings { on: false, ..PlaceSettings::default() });
    let (l0, r0) = (noise(4000, 1), noise(4000, 2));
    let (mut l, mut r) = (l0.clone(), r0.clone());
    p.before(&mut l, &mut r);
    p.after(&mut l, &mut r);
    assert_eq!(l.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), l0.iter().map(|x| x.to_bits()).collect::<Vec<_>>());
    assert_eq!(r.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), r0.iter().map(|x| x.to_bits()).collect::<Vec<_>>());
}

#[test]
fn the_direct_sound_arrives_after_its_distance_at_the_speed_of_sound() {
    let s = PlaceSettings::default();
    let t = taps(&s);
    let direct = &t[0];
    assert!(direct.direct);
    // and every reflection travels further than it
    assert!(t[1..].iter().all(|x| x.metres > direct.metres));
    let mut p = Place::new(SR);
    p.set(s);
    let (mut l, mut r) = (vec![0.0f32; 48_000], vec![0.0f32; 48_000]);
    l[0] = 1.0;
    r[0] = 1.0;
    p.before(&mut l, &mut r);
    let first = l.iter().zip(&r).position(|(a, b)| a.abs() > 1e-4 || b.abs() > 1e-4).unwrap();
    let want = (direct.metres / SPEED_OF_SOUND * SR).round() as usize;
    assert!((first as i64 - want as i64).abs() <= 1, "first arrival {first}, want {want}");
}

#[test]
fn a_room_has_one_first_reflection_per_wall_and_one_each_from_floor_and_ceiling() {
    let s = PlaceSettings { shape: 0, ..PlaceSettings::default() }; // Room: four walls
    assert_eq!(taps(&s).iter().filter(|t| !t.direct).count(), 6);
    let plate = PlaceSettings { shape: 5, ..PlaceSettings::default() };
    assert_eq!(taps(&plate).len(), 1, "a plate has no room, only the direct sound");
}

#[test]
fn the_taps_carry_one_unit_of_energy_wherever_the_sound_is() {
    for dist in [0.0, 40.0, 100.0] {
        for shape in 0..5 {
            let s = PlaceSettings { dist, shape, ..PlaceSettings::default() };
            let e: f32 = taps(&s).iter().map(|t| t.gain * t.gain).sum();
            assert!((e - 1.0).abs() < 1e-4, "shape {shape} dist {dist}: {e}");
        }
    }
    // further away, the direct sound is a smaller share of it
    let near = taps(&PlaceSettings { dist: 0.0, ..PlaceSettings::default() })[0].gain;
    let far = taps(&PlaceSettings { dist: 100.0, ..PlaceSettings::default() })[0].gain;
    assert!(far < near, "{far} vs {near}");
}

#[test]
fn a_sound_on_the_left_is_louder_on_the_left() {
    let s = PlaceSettings { pos: -100.0, ..PlaceSettings::default() };
    let d = &taps(&s)[0];
    assert!(d.left > d.right, "{} vs {}", d.left, d.right);
    let c = &taps(&PlaceSettings { pos: 0.0, ..PlaceSettings::default() })[0];
    assert!((c.left - c.right).abs() < 1e-6);
}

#[test]
fn spread_scales_the_side_and_keeps_the_middle() {
    let mut p = Place::new(SR);
    p.set(PlaceSettings { spread: 200.0, ..PlaceSettings::default() });
    let (mut l, mut r) = (vec![0.5f32, 0.1], vec![0.1f32, 0.5]);
    p.after(&mut l, &mut r);
    // mid 0.3, side ±0.2 doubled to ±0.4
    assert!((l[0] - 0.7).abs() < 1e-6 && (r[0] + 0.1).abs() < 1e-6, "{} {}", l[0], r[0]);
}

#[test]
fn a_higher_sound_is_brighter() {
    let hf = |height: f32| {
        let mut p = Place::new(SR);
        p.set(PlaceSettings { height, shape: 5, ..PlaceSettings::default() });
        let src = noise(48_000, 9);
        let (mut l, mut r) = (src.clone(), src.clone());
        p.before(&mut l, &mut r);
        // energy of the first difference: a crude high-frequency meter
        l.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum::<f32>() / l.iter().map(|x| x * x).sum::<f32>()
    };
    assert!(hf(100.0) > hf(0.0) * 1.05, "{} vs {}", hf(100.0), hf(0.0));
}

#[test]
fn moving_the_sound_does_not_click() {
    let mut p = Place::new(SR);
    p.set(PlaceSettings::default());
    let tone: Vec<f32> = (0..96_000).map(|n| 0.4 * (std::f32::consts::TAU * 220.0 * n as f32 / SR).sin()).collect();
    let (mut l, mut r) = (tone.clone(), tone.clone());
    let mut out = Vec::new();
    for (i, (a, b)) in l.chunks_mut(256).zip(r.chunks_mut(256)).enumerate() {
        if i == 100 {
            p.set(PlaceSettings { dist: 95.0, pos: 80.0, ..PlaceSettings::default() });
        }
        p.before(a, b);
        out.extend_from_slice(a);
    }
    let worst = out.windows(2).skip(30_000).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
    // a 220 Hz tone at 0.4 steps at most 0.0116 a sample; allow its echoes
    assert!(worst < 0.05, "step of {worst}");
    assert!(out.iter().all(|x| x.is_finite()));
}

/// What the room gives back for one click, after `before` frames of silence.
fn click_after(frames: usize) -> (Vec<f32>, Vec<f32>) {
    let mut p = Place::new(SR);
    p.set(PlaceSettings::default());
    let (mut l, mut r) = (vec![0.0f32; 4096], vec![0.0f32; 4096]);
    let mut left = frames;
    while left > 0 {
        let n = left.min(4096);
        l[..n].fill(0.0);
        r[..n].fill(0.0);
        p.before(&mut l[..n], &mut r[..n]);
        left -= n;
    }
    let (mut l, mut r) = (vec![0.0f32; 24_000], vec![0.0f32; 24_000]);
    l[0] = 1.0;
    r[0] = 1.0;
    p.before(&mut l, &mut r);
    (l, r)
}

#[test]
fn the_room_answers_the_same_after_ten_minutes_as_at_the_start() {
    // The write counter ran on for ever and the taps were placed from it as a
    // 32-bit float: after six minutes they sat on whole even samples.
    let (l0, r0) = click_after(0);
    let (l1, r1) = click_after(10 * 60 * SR as usize);
    let peak = l0.iter().chain(&r0).fold(0.0f32, |m, x| m.max(x.abs()));
    let worst = l0.iter().zip(&l1).chain(r0.iter().zip(&r1)).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
    assert!(peak > 0.05, "the room gave nothing back ({peak})");
    assert!(worst < peak * 0.02, "the answer moved by {worst} against a peak of {peak}");
}

#[test]
fn a_reset_room_is_a_new_room_to_the_last_bit() {
    let mut p = Place::new(SR);
    p.set(PlaceSettings::default());
    let (mut l, mut r) = (noise(30_011, 3), noise(30_011, 4));
    p.before(&mut l, &mut r);
    p.reset();
    let (mut l1, mut r1) = (noise(5000, 5), noise(5000, 6));
    p.before(&mut l1, &mut r1);
    let mut fresh = Place::new(SR);
    fresh.set(PlaceSettings::default());
    let (mut l2, mut r2) = (noise(5000, 5), noise(5000, 6));
    fresh.before(&mut l2, &mut r2);
    assert_eq!(l1.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), l2.iter().map(|x| x.to_bits()).collect::<Vec<_>>());
    assert_eq!(r1.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), r2.iter().map(|x| x.to_bits()).collect::<Vec<_>>());
}
