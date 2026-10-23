//! Spectral Dynamics: SoundHack's gate, ducker, expander and compressor on
//! every band separately, as its manual describes them (docs/effects/
//! soundhack-app.md in MultiGrainLive).

const SR: f32 = 48_000.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

/// Choices, by their place in the descriptor.
const GATE: f32 = 0.0;
const EXPAND: f32 = 1.0;
const COMPRESS: f32 = 2.0;
const BELOW: f32 = 0.0;
const ABOVE: f32 = 1.0;

fn dynamics(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("spectral-dynamics", SR).expect("spectral-dynamics is in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.reset();
    k
}

/// The `bands` choice holding `n` bands.
fn bands(n: usize) -> f32 {
    let d = prism_fx::find("spectral-dynamics").unwrap().descriptor;
    let spec = d.spec("bands").unwrap();
    let prism_fx::ParamKind::Choice(labels) = spec.kind else { panic!("bands is not a choice") };
    labels.iter().position(|l| *l == n.to_string()).unwrap_or_else(|| panic!("no {n} bands")) as f32
}

fn db(a: f32) -> f32 {
    20.0 * a.max(1e-12).log10()
}

fn amp(dbfs: f32) -> f32 {
    10f32.powf(dbfs / 20.0)
}

/// Sines at whole hertz, the same on both sides.
fn tones(seconds: f32, parts: &[(f32, f32)]) -> Vec<f32> {
    let n = (seconds * SR) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            parts.iter().map(|&(hz, level)| amp(level) * (std::f32::consts::TAU * hz * t).sin()).sum()
        })
        .collect()
}

fn run(k: &mut Boxed, mono: &[f32]) -> Vec<f32> {
    let (mut l, mut r) = (mono.to_vec(), mono.to_vec());
    k.process(&mut l, &mut r);
    assert_eq!(l, r, "the same on both sides");
    l
}

/// A sine's level in a stretch one second long: exact for whole hertz.
fn level_at(x: &[f32], hz: f32, from: usize) -> f32 {
    let n = SR as usize;
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, s) in x[from..from + n].iter().enumerate() {
        let a = std::f64::consts::TAU * hz as f64 * (from + i) as f64 / SR as f64;
        re += *s as f64 * a.cos();
        im += *s as f64 * a.sin();
    }
    db((2.0 * (re * re + im * im).sqrt() / n as f64) as f32)
}

const SETTLED: usize = 48_000;

#[test]
fn at_a_ratio_of_one_it_only_delays_the_sound_by_its_window() {
    for n in [64usize, 512, 2048] {
        let mut k = dynamics(&[("mode", EXPAND), ("ratio", 1.0), ("bands", bands(n))]);
        assert_eq!(k.latency_frames() as usize, 2 * n, "{n} bands");
        let input = tones(1.0, &[(440.0, -10.0), (2_500.0, -30.0), (7_000.0, -50.0)]);
        let out = run(&mut k, &input);
        let late = 2 * n;
        let worst = (late + 4 * n..input.len()).map(|i| (out[i] - input[i - late]).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1e-4, "{n} bands: {worst}");
    }
}

#[test]
fn a_gate_below_the_threshold_takes_out_the_quiet_band_and_keeps_the_loud() {
    let mut k = dynamics(&[("mode", GATE), ("affect", BELOW), ("thresholdDb", -40.0), ("reductionDb", -60.0), ("bands", bands(2048))]);
    let out = run(&mut k, &tones(2.0, &[(1_000.0, -12.0), (3_000.0, -60.0)]));
    assert!((level_at(&out, 1_000.0, SETTLED) + 12.0).abs() < 0.5, "{}", level_at(&out, 1_000.0, SETTLED));
    assert!(level_at(&out, 3_000.0, SETTLED) < -100.0, "{}", level_at(&out, 3_000.0, SETTLED));
}

