//! 世界：调度器 + 环境事件 + 决策核心 + 玩家指令（意志节点）。
//! 决策流程对应设计稿的四层结构：
//!   1) 生理反射：需求低于临界值 → 强制执行恢复行为（不掷概率）
//!   2) 需求门：树结构筛出当前可用行为
//!   3) 加权选择：softmax(权重/温度)，权重 = 代理权重(含习惯) × 需求紧迫度 × 情境记忆
//!   4) 玩家指令（仅玩家角色）：作为覆盖选择在下一拍执行，违背习惯的指令消耗意志力
//! 代理按编号顺序结算，随机数用独立子流，回放不依赖调度细节。

use crate::agent::Agent;
use crate::config::{BehaviorDef, SimConfig, SpotDef};
use crate::log::{AgentSummary, FactorBreakdown, LogEvent, Logger, LOG_VERSION};
use crate::needs::{urgency, Needs, NEED_KEYS};
use crate::rng::{derive_seed, Rng, WorldSeed};
use crate::time::{is_day_boundary, is_hour_boundary, quarter_of_day, Tick};
use crate::tree::available_behaviors;
use std::collections::BTreeMap;

const STREAM_WORLD: u8 = 200;

/// 一个候选行为的完整评估（决策、菜单、指令定价共用）。
#[derive(Debug, Clone)]
pub struct ChoiceInfo {
    pub behavior: String,
    pub base: f64,
    pub weight: f64,
    pub need_mult: f64,
    pub mem_mult: f64,
    pub eff: f64,
}

/// 玩家指令的结算结果。
#[derive(Debug, Clone)]
pub struct CommandOutcome {
    pub accepted: bool,
    pub cost: f64,
    /// 拒绝原因或执行反应（给玩家看的文字）。
    pub reason: String,
}

/// 菜单项状态（交互工具用）。
#[derive(Debug, Clone)]
pub struct BehaviorStatus {
    pub id: String,
    pub name: String,
    pub available: bool,
    /// 预计意志力消耗（不含打断费）。
    pub est_cost: f64,
    /// 需求门未开时的提示。
    pub gate_note: String,
}

pub struct World {
    pub tick: Tick,
    pub cfg: SimConfig,
    pub agents: Vec<Agent>,
    pub(crate) behaviors: BTreeMap<String, BehaviorDef>,
    pub(crate) spots: BTreeMap<String, SpotDef>,
    /// 环境事件的下一次发生拍（与 cfg.env_events 下标对应）。
    pub(crate) event_schedule: Vec<Tick>,
    pub(crate) world_rng: Rng,
    pub logger: Logger,
    /// 性能统计（基准模式用）。
    pub decisions_done: u64,
    /// 最近事件的环形缓冲（交互工具实时反馈用；基准/常规跑批关闭）。
    pub collect_recent: bool,
    pub recent_events: Vec<LogEvent>,
}

impl World {
    pub fn new(cfg: SimConfig, seed: WorldSeed, agent_names: Vec<String>, logging: bool) -> Self {
        let behaviors: BTreeMap<String, BehaviorDef> = cfg
            .behaviors
            .iter()
            .map(|b| (b.id.clone(), b.clone()))
            .collect();
        let spots: BTreeMap<String, SpotDef> =
            cfg.spots.iter().map(|s| (s.id.clone(), s.clone())).collect();
        let behavior_list: Vec<(String, f64)> = cfg
            .behaviors
            .iter()
            .map(|b| (b.id.clone(), b.base_weight))
            .collect();
        let agents = agent_names
            .into_iter()
            .enumerate()
            .map(|(i, name)| Agent::new(seed, i as u32, name, &behavior_list, &cfg.params))
            .collect();
        let mut world_rng = Rng::seeded(derive_seed(seed, 0, STREAM_WORLD));
        let event_schedule = cfg
            .env_events
            .iter()
            .map(|e| world_rng.uniform_int(e.interval_min, e.interval_max))
            .collect();
        World {
            tick: 0,
            cfg,
            agents,
            behaviors,
            spots,
            event_schedule,
            world_rng,
            logger: Logger::new(logging),
            decisions_done: 0,
            collect_recent: false,
            recent_events: Vec::new(),
        }
    }

