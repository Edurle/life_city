//! 数据驱动的模拟配置（assets/*.json）。
//! 引擎不含任何内容专属代码：行为、树、地点、环境事件全部在这里定义。

use crate::needs::NEED_KEYS;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
pub struct SimConfig {
    /// 每小时需求衰减率（键为需求名，值为每小时下降量）。
    pub needs: BTreeMap<String, f64>,
    /// 第一层生理反射的临界值：需求低于此值时强制执行恢复行为。
    pub critical_need: f64,
    pub params: Params,
    pub behaviors: Vec<BehaviorDef>,
    /// 默认行为树（单物种配置用；多物种配置用 trees 按物种选择）。
    pub tree: TreeNode,
    /// 物种 → 行为树（M3：人类与猫同街生活）。空 = 全部用默认树。
    #[serde(default)]
    pub trees: BTreeMap<String, TreeNode>,
    /// 命名子树（M4.5：Subtree 引用节点的解析表）。内容模块化——
    /// 职业、日常作息等通用结构定义一次、多树复用，不复制粘贴。
    #[serde(default)]
    pub subtrees: BTreeMap<String, TreeNode>,
    /// 职业（M5，设计稿 5.3）：全部由数据定义，引擎无职业专属代码。
    /// 面包师与小偷走同一格式，只是取值不同。
    #[serde(default)]
    pub jobs: Vec<JobDef>,
    pub spots: Vec<SpotDef>,
    /// 地点（M3）：区域定义——街道地图上的矩形，声誉与整洁度的载体。
    #[serde(default)]
    pub areas: Vec<AreaDef>,
    pub env_events: Vec<EnvEventDef>,
}

impl SimConfig {
    /// 物种对应的行为树：优先 trees[species]，否则默认树。
    pub fn tree_for(&self, species: &str) -> &TreeNode {
        self.trees.get(species).unwrap_or(&self.tree)
    }

    /// 物种用到的行为 ID 集合（树叶遍历，含子树展开；权重表只装这些）。
    pub fn species_behaviors(&self, species: &str) -> Vec<(String, f64)> {
        let mut ids: Vec<String> = Vec::new();
        collect_actions(self.tree_for(species), &self.subtrees, &mut ids);
        self.behaviors
            .iter()
            .filter(|b| ids.contains(&b.id))
            .map(|b| (b.id.clone(), b.base_weight))
            .collect()
    }
}

fn collect_actions(node: &TreeNode, subtrees: &BTreeMap<String, TreeNode>, out: &mut Vec<String>) {
    match node {
        TreeNode::Selector { children } => {
            for c in children {
                collect_actions(c, subtrees, out);
            }
        }
        TreeNode::Gate { child, .. }
        | TreeNode::TimeGate { child, .. }
        | TreeNode::JobGate { child, .. }
        | TreeNode::MateGate { child } => collect_actions(child, subtrees, out),
        TreeNode::Subtree { tree_ref } => {
            if let Some(sub) = subtrees.get(tree_ref) {
                collect_actions(sub, subtrees, out);
            }
        }
        TreeNode::Action { behavior } => {
            if !out.contains(behavior) {
                out.push(behavior.clone());
            }
        }
    }
}

/// 职业定义（M5，设计稿 5.3 的统一格式）。面包师与小偷只有取值差异：
/// 时间表写在行为树的 JobGate + 时间门里；技能字段暂占位不启用。
#[derive(Debug, Clone, Deserialize)]
pub struct JobDef {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// 对应的上班行为 ID（完成一次 = 一个班次，工资入账）。
    pub work_behavior: String,
    /// 每班次工资；非法职业为 0（收入来自 crime 规格的赃款）。
    #[serde(default)]
    pub salary: f64,
    /// 岗位数；0 = 无门槛不限量（非法职业）。
    #[serde(default)]
    pub slots: usize,
    /// 转行评估的基准权重。
    #[serde(default = "d_one")]
    pub switch_weight: f64,
    /// 诚实度斜率（-2..2）：正 = 诚实者更倾向此行；小偷为负。
    #[serde(default)]
    pub honesty_bias: f64,
    /// 入职条件（设计稿 5.3 门槛字段；技能门槛暂缓）：
    /// 诚实度须低于此值（非法行当的自筛）。
    #[serde(default)]
    pub require_honesty_below: Option<f64>,
    /// 经济压力须高于此值（走投无路才铤而走险）。
    #[serde(default)]
    pub require_pressure_above: Option<f64>,
}

impl JobDef {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}

/// 犯罪规格（M5，设计稿 5.2）：挂在行为上，引擎按通用规则结算——
/// 作案挑附近有钱者、转移钱包、按环境（照明/巡逻/人流）掷被抓。
#[derive(Debug, Clone, Deserialize)]
pub struct CrimeSpec {
    /// 单次作案上限（实际为受害者钱包的一定比例）。
    pub max_take: f64,
    /// 基础被抓概率（乘以环境的照明、巡逻、人流修正）。
    pub caught_base: f64,
    /// 被抓后的社区服务行为 ID（公开劳动，归还财物之外的正经代价）。
    pub service_behavior: String,
    /// 受害者写入的记忆事件名（数据驱动，如"被偷"）。
    #[serde(default = "d_victim_event")]
    pub victim_event: String,
    /// 受害者记忆的强度与效价（强烈负面）。
    #[serde(default = "d_victim_intensity")]
    pub victim_intensity: f64,
    #[serde(default = "d_victim_valence")]
    pub victim_valence: f64,
    /// 受害者记忆对行为标签的乘数（对夜间外出等行为的抑制）。
    #[serde(default)]
    pub victim_mults: BTreeMap<String, f64>,
}

