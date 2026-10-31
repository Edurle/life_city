//! 目标关卡（M6）：数据驱动的限时目标场景。
//! 设计稿 4.5 是个人行为层的限期目标（M4 已完成）；关卡按其自然延伸落地为
//! 预设目标的小场景——给玩家一个可完成的挑战框架，让涌现故事有舞台。
//! 条件全部由现有模拟状态求值，无新模拟机制；胜负在日界结算并写入日志。

use crate::config::SimConfig;
use crate::log::LogEvent;
use crate::time::Tick;
use serde::Deserialize;
use std::collections::BTreeMap;

/// 条件谓词：六种首批（全部只读现有状态）。
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConditionDef {
    /// 区域声誉达到阈值。
    AreaReputation {
        area: String,
        min: Option<f64>,
        max: Option<f64>,
    },
    /// 行为累计执行次数（agent = "player" 只数玩家角色，"any" 数全镇）；
    /// min/max 任选其一或并用（max 用于戒断型：一次都不做）。
    BehaviorCount {
        behavior: String,
        agent: String,
        min: Option<u64>,
        max: Option<u64>,
    },
    /// 钱包余额（玩家角色）。
    Wallet { min: f64 },
    /// 与任意人的亲密达到阈值（玩家角色）。
    Bond { min: f64 },
    /// 镇上出现恋爱成对（任意代理）。
    CoupleAny,
    /// 出现概率驱动的职业转变（任意次数 ≥ min）。
    JobChange { min: u64 },
}

/// 单个条件的求值快照（进度面板与结算共用）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConditionState {
    pub label: String,
    pub current: f64,
    pub target: String,
    pub met: bool,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct ScenarioDef {
    pub id: String,
    pub name: String,
    pub description: String,
    /// 时限（天）。
    pub days: u64,
    /// 胜利条件（全部满足才算赢）。
    pub win: Vec<ConditionDef>,
    /// 开局注入：强制在第 N 天触发某环境事件（叙事钩子；下标对应配置 env_events）。
    #[serde(default)]
    pub inject_events: Vec<InjectedEvent>,
    /// 结束后是否自动生成玩家故事（默认 true）。
    #[serde(default = "d_true")]
    pub tell_story: bool,
}

fn d_true() -> bool {
    true
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct InjectedEvent {
    pub day: u64,
    pub event_index: usize,
    /// 事件位置（None = 区域事件用区域采样）。
    pub pos: Option<(f64, f64)>,
}

/// 运行中的关卡。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActiveScenario {
    pub def_json: String, // ScenarioDef 序列化（存档与核心解耦）
    pub started_tick: Tick,
    pub job_changes_baseline: u64,
    /// 开局时的行为执行基线（"关卡期间执行 N 次"要减去历史）。
    pub player_exec_baseline: BTreeMap<String, u64>,
    pub town_exec_baseline: BTreeMap<String, u64>,
    /// 已注入的开局事件下标。
    pub injected: Vec<usize>,
    pub finished: Option<bool>,
}

impl ActiveScenario {
    pub fn def(&self) -> ScenarioDef {
        serde_json::from_str(&self.def_json).expect("关卡定义应可解析")
    }

    pub fn deadline_tick(&self) -> Tick {
        let days = self.def().days;
        self.started_tick + days * crate::time::TICKS_PER_DAY
    }
}

pub fn load_scenario(json: &str) -> Result<ScenarioDef, String> {
    let def: ScenarioDef = serde_json::from_str(json).map_err(|e| format!("关卡解析失败: {e}"))?;
    validate(&def)?;
    Ok(def)
}

