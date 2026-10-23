//! ++compand: SoundHack Freesound's compressor and expander, as its code does
//! it.

const SR: f32 = 48_000.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn compand(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("sh-compand", SR).expect("sh-compand is in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.reset();
    k
}

fn run(k: &mut Boxed, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut a, mut b) = (l.to_vec(), r.to_vec());
    k.process(&mut a, &mut b);
    (a, b)
}

fn mono(k: &mut Boxed, x: &[f32]) -> Vec<f32> {
    run(k, x, x).0
}

fn tone(db: f32, seconds: f32) -> Vec<f32> {
    let amp = 10f32.powf(db / 20.0);
    (0..(seconds * SR) as usize).map(|i| amp * (std::f32::consts::TAU * 220.0 * i as f32 / SR).sin()).collect()
}

fn peak_db(x: &[f32]) -> f32 {
    20.0 * x.iter().fold(0.0f32, |m, s| m.max(s.abs())).log10()
}

#[test]
fn at_one_to_one_it_passes_the_sound_untouched() {
    let x = tone(-6.0, 0.5);
    assert_eq!(mono(&mut compand(&[("ratio", 1.0), ("outDb", 0.0)]), &x), x);
}

#[test]
fn two_to_one_halves_the_level_over_the_threshold() {
    // 14 dB over a −20 dB threshold comes out 7 dB over.
    let y = mono(&mut compand(&[("ratio", 0.5), ("threshDb", -20.0), ("outDb", 0.0)]), &tone(-6.0206, 1.0));
    assert!((peak_db(&y[24_000..]) - -13.01).abs() < 0.3, "{}", peak_db(&y[24_000..]));
}

#[test]
fn above_one_it_expands_below_the_threshold() {
    // 10 dB under at 1:2 comes out 20 dB under.
    let y = mono(&mut compand(&[("ratio", 2.0), ("threshDb", -20.0), ("outDb", 0.0)]), &tone(-30.0, 1.0));
    assert!((peak_db(&y[24_000..]) - -40.0).abs() < 0.3, "{}", peak_db(&y[24_000..]));
}

#[test]
fn rms_detection_sees_a_sine_three_db_lower() {
    let settings = |rms: f32| [("ratio", 0.5), ("threshDb", -20.0), ("outDb", 0.0), ("rms", rms)];
    let peak = peak_db(&mono(&mut compand(&settings(0.0)), &tone(-6.0206, 1.0))[24_000..]);
    let rms = peak_db(&mono(&mut compand(&settings(1.0)), &tone(-6.0206, 1.0))[24_000..]);
    assert!((rms - peak - 1.505).abs() < 0.3, "rms {rms}, peak {peak}");
}

#[test]
fn the_attack_lets_an_onset_through() {
    // The original's attack came out infinite, so it never acted. Here the
    // detector rises at most 20 dB per attack time.
    let mut x = vec![0.0; 4_800];
    x.extend(tone(0.0, 0.2));
    let onset = |ms: f32| {
        let y = mono(&mut compand(&[("ratio", 0.025), ("threshDb", -40.0), ("outDb", 0.0), ("attackMs", ms)]), &x);
        // Five to ten milliseconds in: a fast attack has caught up by then.
        peak_db(&y[5_040..5_280])
    };
    assert!(onset(50.0) > onset(1.0) + 6.0, "slow {} fast {}", onset(50.0), onset(1.0));
}

#[test]
fn the_release_recovers_twenty_db_per_release_time() {
    // After a full-scale tone, the detector falls 20 dB a second at a release
    // of one second, so a −40 dB tone a second later is still held 19.5 dB
    // down: 0.975 × the 20 dB the detector stands over the threshold.
    let mut x = tone(0.0, 0.5);
    x.extend(tone(-40.0, 1.5));
    let y = mono(&mut compand(&[("ratio", 0.025), ("threshDb", -40.0), ("outDb", 0.0), ("releaseS", 1.0)]), &x);
    let at = |t: f32| peak_db(&y[((0.5 + t) * SR) as usize - 240..((0.5 + t) * SR) as usize + 240]);
    assert!((at(1.0) - (-40.0 - 19.5)).abs() < 1.0, "{}", at(1.0));
}

#[test]
fn the_expansion_knee_runs_smoothly_through_the_threshold() {
    // The original's expansion knee took a linear level from a dB one, so the
    // gain stepped at the knee's foot and boosted inside it.
    let gain = |db: f32| {
        let y = mono(&mut compand(&[("ratio", 2.0), ("threshDb", -20.0), ("knee", 1.0), ("outDb", 0.0)]), &tone(db, 0.3));
        peak_db(&y[9_600..]) - db
    };
    let curve: Vec<f32> = (-40..=0).map(|db| gain(db as f32)).collect();
    // Inside the knee it lifts a little above the threshold, as the
    // compression knee does below it: at most ¾ dB at full knee and 1:2.
    assert!(curve.windows(2).all(|w| (w[1] - w[0]).abs() < 2.0), "{curve:?}");
    assert!(curve.iter().all(|g| *g < 0.8), "{curve:?}");
}

#[test]
fn linked_sides_duck_together() {
    let loud = tone(-6.0, 1.0);
    let quiet = tone(-30.0, 1.0);
    let settings = |linked: f32| [("ratio", 0.25), ("threshDb", -20.0), ("outDb", 0.0), ("linked", linked)];
    let (_, apart) = run(&mut compand(&settings(0.0)), &loud, &quiet);
    let (_, together) = run(&mut compand(&settings(1.0)), &loud, &quiet);
    assert!((peak_db(&apart[24_000..]) - -30.0).abs() < 0.2, "{}", peak_db(&apart[24_000..]));
    assert!(peak_db(&together[24_000..]) < -38.0, "{}", peak_db(&together[24_000..]));
}

#[test]
fn each_side_has_its_own_rms_window() {
    // The original pushed both sides into one window, so a loud left raised
    // the right's reading even unlinked.
    let settings = [("ratio", 0.25), ("threshDb", -20.0), ("outDb", 0.0), ("rms", 1.0)];
    let (_, r) = run(&mut compand(&settings), &tone(0.0, 1.0), &tone(-30.0, 1.0));
    assert!((peak_db(&r[24_000..]) - -30.0).abs() < 0.2, "{}", peak_db(&r[24_000..]));
}

#[test]
fn nothing_passes_one_and_a_half() {
    let x = tone(0.0, 0.25);
    let (l, r) = run(&mut compand(&[("outDb", 60.0)]), &x, &x);
    assert!(l.iter().chain(&r).all(|s| s.abs() <= 1.5));
}