/// 地点（区域）：街道地图上的矩形。承载声誉（访客情绪累计）与整洁度（客观状态）。
#[derive(Debug, Clone, Deserialize)]
pub struct AreaDef {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// 区域矩形（米）：左下角与宽高。事件位置采样与"行为发生在哪个地点"判定。
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// 初始声誉（0..1）：1 = 口碑很好。
    #[serde(default = "d_one")]
    pub reputation: f64,
    /// 初始整洁度（0..1）：1 = 干净。
    #[serde(default = "d_one")]
    pub cleanliness: f64,
    /// 脏乱系数：公共场所到访留脏（1.0），自家用场所磨损极小（如 0.1）。
    #[serde(default = "d_one")]
    pub litter_scale: f64,
    /// 照明（0..1，静态环境参数，M5）：越高越难作案——
    /// "环境永久影响概率"最直观的展示项之一。
    #[serde(default = "d_lighting")]
    pub lighting: f64,
}

impl AreaDef {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    pub fn contains(&self, pos: (f64, f64)) -> bool {
        pos.0 >= self.x && pos.0 <= self.x + self.w && pos.1 >= self.y && pos.1 <= self.y + self.h
    }
}

fn d_one() -> f64 {
    1.0
}

#[derive(Debug, Clone, Deserialize)]
pub struct Params {
    /// 习惯节点学习率 α：每执行一次权重 ×(1+α)。
    pub alpha: f64,
    /// 习惯每小时向基准值回落的衰减系数（一阶）。
    pub habit_decay_hourly: f64,
    /// 温度采样范围（温度 = 探索性人格，softmax 的温度参数）。
    pub temperature_min: f64,
    pub temperature_max: f64,
    /// 心跳间隔范围（拍）：代理空闲时重新决策的间隔，错开避免同拍拥挤。
    pub heartbeat_min: u64,
    pub heartbeat_max: u64,
    /// 单代理记忆条数上限（超出按强度淘汰最弱者）。
    pub memory_cap: usize,
    /// 记忆查询取相似度前 K 条。
    pub memory_top_k: usize,
    /// 初始权重的个体抖动范围（±比例）。
    pub weight_jitter: f64,
    /// 目睹半径（米）：同伴的显著反应在这个距离内可被看见（一间房内都能看见）。
    #[serde(default = "d_witness_radius")]
    pub witness_radius: f64,
    /// 舒适半径（米）：信任的同伴在这个距离内提供安全感。
    #[serde(default = "d_comfort_radius")]
    pub comfort_radius: f64,
    /// 安全感上限：亲密值 1 的同伴在身边时，负面记忆影响力的最大压制比例。
    #[serde(default = "d_comfort_max")]
    pub comfort_max: f64,
    /// 安全感生效的最低亲密值：低于此值的同伴不提供安全感
    /// （初始随机熟悉度不应让满屋猫互相安慰）。
    #[serde(default = "d_comfort_min_bond")]
    pub comfort_min_bond: f64,
    /// 模仿折扣：目睹学到的强度 = 事件强度 × 亲密值 × 该系数（观察不如亲历）。
    #[serde(default = "d_imitation_factor")]
    pub imitation_factor: f64,
    /// 模仿门槛：亲密值低于此值的同伴，其遭遇不值得学
    /// （初始随机熟悉度不应让全屋猫互通消息）。
    #[serde(default = "d_imitation_min_bond")]
    pub imitation_min_bond: f64,
    /// 惊吓打断的显著性门槛：只有足够新颖的强负面事件才打断当前行为
    /// （第一次吸尘器真吓人；习惯化之后只是"又来了"，个性轨迹得以延续）。
    #[serde(default = "d_startle_min_salience")]
    pub startle_min_salience: f64,
    /// 共同度过社交时光（同地点完成社交行为）的亲密值增长。
    #[serde(default = "d_bond_social_gain")]
    pub bond_social_gain: f64,
    /// 涉及玩家角色的社交时光增长更快（玩家的陪伴更用心）。
    #[serde(default = "d_bond_player_gain")]
    pub bond_player_gain: f64,
    /// 同屋代理的初始熟悉度上限（U(0, 此值)）：同屋檐下有点眼熟，但算不上亲近。
    #[serde(default = "d_initial_bond_max")]
    pub initial_bond_max: f64,
    /// 地图尺寸（米）：事件位置采样与坐标边界。
    #[serde(default = "d_room", alias = "room_width")]
    pub map_width: f64,
    #[serde(default = "d_room", alias = "room_height")]
    pub map_height: f64,
    /// 地点声誉：访客体验进声誉统计的学习率（每次体验的 EMA 权重）。
    #[serde(default = "d_area_exp_alpha")]
    pub area_exp_alpha: f64,
    /// 地点声誉对行为权重的反馈：声誉 0 时乘 area_mult_min，声誉 1 时再 + span。
    #[serde(default = "d_area_mult_min")]
    pub area_mult_min: f64,
    #[serde(default = "d_area_mult_span")]
    pub area_mult_span: f64,
    /// 每次到访留下的脏乱（整洁度下降量）。
    #[serde(default = "d_visit_litter")]
    pub visit_litter: f64,
    /// 整洁度每日恢复量（清洁服务）。
    #[serde(default = "d_clean_daily")]
    pub clean_daily: f64,
    // ---------- M4 LOD 分层（技术文档 4.6） ----------
    /// T0 半径（米）：玩家身边这个距离内的代理完整模拟。
    #[serde(default = "d_lod_t0_radius")]
    pub lod_t0_radius: f64,
    /// T1 半径（米）：T0 之外到这个距离的代理常规模拟。
    #[serde(default = "d_lod_t1_radius")]
    pub lod_t1_radius: f64,
    /// T0 数量上限（超出按距离就近保留，编号兜底）。
    #[serde(default = "d_lod_t0_cap")]
    pub lod_t0_cap: usize,
    /// T1 数量上限。
    #[serde(default = "d_lod_t1_cap")]
    pub lod_t1_cap: usize,
    /// T1 心跳间隔范围（拍）：比 T0 慢（常规居民不必每分钟都想事）。
    #[serde(default = "d_lod_t1_heartbeat_min")]
    pub lod_t1_heartbeat_min: u64,
    #[serde(default = "d_lod_t1_heartbeat_max")]
    pub lod_t1_heartbeat_max: u64,
    /// T1 目睹半径缩放：常规居民只留意更近处同伴的遭遇。
    #[serde(default = "d_lod_t1_witness_scale")]
    pub lod_t1_witness_scale: f64,
    /// T2 简化选择的候选数（只保留高权重行为）。
    #[serde(default = "d_lod_t2_top_k")]
    pub lod_t2_top_k: usize,
    /// 朋友亲密值门槛：达到即至少 T1（跨街的朋友仍值得模拟）。
    #[serde(default = "d_lod_friend_bond")]
    pub lod_friend_bond: f64,
    // ---------- M4 习惯自动化（设计稿 4.4） ----------
    /// 自动化阈值：权重达到基准 × 此值的行为视为已养成习惯
    /// （自动执行、玩家指令不消耗意志力）。
    #[serde(default = "d_habit_auto_ratio")]
    pub habit_auto_ratio: f64,
    // ---------- M4 负面记忆与暴露恢复（设计稿 4.6） ----------
    /// 恐惧标记门槛：行为受负面记忆抑制到记忆乘数低于此值时，
    /// 菜单显示警示、指令额外消耗意志力。
    #[serde(default = "d_fear_mem_mult")]
    pub fear_mem_mult: f64,
    /// 恐惧行为的额外指令成本。
    #[serde(default = "d_fear_command_cost")]
    pub fear_command_cost: f64,
    /// 暴露恢复：完成被负面记忆抑制的行为一次，相关记忆强度 ×此值
    ///（反复低强度尝试 → 恐惧消退，暴露疗法）；身边有亲近同伴时衰减更快。
    #[serde(default = "d_exposure_decay")]
    pub exposure_decay: f64,
    // ---------- M4 密友（设计稿 4.7） ----------
    /// 密友亲密值门槛：达到后可查看对方的需求与负面记忆。
    #[serde(default = "d_close_friend_bond")]
    pub close_friend_bond: f64,
    // ---------- M4 限期目标（设计稿 4.5） ----------
    /// 忍住成本：行为树选中戒断行为、玩家角色靠意志力压住一次的消耗。
    #[serde(default = "d_goal_resist_cost")]
    pub goal_resist_cost: f64,
    /// 违约成本：目标进行期间玩家主动下令戒断行为的额外消耗。
    #[serde(default = "d_goal_break_cost")]
    pub goal_break_cost: f64,
    /// 目标压制倍率：戒断对象的习惯衰减速率 ×此值（对抗使旧习惯加速消退）。
    #[serde(default = "d_goal_decay_mult")]
    pub goal_decay_mult: f64,
    // ---------- M5：经济 ----------
    /// 贫困线（M5）：钱包低于此值产生经济压力（线性，见底 = 1）。
    /// 压力进入带 pressure_bias 的行为权重与职业转行评估。
    #[serde(default = "d_poverty_line")]
    pub poverty_line: f64,
    /// 每日生活费（M5，房租水电的抽象）：日界统一扣减。0 = 无此机制。
    #[serde(default)]
    pub daily_upkeep: f64,
    // ---------- M5 二阶段：社交对象选择与恋爱 ----------
    /// 面对面社交的寻人范围（米）：目标越远权重越低（可及性项的分母），
    /// 但不硬性排除——想见的人走两条街也去（寻人移动）。
    #[serde(default = "d_social_reach_radius")]
    pub social_reach_radius: f64,
    /// 新鲜度半衰期（天）：刚聊过的人降权，隔几天恢复——防天天缠着同一人。
    #[serde(default = "d_freshness_halflife")]
    pub freshness_halflife_days: f64,
    /// 恋爱门槛：两人亲密值达到此值才可能成为恋人（概率评估，无剧本）。
    #[serde(default = "d_couple_bond")]
    pub couple_bond: f64,
    /// 分手线：恋人间亲密值跌破此值，日界评估分手。
    #[serde(default = "d_couple_break_bond")]
    pub couple_break_bond: f64,
    /// 亲密达标的社交互动中"成为恋人"的概率（每次互动掷一次）。
    #[serde(default = "d_couple_form_prob")]
    pub couple_form_prob: f64,
    /// 恋人共同度过社交时光的亲密增长倍率。
    #[serde(default = "d_couple_gain_mult")]
    pub couple_gain_mult: f64,
    // ---------- M5 二阶段：老龄与代际 ----------
    /// 童年时长（模拟日）：高可塑期，不工作、不由自己付账（家长抚养的抽象）。
    #[serde(default = "d_childhood_days")]
    pub childhood_days: u64,
    /// 老龄起点（模拟日，从出生算）：可塑性下降、怀旧倾向出现。
    #[serde(default = "d_old_age_days")]
    pub old_age_days: u64,
    /// 童年记忆可塑性（写入强度乘数，设计稿 3.1"童年期远高于成年期"）。
    #[serde(default = "d_child_plasticity")]
    pub child_plasticity: f64,
    /// 老年记忆可塑性（新记忆写入变慢）。
    #[serde(default = "d_elder_plasticity")]
    pub elder_plasticity: f64,
    /// 怀旧强度（0..1，老龄激活）：正面记忆消退更慢、负面消退更快。
    #[serde(default = "d_elder_nostalgia")]
    pub elder_nostalgia: f64,
    /// 恋人生育概率（每对每天，双方成年且亲密达标才掷）。
    #[serde(default = "d_birth_prob")]
    pub birth_prob: f64,
    /// 恋爱多少天后才可能生育（天）。
    #[serde(default = "d_birth_min_couple_days")]
    pub birth_min_couple_days: u64,
    /// 新生儿与父母的初始亲密值。
    #[serde(default = "d_parent_bond")]
    pub parent_bond: f64,
    /// 人口上限（0 = 不限）：多年代模拟的防爆护栏。
    #[serde(default)]
    pub max_population: usize,
    // ---------- M5 二阶段：流行传播（新奇需求 + 行为模仿） ----------
    /// 新奇需求增速：每重复一件熟悉的事积累一点"想换换花样"。
    #[serde(default = "d_novelty_gain")]
    pub novelty_gain: f64,
    /// 新奇乘数：权重 × (1 + 此值 × 新奇需求 × 行为新颖度)。新颖度 = 1/(1+执行次数)。
    #[serde(default = "d_novelty_mult")]
    pub novelty_mult: f64,
    /// 行为模仿强度：目睹亲近同伴完成某行为，自己该行为权重 × (1 + 此值 × 亲密)。
    #[serde(default = "d_imitation_habit")]
    pub imitation_habit: f64,
    /// 行为观察半径（米）：完成行为时，这个距离内的亲近同伴算目睹者。
    #[serde(default = "d_observe_radius")]
    pub observe_radius: f64,
    /// 习惯权重上限（相对基准的倍数，设计稿 3.4 权重归一化）：
    /// 防止高频行为的权重无限增长把 softmax 压成单选。
    #[serde(default = "d_habit_weight_cap")]
    pub habit_weight_cap: f64,
    // ---------- M6 二阶段：童年实时模仿与人口退出 ----------
    /// 童年实时模仿概率：孩子心跳决策时，父母正在做自己也做得了的事，
    /// 按此概率直接跟着做（跟在父母身边做同样的事）。0 = 关闭。
    #[serde(default = "d_child_imitate_prob")]
    pub child_imitate_prob: f64,
    /// 年迈离场起点（模拟日，0 = 关闭）：过了这个年龄每天按 depart_prob
    /// 掷离场——人口有出有进，多年代模拟的老镇不会只剩老人。
    #[serde(default)]
    pub depart_age_days: u64,
    /// 年迈离场的每日概率。
    #[serde(default = "d_depart_prob")]
    pub depart_prob: f64,
    /// 迁出门槛：无业成年人的经济压力须高于此值才考虑离开（>1 = 永不迁出）。
    #[serde(default = "d_emigrate_pressure_above")]
    pub emigrate_pressure_above: f64,
    /// 迁出的每日概率（满足门槛时掷）。
    #[serde(default = "d_emigrate_prob")]
    pub emigrate_prob: f64,
}

