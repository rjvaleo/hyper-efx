//! The rules every effect keeps, as checks a test can run on any kernel.
//!
//! They live in the library, not only in its tests, so that each check can be
//! proven against a kernel planted to break it, and so that a host can run them
//! on its own adapters if it likes. Each check returns `Err` with a sentence
//! saying what went wrong.

use crate::{Descriptor, DynKernel, Entry, ParamKind, ParamSpec};

/// The rate every check renders at.
pub const SAMPLE_RATE: f32 = 48_000.0;

/// Long enough for glides to settle and for delays and tails to show.
const FRAMES: usize = 8192;

type Make<'a> = &'a dyn Fn() -> Box<dyn DynKernel>;

// ---- descriptors and the catalogue ----------------------------------------------

/// Everything wrong with one descriptor, or nothing.
///
/// Comparisons are written `!(a < b)` on purpose: NaN fails them, so a range or
/// default that is not a number is reported rather than slipping through.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn check_descriptor(d: &Descriptor) -> Vec<String> {
    let mut errors = Vec::new();
    let id = d.id;
    if id.is_empty() {
        errors.push("an effect has an empty id".to_string());
    }
    if d.label.is_empty() {
        errors.push(format!("{id}: empty label"));
    }

    for (i, p) in d.params.iter().enumerate() {
        let at = format!("{id}.{} (#{i})", p.key);
        if p.key.is_empty() {
            errors.push(format!("{id}: parameter #{i} has an empty key"));
        }
        if d.params[..i].iter().any(|q| q.key == p.key) {
            errors.push(format!("{at}: duplicate key"));
        }
        if !(p.min.is_finite() && p.max.is_finite() && p.min < p.max) {
            errors.push(format!("{at}: range {}..{} is empty, backwards or not finite", p.min, p.max));
            continue;
        }
        if !(p.default >= p.min && p.default <= p.max) {
            errors.push(format!("{at}: default {} is outside {}..{}", p.default, p.min, p.max));
        } else if p.clamp(p.default) != p.default {
            errors.push(format!("{at}: default {} is not a value this kind can hold", p.default));
        }
        if p.log && p.min <= 0.0 {
            errors.push(format!("{at}: a log range must start above zero, not at {}", p.min));
        }
        if !(p.smooth_ms >= 0.0) {
            errors.push(format!("{at}: negative glide time"));
        }
        if let Some((key, value)) = p.when {
            match d.params.iter().find(|q| q.key == key) {
                Some(q) if q.key != p.key && q.clamp(value) == value => {}
                _ => errors.push(format!("{at}: applies when '{key}' is {value}, which that control cannot be")),
            }
        }
        if let Some(target) = p.glides {
            if target == p.key || !d.params.iter().any(|q| q.key == target) {
                errors.push(format!("{at}: glides '{target}', which is not another control"));
            }
        }
        if let ParamKind::Choice(labels) = p.kind {
            let slots = p.max - p.min + 1.0;
            if p.min.fract() != 0.0 || p.max.fract() != 0.0 || labels.len() as f32 != slots {
                errors.push(format!(
                    "{at}: {} choice labels for a range of {} values",
                    labels.len(),
                    slots
                ));
            }
        }
    }

    let known = |k: &str| d.params.iter().any(|p| p.key == k);
    for group in d.panel.groups {
        for key in group.keys {
            if !known(key) {
                errors.push(format!("{id}: panel group '{}' names '{key}', which is not a control", group.label));
            }
        }
    }
    if !d.panel.groups.is_empty() {
        for p in d.params.iter().filter(|p| !p.inert) {
            let n = d.panel.groups.iter().flat_map(|g| g.keys.iter()).filter(|k| **k == p.key).count();
            if n != 1 {
                errors.push(format!("{id}.{}: appears in {n} panel groups, not one", p.key));
            }
        }
    }
    // An inert control is kept for documents, not for people: it stays off the panel.
    for p in d.params.iter().filter(|p| p.inert) {
        let grouped = d.panel.groups.iter().any(|g| g.keys.contains(&p.key));
        let pad = d.panel.pad.is_some_and(|pad| pad.x == p.key || pad.y == p.key);
        if grouped || d.panel.primary.contains(&p.key) || pad {
            errors.push(format!("{id}.{}: is inert but on the panel", p.key));
        }
    }
    for key in d.panel.primary {
        if !known(key) {
            errors.push(format!("{id}: primary control '{key}' is not a control"));
        }
    }
    if let Some(pad) = d.panel.pad {
        for (axis, key, (lo, hi)) in [("x", pad.x, pad.x_range), ("y", pad.y, pad.y_range)] {
            match d.spec(key) {
                None => errors.push(format!("{id}: pad {axis} '{key}' is not a control")),
                Some(p) => {
                    if !(lo < hi && lo >= p.min && hi <= p.max) {
                        errors.push(format!(
                            "{id}: pad {axis} range {lo}..{hi} is not inside {key}'s {}..{}",
                            p.min, p.max
                        ));
                    }
                }
            }
        }
        if pad.x == pad.y {
            errors.push(format!("{id}: the pad puts '{}' on both axes", pad.x));
        }
    }
    errors
}

