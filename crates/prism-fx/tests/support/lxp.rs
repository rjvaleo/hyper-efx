//! What all four LXP reverbs do, as checks each one's test file runs on itself.
//!
//! The originals' code is encrypted, so these say what the controls are for —
//! as the manual and the presets describe them — and what this engine does with
//! them, not what Lexicon's code did.

#![allow(dead_code)]

pub const SR: f32 = 48_000.0;

pub type Boxed = Box<dyn prism_fx::DynKernel>;

pub fn make(id: &str, settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make(id, SR).unwrap_or_else(|| panic!("{id} is in the catalogue"));
    for &(key, value) in settings {
        set(&mut k, key, value);
    }
    k.reset();
    k
}

pub fn set(k: &mut Boxed, key: &str, value: f32) {
    let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
    k.set_param(i, value);
}

pub fn run(k: &mut Boxed, left: &[f32], right: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (left.to_vec(), right.to_vec());
    for (l, r) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
        k.process(l, r);
    }
    (l, r)
}

pub fn click(n: usize) -> Vec<f32> {
    let mut x = vec![0.0; n];
    x[0] = 1.0;
    x
}

pub fn noise(n: usize, seed: u32) -> Vec<f32> {
    let mut s = seed | 1;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            s as f32 / u32::MAX as f32 - 0.5
        })
        .collect()
}

pub fn secs(s: f32) -> usize {
    (s * SR) as usize
}

pub fn energy(x: &[f32]) -> f64 {
    x.iter().map(|&s| (s as f64) * (s as f64)).sum()
}

pub fn rms(x: &[f32]) -> f32 {
    (energy(x) / x.len().max(1) as f64).sqrt() as f32
}

pub fn db(ratio: f64) -> f64 {
    10.0 * ratio.max(1e-30).log10()
}

/// A gentle low-pass, to listen to the lows.
pub fn lows(x: &[f32], hz: f32) -> Vec<f32> {
    let a = 1.0 - (-std::f32::consts::TAU * hz / SR).exp();
    let mut y = 0.0;
    x.iter()
        .map(|&s| {
            y += a * (s - y);
            y
        })
        .collect()
}

/// A first difference, to listen to the highs.
pub fn highs(x: &[f32]) -> Vec<f32> {
    let mut last = 0.0;
    x.iter()
        .map(|&s| {
            let d = s - last;
            last = s;
            d
        })
        .collect()
}

/// The first sample louder than `level`.
pub fn onset(x: &[f32], level: f32) -> usize {
    x.iter().position(|s| s.abs() > level).unwrap_or(x.len())
}

/// The energy-weighted mean time of `x`, in samples.
pub fn centroid(x: &[f32]) -> f64 {
    let e = energy(x).max(1e-30);
    x.iter().enumerate().map(|(i, &s)| i as f64 * (s as f64) * (s as f64)).sum::<f64>() / e
}

/// The reverb time from a measured decay: the Schroeder integral's fall from
/// −5 to −25 dB, times three.
pub fn rt60(x: &[f32]) -> f32 {
    let mut tail = vec![0.0f64; x.len() + 1];
    for i in (0..x.len()).rev() {
        tail[i] = tail[i + 1] + (x[i] as f64) * (x[i] as f64);
    }
    let total = tail[0].max(1e-30);
    let at = |level: f64| tail.iter().position(|&e| db(e / total) < level).unwrap_or(x.len());
    3.0 * (at(-25.0) - at(-5.0)) as f32 / SR
}

/// Wet only, the reverb alone and flat: no early voices, no bass or treble
/// shaping, no output filter, no predelay.
pub const TAIL: &[(&str, f32)] = &[
    ("mix", 1.0),
    ("earlyDb", -80.0),
    ("reverbDb", 0.0),
    ("bassRt", 1.0),
    ("rtHiCutHz", 20_000.0),
    ("rolloffHz", 20_000.0),
    ("predelayMs", 0.0),
];