    /// 指定玩家角色（0 号代理）。
    pub fn set_player(&mut self, id: u32) {
        if let Some(a) = self.agents.iter_mut().find(|a| a.id == id) {
            a.is_player = true;
        }
    }

    /// 模拟开始前写日志头（只含确定性字段，保证日志字节级可比较）。
    pub fn log_meta(&mut self, seed: u64, total_ticks: Tick) {
        self.logger.log(&LogEvent::Meta {
            version: LOG_VERSION,
            seed,
            agents: self.agents.len(),
            ticks: total_ticks,
        });
    }

    /// 推进 n 拍。
    pub fn run(&mut self, ticks: Tick) {
        for _ in 0..ticks {
            self.tick();
        }
    }

    pub fn tick(&mut self) {
        self.tick += 1;
        let tick = self.tick;
        let mut events: Vec<LogEvent> = Vec::new();

        // 日界：记忆习惯化回落与强度消退。
        if is_day_boundary(tick) {
            for a in &mut self.agents {
                a.memory.daily_decay();
            }
        }

        // 需求衰减（每拍 = 每模拟分钟）。
        let decay = self.cfg.needs.clone();
        for a in &mut self.agents {
            a.needs.decay(&decay, 1);
        }

        // 环境事件：收集到期者，按定义顺序应用（确定性顺序）。
        let due: Vec<usize> = self
            .event_schedule
            .iter()
            .enumerate()
            .filter(|(_, at)| **at == tick)
            .map(|(i, _)| i)
            .collect();
        let tag_map: BTreeMap<String, Vec<String>> = self
            .behaviors
            .iter()
            .map(|(k, v)| (k.clone(), v.tags.clone()))
            .collect();
        for i in due {
            let def = self.cfg.env_events[i].clone();
            events.push(LogEvent::EnvEvent {
                tick,
                event: def.id.clone(),
            });
            let quarter = quarter_of_day(tick);
            for a in &mut self.agents {
                let immune = def.immune_behavior_tags.iter().any(|t| {
                    a.current
                        .as_ref()
                        .map(|(bid, _, _)| {
                            tag_map.get(bid).map(|ts| ts.contains(t)).unwrap_or(false)
                        })
                        .unwrap_or(false)
                });
                if immune {
                    continue;
                }
                let salience = a.memory.write(
                    &def.id,
                    &def.tags,
                    quarter,
                    def.intensity,
                    1.0,
                    def.behavior_mults.clone(),
                    tick,
                );
                if salience >= 0.05 {
                    events.push(LogEvent::MemoryWrite {
                        tick,
                        agent: a.id,
                        event: def.id.clone(),
                        salience,
                        mults: def.behavior_mults.clone(),
                    });
                }
            }
            self.event_schedule[i] =
                tick + self.world_rng.uniform_int(def.interval_min, def.interval_max);
        }

        // 小时边界：习惯衰减、意志力缓慢回复、状态快照。
        if is_hour_boundary(tick) {
            let rate = self.cfg.params.habit_decay_hourly;
            for a in &mut self.agents {
                a.habit_decay(rate);
                a.willpower = (a.willpower + 0.2).min(100.0);
                if self.logger.enabled {
                    events.push(LogEvent::Snapshot {
                        tick,
                        agent: a.id,
                        needs: a.needs,
                        weights: a.weights.clone(),
                        memories: a.memory.entries.len(),
                        willpower: a.willpower,
                    });
                }
            }
        }

        // 动作完成结算：应用需求影响、习惯强化、睡眠恢复意志力。
        let alpha = self.cfg.params.alpha;
        for a in &mut self.agents {
            let done = match &a.current {
                Some((_, _, end)) => tick >= *end,
                None => false,
            };
            if done {
                let (bid, _, _) = a.current.clone().unwrap();
                if let Some(def) = self.behaviors.get(&bid) {
                    for (need, delta) in &def.effects {
                        a.needs.add(need, *delta);
                    }
                    // 睡眠类行为完成：恢复意志力。
                    if def.tags.iter().any(|t| t == "sleep") {
                        a.willpower = (a.willpower + 25.0).min(100.0);
                    }
                }
                a.habit_boost(&bid, alpha);
                a.record_exec(&bid);
                a.current = None;
                a.next_heartbeat = tick + 2 + a.rng_decision.uniform_int(0, 6);
                events.push(LogEvent::ActionEnd {
                    tick,
                    agent: a.id,
                    behavior: bid,
                });
            }
        }

        // 心跳决策（空闲且到期的代理；字段级借用避免冲突）。
        let agents = &mut self.agents;
        let cfg = &self.cfg;
        let behaviors = &self.behaviors;
        let spots = &self.spots;
        for a in agents.iter_mut() {
            if a.current.is_none() && tick >= a.next_heartbeat {
                events.extend(decide(cfg, behaviors, spots, a, tick));
                self.decisions_done += 1;
            }
        }

        for ev in &events {
            self.logger.log(ev);
        }
        if self.collect_recent {
            self.recent_events.extend(events.iter().cloned());
            let cap = 512;
            if self.recent_events.len() > cap {
                let drop = self.recent_events.len() - cap;
                self.recent_events.drain(0..drop);
            }
        }
    }