fn d_room() -> f64 {
    6.0
}
fn d_witness_radius() -> f64 {
    10.0
}
fn d_comfort_radius() -> f64 {
    2.0
}
fn d_comfort_max() -> f64 {
    0.8
}
fn d_comfort_min_bond() -> f64 {
    0.35
}
fn d_imitation_factor() -> f64 {
    0.7
}
fn d_imitation_min_bond() -> f64 {
    0.35
}
fn d_startle_min_salience() -> f64 {
    0.6
}
fn d_bond_social_gain() -> f64 {
    0.08
}
fn d_bond_player_gain() -> f64 {
    0.15
}
fn d_initial_bond_max() -> f64 {
    0.3
}
fn d_area_exp_alpha() -> f64 {
    0.15
}
fn d_area_mult_min() -> f64 {
    0.35
}
fn d_area_mult_span() -> f64 {
    1.1
}
fn d_visit_litter() -> f64 {
    0.004
}
fn d_clean_daily() -> f64 {
    0.35
}
fn d_lod_t0_radius() -> f64 {
    12.0
}
fn d_lod_t1_radius() -> f64 {
    25.0
}
fn d_lod_t0_cap() -> usize {
    128
}
fn d_lod_t1_cap() -> usize {
    512
}
fn d_lod_t1_heartbeat_min() -> u64 {
    5
}
fn d_lod_t1_heartbeat_max() -> u64 {
    30
}
fn d_lod_t1_witness_scale() -> f64 {
    0.5
}
fn d_lod_t2_top_k() -> usize {
    3
}
fn d_lod_friend_bond() -> f64 {
    0.5
}
fn d_habit_auto_ratio() -> f64 {
    1.6
}
fn d_fear_mem_mult() -> f64 {
    0.75
}
fn d_fear_command_cost() -> f64 {
    10.0
}
fn d_exposure_decay() -> f64 {
    0.88
}
fn d_close_friend_bond() -> f64 {
    0.75
}
fn d_goal_resist_cost() -> f64 {
    6.0
}
fn d_goal_break_cost() -> f64 {
    15.0
}
fn d_goal_decay_mult() -> f64 {
    4.0
}
fn d_poverty_line() -> f64 {
    300.0
}
fn d_social_reach_radius() -> f64 {
    30.0
}
fn d_freshness_halflife() -> f64 {
    2.0
}
fn d_couple_bond() -> f64 {
    0.85
}
fn d_couple_break_bond() -> f64 {
    0.45
}
fn d_couple_form_prob() -> f64 {
    0.08
}
fn d_couple_gain_mult() -> f64 {
    1.6
}
fn d_childhood_days() -> u64 {
    60
}
fn d_old_age_days() -> u64 {
    240
}
fn d_child_plasticity() -> f64 {
    1.8
}
fn d_elder_plasticity() -> f64 {
    0.45
}
fn d_elder_nostalgia() -> f64 {
    0.5
}
fn d_birth_prob() -> f64 {
    0.03
}
fn d_birth_min_couple_days() -> u64 {
    15
}
fn d_parent_bond() -> f64 {
    0.8
}
fn d_novelty_gain() -> f64 {
    0.05
}
fn d_novelty_mult() -> f64 {
    1.4
}
fn d_imitation_habit() -> f64 {
    0.15
}
fn d_observe_radius() -> f64 {
    6.0
}
fn d_habit_weight_cap() -> f64 {
    12.0
}
fn d_child_imitate_prob() -> f64 {
    0.5
}
fn d_depart_prob() -> f64 {
    0.01
}
fn d_emigrate_pressure_above() -> f64 {
    1.1
}
fn d_emigrate_prob() -> f64 {
    0.005
}
fn d_lighting() -> f64 {
    0.5
}
fn d_victim_event() -> String {
    "被偷".to_string()
}
fn d_victim_intensity() -> f64 {
    0.8
}
fn d_victim_valence() -> f64 {
    -0.8
}