pub fn validate(def: &ScenarioDef) -> Result<(), String> {
    if def.days == 0 {
        return Err(format!("关卡 {} 时限应大于 0 天", def.id));
    }
    if def.win.is_empty() {
        return Err(format!("关卡 {} 至少要有一个胜利条件", def.id));
    }
    for c in &def.win {
        match c {
            ConditionDef::AreaReputation { area: _, min, max } => {
                if min.is_none() && max.is_none() {
                    return Err(format!("关卡 {} 的声誉条件缺 min/max", def.id));
                }
            }
            ConditionDef::BehaviorCount { agent, .. } if agent != "player" && agent != "any" => {
                return Err(format!(
                    "关卡 {} 的行为计数 agent 只能是 player 或 any",
                    def.id
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

/// 求值上下文：从世界抽取的最小只读视图（核心不依赖具体关卡内容）。
pub struct ScenarioView<'a> {
    pub tick: Tick,
    pub player: Option<&'a crate::agent::Agent>,
    pub player_bond_max: f64,
    pub any_couple: bool,
    pub job_changes: u64,
    pub area_reputation: &'a dyn Fn(&str) -> Option<f64>,
    pub behavior_count: &'a dyn Fn(&str, &str) -> u64,
}

impl ConditionDef {
    pub fn evaluate(&self, v: &ScenarioView) -> ConditionState {
        match self {
            ConditionDef::AreaReputation { area, min, max } => {
                let cur = (v.area_reputation)(area).unwrap_or(0.0);
                let met =
                    min.map(|m| cur >= m).unwrap_or(true) && max.map(|m| cur <= m).unwrap_or(true);
                ConditionState {
                    label: format!("「{}」口碑", area),
                    current: cur,
                    target: match (min, max) {
                        (Some(m), None) => format!("≥ {m:.2}"),
                        (None, Some(m)) => format!("≤ {m:.2}"),
                        (Some(lo), Some(hi)) => format!("{lo:.2}–{hi:.2}"),
                        _ => "—".into(),
                    },
                    met,
                }
            }
            ConditionDef::BehaviorCount {
                behavior,
                agent,
                min,
                max,
            } => {
                let cur = (v.behavior_count)(behavior, agent) as f64;
                let met = min.map(|m| cur >= m as f64).unwrap_or(true)
                    && max.map(|m| cur <= m as f64).unwrap_or(true);
                let target = match (min, max) {
                    (Some(m), None) => format!("≥ {m} 次"),
                    (None, Some(m)) => format!("≤ {m} 次"),
                    (Some(lo), Some(hi)) => format!("{lo}–{hi} 次"),
                    _ => "—".into(),
                };
                ConditionState {
                    label: format!(
                        "{}执行「{}」",
                        if agent == "player" { "" } else { "全镇" },
                        behavior
                    ),
                    current: cur,
                    target,
                    met,
                }
            }
            ConditionDef::Wallet { min } => {
                let cur = v.player.map(|p| p.wallet).unwrap_or(0.0);
                ConditionState {
                    label: "攒钱".to_string(),
                    current: cur,
                    target: format!("≥ {min:.0}"),
                    met: cur >= *min,
                }
            }
            ConditionDef::Bond { min } => ConditionState {
                label: "交到知心朋友".to_string(),
                current: v.player_bond_max,
                target: format!("≥ {min:.2}"),
                met: v.player_bond_max >= *min,
            },
            ConditionDef::CoupleAny => ConditionState {
                label: "见证一段恋情".to_string(),
                current: if v.any_couple { 1.0 } else { 0.0 },
                target: "成对".into(),
                met: v.any_couple,
            },
            ConditionDef::JobChange { min } => ConditionState {
                label: "概率驱动的转行".to_string(),
                current: v.job_changes as f64,
                target: format!("≥ {min} 次"),
                met: v.job_changes >= *min,
            },
        }
    }
}

/// 日界结算入口（由 world 调用）：到期评估胜负，写 ScenarioResult。
pub fn scenario_daily(world_view: &ScenarioView, active: &mut ActiveScenario) -> Option<LogEvent> {
    if active.finished.is_some() {
        return None;
    }
    let def = active.def();
    let deadline = active.deadline_tick();
    if world_view.tick < deadline {
        return None;
    }
    let states: Vec<ConditionState> = def.win.iter().map(|c| c.evaluate(world_view)).collect();
    let won = states.iter().all(|s| s.met);
    active.finished = Some(won);
    Some(LogEvent::ScenarioResult {
        tick: world_view.tick,
        scenario: def.id.clone(),
        won,
        conditions: states,
    })
}

/// 关卡需要的世界侧校验：引用的区域/行为/事件下标存在。
pub fn check_against_config(def: &ScenarioDef, cfg: &SimConfig) -> Result<(), String> {
    let behavior_ids: Vec<&str> = cfg.behaviors.iter().map(|b| b.id.as_str()).collect();
    for c in &def.win {
        if let ConditionDef::AreaReputation { area, .. } = c {
            if !cfg.areas.iter().any(|a| a.id == *area) {
                return Err(format!("关卡 {} 引用未知区域 {area}", def.id));
            }
        }
        if let ConditionDef::BehaviorCount { behavior, .. } = c {
            if !behavior_ids.contains(&behavior.as_str()) {
                return Err(format!("关卡 {} 引用未知行为 {behavior}", def.id));
            }
        }
    }
    for inj in &def.inject_events {
        if inj.event_index >= cfg.env_events.len() {
            return Err(format!(
                "关卡 {} 注入事件下标越界（{} ≥ {}）",
                def.id,
                inj.event_index,
                cfg.env_events.len()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scenario(days: u64, win: Vec<ConditionDef>) -> ScenarioDef {
        ScenarioDef {
            id: "test".into(),
            name: "测试关卡".into(),
            description: String::new(),
            days,
            win,
            inject_events: vec![],
            tell_story: true,
        }
    }

    #[test]
    fn parse_and_validate() {
        let json = r#"{
            "id": "park", "name": "公园复苏", "description": "", "days": 30,
            "win": [{"kind": "area_reputation", "area": "park", "min": 0.7}]
        }"#;
        let def = load_scenario(json).unwrap();
        assert_eq!(def.days, 30);
        assert!(
            load_scenario(r#"{"id":"x","name":"","description":"","days":0,"win":[]}"#).is_err()
        );
    }

    /// 到期结算：条件全满足 = 胜，有缺 = 负；只结算一次。
    #[test]
    fn deadline_settles_once() {
        let def = scenario(
            3,
            vec![
                ConditionDef::Wallet { min: 10.0 },
                ConditionDef::Bond { min: 0.5 },
            ],
        );
        let mut active = ActiveScenario {
            def_json: serde_json::to_string(&def).unwrap(),
            started_tick: 0,
            job_changes_baseline: 0,
            player_exec_baseline: BTreeMap::new(),
            town_exec_baseline: BTreeMap::new(),
            injected: vec![],
            finished: None,
        };
        // 钱够、朋友不够。
        let rep: &dyn Fn(&str) -> Option<f64> = &|_: &str| None;
        let count: &dyn Fn(&str, &str) -> u64 = &|_: &str, _: &str| 0u64;
        let v = ScenarioView {
            tick: 3 * 1440,
            player: None,
            player_bond_max: 0.3,
            any_couple: false,
            job_changes: 0,
            area_reputation: &rep,
            behavior_count: &count,
        };
        let ev = scenario_daily(&v, &mut active).unwrap();
        let LogEvent::ScenarioResult {
            won, conditions, ..
        } = ev
        else {
            panic!("应产生结算事件");
        };
        assert!(!won, "缺条件应判负");
        assert_eq!(conditions.len(), 2);
        assert!(active.finished == Some(false));
        // 二次调用不再结算。
        assert!(scenario_daily(&v, &mut active).is_none());
    }
}
