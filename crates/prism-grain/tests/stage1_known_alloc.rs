//! Stage 2 regression targets. These deliberately exercise the audited faults.
mod stage1_support {
    pub mod rt;
}
use prism_grain::{
    grain::StreamParams,
    hstream::Parts,
    render::{BlockRenderer, Source},
    stretcher::Stretcher,
};
use stage1_support::rt;
use std::sync::Arc;

#[test]
#[ignore = "stage 2: oversized callback grows norm, render.rs:263"]
fn callback_larger_than_promised_grows_normalisation() {
    rt::verify();
    let source = Source {
        samples: vec![0.1; 48_000],
        channels: 1,
    };
    let params = StreamParams::new(48_000, 48_000);
    let mut renderer = BlockRenderer::new(128);
    let mut output = vec![0.0; 4096];
    renderer.render(&mut output[..128], 1, &source, &params, &mut []);
    rt::start();
    renderer.render(&mut output, 1, &source, &params, &mut []);
    let counts = rt::stop();
    rt::clean("oversized callback", counts);
}

#[test]
#[ignore = "stage 2: set_parts frees the replaced Arc, stretcher.rs:435"]
fn adopting_parts_must_return_the_previous_owner() {
    rt::verify();
    let mut stretcher = Stretcher::new(128, 1, 48_000);
    let parts = Arc::new(Parts::default());
    rt::start();
    stretcher.set_parts(parts);
    let counts = rt::stop();
    rt::clean("set_parts", counts);
}

#[test]
#[ignore = "stage 2: 8192-point spectrum outgrows 1024 peaks, vstream.rs:132"]
fn dense_spectrum_outgrows_the_vocoder_peak_list() {
    rt::verify();
    prism_grain::fft::prime();
    // One isolated peak every three bins, with deterministic phases. Inverse
    // transform off-thread; repeating the period keeps every analysis full.
    let n = 8192;
    let mut re = vec![0.0; n];
    let mut im = vec![0.0; n];
    let mut seed = 19u32;
    for k in (2..n / 2 - 2).step_by(3) {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let phase = seed as f32 / u32::MAX as f32 * std::f32::consts::TAU;
        re[k] = phase.cos();
        im[k] = phase.sin();
        re[n - k] = re[k];
        im[n - k] = -im[k];
    }
    assert!(prism_grain::fft::fft(&mut re, &mut im));
    let input: Vec<f32> = (0..48_000).map(|i| re[i % n] / n as f32).collect();
    let mut vocoder = prism_grain::vstream::VocoderStream::new(128, 1);
    let mut params = prism_grain::stream::StretchParams {
        ratio: 2.0,
        window_ms: 40.0,
        sample_rate: 48_000,
        wsola: Default::default(),
        vocoder: Default::default(),
        grain: Default::default(),
        soundhack: Default::default(),
    };
    let mut output = [0.0; 128];
    // Warm the normal-sized app configuration, then select its largest FFT.
    for _ in 0..750 {
        vocoder.render(&mut output, 1, &input, &params);
    }
    params.vocoder.window_ms = 170.0;
    params.vocoder.peak_width = 1;
    vocoder.seek(0, input.len(), &params);
    rt::start();
    for _ in 0..750 {
        vocoder.render(&mut output, 1, &input, &params);
    }
    let counts = rt::stop();
    rt::clean("8192-point peak list", counts);
}
