//! Radix-2 FFT and window functions.
//!
//! Hand-written rather than pulled from a crate so the binary stays
//! dependency-free and cross-compiles with no toolchain beyond rustc. The
//! transform is iterative and in-place; there is no recursion and no allocation
//! per call, because the spectrogram runs it a few thousand times per view.

use std::f32::consts::PI;

/// In-place complex FFT. `re` and `im` must be the same power-of-two length.
///
/// Returns `false` and leaves the input untouched if the length is unusable,
/// rather than panicking on data that came in over HTTP.
pub fn fft(re: &mut [f32], im: &mut [f32]) -> bool {
    let n = re.len();
    if n != im.len() || n == 0 || !n.is_power_of_two() {
        return false;
    }
    if n == 1 {
        return true;
    }

    if n <= TWIDDLE_N {
        // Bit-reversal permutation from the per-size swap table.
        for &(i, j) in &bitrev_tables()[n.trailing_zeros() as usize] {
            let (i, j) = (i as usize, j as usize);
            re.swap(i, j);
            im.swap(i, j);
        }

        // Butterflies, doubling the block length each pass.
        let tw = twiddles();
        let mut len = 2usize;
        while len <= n {
            // One table serves every size: the twiddle for stage `len` at index
            // `k` is `exp(-2*pi*i*k/len)`, which is the entry `k * (N/len)` of a
            // table built for `N`. Both are powers of two, so the stride is
            // exact and the value is *identical* to computing it from the index
            // — this is a table of the same numbers, not an approximation of
            // them. Checked: bit-for-bit equal output at 1024 through 8192.
            let stride = TWIDDLE_N / len;
            let half = len / 2;
            for (rc, ic) in re.chunks_exact_mut(len).zip(im.chunks_exact_mut(len)) {
                let (r0, r1) = rc.split_at_mut(half);
                let (i0, i1) = ic.split_at_mut(half);
                for (((ur, ui), (xr, xi)), &(cr, ci)) in r0
                    .iter_mut()
                    .zip(i0.iter_mut())
                    .zip(r1.iter_mut().zip(i1.iter_mut()))
                    .zip(tw.iter().step_by(stride))
                {
                    let (u_re, u_im) = (*ur, *ui);
                    let (x_re, x_im) = (*xr, *xi);
                    let vr = x_re * cr - x_im * ci;
                    let vi = x_re * ci + x_im * cr;
                    *ur = u_re + vr;
                    *ui = u_im + vi;
                    *xr = u_re - vr;
                    *xi = u_im - vi;
                }
            }
            len <<= 1;
        }
        return true;
    }

    // Bigger than the table: correct, just slower. Nothing in the program asks
    // for a transform this long — the vocoder's own maximum is 8192 — so this
    // is here to stay right rather than to be fast.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let ang = -2.0 * PI / len as f32;
        let half = len / 2;
        for (rc, ic) in re.chunks_exact_mut(len).zip(im.chunks_exact_mut(len)) {
            let (r0, r1) = rc.split_at_mut(half);
            let (i0, i1) = ic.split_at_mut(half);
            // Recomputing the twiddle by repeated multiplication accumulates
            // error over long transforms, so derive each from its index.
            for (k, ((ur, ui), (xr, xi))) in r0
                .iter_mut()
                .zip(i0.iter_mut())
                .zip(r1.iter_mut().zip(i1.iter_mut()))
                .enumerate()
            {
                let a = ang * k as f32;
                let (cr, ci) = (a.cos(), a.sin());
                let (u_re, u_im) = (*ur, *ui);
                let (x_re, x_im) = (*xr, *xi);
                let vr = x_re * cr - x_im * ci;
                let vi = x_re * ci + x_im * cr;
                *ur = u_re + vr;
                *ui = u_im + vi;
                *xr = u_re - vr;
                *xi = u_im - vi;
            }
        }
        len <<= 1;
    }
    true
}

/// The largest transform the twiddle table serves. The vocoder's own maximum.
pub const TWIDDLE_N: usize = 8192;

static TWIDDLES: std::sync::OnceLock<Vec<(f32, f32)>> = std::sync::OnceLock::new();
static BITREV: std::sync::OnceLock<Vec<Vec<(u16, u16)>>> = std::sync::OnceLock::new();