/// Everything wrong with a catalogue: its descriptors, and ids and codes that
/// repeat, and entries whose kernel describes something else.
pub fn check_catalog(entries: &[Entry]) -> Vec<String> {
    let mut errors = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let d = e.descriptor;
        errors.extend(check_descriptor(d));
        if entries[..i].iter().any(|o| o.descriptor.id == d.id) {
            errors.push(format!("{}: duplicate id", d.id));
        }
        if entries[..i].iter().any(|o| o.descriptor.code == d.code) {
            errors.push(format!("{}: duplicate code {}", d.id, d.code));
        }
        let built = (e.make)(SAMPLE_RATE).descriptor();
        if built.id != d.id || built.code != d.code {
            errors.push(format!("{}: its kernel describes itself as '{}'", d.id, built.id));
        }
    }
    errors
}

// ---- kernels --------------------------------------------------------------------

/// Every behavioural check on one kernel.
pub fn check_kernel(make: Make) -> Vec<String> {
    // An effect that takes a sound is checked with one: without it, a
    // convolution is a delay and most of its controls rightly do nothing.
    let sounded = || {
        let mut k = make();
        if k.descriptor().sound.is_some() {
            let (samples, channels, rate) = test_sound();
            k.set_sound(&samples, channels, rate);
        }
        k
    };
    let make: Make = &sounded;
    [
        frames_match_blocks(make),
        block_size_does_not_matter(make),
        every_control_reaches_the_output(make),
        finite_under_abuse(make),
        reset_is_a_fresh_start(make),
    ]
    .into_iter()
    .filter_map(Result::err)
    .collect()
}

/// `tick` over N frames equals `process` on the same N frames, bit for bit.
pub fn frames_match_blocks(make: Make) -> Result<(), String> {
    let input = signal(FRAMES, 1);
    let by_frame = render(make().as_mut(), &input, 0);
    let by_block = render(make().as_mut(), &input, FRAMES);
    match first_difference(&by_frame, &by_block, 0.0) {
        None => Ok(()),
        Some((i, a, b)) => Err(format!("{}: tick and process differ at frame {i} ({a} vs {b})", id(make))),
    }
}

/// The output is the same whether the host calls 1, 64 or 1024 frames at a time.
pub fn block_size_does_not_matter(make: Make) -> Result<(), String> {
    let input = signal(FRAMES, 2);
    let one = render(make().as_mut(), &input, 1);
    for block in [64, 1024] {
        let other = render(make().as_mut(), &input, block);
        if let Some((i, a, b)) = first_difference(&one, &other, 1e-5) {
            return Err(format!(
                "{}: blocks of {block} differ from blocks of 1 at frame {i} ({a} vs {b})",
                id(make)
            ));
        }
    }
    Ok(())
}

