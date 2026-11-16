//! Place allocates nothing once made: settings move and blocks run on the
//! audio thread. Its own binary, so the counting allocator sees only this.

use prism_fx::place::{Place, PlaceSettings};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 { ALLOCS.fetch_add(1, Ordering::SeqCst); System.alloc(l) }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) { System.dealloc(p, l) }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 { ALLOCS.fetch_add(1, Ordering::SeqCst); System.realloc(p, l, n) }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

#[test]
fn moving_and_running_place_allocates_nothing() {
    let mut p = Place::new(48_000.0);
    let (mut l, mut r) = (vec![0.1f32; 512], vec![0.1f32; 512]);
    let before = ALLOCS.load(Ordering::SeqCst);
    for shape in 0..6u8 {
        p.set(PlaceSettings { shape, dist: shape as f32 * 15.0, ..PlaceSettings::default() });
        p.before(&mut l, &mut r);
        p.after(&mut l, &mut r);
    }
    p.reset();
    assert_eq!(ALLOCS.load(Ordering::SeqCst) - before, 0);
}