/// 行为变体（M5 二阶段，头脑风暴方案 B）："吃饭"带变体列表（吃什么）。
/// 菜单挂在地点上（SpotDef.menu），价格与口味挂变体；softmax 与习惯
/// 在变体级再走一次——"总点同一家外卖"可养成、目标可精确到"不吃泡面"。
#[derive(Debug, Clone, Deserialize)]
pub struct VariantDef {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// 价格；None = 沿用所属行为的价格。
    #[serde(default)]
    pub cost: Option<f64>,
    /// 对需求影响的微调（叠加在所属行为效果上，如更解馋的菜多回娱乐）。
    #[serde(default)]
    pub effects: BTreeMap<String, f64>,
    /// 口味/特征标签（如 辣/甜）：日志与查看器呈现，目标与日记可引用。
    #[serde(default)]
    pub tags: Vec<String>,
}

impl VariantDef {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    /// 实际价格：变体未标价则沿用行为价。
    pub fn price(&self, behavior_cost: f64) -> f64 {
        self.cost.unwrap_or(behavior_cost)
    }
}

/// 行为注册表条目：树只引用行为 ID，引擎解释声明执行。
#[derive(Debug, Clone, Deserialize)]
pub struct BehaviorDef {
    pub id: String,
    /// 显示名（中文，菜单与日志用）；缺省时用 ID。
    #[serde(default)]
    pub name: Option<String>,
    pub base_weight: f64,
    /// 时长范围（拍，含两端）。
    pub duration_min: u64,
    pub duration_max: u64,
    /// 对需求的影响（键为需求名，正值恢复）。
    #[serde(default)]
    pub effects: BTreeMap<String, f64>,
    /// 行为标签：记忆乘数作用于标签而非行为 ID，保证泛化。
    #[serde(default)]
    pub tags: Vec<String>,
    /// 发生地点（spot 的 ID）；None 表示任意位置。
    pub spot: Option<String>,
    /// 需求门（第二层硬门槛）：需求低于 below 才可用。
    pub gate_need: Option<String>,
    pub gate_below: Option<f64>,
    // ---------- M5：经济与治安的通用旋钮 ----------
    /// 价格（M5 经济）：钱包付不起时该行为不可用（资源门槛）。
    #[serde(default)]
    pub cost: f64,
    /// 诚实度斜率（-2..2）：权重 × (1 + bias×(诚实度−0.5)×2)。犯罪的负斜率 =
    /// 越不诚实越想作案（人格轴进入行为权重，设计稿 5.2）。
    #[serde(default)]
    pub honesty_bias: f64,
    /// 经济压力斜率（≥0）：权重 × (1 + bias×压力)。压力 = 钱包见底程度——
    /// 穷则思变（设计稿 5.2 经济压力项的通用化）。
    #[serde(default)]
    pub pressure_bias: f64,
    /// 完成时让所在区域整洁度恢复的量（清洁/社区服务行为）。
    #[serde(default)]
    pub clean_effect: f64,
    /// 完成时让所在区域巡逻度上升的量（警察巡逻；巡逻度进被抓概率）。
    #[serde(default)]
    pub patrol_boost: f64,
    /// 犯罪规格（M5 治安）：完成时按通用规则结算作案与被抓。
    #[serde(default)]
    pub crime: Option<CrimeSpec>,
    /// 社交对象选择（M5 二阶段）：开始时在认识名单里按
    /// 亲密 × 新鲜度 × 可及性 × 人物记忆 选一个对象（和谁）。
    /// 带 remote_social 标签的不移动（电话）；否则寻人移动到对象身边。
    #[serde(default)]
    pub pick_target: bool,
    /// 变体列表（M5 二阶段：吃什么）。带变体的行为，开始时再选一次变体。
    #[serde(default)]
    pub variants: Vec<VariantDef>,
}

