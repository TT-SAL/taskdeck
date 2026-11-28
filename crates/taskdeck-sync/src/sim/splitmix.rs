//! SplitMix64 (engine.md §31.2): the one generator of the fake sealer and the
//! simulator.

/// The simulator's generator (engine.md §31.2).
pub struct SplitMix64 {
    pub state: u64,
}

impl SplitMix64 {
    /// The next output (engine.md §31.2 `next()`).
    // The spec names the method `next`; the generator is not an iterator.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// `next() mod n`, for `n ≥ 1` (engine.md §31.2 `below(n)`).
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n >= 1, "SplitMix64::below needs n >= 1 (engine.md §31.2)");
        self.next() % n
    }

    /// `below(1,000,000) < ppm` (engine.md §31.2 `chance(p)`).
    pub fn chance(&mut self, ppm: u32) -> bool {
        self.below(1_000_000) < u64::from(ppm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix64_matches_the_reference_outputs() {
        // The published SplitMix64 reference outputs for seed 0 and seed
        // 1234567, computed by the algorithm of engine.md §31.2.
        let mut g = SplitMix64 { state: 0 };
        assert_eq!(g.next(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(g.next(), 0x6E78_9E6A_A1B9_65F4);
        assert_eq!(g.next(), 0x06C4_5D18_8009_454F);
        let mut g = SplitMix64 { state: 1_234_567 };
        assert_eq!(g.next(), 6_457_827_717_110_365_317);
        assert_eq!(g.next(), 3_203_168_211_198_807_973);
        assert_eq!(g.next(), 9_817_491_932_198_370_423);
    }

    #[test]
    fn below_and_chance_draw_one_output_each() {
        let mut a = SplitMix64 { state: 42 };
        let mut b = SplitMix64 { state: 42 };
        assert_eq!(a.below(10), b.next() % 10);
        assert_eq!(a.chance(500_000), b.next() % 1_000_000 < 500_000);
        assert_eq!(a.below(1), 0);
        assert!(!a.chance(0));
        assert!(a.chance(1_000_000));
        assert_eq!(a.state, b.state.wrapping_add(0x9E37_79B9_7F4A_7C15_u64.wrapping_mul(3)));
    }

    #[test]
    fn equal_seeds_give_equal_sequences() {
        let mut a = SplitMix64 { state: 7 };
        let mut b = SplitMix64 { state: 7 };
        for _ in 0..64 {
            assert_eq!(a.next(), b.next());
        }
    }
}
