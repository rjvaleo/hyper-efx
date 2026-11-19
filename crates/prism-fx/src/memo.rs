//! Remember the last result of a pure calculation, keyed by input bits.
//!
//! Comparing `f32::to_bits()` rather than `==` keeps `-0.0` and `+0.0`
//! distinct and never mistakes a `NaN` for a match, so a remembered value is
//! always bit-identical to recomputing the expression.

/// Inputs that can be compared strictly by their bit pattern.
pub trait MemoKey: Copy {
    type Bits: Copy + Eq;
    fn to_bits(self) -> Self::Bits;
}

impl MemoKey for f32 {
    type Bits = u32;
    #[inline(always)]
    fn to_bits(self) -> u32 {
        f32::to_bits(self)
    }
}

impl MemoKey for u32 {
    type Bits = u32;
    #[inline(always)]
    fn to_bits(self) -> u32 {
        self
    }
}

impl<A: MemoKey, B: MemoKey> MemoKey for (A, B) {
    type Bits = (A::Bits, B::Bits);
    #[inline(always)]
    fn to_bits(self) -> Self::Bits {
        (self.0.to_bits(), self.1.to_bits())
    }
}

impl<A: MemoKey, B: MemoKey, C: MemoKey> MemoKey for (A, B, C) {
    type Bits = (A::Bits, B::Bits, C::Bits);
    #[inline(always)]
    fn to_bits(self) -> Self::Bits {
        (self.0.to_bits(), self.1.to_bits(), self.2.to_bits())
    }
}

impl<const N: usize, T: MemoKey> MemoKey for [T; N] {
    type Bits = [T::Bits; N];
    #[inline(always)]
    fn to_bits(self) -> Self::Bits {
        self.map(MemoKey::to_bits)
    }
}

/// Caches the last result of a pure calculation for one key's bit pattern.
///
/// Starts empty (`None`), so the first call always computes; subsequent calls
/// with identical input bits return the remembered value without calling `f`.
#[derive(Clone, Copy, Debug)]
pub struct Memo<K: MemoKey, V: Copy> {
    last: Option<(K::Bits, V)>,
}

impl<K: MemoKey, V: Copy> Default for Memo<K, V> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<K: MemoKey, V: Copy> Memo<K, V> {
    #[inline]
    pub const fn new() -> Self {
        Memo { last: None }
    }

    /// Return the remembered value if `key`'s bits match the last call,
    /// otherwise compute `f(key)`, remember it, and return it.
    #[inline(always)]
    pub fn get(&mut self, key: K, f: impl FnOnce(K) -> V) -> V {
        let bits = key.to_bits();
        if let Some((prev, val)) = self.last {
            if prev == bits {
                return val;
            }
        }
        let val = f(key);
        self.last = Some((bits, val));
        val
    }

    /// Forget any remembered value so the next call recomputes.
    #[inline]
    pub fn clear(&mut self) {
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_once_for_same_bits_and_recomputes_on_change() {
        let mut m: Memo<f32, f32> = Memo::new();
        let calls = core::cell::Cell::new(0usize);
        let db = |d: f32| {
            calls.set(calls.get() + 1);
            10f32.powf(d * 0.05)
        };
        let a = m.get(0.0, db);
        assert_eq!(a.to_bits(), 1.0f32.to_bits());
        assert_eq!(calls.get(), 1);
        let b = m.get(0.0, db);
        assert_eq!(b.to_bits(), a.to_bits());
        assert_eq!(calls.get(), 1);

        // -0.0 has different bits from +0.0, so sign-sensitive functions stay exact.
        let _neg = m.get(-0.0, db);
        assert_eq!(calls.get(), 2);

        let c = m.get(-6.0, db);
        assert_eq!(c.to_bits(), 10f32.powf(-6.0 * 0.05).to_bits());
        assert_eq!(calls.get(), 3);
        let d = m.get(-6.0, db);
        assert_eq!(d.to_bits(), c.to_bits());
        assert_eq!(calls.get(), 3);

        m.clear();
        let e = m.get(-6.0, db);
        assert_eq!(e.to_bits(), c.to_bits());
        assert_eq!(calls.get(), 4);
    }

    #[test]
    fn tuple_and_array_keys_track_all_components() {
        let mut m2: Memo<(f32, u32), f32> = Memo::new();
        let mut calls = 0usize;
        let v1 = m2.get((10.0, 48_000), |(ms, sr)| {
            calls += 1;
            (-1.0 / ((ms / 1000.0) * sr as f32)).exp()
        });
        let v2 = m2.get((10.0, 48_000), |_| {
            calls += 1;
            0.0
        });
        assert_eq!(v1.to_bits(), v2.to_bits());
        assert_eq!(calls, 1);
        let _v3 = m2.get((10.0, 44_100), |(ms, sr)| {
            calls += 1;
            (-1.0 / ((ms / 1000.0) * sr as f32)).exp()
        });
        assert_eq!(calls, 2);

        let mut m3: Memo<(f32, f32, f32), f32> = Memo::new();
        let mut c3 = 0usize;
        let _ = m3.get((1.0, 2.0, 3.0), |(a, b, c)| {
            c3 += 1;
            a + b + c
        });
        let _ = m3.get((1.0, 2.0, 3.0), |_| {
            c3 += 1;
            0.0
        });
        assert_eq!(c3, 1);
        let _ = m3.get((1.0, 2.0, 3.5), |(a, b, c)| {
            c3 += 1;
            a + b + c
        });
        assert_eq!(c3, 2);
    }
}
