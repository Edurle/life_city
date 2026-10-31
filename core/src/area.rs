//! 地点状态（M3）：声誉 = 到访者情绪的累计统计；整洁度 = 客观状态。
//! 两者共同构成后来者对该地点行为的权重修正（设计稿 5.1）。
//! 衰退和恢复由同一套统计机制产生：正体验拉高声誉，负体验压低，
//! 声誉低 → 到访行为权重低 → 只剩性格外向者仍会去 → 良好体验逐步拉回。

use crate::config::AreaDef;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AreaState {
    pub reputation: f64,
    pub cleanliness: f64,
    pub visits: u64,
    /// 巡逻覆盖（0..1，M5 治安）：警察巡逻行为累积，缓慢消退。
    /// 直接进入犯罪行为的被抓概率——"环境永久影响概率"的展示项。
    #[serde(default)]
    pub patrol: f64,
}

impl AreaState {
    pub fn from_def(def: &AreaDef) -> Self {
        AreaState {
            reputation: def.reputation.clamp(0.0, 1.0),
            cleanliness: def.cleanliness.clamp(0.0, 1.0),
            visits: 0,
            patrol: 0.0,
        }
    }

    /// 一次体验进声誉（EMA）：体验 exp ∈ −1..1 映射到目标值 (exp+1)/2。
    /// 返回新的声誉值。事件的 alpha 通常放大（一次吓人事件顶多次平淡到访）。
    pub fn experience(&mut self, exp: f64, alpha: f64) -> f64 {
        let target = ((exp + 1.0) / 2.0).clamp(0.0, 1.0);
        self.reputation += (target - self.reputation) * alpha.clamp(0.0, 1.0);
        self.reputation
    }

    /// 到访留下脏乱（整洁度下降）。
    pub fn litter(&mut self, amount: f64) {
        self.cleanliness = (self.cleanliness - amount).max(0.0);
        self.visits += 1;
    }

    /// 每日清洁恢复。
    pub fn daily_clean(&mut self, amount: f64) {
        self.cleanliness = (self.cleanliness + amount).min(1.0);
    }

    /// 巡逻覆盖累积（警察巡逻行为完成时调用）。
    pub fn patrol_raise(&mut self, amount: f64) {
        self.patrol = (self.patrol + amount).min(1.0);
    }

    /// 声誉与整洁度对行为权重的乘数：声誉 0 → area_mult_min，声誉 1 → min+span；
    /// 整洁度轻微修正（脏乱的公园即使口碑好也略减分）。
    pub fn behavior_mult(&self, area_mult_min: f64, area_mult_span: f64) -> f64 {
        (area_mult_min + area_mult_span * self.reputation) * (0.85 + 0.15 * self.cleanliness)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AreaState {
        AreaState {
            reputation: 1.0,
            cleanliness: 1.0,
            visits: 0,
            patrol: 0.0,
        }
    }

    /// 衰退与恢复是同一机制：负体验压低、正体验拉回。
    #[test]
    fn decline_and_recover_same_mechanism() {
        let mut a = state();
        // 一串负面体验：声誉从 1 跌下去。
        for _ in 0..5 {
            a.experience(-0.8, 0.15);
        }
        let low = a.reputation;
        assert!(low < 0.6, "连续负面体验应显著压低声誉：{low}");
        // 负面停止，正面体验逐步拉回。
        for _ in 0..15 {
            a.experience(0.3, 0.15);
        }
        assert!(
            a.reputation > low + 0.1,
            "正面体验应恢复声誉：{low} -> {}",
            a.reputation
        );
    }

    #[test]
    fn behavior_mult_monotonic_in_reputation() {
        let good = state();
        let mut bad = state();
        bad.reputation = 0.1;
        bad.cleanliness = 0.3;
        assert!(bad.behavior_mult(0.5, 0.8) < good.behavior_mult(0.5, 0.8));
        assert!(bad.behavior_mult(0.5, 0.8) < 1.0, "差地点应压制行为权重");
        assert!(good.behavior_mult(0.5, 0.8) > 1.0, "好地点应放大行为权重");
    }
}
