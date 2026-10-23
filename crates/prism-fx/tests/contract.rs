//! The contract every effect in the catalogue keeps, and proof that each check
//! catches the fault it is for.
//!
//! The catalogue is empty at first. Its checks pass trivially then, so each one
//! is also run on kernels and descriptors planted here, good and bad: a check
//! that never fails is not a check.

use prism_fx::contract;
use prism_fx::{
    Category, Descriptor, Entry, Group, Kernel, PadPair, Panel, ParamKind, ParamSpec, Smoothed, CATALOG,
};

/// Named through its path, not imported: importing `DynKernel` as well as `Kernel`
/// would make every call on a planted kernel ambiguous between the two traits.
type Boxed = Box<dyn prism_fx::DynKernel>;

// ---- the catalogue itself ------------------------------------------------------

#[test]
fn the_catalogue_is_consistent() {
    assert_eq!(contract::check_catalog(CATALOG), Vec::<String>::new());
}

#[test]
fn every_effect_in_the_catalogue_keeps_the_contract() {
    for entry in CATALOG {
        let make = || (entry.make)(contract::SAMPLE_RATE);
        assert_eq!(contract::check_kernel(&make), Vec::<String>::new(), "{}", entry.descriptor.id);
    }
}

#[test]
fn make_finds_effects_by_id_and_nothing_else() {
    for entry in CATALOG {
        assert!(prism_fx::make(entry.descriptor.id, 48_000.0).is_some());
    }
    assert!(prism_fx::make("no-such-effect", 48_000.0).is_none());
}

// ---- a planted good kernel: a smoothed gain with a pan -------------------------

static GAIN_PARAMS: &[ParamSpec] = &[
    ParamSpec::new("gainDb", "Gain", -24.0, 24.0, 0.0).unit("dB"),
    ParamSpec::new("pan", "Pan", -1.0, 1.0, 0.0),
];

static GAIN: Descriptor = Descriptor {
    id: "test-gain",
    code: 9001,
    version: 1,
    label: "Test gain",
    category: Category::Utility,
    params: GAIN_PARAMS,
    panel: Panel {
        groups: &[Group { label: "Level", keys: &["gainDb", "pan"] }],
        primary: &["gainDb"],
        displays: &[],
        pad: Some(PadPair { x: "pan", x_range: (-1.0, 1.0), y: "gainDb", y_range: (-12.0, 12.0) }),
    },
    source: "planted by the contract tests",
    sound: None,
};

struct Gain {
    values: [f32; 2],
    gain: Smoothed,
    pan: Smoothed,
}

impl Gain {
    fn targets(&self) -> (f32, f32) {
        (10f32.powf(self.values[0] / 20.0), self.values[1])
    }
}

impl Kernel for Gain {
    fn new(sample_rate: f32) -> Self {
        let mut k = Gain {
            values: [0.0, 0.0],
            gain: Smoothed::new(sample_rate, 5.0),
            pan: Smoothed::new(sample_rate, 5.0),
        };
        k.reset();
        k
    }
    fn descriptor() -> &'static Descriptor {
        &GAIN
    }
    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(spec) = GAIN_PARAMS.get(index) {
            self.values[index] = spec.clamp(value);
            let (g, p) = self.targets();
            self.gain.set(g);
            self.pan.set(p);
        }
    }
    fn param(&self, index: usize) -> f32 {
        self.values.get(index).copied().unwrap_or(0.0)
    }
    fn reset(&mut self) {
        let (g, p) = self.targets();
        self.gain.jump(g);
        self.pan.jump(p);
    }
    fn tick(&mut self, frame: &mut [f32; 2]) {
        let g = self.gain.next();
        let p = self.pan.next();
        frame[0] *= g * (1.0 - p).min(1.0);
        frame[1] *= g * (1.0 + p).min(1.0);
    }
}

fn gain() -> Boxed {
    Box::new(Gain::new(contract::SAMPLE_RATE))
}

#[test]
fn a_good_kernel_passes_every_check() {
    assert_eq!(contract::check_descriptor(&GAIN), Vec::<String>::new());
    assert_eq!(contract::check_kernel(&gain), Vec::<String>::new());
}

// ---- planted bad kernels: each must be caught by its own check -----------------

/// Overrides the block method with something the frame method does not do.
struct BlockDiffers(Gain);
impl Kernel for BlockDiffers {
    fn new(sr: f32) -> Self { BlockDiffers(Gain::new(sr)) }
    fn descriptor() -> &'static Descriptor { &GAIN }
    fn set_param(&mut self, i: usize, v: f32) { self.0.set_param(i, v) }
    fn param(&self, i: usize) -> f32 { self.0.param(i) }
    fn reset(&mut self) { self.0.reset() }
    fn tick(&mut self, f: &mut [f32; 2]) { self.0.tick(f) }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut f = [*l, *r];
            self.0.tick(&mut f);
            *l = f[0] * 0.5;
            *r = f[1];
        }
    }
}

