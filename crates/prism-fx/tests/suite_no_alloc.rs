//! A mode switch allocates nothing on the audio thread (SUITE-PLAN §2.0).
//!
//! The incoming kernel is built off the audio thread; from `begin_switch`
//! through the crossfade to landing, nothing allocates and nothing is freed:
//! the outgoing kernel waits in `take_retired`. Its own binary, so the
//! counting allocator sees only this test.

use prism_fx::suite::{self, ModuleKernel};
use prism_fx::DynKernel;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static FREES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::SeqCst);
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        FREES.fetch_add(1, Ordering::SeqCst);
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::SeqCst);
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[test]
fn switching_modes_neither_allocates_nor_frees_on_the_audio_thread() {
    let sr = 48_000.0;
    let pairs = [("delay", "echo", "reverb", "dattorro-plate"), ("reverb", "lxp-hall", "modulation", "hp-chorus"), ("filter", "eq", "dynamics", "compressor")];
    for (s1, m1, s2, m2) in pairs {
        let from = suite::find_mode(s1, m1).unwrap();
        let to = suite::find_mode(s2, m2).unwrap();
        let mut k = ModuleKernel::new(from, sr).unwrap();
        let next = (prism_fx::find(to.kernel).unwrap().make)(sr);
        let back = (prism_fx::find(from.kernel).unwrap().make)(sr);
        let mut l = vec![0.1f32; 512];
        let mut r = vec![-0.1f32; 512];
        let (a0, f0) = (ALLOCS.load(Ordering::SeqCst), FREES.load(Ordering::SeqCst));
        k.process(&mut l, &mut r);
        assert!(k.begin_switch(to, next).is_ok());
        for _ in 0..4 {
            k.process(&mut l, &mut r);
        }
        let mut fr = [0.2, 0.2];
        k.tick(&mut fr);
        let refused = k.begin_switch(from, back);
        assert_eq!(ALLOCS.load(Ordering::SeqCst) - a0, 0, "{m1} → {m2} allocated");
        assert_eq!(FREES.load(Ordering::SeqCst) - f0, 0, "{m1} → {m2} freed on the audio thread");
        // off the audio thread again: the refused kernel and the retired one are dropped here
        assert!(refused.is_err(), "the retired kernel was not taken yet");
        drop(refused);
        assert!(k.take_retired().is_some());
    }
}