    // ---------- 玩家指令（意志节点） ----------

    /// 下达指令：校验可用性、计算意志力成本、写日志、排入下一拍执行。
    /// 行为树从不否决指令，只让违背习惯的指令付出更高代价；
    /// 意志力不足时指令被拒绝（猫赖着不动）。
    pub fn issue_command(&mut self, agent_id: u32, behavior: &str) -> CommandOutcome {
        let tick = self.tick;
        let a = match self.agents.iter_mut().find(|a| a.id == agent_id) {
            Some(a) if a.is_player => a,
            Some(_) => {
                return CommandOutcome {
                    accepted: false,
                    cost: 0.0,
                    reason: "只能指挥玩家角色".into(),
                }
            }
            None => {
                return CommandOutcome {
                    accepted: false,
                    cost: 0.0,
                    reason: "没有这个角色".into(),
                }
            }
        };
        let def = match self.behaviors.get(behavior) {
            Some(d) => d.clone(),
            None => {
                return CommandOutcome {
                    accepted: false,
                    cost: 0.0,
                    reason: "未知行为".into(),
                }
            }
        };
        let options = compute_options(&self.cfg, &self.behaviors, &self.spots, a, tick);
        // 生理反射压倒一切：临界需求未解决时，只接受对应的恢复行为。
        let forced = forced_choice(&self.cfg, &self.behaviors, &a.needs, &options);
        let (cost, refusal) = match options.iter().find(|o| o.behavior == behavior) {
            Some(o) => {
                let eff_max = options.iter().map(|o| o.eff).fold(f64::NEG_INFINITY, f64::max);
                let ratio = if eff_max > 0.0 { (o.eff / eff_max).clamp(0.0, 1.0) } else { 1.0 };
                (5.0 + 20.0 * (1.0 - ratio), None)
            }
            None if forced.as_deref() == Some(behavior) => (3.0, None),
            None if forced.is_some() => (
                0.0,
                Some("它生理需求告急，现在听不进别的".to_string()),
            ),
            None => (
                0.0,
                Some(format!("它现在没这个念头（{}）", gate_note(&self.cfg, &def))),
            ),
        };
        if let Some(reason) = refusal {
            let out = CommandOutcome {
                accepted: false,
                cost: 0.0,
                reason,
            };
            self.logger.log(&LogEvent::PlayerCommand {
                tick,
                agent: agent_id,
                behavior: behavior.to_string(),
                cost: 0.0,
                accepted: false,
                reason: out.reason.clone(),
            });
            return out;
        }
        // 打断当前行为额外收费。
        let interrupting = a.current.is_some();
        let cost = (cost + if interrupting { 8.0 } else { 0.0 }).round();
        if a.willpower + 1e-9 < cost {
            let out = CommandOutcome {
                accepted: false,
                cost,
                reason: format!("意志力不足：需要 {cost:.0}，当前 {:.0}，它赖着没动", a.willpower),
            };
            self.logger.log(&LogEvent::PlayerCommand {
                tick,
                agent: agent_id,
                behavior: behavior.to_string(),
                cost,
                accepted: false,
                reason: out.reason.clone(),
            });
            return out;
        }
        a.willpower -= cost;
        if interrupting {
            if let Some((bid, _, _)) = a.current.take() {
                self.logger.log(&LogEvent::ActionEnd {
                    tick,
                    agent: agent_id,
                    behavior: bid,
                });
            }
        }
        a.next_heartbeat = tick + 1;
        a.pending_command = Some(behavior.to_string());
        let reaction = if cost <= 6.0 {
            "它立刻照做了"
        } else if cost < 15.0 {
            "它磨蹭了一会儿，还是去了"
        } else {
            "它满脸不情愿，但还是去了"
        };
        let out = CommandOutcome {
            accepted: true,
            cost,
            reason: reaction.to_string(),
        };
        self.logger.log(&LogEvent::PlayerCommand {
            tick,
            agent: agent_id,
            behavior: behavior.to_string(),
            cost,
            accepted: true,
            reason: out.reason.clone(),
        });
        out
    }

