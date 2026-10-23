//! Nothing allocates after `new`.
//!
//! A counting allocator wraps the system one for this test binary. Each kernel
//! is built, then driven through everything a host does on the audio thread —
//! parameter changes, frames, blocks, reset — and the count must not move.
//! Only one test lives in this file, so no other test's allocations are counted.

use prism_fx::{contract, Category, Descriptor, Kernel, Panel, ParamSpec, Smoothed, CATALOG};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Named through its path: importing `DynKernel` beside `Kernel` would make calls
/// on the planted kernel ambiguous between the two traits.
type Dyn = dyn prism_fx::DynKernel;

struct Counting;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        System.realloc(ptr, layout, size)
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Drives a kernel the way a host does and returns how many allocations that took.
fn allocations_while_running(kernel: &mut Dyn, left: &mut [f32], right: &mut [f32]) -> usize {
    let before = ALLOCATIONS.load(Ordering::SeqCst);
    let count = kernel.descriptor().params.len();
    for round in 0..8 {
        for i in 0..count {
            let spec = &kernel.descriptor().params[i];
            let t = round as f32 / 7.0;
            kernel.set_param(i, spec.min + (spec.max - spec.min) * t);
        }
        let mut frame = [0.25, -0.25];
        kernel.tick(&mut frame);
        kernel.process(left, right);
    }
    kernel.reset();
    kernel.process(left, right);
    ALLOCATIONS.load(Ordering::SeqCst) - before
}

static P: &[ParamSpec] = &[ParamSpec::new("gainDb", "Gain", -24.0, 24.0, 0.0)];
static D: Descriptor = Descriptor {
    id: "alloc-probe",
    code: 9100,
    version: 1,
    label: "Allocation probe",
    category: Category::Utility,
    params: P,
    panel: Panel { groups: &[], primary: &[], displays: &[], pad: None },
    source: "planted",
    sound: None,
};

/// Allocates on every frame: the check must see it.
struct Leaky { log: Vec<f32>, gain: Smoothed }
impl Kernel for Leaky {
    fn new(sr: f32) -> Self { Leaky { log: Vec::new(), gain: Smoothed::new(sr, 5.0) } }
    fn descriptor() -> &'static Descriptor { &D }
    fn set_param(&mut self, _: usize, v: f32) { self.gain.set(P[0].clamp(v)) }
    fn param(&self, _: usize) -> f32 { 0.0 }
    fn reset(&mut self) { self.log = Vec::new() }
    fn tick(&mut self, f: &mut [f32; 2]) {
        self.log.push(f[0]);
        self.log.shrink_to_fit();
        f[0] *= self.gain.next();
    }
}

#[test]
fn nothing_allocates_after_new() {
    let mut left = vec![0.1f32; 512];
    let mut right = vec![-0.1f32; 512];

    for entry in CATALOG {
        let mut kernel = (entry.make)(contract::SAMPLE_RATE);
        // Handing over a sound may allocate; running with one may not.
        if entry.descriptor.sound.is_some() {
            let (samples, channels, rate) = contract::test_sound();
            kernel.set_sound(&samples, channels, rate);
        }
        let n = allocations_while_running(kernel.as_mut(), &mut left, &mut right);
        assert_eq!(n, 0, "{} allocated {n} times after new", entry.descriptor.id);
    }

    // And the check itself works: a kernel that allocates per frame is caught.
    let mut leaky: Box<Dyn> = Box::new(Leaky::new(contract::SAMPLE_RATE));
    assert!(allocations_while_running(leaky.as_mut(), &mut left, &mut right) > 0);
}