#[test]
fn a_gate_above_the_threshold_ducks_the_loud_band_and_leaves_the_quiet() {
    let mut k = dynamics(&[("mode", GATE), ("affect", ABOVE), ("thresholdDb", -40.0), ("reductionDb", -60.0), ("bands", bands(2048))]);
    let out = run(&mut k, &tones(2.0, &[(1_000.0, -12.0), (3_000.0, -60.0)]));
    // The tone's own bands duck; the window's skirts either side of it are
    // quieter than the threshold and pass, as they would in SoundHack.
    assert!(level_at(&out, 1_000.0, SETTLED) < -35.0, "{}", level_at(&out, 1_000.0, SETTLED));
    assert!((level_at(&out, 3_000.0, SETTLED) + 60.0).abs() < 1.0, "{}", level_at(&out, 3_000.0, SETTLED));
}

#[test]
fn expanding_below_pushes_quiet_bands_further_down_and_holds_the_loud() {
    let mut k = dynamics(&[("mode", EXPAND), ("affect", BELOW), ("thresholdDb", -40.0), ("ratio", 2.0), ("bands", bands(2048))]);
    let out = run(&mut k, &tones(2.0, &[(1_000.0, -12.0), (3_000.0, -50.0)]));
    assert!((level_at(&out, 1_000.0, SETTLED) + 12.0).abs() < 0.5);
    // Ten under the threshold becomes twenty under it.
    assert!((level_at(&out, 3_000.0, SETTLED) + 60.0).abs() < 3.0, "{}", level_at(&out, 3_000.0, SETTLED));
}

#[test]
fn compressing_above_holds_the_threshold_and_brings_the_loud_band_toward_it() {
    let mut k = dynamics(&[("mode", COMPRESS), ("affect", ABOVE), ("thresholdDb", -30.0), ("ratio", 2.0), ("bands", bands(2048))]);
    let out = run(&mut k, &tones(2.0, &[(1_000.0, -10.0), (3_000.0, -50.0)]));
    // Twenty over becomes ten over; under the threshold nothing changes.
    assert!((level_at(&out, 1_000.0, SETTLED) + 20.0).abs() < 3.0, "{}", level_at(&out, 1_000.0, SETTLED));
    assert!((level_at(&out, 3_000.0, SETTLED) + 50.0).abs() < 1.0);
}

#[test]
fn compressing_below_lifts_the_quiet_band_toward_the_threshold() {
    let mut k = dynamics(&[("mode", COMPRESS), ("affect", BELOW), ("thresholdDb", -40.0), ("ratio", 2.0), ("bands", bands(2048))]);
    let out = run(&mut k, &tones(2.0, &[(1_000.0, -12.0), (3_000.0, -60.0)]));
    assert!((level_at(&out, 1_000.0, SETTLED) + 12.0).abs() < 0.5);
    assert!((level_at(&out, 3_000.0, SETTLED) + 50.0).abs() < 3.0, "{}", level_at(&out, 3_000.0, SETTLED));
}

#[test]
fn outside_the_band_range_nothing_changes() {
    let mut k = dynamics(&[
        ("mode", GATE),
        ("thresholdDb", -40.0),
        ("reductionDb", -60.0),
        ("bands", bands(2048)),
        ("lowHz", 2_000.0),
    ]);
    let out = run(&mut k, &tones(2.0, &[(1_000.0, -60.0), (3_000.0, -60.0)]));
    assert!((level_at(&out, 1_000.0, SETTLED) + 60.0).abs() < 1.0, "below the range: {}", level_at(&out, 1_000.0, SETTLED));
    assert!(level_at(&out, 3_000.0, SETTLED) < -100.0, "inside it: {}", level_at(&out, 3_000.0, SETTLED));
}