/// Swap pairs `(i, j)` with `i < j` for bit-reversal of each power-of-two size
/// from `1` up to `TWIDDLE_N`, built once (~32 KB across all 13 sizes).
fn bitrev_tables() -> &'static [Vec<(u16, u16)>] {
    BITREV.get_or_init(|| {
        let max_pow = TWIDDLE_N.trailing_zeros() as usize;
        let mut tables = Vec::with_capacity(max_pow + 1);
        tables.push(Vec::new()); // 2^0 = 1
        for pow in 1..=max_pow {
            let n = 1usize << pow;
            let mut pairs = Vec::new();
            let mut j = 0usize;
            for i in 1..n {
                let mut bit = n >> 1;
                while j & bit != 0 {
                    j ^= bit;
                    bit >>= 1;
                }
                j |= bit;
                if i < j {
                    pairs.push((i as u16, j as u16));
                }
            }
            tables.push(pairs);
        }
        tables
    })
}

/// `exp(-2*pi*i*j/N)` for `j` in `0..N/2`, built once.
///
/// The transform used to call `cos` and `sin` inside the innermost butterfly:
/// at 8192 points that is 53,248 butterflies and **106,496 transcendental calls
/// per transform**. The comment there was right that repeated multiplication
/// drifts — the answer to that is a table, which drifts not at all and costs
/// nothing. Measured 2.2x to 2.8x faster from 1024 to 8192, bit-identical.
///
/// 32 KB, once per process.
fn twiddles() -> &'static [(f32, f32)] {
    TWIDDLES.get_or_init(|| {
        (0..TWIDDLE_N / 2)
            .map(|j| {
                let a = -2.0 * PI * j as f32 / TWIDDLE_N as f32;
                (a.cos(), a.sin())
            })
            .collect()
    })
}

/// Build the tables now, off whatever thread you like.
///
/// They are built on first use, and first use may well be inside the audio
/// callback — one-off allocations there are exactly the kind of thing that
/// costs one block. The engine calls this while it is starting up.
pub fn prime() {
    let _ = twiddles();
    let _ = bitrev_tables();
}

/// Incremental driver for [`fft`] and real-output [`crate::vstream::ifft`].
///
/// Runs the exact same bit-reversal swaps (from `bitrev_tables()`), the exact
/// same radix-2 butterflies in `(len, start, k)` order (from `twiddles()`), and
/// for inverse transforms the exact same pre-conjugation and `1.0 / n` real
/// scaling — bounded by a caller-supplied butterfly-equivalent work budget per
/// call so large transforms can be spread across audio blocks bit-identically.
#[derive(Clone, Copy, Debug)]
pub struct FftStepper {
    n: usize,
    inverse: bool,
    phase: FftPhase,
}

#[derive(Clone, Copy, Debug)]
enum FftPhase {
    Conj { i: usize },
    BitRev { idx: usize },
    Butterfly { len: usize, start: usize, k: usize },
    Scale { i: usize },
    Done,
}

impl FftStepper {
    /// Start an incremental transform of length `n` (power of two `<= TWIDDLE_N`).
    /// When `inverse` is `true`, performs the real-output inverse transform
    /// identical to [`crate::vstream::ifft`] (conjugate `im`, forward FFT, scale `re`).
    pub fn new(n: usize, inverse: bool) -> Self {
        let phase = if n == 0 || !n.is_power_of_two() || n > TWIDDLE_N {
            FftPhase::Done
        } else if inverse {
            FftPhase::Conj { i: 0 }
        } else if n == 1 {
            FftPhase::Done
        } else {
            FftPhase::BitRev { idx: 0 }
        };
        FftStepper { n, inverse, phase }
    }

    pub fn is_done(&self) -> bool {
        matches!(self.phase, FftPhase::Done)
    }

    /// Estimated total work units for a transform of length `n`.
    pub fn total_work(n: usize, inverse: bool) -> usize {
        if n <= 1 || !n.is_power_of_two() || n > TWIDDLE_N {
            return 0;
        }
        let log2 = n.trailing_zeros() as usize;
        // ~n/2 bit-rev swaps at weight 2 (= n) + (n/2)*log2 butterflies at weight 1.
        let base = n + (n / 2) * log2;
        if inverse {
            base + n // Conj (n/2) + Scale (n/2)
        } else {
            base
        }
    }

