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
    // ---------- M5 二阶段 ----------
    /// 出生拍（负值 = 建镇前；建镇元老出生在"开镇前一个童年"，开镇即成年，
    /// 约半年后步入老年——第一批居民也会老）。年龄 =（当前拍 − 出生拍）
    /// 换算的天数，决定可塑性与怀旧（童年高可塑、老年低可塑+怀旧）。
    #[serde(default)]
    pub birth_tick: i64,
    /// 新奇需求（0..1，设计稿 3.4）：重复熟悉的事会积累，推动尝试低权重行为；
    /// 做了新鲜事就满足。流行起落的个体侧引擎。
    #[serde(default)]
    pub novelty: f64,
    /// 本次社交的对象（决策时选定，完成时结算——"和谁"从空间巧合变成选择）。
    #[serde(default)]
    pub social_target: Option<u32>,
    /// 最近一次与各对象的社交拍（新鲜度：刚聊过的人降权）。
    #[serde(default)]
    pub last_social: BTreeMap<u32, Tick>,
    /// 变体权重表（M5 二阶段吃什么）：键为变体 ID，基准 1.0——
    /// softmax 与习惯在变体级再走一次（"总点同一家外卖"可养成）。
    #[serde(default)]
    pub variant_weights: BTreeMap<String, f64>,
    /// 变体执行次数（统计与日记用）。
    #[serde(default)]
    pub variant_execs: BTreeMap<String, u64>,
    /// 当前动作选中的变体（与 current 同生命周期）。
    #[serde(default)]
    pub current_variant: Option<String>,
    // ---------- M6 二阶段 ----------
    /// 父母（出生时记录，建镇元老为 None）：童年实时模仿的线索来源，
    /// 也是"这是谁家的孩子"的查询入口。编号即下标，离场不清除。
    /// 注意：不能用 skip_serializing_if——bincode 是位置格式，跳过字段会错位。
    #[serde(default)]
    pub parents: Option<(u32, u32)>,
    /// 离场拍（M6 人口退出）：离场后不再参与任何模拟，但记忆与关系保留——
    /// 活人对离去者的记忆仍在（怀念的素材），id=下标的不变量也不破坏。
    #[serde(default)]
    pub departed_tick: Option<Tick>,
    /// 本次决策的模仿来源（M6 童年实时模仿）：行为开始时据此移动到父母身边。
    /// 瞬时状态，决策时清空重设。
    #[serde(default)]
    pub imitate_from: Option<u32>,
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
        Self::create(world_seed, id, name, species, behaviors, params, None, 0)
    }

    /// 出生（M5 二阶段代际）：初始权重 = 父母当前权重的均值 × 个体抖动——
    /// 上一代养成的习惯部分成为下一代的初始倾向（设计稿 5.6，模仿父母）。
    /// 变体口味（"总点的那家外卖"）同样继承。温度与诚实度从自己的子流抽取
    ///（不随父母——人格是自己的，习惯是学来的）。
    #[allow(clippy::too_many_arguments)]
    pub fn born(
        world_seed: WorldSeed,
        id: u32,
        name: String,
        species: String,
        behaviors: &[(String, f64)],
        params: &Params,
        parents: (&Agent, &Agent),
        birth_tick: Tick,
    ) -> Self {
        Self::create(
            world_seed,
            id,
            name,
            species,
            behaviors,
            params,
            Some(parents),
            birth_tick as i64,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn create(
        world_seed: WorldSeed,
        id: u32,
        name: String,
        species: String,
        behaviors: &[(String, f64)],
        params: &Params,
        parents: Option<(&Agent, &Agent)>,
        birth_tick: i64,
    ) -> Self {
        let parent_ids = parents.map(|(pa, pb)| (pa.id, pb.id));
        let mut rng_ind = Rng::seeded(derive_seed(world_seed, id, STREAM_INDIVIDUAL));
        let temperature = rng_ind.uniform(params.temperature_min, params.temperature_max);
        // 建镇元老出生在"开镇前一个童年"：开镇即成年（parents 为 None 时
        // birth 参数只是占位）。这样首批居民约 old_age−childhood 天后自然
        // 步入老年，第一年内就能看到怀旧倾向。
        let birth_tick = if parents.is_some() {
            birth_tick
        } else {
            -((params.childhood_days as i64) * 1440)
        };
        // 诚实度从独立子流抽取（不打扰既有属性序列）。
        let honesty = Rng::seeded(derive_seed(world_seed, id, STREAM_PERSONALITY)).next_f64();
        let mut weights = BTreeMap::new();
        let mut base_weights = BTreeMap::new();
        for (bid, base) in behaviors {
            let jitter = rng_ind.uniform(1.0 - params.weight_jitter, 1.0 + params.weight_jitter);
            let w = match parents {
                // 继承：父母当前权重（含习惯）的均值——父母常年下厨，孩子也爱下厨。
                Some((pa, pb)) => {
                    let avg = (pa.weights.get(bid).copied().unwrap_or(*base)
                        + pb.weights.get(bid).copied().unwrap_or(*base))
                        / 2.0;
                    avg.max(*base * 0.15)
                }
                None => *base,
            };
            weights.insert(bid.clone(), w * jitter);
            base_weights.insert(bid.clone(), *base);
        }
        // 变体权重（吃什么口味）：同样继承父母均值，基准 1.0。
        let mut variant_weights = BTreeMap::new();
        if let Some((pa, pb)) = parents {
            let keys: Vec<String> = pa
                .variant_weights
                .keys()
                .chain(pb.variant_weights.keys())
                .cloned()
                .collect();
            for k in keys {
                let avg = (pa.variant_weights.get(&k).copied().unwrap_or(1.0)
                    + pb.variant_weights.get(&k).copied().unwrap_or(1.0))
                    / 2.0;
                variant_weights.insert(k, avg.max(0.15));
            }
        }
        Agent {
            id,
            name,
            species,
            is_player: false,
            temperature,
            honesty,
            wallet: if parents.is_some() { 0.0 } else { d_wallet() },
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
            birth_tick,
            novelty: 0.0,
            social_target: None,
            last_social: BTreeMap::new(),
            variant_weights,
            variant_execs: BTreeMap::new(),
            current_variant: None,
            parents: parent_ids,
            departed_tick: None,
            imitate_from: None,
            rng_decision: Rng::seeded(derive_seed(world_seed, id, STREAM_DECISION)),
        }
    }

    /// 年龄（模拟日）。
    pub fn age_days(&self, tick: Tick) -> f64 {
        ((tick as i64 - self.birth_tick).max(0)) as f64 / 1440.0
    }

    /// 童年（高可塑期，不工作、家长付账）。
    pub fn is_child(&self, params: &Params, tick: Tick) -> bool {
        self.age_days(tick) < params.childhood_days as f64
    }

    /// 老年（可塑性下降 + 怀旧，设计稿 4.8）。
    pub fn is_elder(&self, params: &Params, tick: Tick) -> bool {
        self.age_days(tick) >= params.old_age_days as f64
    }

    /// 是否仍在镇上（M6 人口退出：离场者保留在数组里但不再活动）。
    pub fn present(&self) -> bool {
        self.departed_tick.is_none()
    }

    /// 记忆可塑性：童年 ×child_plasticity，成年 1.0，老年 ×elder_plasticity。
    pub fn plasticity(&self, params: &Params, tick: Tick) -> f64 {
        if self.is_child(params, tick) {
            params.child_plasticity
        } else if self.is_elder(params, tick) {
            params.elder_plasticity
        } else {
            1.0
        }
    }

    /// 怀旧强度：老年激活（正面记忆权重相对上升的来源之一，另一处在消退）。
    pub fn nostalgia(&self, params: &Params, tick: Tick) -> f64 {
        if self.is_elder(params, tick) {
            params.elder_nostalgia
        } else {
            0.0
        }
    }

    /// 新奇需求对某行为的乘数（设计稿 3.4："推动角色尝试低权重行为"）：
    /// 新颖度 = 基准权重 / 当前权重——权重被习惯推高的"老一套"不吃加成，
    /// 久坐冷板凳的低权重行为被托起。习惯越重、越想换花样，托力越强。
    pub fn novelty_mult(&self, behavior: &str, params: &Params) -> f64 {
        let w = self.weights.get(behavior).copied().unwrap_or(1.0);
        let base = self
            .base_weights
            .get(behavior)
            .copied()
            .unwrap_or(w)
            .max(1e-9);
        let novelty_of = (base / w.max(1e-9)).clamp(0.0, 1.2);
        1.0 + params.novelty_mult * self.novelty * novelty_of
    }

    /// 行为完成后的新奇需求结算：选了低权重的新鲜事 → 满足（回落）；
    /// 又重复权重高高在上的老一套 → 更想换花样（与乘数同一新颖度定义）。
    pub fn novelty_settle(&mut self, behavior: &str, params: &Params) {
        let w = self.weights.get(behavior).copied().unwrap_or(1.0);
        let base = self
            .base_weights
            .get(behavior)
            .copied()
            .unwrap_or(w)
            .max(1e-9);
        let novelty_of = (base / w.max(1e-9)).clamp(0.0, 1.2);
        if novelty_of > 0.8 {
            self.novelty = (self.novelty - 0.25).max(0.0);
        } else {
            self.novelty = (self.novelty + params.novelty_gain).min(1.0);
        }
    }

    /// 与某对象社交的新鲜度乘数（0.35..1）：刚聊过降权，半衰期可配。
    pub fn freshness(&self, other: u32, tick: Tick, halflife_days: f64) -> f64 {
        let Some(last) = self.last_social.get(&other) else {
            return 1.0;
        };
        let days = (tick.saturating_sub(*last)) as f64 / 1440.0;
        let hl = halflife_days.max(0.1);
        0.35 + 0.65 * (-days / hl).exp()
    }

    /// 习惯衰减（每小时调用一次）：权重向基准值一阶回落。
    /// 变体权重同规向 1.0 回落（口味会腻，但慢——同基准一阶）。
    pub fn habit_decay(&mut self, hourly_rate: f64) {
        let factor = 1.0 - (-hourly_rate).exp();
        for (bid, w) in self.weights.iter_mut() {
            if let Some(base) = self.base_weights.get(bid) {
                *w += (*base - *w) * factor;
            }
        }
        for w in self.variant_weights.values_mut() {
            *w += (1.0 - *w) * factor;
        }
    }

    /// 习惯强化（行为完成时调用）。权重上限 = 基准 × habit_weight_cap
    ///（设计稿 3.4 权重归一化：防止单个权重无限增长，softmax 失去区分度）。
    pub fn habit_boost(&mut self, behavior: &str, alpha: f64, cap_ratio: f64) {
        if let Some(w) = self.weights.get_mut(behavior) {
            let cap = self
                .base_weights
                .get(behavior)
                .copied()
                .unwrap_or(*w)
                .max(1e-9)
                * cap_ratio.max(1.0);
            *w = (*w * (1.0 + alpha)).min(cap);
        }
    }

    /// 变体习惯强化（选中的那个变体，M5 二阶段）。基准 1.0，同规设上限。
    pub fn variant_boost(&mut self, variant: &str, alpha: f64, cap_ratio: f64) {
        let w = self
            .variant_weights
            .entry(variant.to_string())
            .or_insert(1.0);
        *w = (*w * (1.0 + alpha)).min(cap_ratio.max(1.0));
    }

    pub fn record_exec(&mut self, behavior: &str) {
        *self.exec_counts.entry(behavior.to_string()).or_insert(0) += 1;
    }

    pub fn record_variant_exec(&mut self, variant: &str) {
        *self.variant_execs.entry(variant.to_string()).or_insert(0) += 1;
    }

    /// 习惯自动化（M4，设计稿 4.4）：权重达到基准 × 阈值的行为视为已养成——
    /// 行为树自然高频选择它；玩家指令它不再消耗意志力（习惯成自然）。
    pub fn is_automated(&self, behavior: &str, auto_ratio: f64) -> bool {
        match (self.weights.get(behavior), self.base_weights.get(behavior)) {
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

    /// 活跃目标的下标，含变体维度（M5 二阶段：目标可精确到"不吃泡面"）。
    /// 目标键 "行为" 匹配该行为的任意变体；"行为#变体" 只匹配那个变体。
    pub fn active_goal_idx_var(
        &self,
        behavior: &str,
        variant: Option<&str>,
        tick: Tick,
    ) -> Option<usize> {
        self.goals
            .iter()
            .position(|g| g.is_active(tick) && goal_key_matches(&g.behavior, behavior, variant))
    }

    /// 目标压制（每小时调用一次）：戒断对象的习惯衰减加速 `mult` 倍，
    /// 相对自然衰减多压掉的部分记入 goal.suppressed（放弃时回升）。
    /// 变体级目标压制变体权重表（对"不吃泡面"压的是泡面的口味）。
    pub fn goal_hourly_suppress(&mut self, hourly_rate: f64, mult: f64) {
        let f = 1.0 - (-hourly_rate).exp();
        let fg = 1.0 - (-(hourly_rate * mult)).exp();
        let goals = &mut self.goals;
        let weights = &mut self.weights;
        let vweights = &mut self.variant_weights;
        let bases = &self.base_weights;
        for g in goals.iter_mut() {
            if let Some((bid, variant)) = g.behavior.split_once('#') {
                let _ = bid;
                let Some(w) = vweights.get_mut(variant) else {
                    continue;
                };
                let normal = *w + (1.0 - *w) * f;
                let boosted = *w + (1.0 - *w) * fg;
                g.suppressed += normal - boosted;
                *w = boosted;
            } else {
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
    }

    /// 放弃目标：suppressed 一次性还回权重（旧习惯反弹，设计稿 4.5）。
    /// 返回还回了多少。
    pub fn goal_rebound(&mut self, gi: usize) -> f64 {
        let g = &self.goals[gi];
        let back = g.suppressed.max(0.0);
        if let Some((_, variant)) = g.behavior.split_once('#') {
            if let Some(w) = self.variant_weights.get_mut(variant) {
                *w += back;
            }
        } else if let Some(w) = self.weights.get_mut(&g.behavior) {
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

/// 目标键匹配（M5 二阶段）：键 "行为" 匹配任意变体；"行为#变体" 精确匹配。
/// "不吃泡面"（行为#变体）不拦"在家吃"（同行为其他变体）。
pub fn goal_key_matches(goal_key: &str, behavior: &str, variant: Option<&str>) -> bool {
    match goal_key.split_once('#') {
        Some((bid, v)) => bid == behavior && variant == Some(v),
        None => goal_key == behavior,
    }
}
