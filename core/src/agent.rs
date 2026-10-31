//! 代理：一只猫（M0）。持有自己的权重表（结构共享、数值分离）、
//! 需求、记忆表、独立随机数子流与当前动作。

use crate::memory::MemoryTable;
use crate::needs::Needs;
use crate::rng::{derive_seed, Rng, WorldSeed};
use crate::time::Tick;
use std::collections::BTreeMap;

/// 随机子流用途编号：决策流与个体属性流分离，改一个不影响另一个的序列。
pub const STREAM_DECISION: u8 = 0;
pub const STREAM_INDIVIDUAL: u8 = 1;

#[derive(Debug, Clone)]
pub struct Agent {
    pub id: u32,
    pub name: String,
    /// 玩家角色标记：只有玩家角色接受指令与消耗意志力。
    pub is_player: bool,
    /// softmax 温度（探索性人格）：高温度更爱尝试低权重行为。
    pub temperature: f64,
    pub needs: Needs,
    /// 意志力（0–100）：违背习惯的指令消耗，睡眠恢复，平时缓慢回一点。
    pub willpower: f64,
    /// 待执行的玩家指令（意志节点）：下一次决策时作为覆盖选择。
    pub pending_command: Option<String>,
    /// 每代理权重表：初始 = base_weight × 个体抖动；习惯执行 ×(1+α)，每小时向基准回落。
    pub weights: BTreeMap<String, f64>,
    /// 各行为的基准权重缓存（习惯衰减的目标）。
    pub base_weights: BTreeMap<String, f64>,
    pub memory: MemoryTable,
    /// 当前动作：(行为 ID, 开始拍, 结束拍)。
    pub current: Option<(String, Tick, Tick)>,
    /// 下次心跳拍（空闲时重新决策）。
    pub next_heartbeat: Tick,
    /// 行为执行次数（统计用）。
    pub exec_counts: BTreeMap<String, u64>,
    pub rng_decision: Rng,
}

impl Agent {
    pub fn new(
        world_seed: WorldSeed,
        id: u32,
        name: String,
        behaviors: &[(String, f64)],
        params: &crate::config::Params,
    ) -> Self {
        let mut rng_ind = Rng::seeded(derive_seed(world_seed, id, STREAM_INDIVIDUAL));
        let temperature = rng_ind.uniform(params.temperature_min, params.temperature_max);
        let mut weights = BTreeMap::new();
        let mut base_weights = BTreeMap::new();
        for (bid, base) in behaviors {
            let jitter = rng_ind.uniform(1.0 - params.weight_jitter, 1.0 + params.weight_jitter);
            weights.insert(bid.clone(), base * jitter);
            base_weights.insert(bid.clone(), *base);
        }
        Agent {
            id,
            name,
            is_player: false,
            temperature,
            needs: Needs::full(),
            willpower: 100.0,
            pending_command: None,
            weights,
            base_weights,
            memory: MemoryTable::new(params.memory_cap, params.memory_top_k),
            current: None,
            next_heartbeat: rng_ind.uniform_int(1, params.heartbeat_max),
            exec_counts: BTreeMap::new(),
            rng_decision: Rng::seeded(derive_seed(world_seed, id, STREAM_DECISION)),
        }
    }

    /// 习惯衰减（每小时调用一次）：权重向基准值一阶回落。
    pub fn habit_decay(&mut self, hourly_rate: f64) {
        let factor = 1.0 - (-hourly_rate).exp();
        for (bid, w) in self.weights.iter_mut() {
            if let Some(base) = self.base_weights.get(bid) {
                *w += (*base - *w) * factor;
            }
        }
    }

    /// 习惯强化（行为完成时调用）。
    pub fn habit_boost(&mut self, behavior: &str, alpha: f64) {
        if let Some(w) = self.weights.get_mut(behavior) {
            *w *= 1.0 + alpha;
        }
    }

    pub fn record_exec(&mut self, behavior: &str) {
        *self.exec_counts.entry(behavior.to_string()).or_insert(0) += 1;
    }

    /// 正在执行的行为是否带有指定标签（环境事件的免疫判定）。
    pub fn current_has_tag(&self, tag: &str, tags_of: impl Fn(&str) -> Vec<String>) -> bool {
        match &self.current {
            Some((bid, _, _)) => tags_of(bid).contains(&tag.to_string()),
            None => false,
        }
    }
}