#[test]
fn a_block_method_that_disagrees_with_the_frame_method_is_caught() {
    let make = || -> Boxed { Box::new(BlockDiffers::new(contract::SAMPLE_RATE)) };
    assert!(contract::frames_match_blocks(&make).is_err());
}

/// Its output depends on how the host slices the audio.
struct SliceSensitive { gain: Gain, calls: u32 }
impl Kernel for SliceSensitive {
    fn new(sr: f32) -> Self { SliceSensitive { gain: Gain::new(sr), calls: 0 } }
    fn descriptor() -> &'static Descriptor { &GAIN }
    fn set_param(&mut self, i: usize, v: f32) { self.gain.set_param(i, v) }
    fn param(&self, i: usize) -> f32 { self.gain.param(i) }
    fn reset(&mut self) { self.gain.reset(); self.calls = 0 }
    fn tick(&mut self, f: &mut [f32; 2]) { self.gain.tick(f) }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.calls += 1;
        let bump = (self.calls % 7) as f32 * 1e-3;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut f = [*l, *r];
            self.gain.tick(&mut f);
            *l = f[0] + bump;
            *r = f[1] + bump;
        }
    }
}

#[test]
fn output_that_depends_on_block_size_is_caught() {
    let make = || -> Boxed { Box::new(SliceSensitive::new(contract::SAMPLE_RATE)) };
    assert!(contract::block_size_does_not_matter(&make).is_err());
}

/// Accepts its pan parameter and then ignores it.
struct DeafPan(Gain);
impl Kernel for DeafPan {
    fn new(sr: f32) -> Self { DeafPan(Gain::new(sr)) }
    fn descriptor() -> &'static Descriptor { &GAIN }
    fn set_param(&mut self, i: usize, v: f32) {
        if i == 0 { self.0.set_param(i, v) } else { self.0.values[1] = GAIN_PARAMS[1].clamp(v) }
    }
    fn param(&self, i: usize) -> f32 { self.0.param(i) }
    fn reset(&mut self) { self.0.reset() }
    fn tick(&mut self, f: &mut [f32; 2]) { self.0.tick(f) }
}

#[test]
fn a_control_that_does_nothing_is_caught() {
    let make = || -> Boxed { Box::new(DeafPan::new(contract::SAMPLE_RATE)) };
    let err = contract::every_control_reaches_the_output(&make).unwrap_err();
    assert!(err.contains("pan"), "{err}");
}

/// Transparent at its defaults, like a crusher at full rate: its mix changes
/// nothing until the drive moves, and that is not deafness. (The contract's
/// test material never goes past ±0.5, so the clip is idle at zero drive.)
static BLEND_PARAMS: &[ParamSpec] = &[
    ParamSpec::new("drive", "Drive", 0.0, 1.0, 0.0),
    ParamSpec::new("mix", "Mix", 0.0, 1.0, 1.0),
];

static BLEND: Descriptor = Descriptor {
    id: "test-blend",
    code: 9002,
    version: 1,
    label: "Test blend",
    category: Category::Distortion,
    params: BLEND_PARAMS,
    panel: Panel { groups: &[], primary: &["drive"], displays: &[], pad: None },
    source: "planted by the contract tests",
    sound: None,
};

struct Blend {
    values: [f32; 2],
}

impl Kernel for Blend {
    fn new(_: f32) -> Self {
        Blend { values: [0.0, 1.0] }
    }
    fn descriptor() -> &'static Descriptor {
        &BLEND
    }
    fn set_param(&mut self, i: usize, v: f32) {
        if let Some(spec) = BLEND_PARAMS.get(i) {
            self.values[i] = spec.clamp(v);
        }
    }
    fn param(&self, i: usize) -> f32 {
        self.values.get(i).copied().unwrap_or(0.0)
    }
    fn reset(&mut self) {}
    fn tick(&mut self, f: &mut [f32; 2]) {
        let [drive, mix] = self.values;
        for s in f.iter_mut() {
            let wet = (*s * (1.0 + 9.0 * drive)).clamp(-0.5, 0.5);
            *s += (wet - *s) * mix;
        }
    }
}

#[test]
fn a_control_that_acts_only_away_from_the_defaults_is_not_deaf() {
    let make = || -> Boxed { Box::new(Blend::new(contract::SAMPLE_RATE)) };
    assert_eq!(contract::check_descriptor(&BLEND), Vec::<String>::new());
    assert_eq!(contract::every_control_reaches_the_output(&make), Ok(()));
}