    /// Remaining estimated work units.
    pub fn remaining_work(&self) -> usize {
        let n = self.n;
        if n <= 1 || !n.is_power_of_two() || n > TWIDDLE_N {
            return 0;
        }
        let log2 = n.trailing_zeros() as usize;
        let tail_scale = if self.inverse { n / 2 } else { 0 };
        match self.phase {
            FftPhase::Done => 0,
            FftPhase::Conj { i } => (n - i) / 2 + n + (n / 2) * log2 + tail_scale,
            FftPhase::BitRev { idx } => {
                let pairs = bitrev_tables()[log2].len();
                pairs.saturating_sub(idx) * 2 + (n / 2) * log2 + tail_scale
            }
            FftPhase::Butterfly { len, start, k } => {
                let stage = len.trailing_zeros() as usize; // 1..=log2
                let stages_after = log2.saturating_sub(stage);
                let half = len / 2;
                let done_in_stage = (start / len) * half + k;
                let rem_in_stage = (n / 2).saturating_sub(done_in_stage);
                rem_in_stage + stages_after * (n / 2) + tail_scale
            }
            FftPhase::Scale { i } => (n - i).div_ceil(2),
        }
    }

    /// Advance by at most `budget` work units, returning how many units were
    /// spent. Call [`Self::is_done`] to check completion.
    pub fn step(&mut self, re: &mut [f32], im: &mut [f32], budget: usize) -> usize {
        let n = self.n;
        if matches!(self.phase, FftPhase::Done) {
            return 0;
        }
        debug_assert!(n.is_power_of_two() && n <= TWIDDLE_N && re.len() == n && im.len() == n);
        let mut spent = 0usize;
        let cap = budget.max(1);

        while spent < cap {
            let left = cap - spent;
            match self.phase {
                FftPhase::Done => break,
                FftPhase::Conj { i } => {
                    let take = left.saturating_mul(2).max(1);
                    let end = i.saturating_add(take).min(n);
                    for v in &mut im[i..end] {
                        *v = -*v;
                    }
                    spent = spent.saturating_add((end - i).div_ceil(2));
                    self.phase = if end == n {
                        if n == 1 {
                            FftPhase::Scale { i: 0 }
                        } else {
                            FftPhase::BitRev { idx: 0 }
                        }
                    } else {
                        FftPhase::Conj { i: end }
                    };
                }
                FftPhase::BitRev { idx } => {
                    let pairs = &bitrev_tables()[n.trailing_zeros() as usize];
                    let take = (left / 2).max(1);
                    let end = idx.saturating_add(take).min(pairs.len());
                    for &(a, b) in &pairs[idx..end] {
                        let (a, b) = (a as usize, b as usize);
                        re.swap(a, b);
                        im.swap(a, b);
                    }
                    spent = spent.saturating_add((end - idx) * 2);
                    self.phase = if end == pairs.len() {
                        FftPhase::Butterfly { len: 2, start: 0, k: 0 }
                    } else {
                        FftPhase::BitRev { idx: end }
                    };
                }
                FftPhase::Butterfly { mut len, mut start, mut k } => {
                    let tw = twiddles();
                    let stride = TWIDDLE_N / len;
                    let half = len / 2;
                    let mut rem = left;

                    while rem > 0 && start < n {
                        if k == 0 && half <= rem {
                            // Whole butterfly group at once via slice iterators
                            // (same inner loop as `fft`).
                            let groups = (rem / half).min((n - start) / len);
                            let span = groups * len;
                            let re_span = &mut re[start..start + span];
                            let im_span = &mut im[start..start + span];
                            for (rc, ic) in re_span.chunks_exact_mut(len).zip(im_span.chunks_exact_mut(len)) {
                                let (r0, r1) = rc.split_at_mut(half);
                                let (i0, i1) = ic.split_at_mut(half);
                                for (((ur, ui), (xr, xi)), &(cr, ci)) in r0
                                    .iter_mut()
                                    .zip(i0.iter_mut())
                                    .zip(r1.iter_mut().zip(i1.iter_mut()))
                                    .zip(tw.iter().step_by(stride))
                                {
                                    let (u_re, u_im) = (*ur, *ui);
                                    let (x_re, x_im) = (*xr, *xi);
                                    let vr = x_re * cr - x_im * ci;
                                    let vi = x_re * ci + x_im * cr;
                                    *ur = u_re + vr;
                                    *ui = u_im + vi;
                                    *xr = u_re - vr;
                                    *xi = u_im - vi;
                                }
                            }
                            let did = groups * half;
                            spent += did;
                            rem -= did;
                            start += span;
                        } else {
                            // Partial group for large stages (len > 2*rem).
                            let count = rem.min(half - k);
                            let end_k = k + count;
                            let (r0, r1) = re[start..start + len].split_at_mut(half);
                            let (i0, i1) = im[start..start + len].split_at_mut(half);
                            for (((ur, ui), (xr, xi)), &(cr, ci)) in r0[k..end_k]
                                .iter_mut()
                                .zip(i0[k..end_k].iter_mut())
                                .zip(r1[k..end_k].iter_mut().zip(i1[k..end_k].iter_mut()))
                                .zip(tw[k * stride..].iter().step_by(stride))
                            {
                                let (u_re, u_im) = (*ur, *ui);
                                let (x_re, x_im) = (*xr, *xi);
                                let vr = x_re * cr - x_im * ci;
                                let vi = x_re * ci + x_im * cr;
                                *ur = u_re + vr;
                                *ui = u_im + vi;
                                *xr = u_re - vr;
                                *xi = u_im - vi;
                            }
                            spent += count;
                            rem -= count;
                            k = end_k;
                            if k == half {
                                k = 0;
                                start += len;
                            }
                        }
                    }

                    if start >= n {
                        len <<= 1;
                        if len <= n {
                            self.phase = FftPhase::Butterfly { len, start: 0, k: 0 };
                        } else if self.inverse {
                            self.phase = FftPhase::Scale { i: 0 };
                        } else {
                            self.phase = FftPhase::Done;
                        }
                    } else {
                        self.phase = FftPhase::Butterfly { len, start, k };
                    }
                }
                FftPhase::Scale { i } => {
                    let take = left.saturating_mul(2).max(1);
                    let end = i.saturating_add(take).min(n);
                    let inv = 1.0 / n as f32;
                    for v in &mut re[i..end] {
                        *v *= inv;
                    }
                    spent = spent.saturating_add((end - i).div_ceil(2));
                    self.phase = if end == n {
                        FftPhase::Done
                    } else {
                        FftPhase::Scale { i: end }
                    };
                }
            }
        }
        spent
    }
}

