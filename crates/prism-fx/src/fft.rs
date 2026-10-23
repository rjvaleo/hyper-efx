//! A radix-2 complex FFT, for the spectral effects.
//!
//! The crate takes no outside crates, so it carries its own. One `Fft` is
//! planned for the largest size an effect uses and then runs any smaller power
//! of two in place, reading its twiddles at a stride, so an effect whose band
//! count is a control allocates once.

pub struct Fft {
    max: usize,
    /// cos and −sin of 2πk/max, for k below max/2.
    cos: Vec<f32>,
    sin: Vec<f32>,
}

impl Fft {
    /// Plan for transforms up to `max` points, a power of two.
    pub fn new(max: usize) -> Self {
        let max = max.next_power_of_two().max(2);
        let half = max / 2;
        let (mut cos, mut sin) = (Vec::with_capacity(half), Vec::with_capacity(half));
        for k in 0..half {
            let a = -core::f64::consts::TAU * k as f64 / max as f64;
            cos.push(a.cos() as f32);
            sin.push(a.sin() as f32);
        }
        Fft { max, cos, sin }
    }

    pub fn max(&self) -> usize {
        self.max
    }

    /// Forward transform of `re + i·im`, in place. The length is a power of
    /// two no larger than the plan.
    pub fn forward(&self, re: &mut [f32], im: &mut [f32]) {
        self.run(re, im, false);
    }

    /// Inverse transform, in place, scaled by 1/n so it undoes `forward`.
    pub fn inverse(&self, re: &mut [f32], im: &mut [f32]) {
        self.run(re, im, true);
        let scale = 1.0 / re.len() as f32;
        for (r, i) in re.iter_mut().zip(im.iter_mut()) {
            *r *= scale;
            *i *= scale;
        }
    }

    fn run(&self, re: &mut [f32], im: &mut [f32], inverse: bool) {
        let n = re.len();
        debug_assert!(n.is_power_of_two() && n <= self.max && im.len() == n);
        if n < 2 {
            return;
        }
        // Bit-reversed order.
        let bits = n.trailing_zeros();
        for i in 0..n {
            let j = i.reverse_bits() >> (usize::BITS - bits);
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let flip = if inverse { -1.0 } else { 1.0 };
        let mut len = 2;
        while len <= n {
            let stride = self.max / len;
            let half = len / 2;
            for start in (0..n).step_by(len) {
                for k in 0..half {
                    let (wr, wi) = (self.cos[k * stride], flip * self.sin[k * stride]);
                    let (a, b) = (start + k, start + k + half);
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len *= 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dft(re: &[f32], im: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let n = re.len();
        let (mut or, mut oi) = (vec![0.0f32; n], vec![0.0f32; n]);
        for k in 0..n {
            let (mut sr, mut si) = (0.0f64, 0.0f64);
            for t in 0..n {
                let a = -core::f64::consts::TAU * (k * t) as f64 / n as f64;
                sr += re[t] as f64 * a.cos() - im[t] as f64 * a.sin();
                si += re[t] as f64 * a.sin() + im[t] as f64 * a.cos();
            }
            or[k] = sr as f32;
            oi[k] = si as f32;
        }
        (or, oi)
    }

    #[test]
    fn it_agrees_with_the_definition_at_every_size_it_plans_for() {
        let fft = Fft::new(256);
        for n in [2, 4, 8, 64, 256] {
            let re: Vec<f32> = (0..n).map(|i| ((i * 7919) % 13) as f32 / 13.0 - 0.5).collect();
            let im: Vec<f32> = (0..n).map(|i| ((i * 104_729) % 11) as f32 / 11.0 - 0.5).collect();
            let (want_r, want_i) = dft(&re, &im);
            let (mut r, mut i) = (re.clone(), im.clone());
            fft.forward(&mut r, &mut i);
            for k in 0..n {
                assert!((r[k] - want_r[k]).abs() < 1e-3 && (i[k] - want_i[k]).abs() < 1e-3, "n {n}, bin {k}");
            }
            fft.inverse(&mut r, &mut i);
            for k in 0..n {
                assert!((r[k] - re[k]).abs() < 1e-5 && (i[k] - im[k]).abs() < 1e-5, "n {n}, round trip at {k}");
            }
        }
    }
}