impl BehaviorDef {
    /// 该行为主要驱动的需求：正面影响最大的那项；无影响时默认"娱乐"（悠闲行为的默认驱动）。
    pub fn drive_need(&self) -> Option<&str> {
        let mut best: Option<(&String, &f64)> = None;
        for (k, v) in &self.effects {
            if *v > 0.0 && NEED_KEYS.contains(&k.as_str()) {
                match best {
                    Some((_, bv)) if *bv >= *v => {}
                    _ => best = Some((k, v)),
                }
            }
        }
        best.map(|(k, _)| k.as_str())
    }

    pub fn effect_on(&self, need: &str) -> f64 {
        self.effects.get(need).copied().unwrap_or(0.0)
    }

    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}

/// 树节点：结构承担"当前有哪些行为可用"（门与组合），
/// 数值层（权重、习惯、记忆乘数）在代理的权重表中，结构共享、数值分离。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TreeNode {
    Selector {
        children: Vec<TreeNode>,
    },
    Gate {
        need: String,
        below: f64,
        child: Box<TreeNode>,
    },
    /// 时间门（M3 昼夜）：只在 from..to 小时窗口内放行（含头不含尾；22→6 跨零点合法）。
    TimeGate {
        from: u32,
        to: u32,
        child: Box<TreeNode>,
    },
    /// 子树引用（M4.5）：按名查 cfg.subtrees，原地展开。
    /// 校验保证引用存在且无环。
    Subtree {
        #[serde(rename = "ref")]
        tree_ref: String,
    },
    /// 职业门（M5，设计稿 5.3）：只有当前职业为该值的代理放行子树。
    /// 工作行为按职业分支挂在人类树上，转行 = 换一门放行。
    JobGate {
        job: String,
        child: Box<TreeNode>,
    },
    /// 伴侣门（M5 二阶段）：只有有恋人的代理放行子树——
    /// 约会类行为只对恋爱中的人开放（状态机的树侧表达）。
    MateGate {
        child: Box<TreeNode>,
    },
    Action {
        behavior: String,
    },
}