    /// 菜单状态：每个行为的可用性、预计意志力消耗、门未开的提示。
    pub fn behaviors_status(&self, agent_id: u32) -> Vec<BehaviorStatus> {
        let a = match self.agents.iter().find(|a| a.id == agent_id) {
            Some(a) => a,
            None => return vec![],
        };
        let options = compute_options(&self.cfg, &self.behaviors, &self.spots, a, self.tick);
        let eff_max = options.iter().map(|o| o.eff).fold(f64::NEG_INFINITY, f64::max);
        self.cfg
            .behaviors
            .iter()
            .map(|def| match options.iter().find(|o| o.behavior == def.id) {
                Some(o) => {
                    let ratio =
                        if eff_max > 0.0 { (o.eff / eff_max).clamp(0.0, 1.0) } else { 1.0 };
                    BehaviorStatus {
                        id: def.id.clone(),
                        name: def.display_name().to_string(),
                        available: true,
                        est_cost: (5.0 + 20.0 * (1.0 - ratio)).round(),
                        gate_note: String::new(),
                    }
                }
                None => BehaviorStatus {
                    id: def.id.clone(),
                    name: def.display_name().to_string(),
                    available: false,
                    est_cost: 0.0,
                    gate_note: gate_note(&self.cfg, def),
                },
            })
            .collect()
    }

    /// 结束时计算汇总（行为分布、与 0 号的总变差距离、主要记忆），写入日志并返回。
    pub fn summarize(&mut self) -> Vec<AgentSummary> {
        let dists: Vec<BTreeMap<String, f64>> = self
            .agents
            .iter()
            .map(|a| share_map(&a.exec_counts))
            .collect();
        let mut summaries = Vec::new();
        for (a, dist) in self.agents.iter().zip(&dists) {
            let tv = tv_distance(dist, &dists[0]);
            let mut top_memories: Vec<(f64, String)> = a
                .memory
                .entries
                .iter()
                .map(|m| {
                    (
                        m.strength,
                        format!("{}(时段{}) → {:?}", m.event, m.quarter, m.mults),
                    )
                })
                .collect();
            top_memories.sort_by(|x, y| y.0.total_cmp(&x.0));
            summaries.push(AgentSummary {
                id: a.id,
                name: a.name.clone(),
                temperature: a.temperature,
                distribution: dist.clone(),
                tv_distance_to_first: tv,
                memory_count: a.memory.entries.len(),
                top_memories: top_memories
                    .into_iter()
                    .take(3)
                    .map(|(s, t)| format!("[强度{:.2}] {}", s, t))
                    .collect(),
            });
        }
        self.logger
            .log(&LogEvent::Summary {
                agents: summaries.clone(),
            });
        summaries
    }
}

