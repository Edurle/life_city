//! 需求系统（决策分层的第一、二层）。
//! 六项需求与设计稿一致：饥饿、精力、娱乐、社交、卫生、如厕。
//! 取值 0–100，100 为完全满足；到达临界值时触发第一层生理反射（强制行为）。

use serde::Serialize;
use std::collections::BTreeMap;

pub const NEED_KEYS: [&str; 6] = ["hunger", "energy", "fun", "social", "hygiene", "bladder"];
pub const NEED_NAMES: [&str; 6] = ["饥饿", "精力", "娱乐", "社交", "卫生", "如厕"];

#[derive(Debug, Clone, Copy, Serialize, serde::Deserialize)]
pub struct Needs {
    pub hunger: f64,
    pub energy: f64,
    pub fun: f64,
    pub social: f64,
    pub hygiene: f64,
    pub bladder: f64,
}

impl Needs {
    pub fn full() -> Self {
        Needs {
            hunger: 100.0,
            energy: 100.0,
            fun: 85.0,
            social: 80.0,
            hygiene: 90.0,
            bladder: 90.0,
        }
    }

    pub fn get(&self, key: &str) -> Option<f64> {
        match key {
            "hunger" => Some(self.hunger),
            "energy" => Some(self.energy),
            "fun" => Some(self.fun),
            "social" => Some(self.social),
            "hygiene" => Some(self.hygiene),
            "bladder" => Some(self.bladder),
            _ => None,
        }
    }

    /// 变化量可正可负；数值限制在 0–100。
    pub fn add(&mut self, key: &str, delta: f64) {
        let v = self.get(key).unwrap_or(0.0) + delta;
        let v = v.clamp(0.0, 100.0);
        match key {
            "hunger" => self.hunger = v,
            "energy" => self.energy = v,
            "fun" => self.fun = v,
            "social" => self.social = v,
            "hygiene" => self.hygiene = v,
            "bladder" => self.bladder = v,
            _ => {}
        }
    }

    /// 每小时衰减率（正数表示下降）。衰减按拍结算，等价于指数下降的欧拉近似。
    pub fn decay(&mut self, hourly: &BTreeMap<String, f64>, ticks: TickCount) {
        let per_tick = ticks as f64 / 60.0;
        for key in NEED_KEYS {
            if let Some(rate) = hourly.get(key) {
                self.add(key, -rate * per_tick);
            }
        }
    }
}

pub type TickCount = u64;

/// 需求紧迫度（决策第二层 → 行为权重乘数）。
/// 饱和时 0.2，极度匮乏时 2.0，二次曲线，匮乏侧变化更陡。
pub fn urgency(need_value: f64) -> f64 {
    let v = (need_value.clamp(0.0, 100.0)) / 100.0;
    0.2 + 1.8 * (1.0 - v).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urgency_extremes() {
        assert!((urgency(100.0) - 0.2).abs() < 1e-9);
        assert!((urgency(0.0) - 2.0).abs() < 1e-9);
        assert!(urgency(50.0) > urgency(80.0));
    }

    #[test]
    fn add_clamps() {
        let mut n = Needs::full();
        n.add("hunger", 60.0);
        assert_eq!(n.hunger, 100.0);
        n.add("hunger", -260.0);
        assert_eq!(n.hunger, 0.0);
    }
}