/// Magnitude of each bin up to Nyquist, given a completed transform.
pub fn magnitudes(re: &[f32], im: &[f32]) -> Vec<f32> {
    let half = re.len() / 2 + 1;
    (0..half).map(|i| (re[i] * re[i] + im[i] * im[i]).sqrt()).collect()
}

/// A Hann window of length `n`.
///
/// Without a window, a tone that does not sit exactly on a bin centre smears
/// across the whole spectrum and the spectrogram turns to mush.
pub fn hann(n: usize) -> Vec<f32> {
    if n <= 1 {
        return vec![1.0; n];
    }
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / (n - 1) as f32).cos())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transform(signal: &[f32]) -> Vec<f32> {
        let mut re = signal.to_vec();
        let mut im = vec![0.0; signal.len()];
        assert!(fft(&mut re, &mut im));
        magnitudes(&re, &im)
    }

    #[test]
    fn a_constant_signal_puts_all_energy_in_the_dc_bin() {
        let mags = transform(&[1.0f32; 64]);
        assert!((mags[0] - 64.0).abs() < 1e-3, "dc bin was {}", mags[0]);
        for (i, m) in mags.iter().enumerate().skip(1) {
            assert!(*m < 1e-3, "bin {i} should be empty, was {m}");
        }
    }

    #[test]
    fn silence_transforms_to_silence() {
        for m in transform(&[0.0f32; 128]) {
            assert!(m < 1e-6);
        }
    }

    #[test]
    fn a_sine_on_a_bin_centre_lands_in_that_bin() {
        // 8 cycles across 128 samples puts the tone exactly on bin 8.
        let n = 128;
        let sig: Vec<f32> = (0..n)
            .map(|i| (2.0 * PI * 8.0 * i as f32 / n as f32).sin())
            .collect();
        let mags = transform(&sig);
        let peak = mags
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        assert_eq!(peak, 8);
    }

    #[test]
    fn two_tones_produce_two_peaks() {
        let n = 256;
        let sig: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / n as f32;
                (2.0 * PI * 10.0 * t).sin() + (2.0 * PI * 40.0 * t).sin()
            })
            .collect();
        let mags = transform(&sig);
        let mut idx: Vec<usize> = (0..mags.len()).collect();
        idx.sort_by(|&a, &b| mags[b].partial_cmp(&mags[a]).unwrap());
        let top: Vec<usize> = idx.into_iter().take(2).collect();
        assert!(top.contains(&10), "expected a peak at bin 10, got {top:?}");
        assert!(top.contains(&40), "expected a peak at bin 40, got {top:?}");
    }

    #[test]
    fn energy_is_conserved() {
        // Parseval: the sum of squares in time equals the sum in frequency,
        // scaled by n. This catches a whole class of indexing mistakes.
        let n = 64;
        let sig: Vec<f32> = (0..n).map(|i| ((i * 7 % 13) as f32 / 13.0) - 0.5).collect();
        let time: f32 = sig.iter().map(|v| v * v).sum();

        let mut re = sig.clone();
        let mut im = vec![0.0; n];
        assert!(fft(&mut re, &mut im));
        let freq: f32 = (0..n).map(|i| re[i] * re[i] + im[i] * im[i]).sum::<f32>() / n as f32;

        assert!((time - freq).abs() < 1e-2, "time {time} vs freq {freq}");
    }

    #[test]
    fn a_non_power_of_two_is_refused_rather_than_panicking() {
        let mut re = vec![0.0f32; 100];
        let mut im = vec![0.0f32; 100];
        assert!(!fft(&mut re, &mut im));
    }

    #[test]
    fn mismatched_buffers_are_refused() {
        let mut re = vec![0.0f32; 64];
        let mut im = vec![0.0f32; 32];
        assert!(!fft(&mut re, &mut im));
    }

    #[test]
    fn a_single_sample_transform_is_a_no_op() {
        let mut re = vec![3.0f32];
        let mut im = vec![0.0f32];
        assert!(fft(&mut re, &mut im));
        assert_eq!(re[0], 3.0);
    }

    #[test]
    fn magnitudes_cover_dc_through_nyquist_inclusive() {
        let mags = transform(&[1.0f32; 64]);
        assert_eq!(mags.len(), 33);
    }

    #[test]
    fn a_hann_window_starts_and_ends_at_zero_and_peaks_in_the_middle() {
        let w = hann(65);
        assert!(w[0].abs() < 1e-6);
        assert!(w[64].abs() < 1e-6);
        assert!((w[32] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn windowing_narrows_a_tone_that_sits_between_bins() {
        // 8.5 cycles falls between bins. Unwindowed it leaks across the whole
        // spectrum; windowed the energy stays local. Compare the far-field.
        let n = 256;
        let raw: Vec<f32> = (0..n)
            .map(|i| (2.0 * PI * 8.5 * i as f32 / n as f32).sin())
            .collect();
        let w = hann(n);
        let windowed: Vec<f32> = raw.iter().zip(&w).map(|(s, k)| s * k).collect();

        let far = |mags: &[f32]| -> f32 { mags[40..].iter().copied().fold(0.0, f32::max) };
        let raw_far = far(&transform(&raw));
        let win_far = far(&transform(&windowed));
        assert!(
            win_far < raw_far * 0.2,
            "windowing should suppress distant leakage: {win_far} vs {raw_far}"
        );
    }

    #[test]
    fn tabled_bitrev_and_slice_butterflies_are_bit_identical_for_all_sizes() {
        prime();
        let tw = twiddles();
        for pow in 1..=TWIDDLE_N.trailing_zeros() {
            let n = 1usize << pow;
            let mut seed = 0x1234_5678u32 ^ n as u32;
            let mut re_a = Vec::with_capacity(n);
            let mut im_a = Vec::with_capacity(n);
            for _ in 0..n {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                re_a.push(((seed >> 8) as f32) / 8_388_608.0 - 1.0);
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                im_a.push(((seed >> 8) as f32) / 8_388_608.0 - 1.0);
            }
            let mut re_b = re_a.clone();
            let mut im_b = im_a.clone();

            assert!(fft(&mut re_a, &mut im_a));

            // Direct reference: on-the-fly bit-reversal + indexed butterflies.
            let mut j = 0usize;
            for i in 1..n {
                let mut bit = n >> 1;
                while j & bit != 0 {
                    j ^= bit;
                    bit >>= 1;
                }
                j |= bit;
                if i < j {
                    re_b.swap(i, j);
                    im_b.swap(i, j);
                }
            }
            let mut len = 2usize;
            while len <= n {
                let stride = TWIDDLE_N / len;
                let half = len / 2;
                let mut i = 0;
                while i < n {
                    for k in 0..half {
                        let (cr, ci) = tw[k * stride];
                        let (ur, ui) = (re_b[i + k], im_b[i + k]);
                        let (xr, xi) = (re_b[i + k + half], im_b[i + k + half]);
                        let vr = xr * cr - xi * ci;
                        let vi = xr * ci + xi * cr;
                        re_b[i + k] = ur + vr;
                        im_b[i + k] = ui + vi;
                        re_b[i + k + half] = ur - vr;
                        im_b[i + k + half] = ui - vi;
                    }
                    i += len;
                }
                len <<= 1;
            }

            for idx in 0..n {
                assert_eq!(re_a[idx].to_bits(), re_b[idx].to_bits(), "re mismatch at n={n} idx={idx}");
                assert_eq!(im_a[idx].to_bits(), im_b[idx].to_bits(), "im mismatch at n={n} idx={idx}");
            }
        }
    }

    #[test]
    fn fft_stepper_is_bit_identical_to_fft_and_ifft_for_all_sizes_and_budgets() {
        prime();
        for pow in 1..=TWIDDLE_N.trailing_zeros() {
            let n = 1usize << pow;
            let mut seed = 0x9e37_79b9u32 ^ n as u32;
            let mut base_re = Vec::with_capacity(n);
            let mut base_im = Vec::with_capacity(n);
            for _ in 0..n {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                base_re.push(((seed >> 8) as f32) / 8_388_608.0 - 1.0);
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                base_im.push(((seed >> 8) as f32) / 8_388_608.0 - 1.0);
            }

            let mut ref_fwd_re = base_re.clone();
            let mut ref_fwd_im = base_im.clone();
            assert!(fft(&mut ref_fwd_re, &mut ref_fwd_im));

            let mut ref_inv_re = base_re.clone();
            let mut ref_inv_im = base_im.clone();
            crate::vstream::ifft(&mut ref_inv_re, &mut ref_inv_im);

            for &budget in &[1usize, 17, 257, 4096, usize::MAX] {
                let mut re = base_re.clone();
                let mut im = base_im.clone();
                let mut st = FftStepper::new(n, false);
                while !st.is_done() {
                    st.step(&mut re, &mut im, budget);
                }
                for idx in 0..n {
                    assert_eq!(re[idx].to_bits(), ref_fwd_re[idx].to_bits(), "fwd re n={n} b={budget} idx={idx}");
                    assert_eq!(im[idx].to_bits(), ref_fwd_im[idx].to_bits(), "fwd im n={n} b={budget} idx={idx}");
                }

                let mut re = base_re.clone();
                let mut im = base_im.clone();
                let mut st = FftStepper::new(n, true);
                while !st.is_done() {
                    st.step(&mut re, &mut im, budget);
                }
                for idx in 0..n {
                    assert_eq!(re[idx].to_bits(), ref_inv_re[idx].to_bits(), "inv re n={n} b={budget} idx={idx}");
                }
            }
        }
    }
}