/// A level with an adjustable glide: the glide time changes nothing while the
/// level holds still, only while it is moving — like an echo's glide.
static SLEW_PARAMS: &[ParamSpec] = &[
    ParamSpec::new("level", "Level", 0.0, 1.0, 1.0),
    ParamSpec::new("slewMs", "Slew", 0.0, 100.0, 20.0).unit("ms").glides("level"),
];

static SLEW: Descriptor = Descriptor {
    id: "test-slew",
    code: 9003,
    version: 1,
    label: "Test slew",
    category: Category::Utility,
    params: SLEW_PARAMS,
    panel: Panel { groups: &[], primary: &["level"], displays: &[], pad: None },
    source: "planted by the contract tests",
    sound: None,
};

struct Slew {
    values: [f32; 2],
    current: f32,
    started: bool,
}

impl Kernel for Slew {
    fn new(_: f32) -> Self {
        Slew { values: [1.0, 20.0], current: 1.0, started: false }
    }
    fn descriptor() -> &'static Descriptor {
        &SLEW
    }
    fn set_param(&mut self, i: usize, v: f32) {
        if let Some(spec) = SLEW_PARAMS.get(i) {
            self.values[i] = spec.clamp(v);
        }
    }
    fn param(&self, i: usize) -> f32 {
        self.values.get(i).copied().unwrap_or(0.0)
    }
    fn reset(&mut self) {
        self.started = false;
    }
    fn tick(&mut self, f: &mut [f32; 2]) {
        let [level, slew] = self.values;
        if !self.started {
            self.current = level;
            self.started = true;
        }
        let k = if slew <= 0.0 { 1.0 } else { 1.0 / (slew * 48.0) };
        self.current += (level - self.current) * k;
        f[0] *= self.current;
        f[1] *= self.current;
    }
}

#[test]
fn a_control_that_acts_only_while_another_moves_is_not_deaf() {
    let make = || -> Boxed { Box::new(Slew::new(contract::SAMPLE_RATE)) };
    assert_eq!(contract::check_descriptor(&SLEW), Vec::<String>::new());
    assert_eq!(contract::every_control_reaches_the_output(&make), Ok(()));
}

/// Blows up at the top of its gain range.
struct Unstable(Gain);
impl Kernel for Unstable {
    fn new(sr: f32) -> Self { Unstable(Gain::new(sr)) }
    fn descriptor() -> &'static Descriptor { &GAIN }
    fn set_param(&mut self, i: usize, v: f32) { self.0.set_param(i, v) }
    fn param(&self, i: usize) -> f32 { self.0.param(i) }
    fn reset(&mut self) { self.0.reset() }
    fn tick(&mut self, f: &mut [f32; 2]) {
        self.0.tick(f);
        if self.0.values[0] > 20.0 { f[0] = f32::NAN }
    }
}

#[test]
fn output_that_goes_non_finite_is_caught() {
    let make = || -> Boxed { Box::new(Unstable::new(contract::SAMPLE_RATE)) };
    assert!(contract::finite_under_abuse(&make).is_err());
}

/// Keeps a running state that reset forgets to clear.
struct Remembers { gain: Gain, drift: f32 }
impl Kernel for Remembers {
    fn new(sr: f32) -> Self { Remembers { gain: Gain::new(sr), drift: 0.0 } }
    fn descriptor() -> &'static Descriptor { &GAIN }
    fn set_param(&mut self, i: usize, v: f32) { self.gain.set_param(i, v) }
    fn param(&self, i: usize) -> f32 { self.gain.param(i) }
    fn reset(&mut self) { self.gain.reset() }
    fn tick(&mut self, f: &mut [f32; 2]) {
        self.gain.tick(f);
        self.drift += 1e-4;
        f[0] += self.drift;
    }
}

#[test]
fn a_reset_that_leaves_state_behind_is_caught() {
    let make = || -> Boxed { Box::new(Remembers::new(contract::SAMPLE_RATE)) };
    assert!(contract::reset_is_a_fresh_start(&make).is_err());
}

#[test]
fn set_param_clamps_and_survives_nonsense() {
    let mut k = gain();
    k.set_param(0, 1e9);
    assert_eq!(k.param(0), 24.0);
    k.set_param(0, f32::NAN);
    assert_eq!(k.param(0), 0.0, "NaN falls back to the default");
    k.set_param(99, 1.0); // an index past the end is ignored, not a panic
}

// ---- planted bad descriptors -----------------------------------------------------

const fn panel(keys: &'static [&'static str]) -> Panel {
    Panel { groups: &[], primary: keys, displays: &[], pad: None }
}

fn descriptor(params: &'static [ParamSpec], panel: Panel) -> Descriptor {
    Descriptor {
        id: "bad",
        code: 9002,
        version: 1,
        label: "Bad",
        category: Category::Utility,
        params,
        panel,
        source: "planted",
        sound: None,
    }
}

