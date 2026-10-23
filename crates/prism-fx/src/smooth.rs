//! A parameter glide, so a moved control does not click.

/// One-pole glide toward a target. Cheap enough to run every sample.
#[derive(Clone, Copy, Debug)]
pub struct Smoothed {
    current: f32,
    target: f32,
    coef: f32,
}

impl Smoothed {
    /// `ms` is the time to cover about 63 % of a jump; 0 jumps at once.
    pub fn new(sample_rate: f32, ms: f32) -> Self {
        let frames = ms.max(0.0) * 0.001 * sample_rate.max(1.0);
        let coef = if frames < 1.0 { 0.0 } else { (-1.0 / frames).exp() };
        Smoothed { current: 0.0, target: 0.0, coef }
    }

    /// Glide toward `value`.
    pub fn set(&mut self, value: f32) {
        self.target = value;
    }

    /// Land on `value` now: for reset, and for a kernel's first value.
    pub fn jump(&mut self, value: f32) {
        self.target = value;
        self.current = value;
    }

    /// The next sample's value. Named like `Iterator::next` on purpose: it is
    /// called once per sample, and it never ends.
    #[inline]
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f32 {
        let next = self.target + (self.current - self.target) * self.coef;
        // Land exactly once a step stops moving the value. Close to the target
        // each step is smaller than half an f32 step there and rounds to nothing,
        // so without this the glide would stall a few millionths short — and a
        // held value would never be bit-stable.
        self.current = if next == self.current || (next - self.target).abs() < 1e-9 {
            self.target
        } else {
            next
        };
        self.current
    }

    pub fn target(&self) -> f32 {
        self.target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glides_and_then_settles_exactly() {
        let mut s = Smoothed::new(48_000.0, 5.0);
        s.jump(0.0);
        s.set(1.0);
        let first = s.next();
        assert!(first > 0.0 && first < 0.01);
        for _ in 0..48_000 {
            s.next();
        }
        assert_eq!(s.next(), 1.0);
    }

    #[test]
    fn zero_time_jumps() {
        let mut s = Smoothed::new(48_000.0, 0.0);
        s.set(0.7);
        assert_eq!(s.next(), 0.7);
    }
}