/// 地点：地图上的位置，带特征标签供记忆匹配。M2 起有坐标（米），M3 起归属区域，
/// M5 二阶段起可挂菜单（这里能点到哪些变体——冰箱/货架/餐馆菜单 = 地点属性）。
#[derive(Debug, Clone, Deserialize)]
pub struct SpotDef {
    pub id: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    /// 所属区域（地点声誉的载体）；None = 不属于任何区域（如街道本身）。
    #[serde(default)]
    pub area: Option<String>,
    /// 菜单（变体 ID 列表）：非空时，在此地点执行的带变体行为只能从中选。
    /// 空 = 不限制（变体行为的全集都可用）。
    #[serde(default)]
    pub menu: Vec<String>,
}

/// 环境事件：按间隔随机发生，在发生位置的影响半径内写入目击代理的记忆。
#[derive(Debug, Clone, Deserialize)]
pub struct EnvEventDef {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub interval_min: u64,
    pub interval_max: u64,
    /// 情绪效价（-1..1）与强度（0..1），参与显著性计算。
    pub valence: f64,
    pub intensity: f64,
    /// 影响半径（米）；0 = 全图（如阳光这种全域事件）。
    #[serde(default)]
    pub radius: f64,
    /// 事件固定发生在该区域（位置在区域矩形内均匀采样）；None = 全图随机。
    #[serde(default)]
    pub area: Option<String>,
    /// 时间窗（小时，含头不含尾，可跨零点）：不设 = 全天可发生。
    #[serde(default)]
    pub from_hour: Option<u32>,
    #[serde(default)]
    pub to_hour: Option<u32>,
    /// 对行为标签的权重乘数（如 {"hide": 5}）。
    #[serde(default)]
    pub behavior_mults: BTreeMap<String, f64>,
    /// 正在执行带这些标签的行为的代理不受影响（如吸尘器吵不到躲着的猫）。
    #[serde(default)]
    pub immune_behavior_tags: Vec<String>,
}