/// 决策核心（自由函数：cfg/behaviors/spots 只读，代理可变）。
/// 返回 [Decision, ActionStart] 两条日志事件。
fn decide(
    cfg: &SimConfig,
    behaviors: &BTreeMap<String, BehaviorDef>,
    spots: &BTreeMap<String, SpotDef>,
    a: &mut Agent,
    tick: Tick,
) -> Vec<LogEvent> {
    let options = compute_options(cfg, behaviors, spots, a, tick);

    let (choice, factors) = if let Some(cmd) = a.pending_command.take() {
        // 第四层：玩家指令覆盖（成本已在 issue_command 结算）。
        match options.iter().find(|o| o.behavior == cmd) {
            Some(o) => (cmd.clone(), FactorBreakdown {
                base: o.base,
                weight: o.weight,
                need_mult: o.need_mult,
                mem_mult: o.mem_mult,
                forced: false,
                will_command: true,
            }),
            None => (cmd.clone(), FactorBreakdown {
                base: behaviors.get(&cmd).map(|d| d.base_weight).unwrap_or(1.0),
                weight: a.weights.get(&cmd).copied().unwrap_or(1.0),
                need_mult: 1.0,
                mem_mult: 1.0,
                forced: false,
                will_command: true,
            }),
        }
    } else if options.is_empty() {
        // 防御：门全关时原地休息（正常配置不会走到）。
        ("rest".to_string(), FactorBreakdown {
            base: 1.0,
            weight: 1.0,
            need_mult: 1.0,
            mem_mult: 1.0,
            forced: true,
            will_command: false,
        })
    } else if let Some(o) = forced_choice(cfg, behaviors, &a.needs, &options) {
        // 第一层：生理反射。
        let def = &behaviors[o.as_str()];
        let drive = def.drive_need().unwrap_or("fun");
        let info = options.iter().find(|x| x.behavior == *o).unwrap();
        (o.to_string(), FactorBreakdown {
            base: info.base,
            weight: info.weight,
            need_mult: urgency(a.needs.get(drive).unwrap_or(100.0)),
            mem_mult: 1.0,
            forced: true,
            will_command: false,
        })
    } else {
        // 第三层：加权随机选择。
        let effs: Vec<f64> = options.iter().map(|o| o.eff).collect();
        let idx = softmax_pick(&effs, a.temperature, &mut a.rng_decision);
        let o = &options[idx];
        (o.behavior.clone(), FactorBreakdown {
            base: o.base,
            weight: o.weight,
            need_mult: o.need_mult,
            mem_mult: o.mem_mult,
            forced: false,
            will_command: false,
        })
    };

    // 设置动作。
    let def = behaviors.get(&choice);
    let duration = match def {
        Some(d) => a.rng_decision.uniform_int(d.duration_min, d.duration_max),
        None => 30,
    };
    let until = tick + duration;
    a.current = Some((choice.clone(), tick, until));
    a.next_heartbeat = until;

    vec![
        LogEvent::Decision {
            tick,
            agent: a.id,
            temperature: a.temperature,
            factors,
            choice: choice.clone(),
        },
        LogEvent::ActionStart {
            tick,
            agent: a.id,
            behavior: choice,
            until,
        },
    ]
}

/// 计算当前全部可用行为及其权重分解（决策、菜单、指令定价共用）。
fn compute_options(
    cfg: &SimConfig,
    behaviors: &BTreeMap<String, BehaviorDef>,
    spots: &BTreeMap<String, SpotDef>,
    a: &Agent,
    tick: Tick,
) -> Vec<ChoiceInfo> {
    let mut avail: Vec<&str> = Vec::new();
    available_behaviors(&cfg.tree, &a.needs, behaviors, &mut avail);
    let quarter = quarter_of_day(tick);
    avail
        .iter()
        .map(|b| {
            let def = &behaviors[*b];
            let w = a.weights.get(*b).copied().unwrap_or(def.base_weight);
            let drive = def.drive_need().unwrap_or("fun");
            let nm = if def.effect_on(drive) > 0.0 {
                urgency(a.needs.get(drive).unwrap_or(100.0))
            } else {
                0.6 // 非恢复型悠闲行为的温和默认驱力
            };
            let spot_tags: Vec<String> = def
                .spot
                .as_ref()
                .and_then(|s| spots.get(s))
                .map(|s| s.tags.clone())
                .unwrap_or_default();
            let mem_map = a.memory.query(quarter, &spot_tags);
            let mm: f64 = def
                .tags
                .iter()
                .map(|t| mem_map.get(t).copied().unwrap_or(1.0))
                .product::<f64>()
                .clamp(0.05, 20.0);
            ChoiceInfo {
                behavior: b.to_string(),
                base: def.base_weight,
                weight: w,
                need_mult: nm,
                mem_mult: mm,
                eff: w * nm * mm,
            }
        })
        .collect()
}

