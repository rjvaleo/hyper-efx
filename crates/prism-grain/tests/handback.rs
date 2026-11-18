//! A map or a bank the stretcher replaces is handed back, not dropped: the
//! stretcher lives on the audio thread, and freeing megabytes there is a
//! block that may arrive late.

use prism_grain::stretch::Algorithm;
use prism_grain::stretcher::LayerBank;
use prism_grain::transient::TimeMap;
use prism_grain::Stretcher;
use std::sync::Arc;

const SR: u32 = 48_000;

#[test]
fn a_replaced_map_comes_back_with_no_other_holder() {
    let mut s = Stretcher::new(512, 2, SR);
    assert!(s.replace_map(Some(Arc::new(TimeMap::linear(48_000, 2.0)))).is_none(), "there was none to replace");
    let first = Arc::clone(s.map().expect("the map is in force"));
    // The stretcher and its engines hold it too.
    assert!(Arc::strong_count(&first) >= 2);
    drop(first);

    let second = Arc::new(TimeMap::linear(48_000, 3.0));
    let old = s.replace_map(Some(Arc::clone(&second))).expect("the first map comes back");
    assert_eq!(Arc::strong_count(&old), 1, "the caller is the old map's last holder");
    assert!(Arc::ptr_eq(s.map().unwrap(), &second), "and the new one is in force");

    let old = s.replace_map(None).expect("the second comes back when the map is taken away");
    assert!(Arc::ptr_eq(&old, &second));
    assert_eq!(Arc::strong_count(&old), 2, "held by this test and by what came back, nothing else");
    assert!(s.map().is_none());
}

#[test]
fn a_replaced_bank_comes_back() {
    let mut s = Stretcher::new(512, 2, SR);
    let bank = |layers| LayerBank::build(Algorithm::Wsola, layers, 512, 2, SR);
    assert!(s.replace_bank(bank(3)).is_none(), "a new stretcher has no bank to give back");
    let old = s.replace_bank(bank(2)).expect("the one just given comes back");
    assert_eq!(old.layers, 3);
}

#[test]
fn a_replaced_separation_comes_back() {
    use prism_grain::hstream::Parts;
    let mut s = Stretcher::new(512, 2, SR);
    let first = Arc::new(Parts::default());
    let own = s.replace_parts(Arc::clone(&first));
    assert_eq!(Arc::strong_count(&own), 1, "the empty one it was built with, and the caller is its last holder");
    let old = s.replace_parts(Arc::new(Parts::default()));
    assert!(Arc::ptr_eq(&old, &first), "then the one it was given");
}