impl EnvEventDef {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    /// 事件是否在时间窗内（无窗口 = 恒真）。
    pub fn in_window(&self, hour: u32) -> bool {
        match (self.from_hour, self.to_hour) {
            (Some(from), Some(to)) => crate::time::in_hour_window(hour, from, to),
            _ => true,
        }
    }
}

pub fn load_config(json: &str) -> Result<SimConfig, String> {
    let cfg: SimConfig = serde_json::from_str(json).map_err(|e| format!("配置解析失败: {e}"))?;
    validate(&cfg)?;
    Ok(cfg)
}

/// 内容静态校验（CI 同款逻辑）：悬空引用、需求名错误。
pub fn validate(cfg: &SimConfig) -> Result<(), String> {
    let behavior_ids: Vec<&str> = cfg.behaviors.iter().map(|b| b.id.as_str()).collect();
    let spots: Vec<&str> = cfg.spots.iter().map(|s| s.id.as_str()).collect();
    let areas: Vec<&str> = cfg.areas.iter().map(|a| a.id.as_str()).collect();
    for b in &cfg.behaviors {
        if b.base_weight <= 0.0 {
            return Err(format!("行为 {} 的 base_weight 必须为正", b.id));
        }
        if b.duration_min == 0 || b.duration_max < b.duration_min {
            return Err(format!("行为 {} 的时长范围非法", b.id));
        }
        for k in b.effects.keys() {
            if !NEED_KEYS.contains(&k.as_str()) {
                return Err(format!("行为 {} 引用了未知需求 {}", b.id, k));
            }
        }
        if let Some(s) = &b.spot {
            if !spots.contains(&s.as_str()) {
                return Err(format!("行为 {} 引用了未知地点 {}", b.id, s));
            }
        }
        // 变体（M5 二阶段）：效果键合法、价格非负、ID 全局唯一
        //（菜单按裸 ID 引用，代理的变体权重表也以 ID 为键）。
        for v in &b.variants {
            for k in v.effects.keys() {
                if !NEED_KEYS.contains(&k.as_str()) {
                    return Err(format!("变体 {} 引用了未知需求 {}", v.id, k));
                }
            }
            if let Some(c) = v.cost {
                if c < 0.0 {
                    return Err(format!("变体 {} 的价格不应为负", v.id));
                }
            }
        }
    }
    let mut variant_ids: Vec<&str> = Vec::new();
    for b in &cfg.behaviors {
        for v in &b.variants {
            if variant_ids.contains(&v.id.as_str()) {
                return Err(format!("变体 ID {} 在多个行为间重复", v.id));
            }
            variant_ids.push(v.id.as_str());
        }
    }
    // 菜单引用的变体必须存在。
    for s in &cfg.spots {
        for v in &s.menu {
            if !variant_ids.contains(&v.as_str()) {
                return Err(format!("地点 {} 的菜单引用了未定义的变体 {}", s.id, v));
            }
        }
    }
    // 职业（M5）：工作行为必须存在；岗位工资非负。
    let job_ids: Vec<&str> = cfg.jobs.iter().map(|j| j.id.as_str()).collect();
    let mut seen_work = std::collections::BTreeSet::new();
    for j in &cfg.jobs {
        if !behavior_ids.contains(&j.work_behavior.as_str()) {
            return Err(format!(
                "职业 {} 的工作行为 {} 未注册",
                j.id, j.work_behavior
            ));
        }
        if seen_work.contains(&j.work_behavior) {
            return Err(format!("行为 {} 被多个职业用作工作行为", j.work_behavior));
        }
        seen_work.insert(j.work_behavior.clone());
        if j.salary < 0.0 {
            return Err(format!("职业 {} 的工资不应为负", j.id));
        }
        if cfg.jobs.iter().filter(|x| x.id == j.id).count() > 1 {
            return Err(format!("职业 {} 重复定义", j.id));
        }
    }
    // 犯罪规格：社区服务行为必须存在。
    for b in &cfg.behaviors {
        if let Some(c) = &b.crime {
            if !behavior_ids.contains(&c.service_behavior.as_str()) {
                return Err(format!(
                    "行为 {} 的犯罪规格引用了未注册的社区服务行为 {}",
                    b.id, c.service_behavior
                ));
            }
            if !(0.0..=1.0).contains(&c.caught_base) {
                return Err(format!("行为 {} 的基础被抓概率应在 0..1", b.id));
            }
        }
        if b.cost < 0.0 {
            return Err(format!("行为 {} 的价格不应为负", b.id));
        }
    }
    validate_tree(
        &cfg.tree,
        &behavior_ids,
        &job_ids,
        &cfg.subtrees,
        &mut Vec::new(),
    )?;
    for (species, tree) in &cfg.trees {
        validate_tree(
            tree,
            &behavior_ids,
            &job_ids,
            &cfg.subtrees,
            &mut Vec::new(),
        )
        .map_err(|e| format!("物种 {species} 的树：{e}"))?;
    }
    for (name, tree) in &cfg.subtrees {
        validate_tree(
            tree,
            &behavior_ids,
            &job_ids,
            &cfg.subtrees,
            &mut Vec::new(),
        )
        .map_err(|e| format!("子树 {name}：{e}"))?;
    }
    for s in &cfg.spots {
        if let Some(a) = &s.area {
            if !areas.contains(&a.as_str()) {
                return Err(format!("地点 {} 引用了未知区域 {}", s.id, a));
            }
        }
    }
    for a in &cfg.areas {
        if a.reputation < 0.0 || a.reputation > 1.0 {
            return Err(format!("区域 {} 初始声誉应在 0..1", a.id));
        }
        if a.w <= 0.0 || a.h <= 0.0 {
            return Err(format!("区域 {} 的宽高应为正", a.id));
        }
    }
    for e in &cfg.env_events {
        if e.intensity < 0.0 || e.intensity > 1.0 {
            return Err(format!("事件 {} 强度应在 0..1", e.id));
        }
        if e.interval_min == 0 {
            return Err(format!("事件 {} 间隔下限应大于 0", e.id));
        }
        if e.radius < 0.0 {
            return Err(format!("事件 {} 半径不应为负", e.id));
        }
        if let Some(a) = &e.area {
            if !areas.contains(&a.as_str()) {
                return Err(format!("事件 {} 引用了未知区域 {}", e.id, a));
            }
        }
        for h in [e.from_hour, e.to_hour].into_iter().flatten() {
            if h > 23 {
                return Err(format!("事件 {} 的小时窗口应在 0..24", e.id));
            }
        }
    }
    let p = &cfg.params;
    if p.lod_t0_radius < 0.0 || p.lod_t1_radius < p.lod_t0_radius {
        return Err("LOD 半径应满足 0 ≤ t0 ≤ t1".into());
    }
    if p.lod_t0_cap == 0 || p.lod_t1_cap < p.lod_t0_cap {
        return Err("LOD 数量上限应满足 0 < t0cap ≤ t1cap".into());
    }
    if p.lod_t1_heartbeat_min == 0 || p.lod_t1_heartbeat_max < p.lod_t1_heartbeat_min {
        return Err("T1 心跳范围非法".into());
    }
    if p.lod_t2_top_k == 0 {
        return Err("T2 候选数应为正".into());
    }
    if p.habit_auto_ratio <= 1.0 {
        return Err("习惯自动化阈值应大于 1（权重超过基准才叫习惯）".into());
    }
    if !(0.0..=1.0).contains(&p.exposure_decay) || p.exposure_decay >= 1.0 {
        return Err("暴露恢复系数应在 0..1（每次尝试衰减一点恐惧）".into());
    }
    if p.goal_decay_mult < 1.0 {
        return Err("目标压制倍率应 ≥ 1".into());
    }
    // M6 二阶段：童年模仿与人口退出的概率参数。
    if !(0.0..=1.0).contains(&p.child_imitate_prob) {
        return Err("童年实时模仿概率应在 0..1".into());
    }
    for (name, v) in [
        ("年迈离场概率", p.depart_prob),
        ("迁出概率", p.emigrate_prob),
    ] {
        if !(0.0..=1.0).contains(&v) {
            return Err(format!("{name}应在 0..1"));
        }
    }
    if p.emigrate_pressure_above < 0.0 {
        return Err("迁出压力门槛不应为负".into());
    }
    Ok(())
}

