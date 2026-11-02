//! A small, portable random number generator.
//!
//! Mulberry32: one 32-bit word of state, a handful of multiplies. It is chosen
//! for one property above all — it is trivial to reproduce bit for bit in
//! JavaScript with `Math.imul`, so a browser can regenerate exactly the island
//! the server did. It is not cryptographic and does not need to be.

use serde::{Deserialize, Serialize};

/// A seed. Newtype so a seed is never confused with a cell index or a count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Seed(pub u32);

impl Seed {
    /// The seed for a word: 32-bit FNV-1a over its UTF-8 bytes, after trimming
    /// and lower-casing, so "Avalon" and " avalon " are the same island.
    pub fn from_word(word: &str) -> Self {
        let mut h: u32 = 0x811c_9dc5;
        for b in word.trim().to_lowercase().bytes() {
            h ^= u32::from(b);
            h = h.wrapping_mul(0x0100_0193);
        }
        Self(h)
    }

    /// A derived seed: the same seed salted, for restarts and repairs.
    #[must_use]
    pub const fn salted(self, salt: u32) -> Self {
        Self(self.0 ^ salt.wrapping_mul(0x9e37_79b9))
    }
}

/// Mulberry32.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u32,
}

impl Rng {
    /// A generator from a seed.
    pub const fn new(seed: Seed) -> Self {
        Self { state: seed.0 }
    }

    /// The next 32 bits.
    pub const fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_add(0x6d2b_79f5);
        let mut t = self.state;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 0x3d));
        t ^ (t >> 14)
    }

    /// A value in `0..n`. `n == 0` yields 0. Modulo bias is accepted: it is
    /// identical on both sides of the wire, which is what matters here.
    pub const fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next_u32() % n }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mulberry32_reference_values() {
        // Reference: the canonical JavaScript mulberry32 seeded with 1.
        let mut r = Rng::new(Seed(1));
        let got: Vec<u32> = (0..3).map(|_| r.next_u32()).collect();
        assert_eq!(got, vec![2_693_262_067, 11_749_833, 2_265_367_787]);
    }

    #[test]
    fn words_fold_case_and_space() {
        assert_eq!(Seed::from_word("Avalon"), Seed::from_word("  avalon "));
        assert_ne!(Seed::from_word("avalon"), Seed::from_word("lyonesse"));
        assert_eq!(Seed::from_word(""), Seed(0x811c_9dc5));
    }
}
