//! 确定性随机数：SplitMix64 播种 + xoshiro256++ 生成器。
//! 不依赖 rand crate，保证跨版本字节级稳定。
//! 每个代理持有独立子流（种子由世界种子、代理编号、用途派生），
//! 因此代理求值顺序不影响结果——回放不依赖调度细节。

pub type WorldSeed = u64;

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// 从（世界种子, 代理编号, 用途编号）派生子流种子。
pub fn derive_seed(world_seed: WorldSeed, agent: u32, purpose: u8) -> u64 {
    let mut s = world_seed ^ 0xA076_1D64_78BD_642F;
    for part in [agent as u64, purpose as u64] {
        s ^= part.wrapping_mul(0xE703_7ED1_A0B4_28DB);
        s = splitmix64(&mut s);
    }
    s
}

/// xoshiro256++ 生成器。
#[derive(Clone, Debug)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    pub fn seeded(seed: u64) -> Self {
        let mut sm = seed;
        let s = [
            splitmix64(&mut sm),
            splitmix64(&mut sm),
            splitmix64(&mut sm),
            splitmix64(&mut sm),
        ];
        Rng { s }
    }

    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[0]
            .wrapping_add(self.s[3])
            .rotate_left(23)
            .wrapping_add(self.s[0]);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// [0, 1) 均匀分布。
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// [lo, hi) 均匀分布。
    pub fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_f64()
    }

    /// 均匀取整数 [lo, hi]（闭区间）。
    pub fn uniform_int(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo + self.next_u64() % (hi - lo + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::seeded(42);
        let mut b = Rng::seeded(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_purpose_different_stream() {
        assert_ne!(
            derive_seed(7, 1, 0),
            derive_seed(7, 1, 1),
        );
    }

    #[test]
    fn uniform_stays_in_range() {
        let mut r = Rng::seeded(1);
        for _ in 0..1000 {
            let v = r.uniform(3.0, 5.0);
            assert!((3.0..5.0).contains(&v));
        }
    }
}