/// 树校验：行为引用合法；子树引用存在且无环（正在展开路径上的再次引用即环）。
fn validate_tree(
    node: &TreeNode,
    ids: &[&str],
    job_ids: &[&str],
    subtrees: &BTreeMap<String, TreeNode>,
    visiting: &mut Vec<String>,
) -> Result<(), String> {
    match node {
        TreeNode::Selector { children } => {
            for c in children {
                validate_tree(c, ids, job_ids, subtrees, visiting)?;
            }
            Ok(())
        }
        TreeNode::Gate { child, .. } | TreeNode::TimeGate { child, .. } => {
            validate_tree(child, ids, job_ids, subtrees, visiting)
        }
        TreeNode::JobGate { job, child } => {
            if !job_ids.iter().any(|j| j == job) {
                return Err(format!("职业门引用了未定义的职业 {job}"));
            }
            validate_tree(child, ids, job_ids, subtrees, visiting)
        }
        TreeNode::MateGate { child } => validate_tree(child, ids, job_ids, subtrees, visiting),
        TreeNode::Subtree { tree_ref } => {
            if !subtrees.contains_key(tree_ref) {
                return Err(format!("引用了未定义的子树 {tree_ref}"));
            }
            if visiting.iter().any(|r| r == tree_ref) {
                return Err(format!("子树 {tree_ref} 出现循环引用"));
            }
            visiting.push(tree_ref.clone());
            let r = validate_tree(&subtrees[tree_ref], ids, job_ids, subtrees, visiting);
            visiting.pop();
            r
        }
        TreeNode::Action { behavior } => {
            if ids.contains(&behavior.as_str()) {
                Ok(())
            } else {
                Err(format!("树引用了未注册的行为 {behavior}"))
            }
        }
    }
}