/// Moving each control to either end of its range changes the output.
///
/// Each control is tried from the defaults, and if that changes nothing, with
/// every other control at the end farthest from its default: an effect can be
/// transparent at rest — a crusher at full rate and depth — and then its mix
/// rightly does nothing there. Then with every other control a quarter of
/// the way along: off its ends and off its middle, where a delay's far ends
/// (the longest time, all dry) hide its wet sound, its rest (no depth, no
/// feedback) silences its LFO and polarity, and a pitch shifter at no shift
/// or a whole octave sounds the same in just and equal tuning. Then with
/// every switch on and the rest at their defaults: a granulator's tuning
/// waits for keys other than the root to be held.
/// A glide time acts only on a move, so it is
/// tried while the control it glides moves half a second in; a control that
/// applies only in one setting of another is tried in that setting. A
/// control is deaf only if it changes nothing from any of these. A control
/// marked inert is held to the opposite: it must change nothing from all of
/// them.
///
/// The move is only ever the glided control's, never a general one: a kernel
/// that stores a value and applies it only when some other control next moves
/// would look like a glide, and it is a fault.
///
/// It listens for two seconds, long enough for an echo's default repeats:
/// Hyperprism's delays start at a second, so their first fed-back repeat
/// lands two seconds in at the defaults, and one second in at a quarter of
/// the delay's travel.
///
/// Parameters are set after `new` and never reset, so a kernel that stores a
/// value and forgets to act on it is caught.
pub fn every_control_reaches_the_output(make: Make) -> Result<(), String> {
    let frames = 2 * SAMPLE_RATE as usize;
    let input = signal(frames, 3);
    let params = make().descriptor().params;
    let far = |spec: &ParamSpec| if spec.to_unit(spec.default) < 0.5 { spec.max } else { spec.min };
    // Settings applied from the start, and settings applied halfway.
    let render_with = |start: &[(usize, f32)], halfway: &[(usize, f32)]| {
        let mut k = make();
        let (mut left, mut right) = (input.0.clone(), input.1.clone());
        let half = SAMPLE_RATE as usize / 2;
        for (part, settings) in [(0..half, start), (half..frames, halfway)] {
            for &(i, v) in settings {
                k.set_param(i, v);
            }
            for (l, r) in left[part.clone()].chunks_mut(256).zip(right[part].chunks_mut(256)) {
                k.process(l, r);
            }
        }
        (left, right)
    };
    let mut deaf = Vec::new();
    let mut audible = Vec::new();
    for (i, spec) in params.iter().enumerate() {
        let voiced: Vec<(usize, f32)> =
            params.iter().enumerate().filter(|&(j, _)| j != i).map(|(j, s)| (j, far(s))).collect();
        let quarter: Vec<(usize, f32)> =
            params.iter().enumerate().filter(|&(j, _)| j != i).map(|(j, s)| (j, s.from_unit(0.25))).collect();
        let switched: Vec<(usize, f32)> = params
            .iter()
            .enumerate()
            .filter(|&(j, s)| j != i && s.kind == ParamKind::Toggle)
            .map(|(j, s)| (j, s.max))
            .collect();
        let mut states = vec![(Vec::new(), Vec::new()), (voiced, Vec::new()), (quarter, Vec::new())];
        if !switched.is_empty() {
            states.push((switched, Vec::new()));
        }
        if let Some(j) = spec.glides.and_then(|key| params.iter().position(|p| p.key == key)) {
            states.push((Vec::new(), vec![(j, far(&params[j]))]));
        }
        if let Some((j, v)) = spec.when.and_then(|(key, v)| Some((params.iter().position(|p| p.key == key)?, v))) {
            states.push((vec![(j, v)], Vec::new()));
        }
        let moved = states.iter().any(|(start, halfway)| {
            let reference = render_with(start, halfway);
            [spec.min, spec.max].iter().any(|&end| {
                let mut start = start.clone();
                start.push((i, end));
                first_difference(&reference, &render_with(&start, halfway), 1e-6).is_some()
            })
        });
        if spec.inert && moved {
            audible.push(spec.key);
        } else if !spec.inert && !moved {
            deaf.push(spec.key);
        }
    }
    let mut errors = Vec::new();
    if !deaf.is_empty() {
        errors.push(format!("{}: controls that change nothing: {}", id(make), deaf.join(", ")));
    }
    if !audible.is_empty() {
        errors.push(format!("{}: controls marked inert that change the output: {}", id(make), audible.join(", ")));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Full-scale noise, and every control swept every 64 frames to its ends, to
/// random values, past its range and to NaN, never makes a non-finite sample.
pub fn finite_under_abuse(make: Make) -> Result<(), String> {
    let mut k = make();
    let params = k.descriptor().params;
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut left, mut right) = (vec![0.0; 64], vec![0.0; 64]);
    for block in 0..(SAMPLE_RATE as usize / 64) {
        for (i, spec) in params.iter().enumerate() {
            let v = match rng.below(6) {
                0 => spec.min,
                1 => spec.max,
                2 => spec.min + (spec.max - spec.min) * rng.unit(),
                3 => spec.max * 1e6 + 1.0,
                4 => f32::NAN,
                _ => f32::NEG_INFINITY,
            };
            k.set_param(i, v);
        }
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            *l = rng.unit() * 2.0 - 1.0;
            *r = rng.unit() * 2.0 - 1.0;
        }
        k.process(&mut left, &mut right);
        if let Some(i) = left.iter().chain(right.iter()).position(|s| !s.is_finite()) {
            return Err(format!("{}: non-finite output in block {block}, sample {i}", id(make)));
        }
    }
    Ok(())
}