#[test]
fn a_threshold_relative_to_the_peak_follows_the_loudest_band() {
    let input = tones(2.0, &[(1_000.0, -30.0), (3_000.0, -50.0)]);
    let gate = [("mode", GATE), ("thresholdDb", -28.0), ("reductionDb", -60.0), ("bands", bands(2048))];
    // Absolute: both are under −28 dB.
    let out = run(&mut dynamics(&gate), &input);
    assert!(level_at(&out, 1_000.0, SETTLED) < -80.0 && level_at(&out, 3_000.0, SETTLED) < -100.0);
    // Relative: the peak is about −30, so the gate acts below about −58, under
    // the quieter tone and the bands either side of it.
    let out = run(&mut dynamics(&[&gate[..], &[("relativeToPeak", 1.0)]].concat()), &input);
    assert!((level_at(&out, 1_000.0, SETTLED) + 30.0).abs() < 1.0);
    assert!((level_at(&out, 3_000.0, SETTLED) + 50.0).abs() < 2.0, "{}", level_at(&out, 3_000.0, SETTLED));
}

#[test]
fn third_octave_grouping_gives_every_band_in_a_group_its_average_gain() {
    // SoundHack averages the gains across each third of an octave, counted
    // down from Nyquist. 1000 and 1060 Hz share one group, so they are turned
    // down by the same amount — part way, since some of the group is open and
    // some gated.
    let input = tones(2.0, &[(1_000.0, -10.0), (1_060.0, -60.0)]);
    let gate = [("mode", GATE), ("thresholdDb", -40.0), ("reductionDb", -60.0), ("bands", bands(2048))];
    let alone = run(&mut dynamics(&gate), &input);
    assert!(level_at(&alone, 1_060.0, SETTLED) < -90.0, "band by band, the quiet one is gated");
    assert!((level_at(&alone, 1_000.0, SETTLED) + 10.0).abs() < 0.5, "and the loud one is not");
    let grouped = run(&mut dynamics(&[&gate[..], &[("thirdOctave", 1.0)]].concat()), &input);
    let loud = level_at(&grouped, 1_000.0, SETTLED) + 10.0;
    let quiet = level_at(&grouped, 1_060.0, SETTLED) + 60.0;
    assert!((loud - quiet).abs() < 1.5, "one gain for the group: {loud} and {quiet} dB");
    assert!(loud < -1.0 && loud > -40.0, "part way: {loud} dB");
}

/// Energy in a stretch of samples.
fn energy(x: &[f32]) -> f32 {
    x.iter().map(|s| s * s).sum()
}

#[test]
fn a_smoothed_amplitude_opens_a_band_later_and_less_abruptly() {
    // A tone arrives out of silence. Band by band, the gate opens on the first
    // frame that hears it; on the average of the last four frames, not until
    // most of them have.
    let mut input = vec![0.0; 48_000];
    for (i, s) in input[24_000..].iter_mut().enumerate() {
        *s = amp(-12.0) * (std::f32::consts::TAU * 1_000.0 * i as f32 / SR).sin();
    }
    let gate = [("mode", GATE), ("thresholdDb", -15.0), ("reductionDb", -60.0), ("bands", bands(512))];
    let onset = 24_000 + 1_024;
    let early = |x: Vec<f32>| energy(&x[onset..onset + 768]);
    let raw = early(run(&mut dynamics(&gate), &input));
    let smoothed = early(run(&mut dynamics(&[&gate[..], &[("smoothed", 1.0)]].concat()), &input));
    assert!(smoothed < raw * 0.6, "smoothed {smoothed} against {raw}");
}

#[test]
fn a_slower_attack_opens_the_band_more_gradually() {
    let mut input = vec![0.0; 48_000];
    for (i, s) in input[24_000..].iter_mut().enumerate() {
        *s = amp(-12.0) * (std::f32::consts::TAU * 1_000.0 * i as f32 / SR).sin();
    }
    let gate = [("mode", GATE), ("thresholdDb", -40.0), ("reductionDb", -60.0), ("bands", bands(512))];
    let onset = 24_000 + 1_024;
    let early = |x: Vec<f32>| energy(&x[onset..onset + 2_400]);
    let quick = early(run(&mut dynamics(&gate), &input));
    let slow = early(run(&mut dynamics(&[&gate[..], &[("timeMs", 300.0)]].concat()), &input));
    assert!(slow < quick * 0.25, "slow {slow} against quick {quick}");
}

