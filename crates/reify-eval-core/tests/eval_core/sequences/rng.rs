//! A small fixed-seed generator, xorshift64*, as the workspace's other
//! randomised tests use (no property-testing library, PRD 5.5).

pub struct XorShift64Star(u64);

impl XorShift64Star {
    /// A generator for `seed`, spread over the state so that nearby seeds
    /// start far apart; seed 0, which xorshift cannot leave, is mapped to
    /// a fixed state that is not zero.
    pub fn new(seed: u64) -> Self {
        const SPREAD: u64 = 0x9E37_79B9_7F4A_7C15;
        match seed {
            0 => Self(SPREAD),
            seed => Self(seed.wrapping_mul(SPREAD)),
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in `0..n`, for `n` above zero.
    pub fn below(&mut self, n: usize) -> usize {
        assert!(n > 0, "below needs a non-empty range");
        (self.next_u64() % n as u64) as usize
    }

    /// True `numerator` times in `denominator`.
    pub fn chance(&mut self, numerator: u64, denominator: u64) -> bool {
        self.next_u64() % denominator < numerator
    }

    pub fn choose<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}
