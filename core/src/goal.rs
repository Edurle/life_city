//! 限期目标（M4，设计稿 4.5）：玩家给自己定一个戒断目标（如"这三天不吃夜宵"），
//! 呈现"对抗习惯"的完整过程：
//!
//! - 设定后，戒断对象的习惯权重按加速速率向基准衰减（对抗使旧习惯消退），
//!   累计被额外压掉的量记在 `suppressed`；
//! - 行为树仍选中它时，玩家角色消耗意志力"忍住"（换选次优行为）；
//!   意志力不足则失守（执行 + 违约计数）；
//! - 玩家主动下令戒断行为：额外违约成本 + 违约计数（行为树从不禁止指令）；
//! - 到期零违约 → 成功（压制已转化为自然衰减，习惯真的改掉了）；
//!   中途放弃 → suppressed 一次性还回权重（旧习惯立刻反弹）。

use crate::time::Tick;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Goal {
    /// 戒断对象（行为 ID）。
    pub behavior: String,
    pub started: Tick,
    pub deadline: Tick,
    /// 失守次数：行为树选中但没忍住 + 玩家主动违约。
    pub violations: u32,
    /// 忍住次数（每次消耗意志力）。
    pub resists: u32,
    /// 对抗期间被额外压制掉的权重累计（放弃时回升——反弹幅度）。
    pub suppressed: f64,
}

impl Goal {
    pub fn new(behavior: &str, started: Tick, deadline: Tick) -> Self {
        Goal {
            behavior: behavior.to_string(),
            started,
            deadline,
            violations: 0,
            resists: 0,
            suppressed: 0.0,
        }
    }

    pub fn is_active(&self, tick: Tick) -> bool {
        tick < self.deadline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_window() {
        let g = Goal::new("late_snack", 100, 100 + 3 * 1440);
        assert!(g.is_active(100));
        assert!(g.is_active(100 + 3 * 1440 - 1));
        assert!(!g.is_active(100 + 3 * 1440), "到期即结束");
    }
}
