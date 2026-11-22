//! The generator's own randomness: SplitMix64 (Steele, Lea and Flood, 2014),
//! seeded explicitly everywhere. No `rand`, no clock, no entropy source, so a
//! seed names one sequence on every machine.

#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number in `0..n` (`n` > 0). Plain modulo: the bias is far below
    /// anything a fixture could notice, and it keeps the sequence obvious.
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "below(0)");
        self.next_u64() % n
    }

    /// A number in `low..=high`.
    pub fn between(&mut self, low: i64, high: i64) -> i64 {
        assert!(low <= high, "between({low}, {high})");
        low + self.below((high - low + 1) as u64) as i64
    }

    /// True `numerator` times in `denominator`.
    pub fn chance(&mut self, numerator: u64, denominator: u64) -> bool {
        self.below(denominator) < numerator
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_reference_sequence() {
        // The first outputs of the reference implementation for seed 0
        // (as published with the algorithm, and in Java's SplittableRandom).
        let mut rng = SplitMix64::new(0);
        assert_eq!(rng.next_u64(), 0xe220_a839_7b1d_cdaf);
        assert_eq!(rng.next_u64(), 0x6e78_9e6a_a1b9_65f4);
        assert_eq!(rng.next_u64(), 0x06c4_5d18_8009_454f);
    }

    #[test]
    fn a_seed_names_one_sequence() {
        let a: Vec<u64> = { let mut r = SplitMix64::new(42); (0..100).map(|_| r.next_u64()).collect() };
        let b: Vec<u64> = { let mut r = SplitMix64::new(42); (0..100).map(|_| r.next_u64()).collect() };
        let c: Vec<u64> = { let mut r = SplitMix64::new(43); (0..100).map(|_| r.next_u64()).collect() };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn ranges_stay_in_range() {
        let mut rng = SplitMix64::new(7);
        for _ in 0..10_000 {
            assert!(rng.below(13) < 13);
            let x = rng.between(-5, 5);
            assert!((-5..=5).contains(&x));
        }
        let picks: std::collections::BTreeSet<u8> = (0..1000).map(|_| *rng.pick(&[1u8, 2, 3])).collect();
        assert_eq!(picks.into_iter().collect::<Vec<_>>(), vec![1, 2, 3]);
        assert!(!rng.chance(0, 5));
        assert!(rng.chance(5, 5));
    }
}