#[test]
fn a_smoothed_amplitude_is_a_soft_knee_halfway_at_the_threshold() {
    // On the averaged level, a band exactly at the threshold gets half the
    // gate's travel: SoundHack's avg / (threshold + avg) is a half there.
    let input = tones(2.0, &[(1_000.0, -30.0)]);
    let mut k = dynamics(&[("mode", GATE), ("thresholdDb", -30.0), ("reductionDb", -60.0), ("bands", bands(2048)), ("smoothed", 1.0)]);
    let out = run(&mut k, &input);
    // Halfway between a gain of 1 and 0.001 is about 0.5: −6 dB.
    assert!((level_at(&out, 1_000.0, SETTLED) + 36.0).abs() < 2.5, "{}", level_at(&out, 1_000.0, SETTLED));
}

/// A hum print: a sound with energy at 3 kHz and little elsewhere.
fn hum(seconds: f32, start_silent: usize) -> Vec<f32> {
    let mut x = tones(seconds, &[(3_000.0, -30.0)]);
    for s in x.iter_mut().take(start_silent) {
        *s = 0.0;
    }
    x
}

fn with_sound(settings: &[(&str, f32)], sound: &[f32]) -> Boxed {
    let mut k = prism_fx::make("spectral-dynamics", SR).unwrap();
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.set_sound(sound, 1, SR);
    k.reset();
    k
}

#[test]
fn a_threshold_sound_sets_each_bands_threshold_from_its_spectrum() {
    // Gating below a hum print, 6 dB above it: the 3 kHz hum at the print's
    // own level is under its band's threshold and goes; a 1 kHz tone, where
    // the print has nothing, stays.
    let input = tones(2.0, &[(1_000.0, -40.0), (3_000.0, -30.0)]);
    let settings = [
        ("mode", GATE),
        ("reductionDb", -60.0),
        ("bands", bands(2048)),
        ("useSound", 1.0),
        ("thresholdDb", 6.0),
    ];
    let out = run(&mut with_sound(&settings, &hum(1.0, 0)), &input);
    assert!(level_at(&out, 3_000.0, SETTLED) < -80.0, "the hum: {}", level_at(&out, 3_000.0, SETTLED));
    assert!((level_at(&out, 1_000.0, SETTLED) + 40.0).abs() < 1.0, "the tone: {}", level_at(&out, 1_000.0, SETTLED));
    // Six decibels under the print instead, the hum is above its threshold.
    let under = [&settings[..4], &[("thresholdDb", -6.0)]].concat();
    let out = run(&mut with_sound(&under, &hum(1.0, 0)), &input);
    assert!((level_at(&out, 3_000.0, SETTLED) + 30.0).abs() < 1.0, "{}", level_at(&out, 3_000.0, SETTLED));
}

#[test]
fn only_the_first_five_windows_of_the_threshold_sound_count() {
    // A print that is silent for its first five windows has no thresholds at
    // all, however loud it is afterwards: nothing is gated below nothing.
    let input = tones(2.0, &[(3_000.0, -30.0)]);
    let settings = [("mode", GATE), ("reductionDb", -60.0), ("bands", bands(2048)), ("useSound", 1.0), ("thresholdDb", 6.0)];
    let out = run(&mut with_sound(&settings, &hum(1.0, 5 * 4_096)), &input);
    assert!((level_at(&out, 3_000.0, SETTLED) + 30.0).abs() < 1.0, "{}", level_at(&out, 3_000.0, SETTLED));
}

#[test]
fn without_use_sound_the_threshold_sound_is_ignored() {
    let input = tones(2.0, &[(3_000.0, -30.0)]);
    let settings = [("mode", GATE), ("reductionDb", -60.0), ("bands", bands(2048)), ("thresholdDb", -50.0)];
    let out = run(&mut with_sound(&settings, &hum(1.0, 0)), &input);
    assert!((level_at(&out, 3_000.0, SETTLED) + 30.0).abs() < 1.0);
}