/// 第一层生理反射：任一需求低于临界值时，返回恢复量最大的可用行为。
fn forced_choice(
    cfg: &SimConfig,
    behaviors: &BTreeMap<String, BehaviorDef>,
    needs: &Needs,
    options: &[ChoiceInfo],
) -> Option<String> {
    for need in NEED_KEYS {
        if needs.get(need).unwrap_or(100.0) < cfg.critical_need {
            let best = options
                .iter()
                .filter(|o| behaviors[o.behavior.as_str()].effect_on(need) > 0.0)
                .max_by(|x, y| {
                    behaviors[x.behavior.as_str()]
                        .effect_on(need)
                        .total_cmp(&behaviors[y.behavior.as_str()].effect_on(need))
                });
            if best.is_some() {
                return best.map(|o| o.behavior.clone());
            }
        }
    }
    None
}

/// 需求门未开时给玩家看的提示文字。
fn gate_note(_cfg: &SimConfig, def: &BehaviorDef) -> String {
    let words = [
        ("hunger", "它现在不饿"),
        ("energy", "它现在不困"),
        ("fun", "它不缺玩"),
        ("social", "它不缺陪伴"),
        ("hygiene", "它不脏"),
        ("bladder", "它不急"),
    ];
    match &def.gate_need {
        Some(need) => words
            .iter()
            .find(|(k, _)| k == need)
            .map(|(_, w)| w.to_string())
            .unwrap_or_else(|| format!("条件未满足：{need}")),
        None => "现在做不了".to_string(),
    }
}

/// softmax 加权随机选择：p_i ∝ exp(w_i / T)。减最大值做数值稳定。
pub fn softmax_pick(weights: &[f64], temperature: f64, rng: &mut Rng) -> usize {
    let t = temperature.max(0.05);
    let max = weights.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = weights
        .iter()
        .map(|w| (((w - max) / t).min(30.0)).exp())
        .collect();
    let sum: f64 = exps.iter().sum();
    let mut r = rng.next_f64() * sum;
    for (i, e) in exps.iter().enumerate() {
        r -= e;
        if r < 0.0 {
            return i;
        }
    }
    exps.len() - 1
}

fn share_map(counts: &BTreeMap<String, u64>) -> BTreeMap<String, f64> {
    let total: u64 = counts.values().sum();
    if total == 0 {
        return BTreeMap::new();
    }
    counts
        .iter()
        .map(|(k, v)| (k.clone(), *v as f64 / total as f64))
        .collect()
}

/// 总变差距离：0.5 × Σ|p − q|。0 分布相同，1 完全不同。
pub fn tv_distance(a: &BTreeMap<String, f64>, b: &BTreeMap<String, f64>) -> f64 {
    let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
    keys.sort();
    keys.dedup();
    0.5 * keys
        .iter()
        .map(|k| {
            (a.get(*k).copied().unwrap_or(0.0) - b.get(*k).copied().unwrap_or(0.0)).abs()
        })
        .sum::<f64>()
}

/// 生成猫的默认名字。
pub fn cat_names(n: usize) -> Vec<String> {
    const NAMES: [&str; 12] = [
        "小白", "小黑", "小花", "小橘", "小灰", "小黄", "奶牛", "三花", "狸花", "狮子", "布丁", "雪球",
    ];
    (0..n)
        .map(|i| {
            if i < NAMES.len() {
                NAMES[i].to_string()
            } else {
                format!("猫{}", i + 1)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn softmax_prefers_higher_weight() {
        let mut rng = Rng::seeded(7);
        let mut high = 0;
        for _ in 0..2000 {
            if softmax_pick(&[5.0, 1.0], 1.0, &mut rng) == 0 {
                high += 1;
            }
        }
        // exp(5)/(exp(5)+exp(1)) ≈ 98%
        assert!(high > 1900, "high = {high}");
    }

    #[test]
    fn high_temperature_flattens_distribution() {
        let mut rng = Rng::seeded(7);
        let mut hot = 0;
        for _ in 0..2000 {
            if softmax_pick(&[5.0, 1.0], 10.0, &mut rng) == 0 {
                hot += 1;
            }
        }
        assert!(hot < 1600, "hot = {hot}");
    }
}
