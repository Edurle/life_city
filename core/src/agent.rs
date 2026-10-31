//! 代理：一只猫（M0）。持有自己的权重表（结构共享、数值分离）、
//! 需求、记忆表、独立随机数子流与当前动作。

use crate::config::Params;
use crate::goal::Goal;
use crate::memory::MemoryTable;
use crate::needs::Needs;
use crate::rng::{derive_seed, Rng, WorldSeed};
use crate::space::Pos;
use crate::time::Tick;
use std::collections::BTreeMap;

/// 随机子流用途编号：决策流与个体属性流分离，改一个不影响另一个的序列。
pub const STREAM_DECISION: u8 = 0;
pub const STREAM_INDIVIDUAL: u8 = 1;
/// M5 人格轴（诚实度）专用流：新增抽样不扰动既有序列（旧存档/旧日志的
/// 决策轨迹不受影响）。
pub const STREAM_PERSONALITY: u8 = 3;

fn d_species_cat() -> String {
    "cat".to_string()
}

fn d_lod() -> u8 {
    0
}
fn d_half() -> f64 {
    0.5
}
fn d_wallet() -> f64 {
    300.0
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Agent {
    pub id: u32,
    pub name: String,
    /// 物种（M3）：决定用哪棵行为树（人类/猫同街生活）。
    #[serde(default = "d_species_cat")]
    pub species: String,
    /// 玩家角色标记：只有玩家角色接受指令与消耗意志力。
    pub is_player: bool,
    /// softmax 温度（探索性人格）：高温度更爱尝试低权重行为。
    pub temperature: f64,
    /// 诚实度（0..1，M5 人格轴）：出生随机。斜率进犯罪与职业权重——
    /// 越不诚实越倾向非法行当，但只是概率差异，无人被指定为小偷。
    #[serde(default = "d_half")]
    pub honesty: f64,
    /// 钱包（M5 经济）：工资入账、消费扣款；见底产生经济压力。
    #[serde(default = "d_wallet")]
    pub wallet: f64,
    /// 今日已领薪的天数标记（日薪制：加班到第二个班次不重复发钱）。
    #[serde(default)]
    pub salary_day: u64,
    /// 当前职业（M5）：None = 无业。职业门按此放行工作分支。
    #[serde(default)]
    pub job: Option<String>,
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
    /// LOD 层级（M4，技术文档 4.6）：0 = T0 完整，1 = T1 常规，2 = T2 背景。
    /// 世界状态的确定性函数（见 world::refresh_lod），不进随机数。
    #[serde(default = "d_lod")]
    pub lod: u8,
    /// 进行中的限期目标（M4，设计稿 4.5；通常只有玩家角色有）。
    #[serde(default)]
    pub goals: Vec<Goal>,
    /// 位置（米）：当前行为地点；行为开始时移动过去，空闲时留在原地。
    pub pos: Pos,
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
        species: String,
        behaviors: &[(String, f64)],
        params: &Params,
    ) -> Self {
        let mut rng_ind = Rng::seeded(derive_seed(world_seed, id, STREAM_INDIVIDUAL));
        let temperature = rng_ind.uniform(params.temperature_min, params.temperature_max);
        // 诚实度从独立子流抽取（不打扰既有属性序列）。
        let honesty =
            Rng::seeded(derive_seed(world_seed, id, STREAM_PERSONALITY)).next_f64();
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
            species,
            is_player: false,
            temperature,
            honesty,
            wallet: d_wallet(),
            salary_day: 0,
            job: None,
            needs: Needs::full(),
            willpower: 100.0,
            pending_command: None,
            weights,
            base_weights,
            memory: MemoryTable::new(params.memory_cap, params.memory_top_k),
            current: None,
            lod: 0,
            goals: Vec::new(),
            pos: (params.map_width / 2.0, params.map_height / 2.0),
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

    /// 习惯自动化（M4，设计稿 4.4）：权重达到基准 × 阈值的行为视为已养成——
    /// 行为树自然高频选择它；玩家指令它不再消耗意志力（习惯成自然）。
    pub fn is_automated(&self, behavior: &str, auto_ratio: f64) -> bool {
        match (
            self.weights.get(behavior),
            self.base_weights.get(behavior),
        ) {
            (Some(w), Some(base)) => *w >= *base * auto_ratio,
            _ => false,
        }
    }

    /// 当前拍活跃、且针对指定行为的戒断目标（决策与指令定价用）。
    pub fn active_goal_on(&self, behavior: &str, tick: Tick) -> Option<&Goal> {
        self.goals
            .iter()
            .find(|g| g.behavior == behavior && g.is_active(tick))
    }

    /// 活跃目标的下标（需要递增计数时用）。
    pub fn active_goal_idx(&self, behavior: &str, tick: Tick) -> Option<usize> {
        self.goals
            .iter()
            .position(|g| g.behavior == behavior && g.is_active(tick))
    }

    /// 目标压制（每小时调用一次）：戒断对象的习惯衰减加速 `mult` 倍，
    /// 相对自然衰减多压掉的部分记入 goal.suppressed（放弃时回升）。
    pub fn goal_hourly_suppress(&mut self, hourly_rate: f64, mult: f64) {
        let f = 1.0 - (-hourly_rate).exp();
        let fg = 1.0 - (-(hourly_rate * mult)).exp();
        let goals = &mut self.goals;
        let weights = &mut self.weights;
        let bases = &self.base_weights;
        for g in goals.iter_mut() {
            let (Some(w), Some(base)) = (weights.get_mut(&g.behavior), bases.get(&g.behavior))
            else {
                continue;
            };
            let normal = *w + (*base - *w) * f;
            let boosted = *w + (*base - *w) * fg;
            g.suppressed += normal - boosted;
            *w = boosted;
        }
    }

    /// 放弃目标：suppressed 一次性还回权重（旧习惯反弹，设计稿 4.5）。
    /// 返回还回了多少。
    pub fn goal_rebound(&mut self, gi: usize) -> f64 {
        let g = &self.goals[gi];
        let back = g.suppressed.max(0.0);
        if let Some(w) = self.weights.get_mut(&g.behavior) {
            *w += back;
        }
        back
    }

    /// 正在执行的行为是否带有指定标签（环境事件的免疫判定）。
    pub fn current_has_tag(&self, tag: &str, tags_of: impl Fn(&str) -> Vec<String>) -> bool {
        match &self.current {
            Some((bid, _, _)) => tags_of(bid).contains(&tag.to_string()),
            None => false,
        }
    }
}