/// After `reset`, the kernel renders exactly what a new one does.
pub fn reset_is_a_fresh_start(make: Make) -> Result<(), String> {
    let first = signal(FRAMES, 4);
    let other = signal(FRAMES, 5);
    let fresh = render(make().as_mut(), &first, 256);
    let mut k = make();
    render(k.as_mut(), &first, 256);
    render(k.as_mut(), &other, 256);
    k.reset();
    let again = render(k.as_mut(), &first, 256);
    match first_difference(&fresh, &again, 1e-6) {
        None => Ok(()),
        Some((i, a, b)) => Err(format!("{}: after reset, frame {i} is {b}, not {a}", id(make))),
    }
}

/// The sound handed to an effect that takes one: a quarter second of decaying
/// noise, different on each side, interleaved, at the checks' own rate.
pub fn test_sound() -> (Vec<f32>, usize, f32) {
    let frames = SAMPLE_RATE as usize / 4;
    let mut rng = Rng(0x5eed_5eed_5eed_5eed);
    let mut out = Vec::with_capacity(frames * 2);
    for n in 0..frames {
        let decay = (-6.0 * n as f32 / frames as f32).exp();
        out.push((rng.unit() - 0.5) * decay);
        out.push((rng.unit() - 0.5) * decay * 0.7);
    }
    (out, 2, SAMPLE_RATE)
}

// ---- helpers --------------------------------------------------------------------

fn id(make: Make) -> &'static str {
    make().descriptor().id
}

/// Stereo test material: a tone, noise and silence in turn, different per side,
/// so tails, gates and stereo handling all show. Deterministic per `seed`.
fn signal(frames: usize, seed: u64) -> (Vec<f32>, Vec<f32>) {
    let mut rng = Rng(seed.wrapping_mul(0x2545_f491_4f6c_dd1d) | 1);
    let mut left = Vec::with_capacity(frames);
    let mut right = Vec::with_capacity(frames);
    for n in 0..frames {
        let t = n as f32 / SAMPLE_RATE;
        let section = (n / 1024) % 3;
        let tone = (2.0 * core::f32::consts::PI * 220.0 * t).sin();
        let (l, r) = match section {
            0 => (0.5 * tone, 0.4 * (2.0 * core::f32::consts::PI * 330.0 * t).sin()),
            1 => (rng.unit() - 0.5, rng.unit() - 0.5),
            _ => (0.0, 0.0),
        };
        left.push(l);
        right.push(r);
    }
    (left, right)
}

/// Run `input` through a kernel. `block` 0 means a frame at a time through `tick`.
fn render(k: &mut dyn DynKernel, input: &(Vec<f32>, Vec<f32>), block: usize) -> (Vec<f32>, Vec<f32>) {
    let (mut left, mut right) = (input.0.clone(), input.1.clone());
    if block == 0 {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut frame = [*l, *r];
            k.tick(&mut frame);
            *l = frame[0];
            *r = frame[1];
        }
    } else {
        for (l, r) in left.chunks_mut(block).zip(right.chunks_mut(block)) {
            k.process(l, r);
        }
    }
    (left, right)
}

/// The first frame where two renders differ by more than `tolerance`.
fn first_difference(a: &(Vec<f32>, Vec<f32>), b: &(Vec<f32>, Vec<f32>), tolerance: f32) -> Option<(usize, f32, f32)> {
    for side in [(&a.0, &b.0), (&a.1, &b.1)] {
        for (i, (x, y)) in side.0.iter().zip(side.1.iter()).enumerate() {
            let differs = if tolerance == 0.0 { x.to_bits() != y.to_bits() } else { (x - y).abs() > tolerance || x.is_nan() != y.is_nan() };
            if differs {
                return Some((i, *x, *y));
            }
        }
    }
    None
}

/// A small, fixed random source, so every check renders the same thing every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}
