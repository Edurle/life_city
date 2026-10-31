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
    pub tree: TreeNode,
    pub spots: Vec<SpotDef>,
    pub env_events: Vec<EnvEventDef>,
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
    Selector { children: Vec<TreeNode> },
    Gate {
        need: String,
        below: f64,
        child: Box<TreeNode>,
    },
    Action { behavior: String },
}

/// 地点（M0 为房间内的几个位置，带特征标签供记忆匹配）。
#[derive(Debug, Clone, Deserialize)]
pub struct SpotDef {
    pub id: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// 环境事件：按间隔随机发生，写入目击代理的记忆。
#[derive(Debug, Clone, Deserialize)]
pub struct EnvEventDef {
    pub id: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub interval_min: u64,
    pub interval_max: u64,
    /// 情绪效价（-1..1）与强度（0..1），参与显著性计算。
    pub valence: f64,
    pub intensity: f64,
    /// 对行为标签的权重乘数（如 {"hide": 5}）。
    #[serde(default)]
    pub behavior_mults: BTreeMap<String, f64>,
    /// 正在执行带这些标签的行为的代理不受影响（如吸尘器吵不到躲着的猫）。
    #[serde(default)]
    pub immune_behavior_tags: Vec<String>,
}

pub fn load_config(json: &str) -> Result<SimConfig, String> {
    let cfg: SimConfig =
        serde_json::from_str(json).map_err(|e| format!("配置解析失败: {e}"))?;
    validate(&cfg)?;
    Ok(cfg)
}

/// 内容静态校验（CI 同款逻辑）：悬空引用、需求名错误。
pub fn validate(cfg: &SimConfig) -> Result<(), String> {
    let behavior_ids: Vec<&str> = cfg.behaviors.iter().map(|b| b.id.as_str()).collect();
    let spots: Vec<&str> = cfg.spots.iter().map(|s| s.id.as_str()).collect();
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
    for e in &cfg.env_events {
        if e.intensity < 0.0 || e.intensity > 1.0 {
            return Err(format!("事件 {} 强度应在 0..1", e.id));
        }
        if e.interval_min == 0 {
            return Err(format!("事件 {} 间隔下限应大于 0", e.id));
        }
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
        TreeNode::Gate { child, .. } => validate_tree(child, ids),
        TreeNode::Action { behavior } => {
            if ids.contains(&behavior.as_str()) {
                Ok(())
            } else {
                Err(format!("树引用了未注册的行为 {behavior}"))
            }
        }
    }
}
