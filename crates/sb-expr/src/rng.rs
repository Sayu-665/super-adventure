//! Small deterministic random number generator (SplitMix64) for `random()` and
//! `randomInt()`.

/// Seed used until [`CustomUniforms::set_seed`](crate::CustomUniforms::set_seed) is called.
pub const DEFAULT_SEED: u64 = 0x5B1D_6E5E_ED00_0001;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rng {
    state: u64,
}

impl Rng {
    pub(crate) const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform float in `[0, 1)`.
    pub(crate) fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 * (1.0 / (1u32 << 24) as f32)
    }

    /// Uniform `i32` over the full range.
    pub(crate) fn next_i32(&mut self) -> i32 {
        (self.next_u64() >> 32) as u32 as i32
    }

    /// Uniform integer in `[0, bound)`; 0 when `bound <= 0`.
    pub(crate) fn below(&mut self, bound: i32) -> i32 {
        if bound <= 0 {
            return 0;
        }
        (self.next_u64() % bound as u64) as i32
    }

    /// Uniform integer in `[lo, hi)`; `lo` when the range is empty.
    pub(crate) fn range(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        let span = (i64::from(hi) - i64::from(lo)) as u64;
        (i64::from(lo) + (self.next_u64() % span) as i64) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_in_range() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            let f = a.next_f32();
            assert_eq!(f, b.next_f32());
            assert!((0.0..1.0).contains(&f));
            let r = a.range(-5, 5);
            assert_eq!(r, b.range(-5, 5));
            assert!((-5..5).contains(&r));
            let k = a.below(7);
            assert_eq!(k, b.below(7));
            assert!((0..7).contains(&k));
        }
        assert_eq!(a.range(3, 3), 3);
        assert!(a.range(i32::MIN, i32::MAX) < i32::MAX);
        assert_eq!(a.below(0), 0);
        assert_ne!(Rng::new(1).next_u64(), Rng::new(2).next_u64());
    }
}
