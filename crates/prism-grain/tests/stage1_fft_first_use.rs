//! Its own executable keeps the FFT OnceLock cold regardless of test order.
mod stage1_support {
    pub mod rt;
}
use stage1_support::rt;
#[test]
#[ignore = "stage 2: first FFT builds the twiddle table, fft.rs:115"]
fn first_fft_must_not_allocate_on_the_callback() {
    rt::verify();
    let mut re = vec![0.0; 8192];
    let mut im = vec![0.0; 8192];
    re[0] = 1.0;
    rt::start();
    let valid = prism_grain::fft::fft(&mut re, &mut im);
    let counts = rt::stop();
    assert!(valid);
    rt::clean("first FFT", counts);
}