#[test]
fn a_default_outside_its_range_is_caught() {
    static P: &[ParamSpec] = &[ParamSpec::new("x", "X", 0.0, 1.0, 2.0)];
    assert!(!contract::check_descriptor(&descriptor(P, panel(&[]))).is_empty());
}

#[test]
fn an_empty_or_backwards_range_is_caught() {
    static P: &[ParamSpec] = &[ParamSpec::new("x", "X", 1.0, 1.0, 1.0)];
    assert!(!contract::check_descriptor(&descriptor(P, panel(&[]))).is_empty());
}

#[test]
fn a_log_range_that_reaches_zero_is_caught() {
    static P: &[ParamSpec] = &[ParamSpec::new("hz", "Hz", 0.0, 100.0, 10.0).log()];
    assert!(!contract::check_descriptor(&descriptor(P, panel(&[]))).is_empty());
}

#[test]
fn duplicate_keys_are_caught() {
    static P: &[ParamSpec] = &[
        ParamSpec::new("x", "X", 0.0, 1.0, 0.5),
        ParamSpec::new("x", "X again", 0.0, 1.0, 0.5),
    ];
    assert!(!contract::check_descriptor(&descriptor(P, panel(&[]))).is_empty());
}

#[test]
fn a_choice_whose_labels_do_not_fit_its_range_is_caught() {
    static P: &[ParamSpec] =
        &[ParamSpec::new("mode", "Mode", 0.0, 3.0, 0.0).kind(ParamKind::Choice(&["a", "b"]))];
    assert!(!contract::check_descriptor(&descriptor(P, panel(&[]))).is_empty());
}

#[test]
fn a_glide_for_a_missing_control_is_caught() {
    static P: &[ParamSpec] = &[
        ParamSpec::new("a", "A", 0.0, 1.0, 0.0).glides("nope"),
        ParamSpec::new("b", "B", 0.0, 1.0, 0.0).glides("b"),
    ];
    let errors = contract::check_descriptor(&descriptor(P, Panel { groups: &[], primary: &[], displays: &[], pad: None }));
    assert_eq!(errors.iter().filter(|e| e.contains("glides")).count(), 2, "{errors:?}");
}

#[test]
fn a_condition_on_a_missing_control_or_value_is_caught() {
    static P: &[ParamSpec] = &[
        ParamSpec::new("a", "A", 0.0, 1.0, 0.0).when("nope", 0.0),
        ParamSpec::new("b", "B", 0.0, 1.0, 0.0).when("a", 5.0),
        ParamSpec::new("c", "C", 0.0, 1.0, 0.0).when("a", 1.0),
    ];
    let errors = contract::check_descriptor(&descriptor(P, Panel { groups: &[], primary: &[], displays: &[], pad: None }));
    assert_eq!(errors.iter().filter(|e| e.contains("applies when")).count(), 2, "{errors:?}");
}

#[test]
fn a_panel_naming_a_missing_control_is_caught() {
    static P: &[ParamSpec] = &[ParamSpec::new("x", "X", 0.0, 1.0, 0.5)];
    assert!(!contract::check_descriptor(&descriptor(P, panel(&["y"]))).is_empty());
    let grouped = Panel { groups: &[Group { label: "G", keys: &["y"] }], primary: &[], displays: &[], pad: None };
    assert!(!contract::check_descriptor(&descriptor(P, grouped)).is_empty());
}

#[test]
fn a_pad_range_outside_its_control_is_caught() {
    static P: &[ParamSpec] = &[
        ParamSpec::new("x", "X", 0.0, 1.0, 0.5),
        ParamSpec::new("y", "Y", 0.0, 1.0, 0.5),
    ];
    let pad = Panel {
        groups: &[],
        primary: &[],
        displays: &[],
        pad: Some(PadPair { x: "x", x_range: (0.0, 2.0), y: "y", y_range: (0.0, 1.0) }),
    };
    assert!(!contract::check_descriptor(&descriptor(P, pad)).is_empty());
}

#[test]
fn duplicate_ids_or_codes_in_a_catalogue_are_caught() {
    fn make(sr: f32) -> Boxed { Box::new(Gain::new(sr)) }
    let twice = [Entry { descriptor: &GAIN, make }, Entry { descriptor: &GAIN, make }];
    let errors = contract::check_catalog(&twice);
    assert!(errors.iter().any(|e| e.contains("id")), "{errors:?}");
    assert!(errors.iter().any(|e| e.contains("code")), "{errors:?}");
}

#[test]
fn a_kernel_whose_descriptor_does_not_match_its_entry_is_caught() {
    static OTHER: Descriptor = Descriptor { id: "other", code: 9003, ..GAIN };
    fn make(sr: f32) -> Boxed { Box::new(Gain::new(sr)) }
    let wrong = [Entry { descriptor: &OTHER, make }];
    assert!(!contract::check_catalog(&wrong).is_empty());
}
