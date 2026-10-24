//! Hyperprism Single Delay (1.2.1's EDelay2, `Delay2` with no feedback): the
//! pad's Y is the mix, and the time changes by crossfades.

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn single(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-single-delay", SR).expect("hp-single-delay is in the catalogue");
    for &(key, value) in settings {
        set(&mut k, key, value);
    }
    k
}

fn set(k: &mut Boxed, key: &str, value: f32) {
    let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
    k.set_param(i, value);
}

fn run(k: &mut Boxed, x: &[f32]) -> Vec<f32> {
    let (mut l, mut r) = (x.to_vec(), x.to_vec());
    k.process(&mut l, &mut r);
    l
}

fn click(n: usize) -> Vec<f32> {
    let mut x = vec![0.0; n];
    x[0] = 1.0;
    x
}

#[test]
fn one_repeat_at_the_whole_sample_delay_and_no_more() {
    let mut k = single(&[("mix", 100.0), ("delayMs", 10.01)]);
    let y = run(&mut k, &click(5_000));
    assert_eq!(y[441], 1.0);
    assert_eq!(y.iter().filter(|v| **v != 0.0).count(), 1, "no feedback");
}

#[test]
fn the_y_axis_is_the_mix() {
    let mut k = single(&[("mix", 30.0), ("delayMs", 10.0)]);
    let y = run(&mut k, &click(1_000));
    assert!((y[0] - 0.7).abs() < 1e-7, "{}", y[0]);
    assert!((y[441] - 0.3).abs() < 1e-7, "{}", y[441]);
}

#[test]
fn a_time_change_is_a_crossfade_of_whole_steps() {
    // The same walk as Echo: halves of the remaining distance, each step the
    // larger of 4096 samples and its distance.
    let mut k = single(&[("mix", 100.0), ("delayMs", 1000.0)]);
    run(&mut k, &vec![1.0; 2049 * 22]);
    set(&mut k, "delayMs", 500.0);
    let y = run(&mut k, &vec![1.0; 3 * 44_100]);
    let mut runs = Vec::new();
    let mut n = 0;
    for &v in &y {
        if v > 1.0 + 1e-6 {
            n += 1;
        } else if n > 0 {
            runs.push(n);
            n = 0;
        }
    }
    assert_eq!(&runs[..4], &[22_049, 11_024, 5_511, 4_095], "{runs:?}");
}

#[test]
fn setting_the_delay_leaves_the_mix_alone() {
    // The original's missing break set the mix to 0.01 × the delay in ms.
    let mut k = single(&[("mix", 30.0)]);
    set(&mut k, "delayMs", 10.0);
    let y = run(&mut k, &click(1_000));
    assert!((y[0] - 0.7).abs() < 1e-7, "{}", y[0]);
}

#[test]
fn controls_are_the_originals() {
    let d = prism_fx::find("hp-single-delay").unwrap().descriptor;
    assert_eq!((d.code, d.label, d.source), (40, "Hyperprism Single Delay", "Hyperprism-PPC 1.2.1: Single Delay"));
    assert_eq!(d.category, prism_fx::Category::Delay);
    let keys: Vec<&str> = d.params.iter().map(|p| p.key).collect();
    assert_eq!(keys, ["delayMs", "mix", "speed"]);
    let p = |key: &str| d.spec(key).unwrap();
    assert_eq!((p("delayMs").min, p("delayMs").max, p("delayMs").default), (0.0, 2000.0, 1000.0));
    assert_eq!((p("mix").min, p("mix").max, p("mix").default), (0.0, 100.0, 50.0));
    assert_eq!((p("speed").min, p("speed").max, p("speed").default), (1.0, 100.0, 50.0));
    assert!(d.params.iter().all(|s| !s.log));
    let pad = d.panel.pad.unwrap();
    assert_eq!((pad.x, pad.x_range, pad.y, pad.y_range), ("delayMs", (0.0, 2000.0), "mix", (0.0, 100.0)));
}