/// `TAIL` and `more`, with any echoes' feedback off too: echoes feed back into
/// the input, so the reverb hears them.
pub fn tail<'a>(id: &str, more: &[(&'a str, f32)]) -> Vec<(&'a str, f32)> {
    let mut settings = with(TAIL, more);
    if prism_fx::find(id).is_some_and(|e| e.descriptor.index_of("ekoFbMaster").is_some()) {
        settings.insert(0, ("ekoFbMaster", 0.0));
    }
    settings
}

/// Wet only, the early voices alone.
pub const EARLY: &[(&str, f32)] = &[("mix", 1.0), ("earlyDb", 0.0), ("reverbDb", -80.0), ("rolloffHz", 20_000.0)];

fn with<'a>(base: &[(&'a str, f32)], more: &[(&'a str, f32)]) -> Vec<(&'a str, f32)> {
    [base, more].concat()
}

/// Both sides' click response, summed.
pub fn response(id: &str, settings: &[(&str, f32)], seconds: f32) -> Vec<f32> {
    let x = click(secs(seconds));
    let (l, r) = run(&mut make(id, settings), &x, &x);
    l.iter().zip(&r).map(|(a, b)| a + b).collect()
}

// ---- the scenarios -------------------------------------------------------------

pub fn reverb_time_follows_the_control(id: &str, times: &[f32]) {
    for &t in times {
        // Spread shut, so the measure is of the tail and not of its onset.
        let y = response(id, &tail(id, &[("reverbTimeS", t), ("spread", 0.0)]), t * 1.6 + 0.5);
        let measured = rt60(&y);
        assert!((measured / t - 1.0).abs() < 0.3, "{id}: set {t} s, measured {measured} s");
    }
}

pub fn size_changes_the_space_but_not_the_reverb_time(id: &str) {
    let settings = |size: f32| tail(id, &[("reverbTimeS", 1.5), ("sizeM", size), ("spread", 0.0), ("diffusion", 0.0)]);
    let small = response(id, &settings(8.0), 3.0);
    let large = response(id, &settings(60.0), 3.0);
    for (size, y) in [(8.0, &small), (60.0, &large)] {
        let measured = rt60(y);
        assert!((measured / 1.5 - 1.0).abs() < 0.3, "{id}: {size} m measured {measured} s");
    }
    // The tank's first output lands later in a larger space.
    let first = |y: &[f32]| onset(y, 1e-3);
    assert!(first(&large) > first(&small) + secs(0.01), "{id}: {} against {}", first(&large), first(&small));
}

pub fn bass_rt_lengthens_the_lows(id: &str) {
    let tail = |bass: f32| {
        let y = response(id, &tail(id, &[("reverbTimeS", 1.0), ("bassRt", bass), ("bassXovHz", 500.0)]), 2.5);
        energy(&lows(&y, 120.0)[secs(1.0)..secs(2.0)])
    };
    let (short, long) = (tail(0.5), tail(3.0));
    assert!(db(long / short) > 10.0, "{id}: lows {} dB louder", db(long / short));
}

pub fn rt_hi_cut_shortens_the_highs(id: &str) {
    let tail = |cut: f32| {
        let y = response(id, &tail(id, &[("reverbTimeS", 1.5), ("rtHiCutHz", cut)]), 2.0);
        energy(&highs(&y)[secs(0.5)..secs(1.5)])
    };
    let (open, cut) = (tail(20_000.0), tail(1_000.0));
    assert!(db(open / cut) > 10.0, "{id}: highs {} dB quieter", db(open / cut));
}

pub fn predelay_holds_back_the_tail_and_not_the_early_voices(id: &str) {
    let tail = |ms: f32| onset(&response(id, &tail(id, &[("predelayMs", ms), ("reverbTimeS", 1.0)]), 1.0), 1e-4);
    let (none, held) = (tail(0.0), tail(100.0));
    assert!(held >= none + secs(0.1) - 2 && held < none + secs(0.1) + secs(0.01), "{id}: {none} then {held}");
    let early = |ms: f32| onset(&response(id, &with(EARLY, &[("predelayMs", ms)]), 1.0), 1e-4);
    assert_eq!(early(0.0), early(100.0), "{id}: the early voices moved with the predelay");
}

pub fn the_bottom_of_each_level_is_off(id: &str) {
    let x = noise(secs(1.0), 7);
    let (l, r) = run(&mut make(id, &[("mix", 1.0), ("earlyDb", -80.0), ("reverbDb", -80.0)]), &x, &x);
    assert!(l.iter().chain(&r).all(|&s| s == 0.0), "{id}: something got through");
}

pub fn mix_at_zero_is_the_dry_sound(id: &str) {
    let (a, b) = (noise(secs(0.5), 3), noise(secs(0.5), 5));
    let (l, r) = run(&mut make(id, &[("mix", 0.0)]), &a, &b);
    assert_eq!((l, r), (a, b), "{id}");
}

pub fn shape_moves_the_injection_later(id: &str) {
    let settings = |shape: f32| tail(id, &[("spread", 1.0), ("sizeM", 60.0), ("reverbTimeS", 0.3), ("shape", shape)]);
    let early = centroid(&response(id, &settings(0.0), 1.0)[..secs(0.6)]);
    let late = centroid(&response(id, &settings(64.0), 1.0)[..secs(0.6)]);
    assert!(late > early + SR as f64 * 0.04, "{id}: centroid {early} then {late}");
}

pub fn spread_widens_the_injection(id: &str) {
    let settings = |spread: f32| tail(id, &[("spread", spread), ("sizeM", 60.0), ("reverbTimeS", 0.3), ("shape", 32.0)]);
    let narrow = centroid(&response(id, &settings(0.0), 1.0)[..secs(0.6)]);
    let wide = centroid(&response(id, &settings(1.0), 1.0)[..secs(0.6)]);
    assert!(wide > narrow + SR as f64 * 0.04, "{id}: centroid {narrow} then {wide}");
}

pub fn tap_slope_tilts_the_output(id: &str) {
    let settings = |slope: f32| tail(id, &[("reverbTimeS", 0.2), ("sizeM", 60.0), ("tapSlope", slope)]);
    let falling = centroid(&response(id, &settings(-10.0), 1.0));
    let rising = centroid(&response(id, &settings(10.0), 1.0));
    assert!(rising > falling + SR as f64 * 0.01, "{id}: centroid {falling} then {rising}");
}

pub fn spin_at_zero_holds_the_wander_still(id: &str) {
    let x = noise(secs(1.0), 11);
    let render = |spin: f32, wander: f32| run(&mut make(id, &[("spinHz", spin), ("wanderMs", wander)]), &x, &x);
    assert_eq!(render(0.0, 0.0), render(0.0, 24.0), "{id}: wander moved with spin at zero");
    assert_ne!(render(2.0, 0.0), render(2.0, 24.0), "{id}: wander did nothing");
}

pub fn wander_smooths_a_ringing_tail(id: &str) {
    // A steady tone into a long tail rings at the lines' modes; the randomizer
    // keeps moving them, so its level wavers.
    let x: Vec<f32> = (0..secs(3.0)).map(|i| 0.3 * (std::f32::consts::TAU * 500.0 * i as f32 / SR).sin()).collect();
    let spread = |wander: f32| {
        let (l, _) = run(&mut make(id, &tail(id, &[("reverbTimeS", 3.0), ("spinHz", 2.0), ("wanderMs", wander)])), &x, &x);
        let blocks: Vec<f32> = l[secs(1.5)..].chunks(2_400).map(rms).collect();
        let mean = blocks.iter().sum::<f32>() / blocks.len() as f32;
        blocks.iter().map(|b| (b - mean).abs()).sum::<f32>() / blocks.len() as f32 / mean
    };
    assert!(spread(20.0) > spread(0.0) * 1.5, "{id}: {} against {}", spread(20.0), spread(0.0));
}

pub fn rolloff_darkens_the_wet_sound(id: &str) {
    let x = noise(secs(1.0), 13);
    let bright = |hz: f32| {
        let (l, _) = run(&mut make(id, &[("mix", 1.0), ("rolloffHz", hz)]), &x, &x);
        energy(&highs(&l)) / energy(&l)
    };
    assert!(bright(1_000.0) < bright(20_000.0) * 0.3, "{id}");
}

pub fn diffusion_thickens_the_attack(id: &str) {
    // Smeared, the first 20 ms has many more samples near its loudest.
    let loud = |diffusion: f32| {
        let y = response(id, &tail(id, &[("diffusion", diffusion), ("spread", 0.0), ("reverbTimeS", 1.0)]), 0.5);
        let start = onset(&y, 1e-5);
        let w = &y[start..start + secs(0.02)];
        let peak = w.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        w.iter().filter(|s| s.abs() > 0.1 * peak).count()
    };
    assert!(loud(1.0) > 2 * loud(0.0), "{id}: {} against {}", loud(1.0), loud(0.0));
}

pub fn one_side_in_reaches_both_sides_out(id: &str) {
    let x = click(secs(1.5));
    let silent = vec![0.0; x.len()];
    let (l, r) = run(&mut make(id, &tail(id, &[("reverbTimeS", 1.0)])), &x, &silent);
    assert!(energy(&r) > 0.3 * energy(&l), "{id}: left {} right {}", energy(&l), energy(&r));
    // And the two sides are not the same sound.
    let dot: f64 = l.iter().zip(&r).map(|(a, b)| (*a as f64) * (*b as f64)).sum();
    assert!(dot.abs() < 0.5 * (energy(&l) * energy(&r)).sqrt(), "{id}: the sides are correlated");
}

pub fn the_tail_sits_near_the_input_level(id: &str) {
    let x = noise(secs(3.0), 17);
    let (l, r) = run(&mut make(id, &tail(id, &[("reverbTimeS", 1.5)])), &x, &x);
    let level = 20.0 * ((rms(&l[secs(1.5)..]) + rms(&r[secs(1.5)..])) / 2.0 / rms(&x)).log10();
    assert!((-12.0..6.0).contains(&level), "{id}: {level} dB");
}

pub fn the_tail_dies_away(id: &str) {
    let x = noise(secs(0.5), 19);
    let mut sound = x.clone();
    sound.resize(secs(8.0), 0.0);
    let (l, _) = run(&mut make(id, &tail(id, &[("reverbTimeS", 1.0)])), &sound, &sound);
    assert!(rms(&l[secs(7.0)..]) < 1e-3 * rms(&l[..secs(0.5)]), "{id}");
}

/// A reflection is a diffused delay: its first sample lands on its time, and
/// what it puts out sums to its gain.
pub fn a_reflection_lands_on_its_time<'a>(
    id: &str,
    delay: &'a str,
    gain: &'a str,
    from_left: bool,
    to_left: bool,
    all_gains: &[&'a str],
) {
    let mut settings: Vec<(&'a str, f32)> = with(EARLY, &[("rflTimeMaster", 1.0), (delay, 30.0)]);
    for g in all_gains {
        settings.push((g, if *g == gain { 0.0 } else { -80.0 }));
    }
    if make(id, &[]).descriptor().index_of("ekoFbMaster").is_some() {
        // Echoes, which also sound once with no feedback, out of the window.
        settings.push(("lEkoFbDb", -80.0));
        settings.push(("rEkoFbDb", -80.0));
        settings.push(("ekoTimeMaster", 1.0));
        settings.push(("lEkoMs", 900.0));
        settings.push(("rEkoMs", 900.0));
    }
    let x = click(secs(0.5));
    let silent = vec![0.0; x.len()];
    let (inl, inr) = if from_left { (&x, &silent) } else { (&silent, &x) };
    let window = secs(0.4);
    for (master, ms) in [(1.0, 30.0), (0.5, 15.0)] {
        settings.push(("rflTimeMaster", master));
        let (l, r) = run(&mut make(id, &settings), inl, inr);
        let (hit, quiet) = if to_left { (&l, &r) } else { (&r, &l) };
        let at = (ms * SR / 1000.0).round() as usize;
        assert_eq!(onset(hit, 1e-6), at, "{id} {delay} at {master}");
        let sum: f32 = hit[..window].iter().sum();
        assert!((sum - 1.0).abs() < 0.02, "{id} {delay}: sums to {sum}");
        assert!(quiet[..window].iter().all(|&s| s == 0.0), "{id} {delay}: leaked to the other side");
    }
}
