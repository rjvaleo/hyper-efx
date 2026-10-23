//! Tremolo, and Auto-Pan as its stereo phase: what HYPERPRISM.md says they do.
//!
//! Most checks feed a constant 1.0, so the output is the gain envelope itself.

const SR: f32 = 48_000.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

/// A tremolo with these settings, glides landed.
fn tremolo(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("tremolo", SR).expect("tremolo is in the catalogue");
    let d = k.descriptor();
    for &(key, value) in settings {
        k.set_param(d.index_of(key).unwrap_or_else(|| panic!("no control {key}")), value);
    }
    k.reset();
    k
}

/// The gain on each side, frame by frame, for `seconds` of audio.
fn envelope(k: &mut Boxed, seconds: f32) -> (Vec<f32>, Vec<f32>) {
    let n = (seconds * SR) as usize;
    let (mut left, mut right) = (vec![1.0; n], vec![1.0; n]);
    k.process(&mut left, &mut right);
    (left, right)
}

/// How many times the envelope falls to silence, counting each visit once.
fn visits_to_silence(gain: &[f32]) -> usize {
    let mut visits = 0;
    let mut silent = false;
    for &g in gain {
        if g < 1e-3 && !silent {
            visits += 1;
        }
        silent = g < 1e-3;
    }
    visits
}

fn noise(n: usize) -> Vec<f32> {
    let mut x = 0x1234_5678_u32;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x as f32 / u32::MAX as f32) * 2.0 - 1.0
        })
        .collect()
}

const SHAPES: [f32; 3] = [0.0, 1.0, 2.0];

#[test]
fn at_depth_zero_the_audio_is_unchanged() {
    for shape in SHAPES {
        let mut k = tremolo(&[("depth", 0.0), ("shape", shape), ("stereo", 0.7), ("rateHz", 13.0)]);
        let input = noise(SR as usize);
        let (mut left, mut right) = (input.clone(), input.iter().map(|s| -s).collect::<Vec<_>>());
        k.process(&mut left, &mut right);
        assert_eq!(left, input, "shape {shape}");
        assert!(right.iter().zip(&input).all(|(r, i)| *r == -i), "shape {shape}");
    }
}

#[test]
fn at_full_depth_it_reaches_silence_once_a_cycle() {
    for shape in SHAPES {
        for (rate, seconds, cycles) in [(4.0, 1.0, 4), (0.5, 4.0, 2), (10.0, 1.0, 10)] {
            let mut k = tremolo(&[("rateHz", rate), ("depth", 1.0), ("shape", shape)]);
            let (left, _) = envelope(&mut k, seconds);
            assert_eq!(visits_to_silence(&left), cycles, "shape {shape} at {rate} Hz");
            let top = left.iter().cloned().fold(0.0, f32::max);
            assert!((top - 1.0).abs() < 1e-3, "shape {shape} at {rate} Hz peaks at {top}");
        }
    }
}

#[test]
fn it_never_adds_gain() {
    for shape in SHAPES {
        let mut k = tremolo(&[("rateHz", 7.0), ("depth", 0.8), ("shape", shape), ("stereo", 0.3)]);
        let (left, right) = envelope(&mut k, 1.0);
        assert!(left.iter().chain(&right).all(|g| (0.0..=1.0).contains(g)), "shape {shape}");
    }
}

#[test]
fn half_depth_dips_halfway() {
    let mut k = tremolo(&[("rateHz", 2.0), ("depth", 0.5)]);
    let (left, _) = envelope(&mut k, 1.0);
    let bottom = left.iter().cloned().fold(1.0, f32::min);
    assert!((bottom - 0.5).abs() < 1e-3, "bottom {bottom}");
}

#[test]
fn a_fresh_tremolo_starts_at_the_top_of_its_cycle() {
    let mut k = tremolo(&[("depth", 1.0)]);
    let (left, right) = envelope(&mut k, 0.001);
    assert_eq!((left[0], right[0]), (1.0, 1.0));
}

#[test]
fn with_no_stereo_phase_both_sides_move_together() {
    for shape in SHAPES {
        let mut k = tremolo(&[("depth", 1.0), ("shape", shape)]);
        let (left, right) = envelope(&mut k, 1.0);
        assert_eq!(left, right, "shape {shape}");
    }
}

#[test]
fn auto_pan_puts_the_two_sides_in_antiphase() {
    for shape in SHAPES {
        let mut k = tremolo(&[("rateHz", 3.0), ("depth", 1.0), ("shape", shape), ("stereo", 1.0)]);
        let (left, right) = envelope(&mut k, 1.25);
        // One side is up exactly as far as the other is down: the sound moves
        // between the speakers and its sum stays level.
        for (i, (l, r)) in left.iter().zip(&right).enumerate() {
            assert!((l + r - 1.0).abs() < 1e-4, "shape {shape}, frame {i}: {l} + {r}");
        }
        // The right side starts in a trough, so count over one second that
        // starts a quarter cycle in, where neither side is silent at the edges.
        let second = 4_000..52_000;
        assert_eq!(visits_to_silence(&left[second.clone()]), 3, "shape {shape}");
        assert_eq!(visits_to_silence(&right[second]), 3, "shape {shape}");
    }
}

#[test]
fn stereo_phase_puts_the_right_side_behind_the_left() {
    // At 1 Hz, half the stereo phase is a quarter of a cycle: 12 000 frames.
    let mut k = tremolo(&[("rateHz", 1.0), ("depth", 1.0), ("stereo", 0.5)]);
    let (left, right) = envelope(&mut k, 1.0);
    let lowest = |g: &[f32]| g.iter().enumerate().fold((0, 2.0), |m, (i, &v)| if v < m.1 { (i, v) } else { m }).0;
    let (l, r) = (lowest(&left) as i64, lowest(&right) as i64);
    assert!((l - 24_000).abs() <= 2, "left trough at {l}");
    assert!((r - 36_000).abs() <= 2, "right trough at {r}");
}

#[test]
fn a_moved_rate_glides_rather_than_jumping_the_phase() {
    // Rate changes move the phase on smoothly: no gain step bigger than the
    // fastest sweep could make in one frame.
    let mut k = tremolo(&[("rateHz", 0.5), ("depth", 1.0)]);
    let (mut left, mut right) = (vec![1.0; 4800], vec![1.0; 4800]);
    k.process(&mut left, &mut right);
    k.set_param(k.descriptor().index_of("rateHz").unwrap(), 20.0);
    let (mut after, mut r2) = (vec![1.0; 4800], vec![1.0; 4800]);
    k.process(&mut after, &mut r2);
    let steepest = 2.0 * std::f32::consts::PI * 20.0 / SR * 0.5;
    let mut last = left[4799];
    for g in after {
        assert!((g - last).abs() <= steepest * 1.01, "jump of {}", (g - last).abs());
        last = g;
    }
}
