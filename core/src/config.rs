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

    /// 物种用到的行为 ID 集合（树叶遍历；权重表只装这些）。
    pub fn species_behaviors(&self, species: &str) -> Vec<(String, f64)> {
        let mut ids: Vec<String> = Vec::new();
        collect_actions(self.tree_for(species), &mut ids);
        self.behaviors
            .iter()
            .filter(|b| ids.contains(&b.id))
            .map(|b| (b.id.clone(), b.base_weight))
            .collect()
    }
}

fn collect_actions(node: &TreeNode, out: &mut Vec<String>) {
    match node {
        TreeNode::Selector { children } => {
            for c in children {
                collect_actions(c, out);
            }
        }
        TreeNode::Gate { child, .. } | TreeNode::TimeGate { child, .. } => {
            collect_actions(child, out)
        }
        TreeNode::Action { behavior } => {
            if !out.contains(behavior) {
                out.push(behavior.clone());
            }
        }
    }
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
    0.25
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
    Action {
        behavior: String,
    },
}

/// 地点：地图上的位置，带特征标签供记忆匹配。M2 起有坐标（米），M3 起归属区域。
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
    }
    validate_tree(&cfg.tree, &behavior_ids)?;
    for (species, tree) in &cfg.trees {
        validate_tree(tree, &behavior_ids).map_err(|e| format!("物种 {species} 的树：{e}"))?;
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
    Ok(())
}

fn validate_tree(node: &TreeNode, ids: &[&str]) -> Result<(), String> {
    match node {
        TreeNode::Selector { children } => {
            for c in children {
                validate_tree(c, ids)?;
            }
            Ok(())
        }
        TreeNode::Gate { child, .. } | TreeNode::TimeGate { child, .. } => {
            validate_tree(child, ids)
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
