//! 世界：调度器 + 空间与关系 + 环境事件 + 决策核心 + 玩家指令（意志节点）。
//! 决策流程对应设计稿的四层结构：
//!   1) 生理反射：需求低于临界值 → 强制执行恢复行为（不掷概率）
//!   2) 需求门：树结构筛出当前可用行为
//!   3) 加权选择：softmax(权重/温度)，权重 = 代理权重(含习惯) × 需求紧迫度 × 情境记忆
//!   4) 玩家指令（仅玩家角色）：作为覆盖选择在下一拍执行，违背习惯的指令消耗意志力
//!
//! M2 猫群机制：
//! - 环境事件有发生位置与影响半径，受影响者经空间网格查询；
//! - 强负面事件造成惊吓（打断当前行为、立即重新决策），反应可被同伴目睹；
//! - 目睹者按"亲密值 × 模仿折扣"写入间接记忆（模仿节点，班杜拉观察学习）；
//! - 共同度过社交时光增长亲密值；信任的同伴在身边提供安全感，压制负面记忆。
//!
//! M4 玩家的人生：
//! - LOD 分层（技术文档 4.6）：玩家身边 T0 完整模拟，街区内 T1 常规（慢心跳、
//!   小目睹半径），其余 T2 背景（简化选择 + 按天批量结算）。层级是状态的
//!   确定性函数；降层不丢数据（记忆、关系、习惯全保留），升层即恢复完整模拟；
//! - 习惯自动化（设计稿 4.4）：权重超阈值的行为自动执行、指令零消耗；
//! - 负面记忆恢复（设计稿 4.6）：被恐惧抑制的行为指令加价并警示；
//!   反复完成同类行为使恐惧消退（暴露疗法），亲近同伴加速恢复；
//! - 密友查看（设计稿 4.7）：亲密值达标后可读对方需求与负面记忆；
//! - 限期目标（设计稿 4.5）：戒断目标压制习惯权重，行为树仍选中时靠意志力
//!   忍住，放弃则权重反弹。
//!
//! 代理按编号顺序结算，随机数用独立子流，回放不依赖调度细节。

use crate::agent::Agent;
use crate::area::AreaState;
use crate::config::{BehaviorDef, Params, SimConfig, SpotDef};
use crate::goal::Goal;
use crate::log::{AgentSummary, AreaSummary, FactorBreakdown, LogEvent, Logger, LOG_VERSION};
use crate::needs::{urgency, Needs, NEED_KEYS};
use crate::relation::Relations;
use crate::rng::{derive_seed, Rng, WorldSeed};
use crate::space::{dist, Pos, SpatialGrid};
use crate::time::{hour_of, is_day_boundary, is_hour_boundary, quarter_of_day, Tick};
use crate::tree::available_behaviors;
use std::collections::BTreeMap;

const STREAM_WORLD: u8 = 200;
const STREAM_RELATION: u8 = 201;

/// 初始熟悉度只在小群体（一间房）里生成；大世界（M3+ 街道）从陌生开始，
/// 关系由真实互动建立——也避免大规模世界的 O(N²) 初始化。
const INITIAL_BOND_GROUP_CAP: usize = 32;

/// 一个候选行为的完整评估（决策、菜单、指令定价共用）。
#[derive(Debug, Clone)]
pub struct ChoiceInfo {
    pub behavior: String,
    pub base: f64,
    pub weight: f64,
    pub need_mult: f64,
    pub mem_mult: f64,
    /// 地点声誉乘数（M3）：行为所在区域的口碑与整洁度修正。
    pub area_mult: f64,
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
    /// 预计意志力消耗（不含打断费；含恐惧/违约加价，自动化为 0）。
    pub est_cost: f64,
    /// 需求门未开时的提示。
    pub gate_note: String,
    /// 习惯自动化（M4）：无需指令自动执行，指令零消耗。
    pub automated: bool,
    /// 恐惧警示（M4）：被正在生效的负面记忆抑制，执行要过心理关。
    pub feared: bool,
    /// 戒断目标进行中（M4）：下令即违约。
    pub goal_avoided: bool,
}

/// 习惯报告条目（M4 验收："我的角色养成了某习惯，因为某经历"的载体）。
#[derive(Debug, Clone)]
pub struct HabitReport {
    pub behavior: String,
    pub weight: f64,
    pub base: f64,
    /// 权重相对基准的倍数（≥ 自动化阈值即已养成）。
    pub ratio: f64,
    pub execs: u64,
    pub automated: bool,
    /// 与该行为标签相关的记忆（强度降序），解释"因为某经历"。
    pub related_memories: Vec<String>,
}

/// 密友查看（M4，设计稿 4.7）：亲密值达标后可见的对方部分心理状态。
#[derive(Debug, Clone)]
pub struct FriendView {
    pub name: String,
    pub bond: f64,
    pub needs: Needs,
    /// 正在做的行为（None = 空闲）。
    pub current: Option<String>,
    /// 正在生效的负面记忆（事件名, 强度），强度降序。
    pub fears: Vec<(String, f64)>,
}

/// 限期目标状态（界面进度显示用）。
#[derive(Debug, Clone)]
pub struct GoalStatus {
    pub behavior: String,
    /// 剩余天数（向上取整）。
    pub days_left: u64,
    pub violations: u32,
    pub resists: u32,
    /// 目标压制已累计的量（放弃时会反弹回去）。
    pub suppressed: f64,
}

pub struct World {
    pub tick: Tick,
    pub cfg: SimConfig,
    pub agents: Vec<Agent>,
    pub relations: Relations,
    pub(crate) grid: SpatialGrid,
    /// 地点状态（M3）：声誉与整洁度，键为区域 ID。
    pub area_states: BTreeMap<String, AreaState>,
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
    /// 建世界。roster = (名字, 物种)——物种决定用哪棵行为树（M3 人类与猫同街）。
    pub fn new(
        cfg: SimConfig,
        seed: WorldSeed,
        roster: Vec<(String, String)>,
        logging: bool,
    ) -> Self {
        let behaviors: BTreeMap<String, BehaviorDef> = cfg
            .behaviors
            .iter()
            .map(|b| (b.id.clone(), b.clone()))
            .collect();
        let spots: BTreeMap<String, SpotDef> = cfg
            .spots
            .iter()
            .map(|s| (s.id.clone(), s.clone()))
            .collect();
        let agents = roster
            .into_iter()
            .enumerate()
            .map(|(i, (name, species))| {
                let behavior_list = cfg.species_behaviors(&species);
                Agent::new(seed, i as u32, name, species, &behavior_list, &cfg.params)
            })
            .collect::<Vec<_>>();
        let area_states: BTreeMap<String, AreaState> = cfg
            .areas
            .iter()
            .map(|a| (a.id.clone(), AreaState::from_def(a)))
            .collect();
        // 同屋檐下的猫互相有点眼熟（仅小群体；见常量注释）。
        let mut relations = Relations::new(cfg.params.comfort_min_bond);
        if agents.len() <= INITIAL_BOND_GROUP_CAP {
            let mut rel_rng = Rng::seeded(derive_seed(seed, 0, STREAM_RELATION));
            let n = agents.len();
            for i in 0..n {
                for j in (i + 1)..n {
                    relations.set_bond(
                        i as u32,
                        j as u32,
                        rel_rng.uniform(0.0, cfg.params.initial_bond_max),
                    );
                }
            }
        }
        let mut grid = SpatialGrid::new(cfg.params.comfort_radius.max(0.5));
        for a in &agents {
            grid.move_to(a.id, a.pos);
        }
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
            relations,
            grid,
            area_states,
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

    /// 指定玩家角色（0 号代理）。指定后立即分配 LOD 层级。
    pub fn set_player(&mut self, id: u32) {
        if let Some(a) = self.agents.iter_mut().find(|a| a.id == id) {
            a.is_player = true;
        }
        self.refresh_lod();
    }

    /// LOD 层级分配（M4，技术文档 4.6）。状态的确定性函数：
    /// 距玩家距离 + 与玩家的关系，按 (距离, 编号) 排序截断数量上限——
    /// 不掷随机数、不依赖结算顺序，回放一致。无玩家角色时全体 T0（等价全模拟）。
    /// - T0：玩家、身边 lod_t0_radius 内、密友（亲密 ≥ close_friend_bond）
    /// - T1：lod_t1_radius 内、朋友（亲密 ≥ lod_friend_bond）
    /// - T2：其余背景代理（不逐拍决策，按天批量结算）
    pub fn refresh_lod(&mut self) {
        let Some(ppos) = self
            .agents
            .iter()
            .find(|a| a.is_player)
            .map(|a| a.pos)
        else {
            for a in &mut self.agents {
                a.lod = 0;
            }
            return;
        };
        let p = &self.cfg.params;
        let pid = self
            .agents
            .iter()
            .find(|a| a.is_player)
            .map(|a| a.id)
            .unwrap_or(0);
        // 先按规则给出层级，再依 (距离, 编号) 截断数量上限（超额降一层）。
        // 玩家与密友（剧情关联者）以距离 0 参与排名——关系和身边同权重，
        // 拥挤时不会被挤出局；编号兜底保证确定性。
        let mut t0: Vec<(f64, u32)> = Vec::new();
        let mut t1: Vec<(f64, u32)> = Vec::new();
        for a in &mut self.agents {
            if a.is_player {
                a.lod = 0;
                t0.push((0.0, a.id));
                continue;
            }
            let d = dist(a.pos, ppos);
            let bond = self.relations.bond(pid, a.id);
            if d <= p.lod_t0_radius || bond >= p.close_friend_bond {
                a.lod = 0;
                t0.push((if bond >= p.close_friend_bond { 0.0 } else { d }, a.id));
            } else if d <= p.lod_t1_radius || bond >= p.lod_friend_bond {
                a.lod = 1;
                t1.push((d, a.id));
            } else {
                a.lod = 2;
            }
        }
        t0.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        // T0 超额部分降为 T1 候选，与原 T1 候选合并后再截 T1 上限。
        let mut t1_all: Vec<(f64, u32)> = t1;
        if t0.len() > p.lod_t0_cap {
            t1_all.extend(t0[p.lod_t0_cap..].iter().copied());
        }
        t1_all.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        let demote_to_t1: Vec<u32> = t0[p.lod_t0_cap.min(t0.len())..]
            .iter()
            .map(|(_, id)| *id)
            .collect();
        let demote_to_t2: Vec<u32> = t1_all[p.lod_t1_cap.min(t1_all.len())..]
            .iter()
            .map(|(_, id)| *id)
            .collect();
        for a in &mut self.agents {
            // 注意先后：T0 溢出者可能又挤不进 T1（两级都超），先判 T2。
            if demote_to_t2.contains(&a.id) {
                a.lod = 2;
            } else if demote_to_t1.contains(&a.id) {
                a.lod = 1;
            }
            // 升层唤醒：刚进 T0/T1 的空闲代理别等过时的心跳。
            if a.lod < 2 && a.current.is_none() {
                a.next_heartbeat = a.next_heartbeat.min(self.tick + 5);
            }
        }
    }

    /// 代理当前 LOD 层级（工具与测试用）。
    pub fn agent_lod(&self, id: u32) -> u8 {
        self.agents
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.lod)
            .unwrap_or(0)
    }

    // ---------- M2 猫群：关系与空间（测试与工具用入口） ----------

    pub fn bond(&self, a: u32, b: u32) -> f64 {
        self.relations.bond(a, b)
    }

    pub fn set_bond(&mut self, a: u32, b: u32, v: f64) {
        self.relations.set_bond(a, b, v);
    }

    /// 直接移动代理（测试与调试工具用；正常模拟中位置由行为决定）。
    /// 玩家移动会改变 LOD 层级分配，立即重算。
    pub fn teleport(&mut self, id: u32, pos: Pos) {
        if let Some(a) = self.agents.iter_mut().find(|a| a.id == id) {
            a.pos = pos;
        }
        self.grid.move_to(id, pos);
        if self.agents.iter().any(|a| a.id == id && a.is_player) {
            self.refresh_lod();
        }
    }

    /// 密友名单（亲密值 ≥ close_friend_bond，降序）——查看入口用。
    pub fn close_friends(&self, id: u32) -> Vec<(u32, String, f64)> {
        let threshold = self.cfg.params.close_friend_bond;
        let mut v: Vec<(u32, String, f64)> = self
            .relations
            .top_for(id, usize::MAX)
            .into_iter()
            .filter(|(_, b)| *b >= threshold)
            .filter_map(|(oid, b)| {
                self.agents
                    .iter()
                    .find(|a| a.id == oid)
                    .map(|a| (oid, a.name.clone(), b))
            })
            .collect();
        v.sort_by(|x, y| y.2.total_cmp(&x.2));
        v
    }

    /// 读取第 idx 个事件的下次发生拍（测试用）。
    pub fn event_next_at(&self, idx: usize) -> Tick {
        self.event_schedule[idx]
    }

    /// 手动排程第 idx 个事件到指定拍（测试用）。
    pub fn schedule_event_at(&mut self, idx: usize, at: Tick) {
        self.event_schedule[idx] = at;
    }

    /// 代理的当前位置。
    pub fn agent_pos(&self, id: u32) -> Option<Pos> {
        self.agents.iter().find(|a| a.id == id).map(|a| a.pos)
    }

    /// 手动触发第 idx 个环境事件于指定位置（测试与调试工具用；
    /// 与随机排程走完全相同的应用逻辑，含地点声誉冲击）。
    pub fn trigger_env_event(&mut self, idx: usize, pos: Pos) {
        let quarter = quarter_of_day(self.tick);
        let mut events = Vec::new();
        let tick = self.tick;
        let def = self.cfg.env_events[idx].clone();
        events.push(LogEvent::EnvEvent {
            tick,
            event: def.id.clone(),
            pos,
        });
        let had_direct_victims = self.apply_env_event(idx, pos, quarter, &mut events);
        self.area_reputation_hit(&def, had_direct_victims, tick, &mut events);
        self.emit(events);
    }

    /// 区域事件的口碑冲击：效价×强度进声誉统计（alpha 放大——
    /// 一次吓人事件顶多次平淡到访）。
    fn area_reputation_hit(
        &mut self,
        def: &crate::config::EnvEventDef,
        had_direct_victims: bool,
        tick: Tick,
        events: &mut Vec<LogEvent>,
    ) {
        if let (Some(aid), true) = (&def.area, had_direct_victims) {
            if let Some(state) = self.area_states.get_mut(aid) {
                let exp = def.valence * def.intensity;
                let alpha = self.cfg.params.area_exp_alpha * 2.5;
                let value = state.experience(exp, alpha);
                events.push(LogEvent::ReputationChange {
                    tick,
                    area: aid.clone(),
                    experience: exp,
                    value,
                });
            }
        }
    }

    /// 代理当前的安全感（0..comfort_max）：身边最近的、亲密值达标的同伴决定。
    pub fn agent_comfort(&self, id: u32) -> f64 {
        let a = match self.agents.iter().find(|a| a.id == id) {
            Some(a) => a,
            None => return 0.0,
        };
        comfort_of(&self.grid, &self.relations, &self.cfg.params, a)
    }

    /// 计算某代理当前的候选行为评估（测试与调试用）。
    pub fn debug_options(&mut self, id: u32) -> Vec<ChoiceInfo> {
        let tick = self.tick;
        let comfort = self.agent_comfort(id);
        if let Some(a) = self.agents.iter_mut().find(|a| a.id == id) {
            compute_options(
                &self.cfg,
                &self.behaviors,
                &self.spots,
                &self.area_states,
                a,
                tick,
                comfort,
            )
        } else {
            Vec::new()
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

    /// 事件落日志 + 最近事件缓冲。
    fn emit(&mut self, events: Vec<LogEvent>) {
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

    /// 单条事件直接落日志与最近事件缓冲。指令与目标操作发生在 tick 之外，
    /// 走这里——它们也应出现在交互工具的实时反馈里。
    fn note(&mut self, ev: LogEvent) {
        self.logger.log(&ev);
        if self.collect_recent {
            self.recent_events.push(ev);
            if self.recent_events.len() > 512 {
                self.recent_events.drain(0..self.recent_events.len() - 512);
            }
        }
    }

    pub fn tick(&mut self) {
        self.tick += 1;
        let tick = self.tick;
        let mut events: Vec<LogEvent> = Vec::new();

        // 日界：记忆习惯化回落与强度消退、地点整洁度恢复（清洁服务）、
        // T2 背景代理的按天批量结算（LOD，M4）。
        if is_day_boundary(tick) {
            for a in &mut self.agents {
                a.memory.daily_decay();
            }
            let clean = self.cfg.params.clean_daily;
            for state in self.area_states.values_mut() {
                state.daily_clean(clean);
            }
            self.settle_background_day(&mut events);
        }

        // 需求衰减（每拍 = 每模拟分钟）。T2 背景代理跳过逐拍衰减，
        // 改在日界批量结算时一次扣一整天（LOD：统计近似，需求在时段间静止）。
        let decay = self.cfg.needs.clone();
        for a in &mut self.agents {
            if a.lod != 2 {
                a.needs.decay(&decay, 1);
            }
        }

        // 环境事件：收集到期者，按定义顺序应用（确定性顺序）。
        let due: Vec<usize> = self
            .event_schedule
            .iter()
            .enumerate()
            .filter(|(_, at)| **at == tick)
            .map(|(i, _)| i)
            .collect();
        for i in due {
            let def = self.cfg.env_events[i].clone();
            let hour = hour_of(tick) as u32;
            // 时间窗事件不在窗内：跳到下一个窗口起点再触发（昼夜节律）。
            if !def.in_window(hour) {
                let from = def.from_hour.unwrap_or(0);
                self.event_schedule[i] = crate::time::next_window_tick(tick, from);
                continue;
            }
            let quarter = quarter_of_day(tick);
            // 事件发生位置：区域事件在区域矩形内采样，全图事件均匀采样，
            // 全域事件（radius 0 且无区域）记地图中心。
            let pos = if let Some(aid) = &def.area {
                let a = self
                    .cfg
                    .areas
                    .iter()
                    .find(|a| &a.id == aid)
                    .expect("校验已保证区域存在");
                (
                    self.world_rng.uniform(a.x, a.x + a.w),
                    self.world_rng.uniform(a.y, a.y + a.h),
                )
            } else if def.radius > 0.0 {
                (
                    self.world_rng.uniform(0.0, self.cfg.params.map_width),
                    self.world_rng.uniform(0.0, self.cfg.params.map_height),
                )
            } else {
                (
                    self.cfg.params.map_width / 2.0,
                    self.cfg.params.map_height / 2.0,
                )
            };
            events.push(LogEvent::EnvEvent {
                tick,
                event: def.id.clone(),
                pos,
            });
            let had_direct_victims = self.apply_env_event(i, pos, quarter, &mut events);
            // 地点声誉（M3）：区域内发生的显著事件冲击口碑。
            self.area_reputation_hit(&def, had_direct_victims, tick, &mut events);
            self.event_schedule[i] = tick
                + self
                    .world_rng
                    .uniform_int(def.interval_min, def.interval_max);
        }

        // 小时边界：LOD 重分配（确定性函数）、习惯衰减（含目标压制）、
        // 限期目标到期结算、意志力缓慢回复、状态快照、地点状态快照。
        if is_hour_boundary(tick) {
            self.refresh_lod();
            let rate = self.cfg.params.habit_decay_hourly;
            let gmult = self.cfg.params.goal_decay_mult;
            for a in &mut self.agents {
                a.habit_decay(rate);
                a.goal_hourly_suppress(rate, gmult);
                a.willpower = (a.willpower + 0.2).min(100.0);
                if self.logger.enabled {
                    events.push(LogEvent::Snapshot {
                        tick,
                        agent: a.id,
                        needs: a.needs,
                        weights: a.weights.clone(),
                        memories: a.memory.entries.len(),
                        willpower: a.willpower,
                        lod: a.lod,
                    });
                }
            }
            // 限期目标到期：零违约 = 成功（压制已转化为自然衰减，习惯真的改掉了）。
            for ai in 0..self.agents.len() {
                let ended: Vec<usize> = self.agents[ai]
                    .goals
                    .iter()
                    .enumerate()
                    .filter(|(_, g)| !g.is_active(tick))
                    .map(|(i, _)| i)
                    .collect();
                for gi in ended.into_iter().rev() {
                    let g = self.agents[ai].goals.remove(gi);
                    events.push(LogEvent::GoalEnd {
                        tick,
                        agent: self.agents[ai].id,
                        behavior: g.behavior.clone(),
                        success: g.violations == 0,
                        abandoned: false,
                        violations: g.violations,
                        resists: g.resists,
                        rebound: 0.0,
                    });
                }
            }
            if self.logger.enabled {
                for (aid, state) in &self.area_states {
                    events.push(LogEvent::LocationState {
                        tick,
                        area: aid.clone(),
                        reputation: state.reputation,
                        cleanliness: state.cleanliness,
                        visits: state.visits,
                    });
                }
            }
        }

        // 动作完成结算：需求影响、习惯强化、睡眠恢复意志力、社交亲密增长。
        let alpha = self.cfg.params.alpha;
        let completed: Vec<(usize, String)> = self
            .agents
            .iter()
            .enumerate()
            .filter(|(_, a)| matches!(&a.current, Some((_, _, end)) if tick >= *end))
            .map(|(i, a)| (i, a.current.as_ref().unwrap().0.clone()))
            .collect();
        for (idx, bid) in completed {
            let def = self.behaviors.get(&bid).cloned();
            // 社交结算前先（只读）找同地点的同伴。
            // 一次社交时光最多与 3 位同伴结缘（按编号序）：密集世界里猫的
            // 注意力有限，也避免人人相识导致的 O(度数) 关系爆炸。
            let mut companions: Vec<(u32, bool)> = Vec::new();
            if let Some(d) = &def {
                if d.tags.iter().any(|t| t == "social") {
                    if let Some(spot_id) = &d.spot {
                        let spot_pos = self.spots.get(spot_id).map(|s| (s.x, s.y));
                        if let Some(sp) = spot_pos {
                            for oid in self.grid.query_radius(sp, 0.1) {
                                if oid == idx as u32 || companions.len() >= 3 {
                                    continue;
                                }
                                let o = &self.agents[oid as usize];
                                let same_spot = o
                                    .current
                                    .as_ref()
                                    .map(|(b, _, _)| {
                                        self.behaviors.get(b).and_then(|x| x.spot.as_deref())
                                            == Some(spot_id.as_str())
                                    })
                                    .unwrap_or(false);
                                if same_spot {
                                    companions.push((o.id, o.is_player));
                                }
                            }
                        }
                    }
                }
            }
            let a = &mut self.agents[idx];
            if let Some(def) = &def {
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
            // 完成后的下次心跳：T1 常规代理慢一拍（LOD）；T0 维持灵敏。
            a.next_heartbeat = tick
                + if a.lod == 1 {
                    a.rng_decision.uniform_int(
                        self.cfg.params.lod_t1_heartbeat_min,
                        self.cfg.params.lod_t1_heartbeat_max,
                    )
                } else {
                    2 + a.rng_decision.uniform_int(0, 6)
                };
            // M4 暴露恢复（设计稿 4.6）：完成"正在被恐惧抑制"的行为一次——
            // 该行为的记忆乘数低于恐惧门槛（角色是硬着头皮做的）——相关负面
            // 记忆强度消退。反复低强度尝试恢复的是功能而非删除记忆：乘数回到
            // 门槛以上后暴露不再触发，恐惧停留在"不再碍事"的水平。
            // 亲近同伴在身边（安全感 > 0）时恢复更快（朋友的鼓励）。
            if let Some(def) = &def {
                let comfort = comfort_of(&self.grid, &self.relations, &self.cfg.params, a);
                let spot_tags: Vec<String> = def
                    .spot
                    .as_ref()
                    .and_then(|s| self.spots.get(s))
                    .map(|s| s.tags.clone())
                    .unwrap_or_default();
                let mem_map = a.memory.query(quarter_of_day(tick), &spot_tags, comfort);
                let mm: f64 = def
                    .tags
                    .iter()
                    .map(|t| mem_map.get(t).copied().unwrap_or(1.0))
                    .product::<f64>()
                    .clamp(0.05, 20.0);
                if mm < self.cfg.params.fear_mem_mult {
                    let faded =
                        a.memory
                            .expose(&def.tags, self.cfg.params.exposure_decay, comfort > 0.05);
                    if faded > 0.005 {
                        events.push(LogEvent::Exposure {
                            tick,
                            agent: a.id,
                            behavior: bid.clone(),
                            faded,
                        });
                    }
                }
            }
            events.push(LogEvent::ActionEnd {
                tick,
                agent: a.id,
                behavior: bid,
            });
            // M2：共同度过社交时光 → 亲密值增长（涉及玩家的陪伴更用心）。
            if !companions.is_empty() {
                let me_id = a.id;
                let me_player = a.is_player;
                for (oid, other_player) in companions {
                    let gain = if me_player || other_player {
                        self.cfg.params.bond_player_gain
                    } else {
                        self.cfg.params.bond_social_gain
                    };
                    let value = self.relations.add_bond(me_id, oid, gain);
                    events.push(LogEvent::BondChange {
                        tick,
                        a: me_id,
                        b: oid,
                        delta: gain,
                        value,
                    });
                }
            }
            // M3 地点声誉：行为完成 = 一次到访体验。体验 = 需求净值（饿了吃饭
            // 是好体验），到访同时留下脏乱。正体验拉高声誉、负体验压低——
            // 衰退与恢复由同一机制产生（设计稿 5.1）。
            if let Some(d) = &def {
                let area_id = d
                    .spot
                    .as_ref()
                    .and_then(|s| self.spots.get(s))
                    .and_then(|s| s.area.clone());
                if let Some(aid) = area_id {
                    if let Some(state) = self.area_states.get_mut(&aid) {
                        let exp = behavior_experience(d);
                        let value = state.experience(exp, self.cfg.params.area_exp_alpha);
                        let litter_scale = self
                            .cfg
                            .areas
                            .iter()
                            .find(|a| a.id == aid)
                            .map(|a| a.litter_scale)
                            .unwrap_or(1.0);
                        state.litter(self.cfg.params.visit_litter * litter_scale);
                        if self.logger.enabled {
                            events.push(LogEvent::ReputationChange {
                                tick,
                                area: aid.clone(),
                                experience: exp,
                                value,
                            });
                        }
                    }
                }
            }
        }

        // 心跳决策（空闲且到期的代理；字段级借用避免冲突）。
        // T2 背景代理不逐拍决策（LOD）：它们的每一天在日界批量结算。
        let agents = &mut self.agents;
        let cfg = &self.cfg;
        let behaviors = &self.behaviors;
        let spots = &self.spots;
        let area_states = &self.area_states;
        let relations = &self.relations;
        let grid = &mut self.grid;
        for a in agents.iter_mut() {
            if a.lod == 2 || a.current.is_some() || tick < a.next_heartbeat {
                continue;
            }
            let comfort = comfort_of(grid, relations, &cfg.params, a);
            events.extend(decide(cfg, behaviors, spots, area_states, a, tick, comfort));
            self.decisions_done += 1;
            // 行为开始 → 移动到行为地点（无地点行为原地不动）。
            if let Some((bid, _, _)) = &a.current {
                if let Some(spot_id) = behaviors.get(bid).and_then(|d| d.spot.clone()) {
                    if let Some(sd) = spots.get(&spot_id) {
                        a.pos = (sd.x, sd.y);
                        grid.move_to(a.id, a.pos);
                    }
                }
            }
        }

        self.emit(events);
    }

    /// 应用一个环境事件于指定位置：直接经历写入、惊吓反应、目睹传播。
    /// 返回是否有直接受影响的代理（供地点声誉结算判定）。
    fn apply_env_event(
        &mut self,
        idx: usize,
        pos: Pos,
        quarter: u8,
        events: &mut Vec<LogEvent>,
    ) -> bool {
        let def = self.cfg.env_events[idx].clone();
        let tick = self.tick;
        let tag_map: BTreeMap<String, Vec<String>> = self
            .behaviors
            .iter()
            .map(|(k, v)| (k.clone(), v.tags.clone()))
            .collect();
        let affected: Vec<u32> = if def.radius > 0.0 {
            self.grid.query_radius(pos, def.radius)
        } else {
            self.agents.iter().map(|a| a.id).collect()
        };
        // 事件情境标签 = 事件自身标签 + 影响半径内地点的特征标签。
        // "在纸箱附近被吸尘器吓到"记住的是那个角落的特征——
        // 恐惧泛化到带相似特征的其他地点（设计稿 3.2 泛化）。
        let ctx_tags: Vec<String> = if def.radius > 0.0 {
            let mut t = def.tags.clone();
            for s in self.spots.values() {
                if crate::space::dist((s.x, s.y), pos) <= def.radius {
                    t.extend(s.tags.clone());
                }
            }
            t.sort();
            t.dedup();
            t
        } else {
            def.tags.clone()
        };
        // 反应明显（直接显著性 ≥ 0.3）的遭遇，会被附近的同伴目睹。
        let affected_set: std::collections::HashSet<u32> = affected.iter().copied().collect();
        let mut notable: Vec<u32> = Vec::new();
        let mut had_direct_victims = false;
        for a in &mut self.agents {
            if !affected_set.contains(&a.id) {
                continue;
            }
            let immune = def.immune_behavior_tags.iter().any(|t| {
                a.current
                    .as_ref()
                    .map(|(bid, _, _)| tag_map.get(bid).map(|ts| ts.contains(t)).unwrap_or(false))
                    .unwrap_or(false)
            });
            if immune {
                continue;
            }
            let salience = a.memory.write(
                &def.id,
                &ctx_tags,
                quarter,
                def.intensity,
                1.0,
                def.valence,
                false,
                def.behavior_mults.clone(),
                tick,
            );
            if salience >= 0.05 {
                had_direct_victims = true;
                events.push(LogEvent::MemoryWrite {
                    tick,
                    agent: a.id,
                    event: def.id.clone(),
                    salience,
                    mults: def.behavior_mults.clone(),
                    source: None,
                    vicarious: false,
                });
            }
            // 惊吓：足够新颖的强负面事件打断当前行为，本拍稍后立即重新决策
            // （习惯化后显著性低于门槛，猫学会对它淡定，不再被打断）。
            if def.valence <= -0.5
                && salience >= self.cfg.params.startle_min_salience
                && a.current.is_some()
            {
                a.current = None;
                a.next_heartbeat = tick;
                events.push(LogEvent::Startled {
                    tick,
                    agent: a.id,
                    event: def.id.clone(),
                });
            }
            if salience >= 0.3 {
                notable.push(a.id);
            }
        }
        // 模仿节点：目睹亲近同伴的显著遭遇，按亲密值折扣写入间接记忆。
        // T1 常规代理只留意更近处的动静（LOD：事件监听范围小）。
        let witness_radius = self.cfg.params.witness_radius;
        let t1_scale = self.cfg.params.lod_t1_witness_scale;
        let imitation = self.cfg.params.imitation_factor;
        let min_bond = self.cfg.params.imitation_min_bond;
        for vid in notable {
            let vpos = self.grid.pos(vid).unwrap_or((0.0, 0.0));
            let witness_set: std::collections::HashSet<u32> = self
                .grid
                .query_radius(vpos, witness_radius)
                .into_iter()
                .collect();
            for a in &mut self.agents {
                if a.id == vid || !witness_set.contains(&a.id) {
                    continue;
                }
                let wr = if a.lod == 1 {
                    witness_radius * t1_scale
                } else {
                    witness_radius
                };
                if dist(a.pos, vpos) > wr {
                    continue;
                }
                let bond = self.relations.bond(a.id, vid);
                if bond < min_bond {
                    continue; // 不够亲近的同伴，它的遭遇不值得一学
                }
                let ev_name = crate::memory::vicarious_event(&def.id);
                let intensity = def.intensity * bond * imitation;
                let s = a.memory.write(
                    &ev_name,
                    &ctx_tags,
                    quarter,
                    intensity,
                    1.0,
                    def.valence,
                    true,
                    def.behavior_mults.clone(),
                    tick,
                );
                if s >= 0.05 {
                    events.push(LogEvent::MemoryWrite {
                        tick,
                        agent: a.id,
                        event: ev_name,
                        salience: s,
                        mults: def.behavior_mults.clone(),
                        source: Some(vid),
                        vicarious: true,
                    });
                }
            }
        }
        had_direct_victims
    }

    /// T2 背景代理的按天批量结算（M4，LOD）：替代逐拍决策的统计近似。
    /// 每个时段（凌晨/上午/下午/夜晚的代表小时）做一次简化选择——需求门
    /// 放行的行为里取高权重前 K；需求临界时强制恢复（生理反射的批量版），
    /// 立即结算效果、习惯与到访体验。记忆查询跳过（这正是 LOD 省的大头），
    /// 但记忆、关系、习惯权重全保留——升层后"这些年"的经历都在。
    fn settle_background_day(&mut self, events: &mut Vec<LogEvent>) {
        let top_k = self.cfg.params.lod_t2_top_k;
        let critical = self.cfg.critical_need;
        let alpha = self.cfg.params.alpha;
        for idx in 0..self.agents.len() {
            if self.agents[idx].lod != 2 {
                continue;
            }
            for hour in [3u32, 9, 15, 21] {
                // 本时段前先扣掉一整天的需求衰减（T2 不做逐拍衰减；
                // 升降层切换日的几小时误差属于统计近似的固有精度）。
                if hour == 3 {
                    self.agents[idx].needs.decay(&self.cfg.needs, crate::time::TICKS_PER_DAY);
                }
                // 简化树：门放行的行为（需求门 + 时间门，与完整树同规）。
                let mut ids: Vec<&str> = Vec::new();
                available_behaviors(
                    self.cfg.tree_for(&self.agents[idx].species),
                    &self.agents[idx].needs,
                    &self.behaviors,
                    hour,
                    &mut ids,
                );
                let avail: Vec<String> = ids.into_iter().map(|s| s.to_string()).collect();
                if avail.is_empty() {
                    continue;
                }
                // 生理反射批量版：任一需求临界 → 可用的最强恢复行为。
                let forced = NEED_KEYS
                    .iter()
                    .find(|need| {
                        self.agents[idx].needs.get(need).unwrap_or(100.0) < critical
                    })
                    .and_then(|need| {
                        avail
                            .iter()
                            .find(|b| self.behaviors[*b].effect_on(need) > 0.0)
                            .cloned()
                    });
                let choice = match forced {
                    Some(b) => b,
                    None => {
                        // 高权重前 K 里加权抽一个（跳过记忆查询的简化选择）。
                        let a = &self.agents[idx];
                        let mut scored: Vec<(String, f64)> = avail
                            .into_iter()
                            .map(|b| {
                                let def = &self.behaviors[&b];
                                let w = a.weights.get(&b).copied().unwrap_or(def.base_weight);
                                let drive = def.drive_need().unwrap_or("fun");
                                let nm = if def.effect_on(drive) > 0.0 {
                                    urgency(a.needs.get(drive).unwrap_or(100.0))
                                } else {
                                    0.6
                                };
                                (b, w * nm)
                            })
                            .collect();
                        scored.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
                        scored.truncate(top_k.max(1));
                        let effs: Vec<f64> = scored.iter().map(|(_, e)| *e).collect();
                        let temp = self.agents[idx].temperature;
                        let pick = softmax_pick(
                            &effs,
                            temp,
                            &mut self.agents[idx].rng_decision,
                        );
                        scored.swap_remove(pick).0
                    }
                };
                // 立即结算：需求效果、意志力恢复、习惯强化、到访体验。
                let def = self.behaviors[&choice].clone();
                let a = &mut self.agents[idx];
                for (need, delta) in &def.effects {
                    a.needs.add(need, *delta);
                }
                if def.tags.iter().any(|t| t == "sleep") {
                    a.willpower = (a.willpower + 25.0).min(100.0);
                }
                a.habit_boost(&choice, alpha);
                a.record_exec(&choice);
                let area_id = def
                    .spot
                    .as_ref()
                    .and_then(|s| self.spots.get(s))
                    .and_then(|s| s.area.clone());
                if let Some(aid) = area_id {
                    if let Some(state) = self.area_states.get_mut(&aid) {
                        let exp = behavior_experience(&def);
                        state.experience(exp, self.cfg.params.area_exp_alpha);
                        let litter_scale = self
                            .cfg
                            .areas
                            .iter()
                            .find(|a| a.id == aid)
                            .map(|a| a.litter_scale)
                            .unwrap_or(1.0);
                        state.litter(self.cfg.params.visit_litter * litter_scale);
                        if self.logger.enabled {
                            events.push(LogEvent::ReputationChange {
                                tick: self.tick,
                                area: aid.clone(),
                                experience: exp,
                                value: state.reputation,
                            });
                        }
                    }
                }
            }
        }
    }

    // ---------- 玩家指令（意志节点） ----------

    /// 下达指令：校验可用性、计算意志力成本、写日志、排入下一拍执行。
    /// 行为树从不否决指令，只让违背习惯的指令付出更高代价；
    /// 意志力不足时指令被拒绝（猫赖着不动）。
    pub fn issue_command(&mut self, agent_id: u32, behavior: &str) -> CommandOutcome {
        let tick = self.tick;
        if !self.agents.iter().any(|a| a.id == agent_id && a.is_player) {
            return CommandOutcome {
                accepted: false,
                cost: 0.0,
                reason: if self.agents.iter().any(|a| a.id == agent_id) {
                    "只能指挥玩家角色".into()
                } else {
                    "没有这个角色".into()
                },
            };
        }
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
        let comfort = self.agent_comfort(agent_id);
        let a = self.agents.iter_mut().find(|a| a.id == agent_id).unwrap();
        let options = compute_options(
            &self.cfg,
            &self.behaviors,
            &self.spots,
            &self.area_states,
            a,
            tick,
            comfort,
        );
        // 生理反射压倒一切：临界需求未解决时，只接受对应的恢复行为。
        let forced = forced_choice(&self.cfg, &self.behaviors, &a.needs, &options);
        // 定价（M4 完整意志系统）：
        // - 基准 5 + 20×(1−合拍度)；
        // - 习惯自动化（权重达阈值）→ 零消耗，习惯成自然；
        // - 恐惧抑制（记忆乘数低于门槛）→ 加价：过心理关要花力气；
        // - 戒断目标进行中 → 违约加价（行为树从不禁止指令，只收代价）。
        let auto_ratio = self.cfg.params.habit_auto_ratio;
        let fear_gate = self.cfg.params.fear_mem_mult;
        let (mut cost, refusal, automated, feared) = match options
            .iter()
            .find(|o| o.behavior == behavior)
        {
            Some(o) => {
                let eff_max = options
                    .iter()
                    .map(|o| o.eff)
                    .fold(f64::NEG_INFINITY, f64::max);
                let ratio = if eff_max > 0.0 {
                    (o.eff / eff_max).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let automated = a.is_automated(behavior, auto_ratio);
                let base = if automated {
                    0.0
                } else {
                    5.0 + 20.0 * (1.0 - ratio)
                };
                let feared = o.mem_mult < fear_gate;
                (
                    base + if feared {
                        self.cfg.params.fear_command_cost
                    } else {
                        0.0
                    },
                    None,
                    automated,
                    feared,
                )
            }
            None if forced.as_deref() == Some(behavior) => (3.0, None, false, false),
            None if forced.is_some() => (
                0.0,
                Some("它生理需求告急，现在听不进别的".to_string()),
                false,
                false,
            ),
            None => (
                0.0,
                Some(format!(
                    "它现在没这个念头（{}）",
                    gate_note(&self.cfg, &def)
                )),
                false,
                false,
            ),
        };
        if let Some(reason) = refusal {
            let out = CommandOutcome {
                accepted: false,
                cost: 0.0,
                reason,
            };
            self.note(LogEvent::PlayerCommand {
                tick,
                agent: agent_id,
                behavior: behavior.to_string(),
                cost: 0.0,
                accepted: false,
                reason: out.reason.clone(),
            });
            return out;
        }
        // 戒断目标进行中：下令即违约（额外代价 + 违约计数）。
        let goal_break = a.active_goal_idx(behavior, tick).is_some();
        if goal_break {
            cost += self.cfg.params.goal_break_cost;
        }
        // 打断当前行为额外收费（自动化习惯也不例外——打断谁都要费劲）。
        let interrupting = a.current.is_some();
        let cost = (cost + if interrupting { 8.0 } else { 0.0 }).round();
        if a.willpower + 1e-9 < cost {
            let out = CommandOutcome {
                accepted: false,
                cost,
                reason: format!(
                    "意志力不足：需要 {cost:.0}，当前 {:.0}，它赖着没动",
                    a.willpower
                ),
            };
            self.note(LogEvent::PlayerCommand {
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
        let mut notes: Vec<LogEvent> = Vec::new();
        if goal_break {
            if let Some(gi) = a.active_goal_idx(behavior, tick) {
                a.goals[gi].violations += 1;
                notes.push(LogEvent::GoalViolation {
                    tick,
                    agent: agent_id,
                    behavior: behavior.to_string(),
                    commanded: true,
                });
            }
        }
        if interrupting {
            if let Some((bid, _, _)) = a.current.take() {
                notes.push(LogEvent::ActionEnd {
                    tick,
                    agent: agent_id,
                    behavior: bid,
                });
            }
        }
        a.next_heartbeat = tick + 1;
        a.pending_command = Some(behavior.to_string());
        let reaction = if goal_break {
            "它违背了对自己的承诺，一脸愧疚地去了"
        } else if feared {
            "它心里发怵，磨蹭了半天还是硬着头皮去了"
        } else if automated {
            "习惯成自然，它自己就去了"
        } else if cost <= 6.0 {
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
        for ev in notes {
            self.note(ev);
        }
        self.note(LogEvent::PlayerCommand {
            tick,
            agent: agent_id,
            behavior: behavior.to_string(),
            cost,
            accepted: true,
            reason: out.reason.clone(),
        });
        out
    }

    /// 菜单状态：每个行为的可用性、预计意志力消耗（与 issue_command 同价，
    /// 不含打断费）、门未开的提示、M4 标记（习惯自动化 / 恐惧警示 / 戒断目标）。
    /// 只列该代理物种树上的行为（多物种同街时菜单不串树）。
    pub fn behaviors_status(&mut self, agent_id: u32) -> Vec<BehaviorStatus> {
        let species = match self.agents.iter().find(|a| a.id == agent_id) {
            Some(a) => a.species.clone(),
            None => return vec![],
        };
        let a = match self.agents.iter_mut().find(|a| a.id == agent_id) {
            Some(a) => a,
            None => return vec![],
        };
        let tick = self.tick;
        let comfort = comfort_of(&self.grid, &self.relations, &self.cfg.params, a);
        let options = compute_options(
            &self.cfg,
            &self.behaviors,
            &self.spots,
            &self.area_states,
            a,
            tick,
            comfort,
        );
        let eff_max = options
            .iter()
            .map(|o| o.eff)
            .fold(f64::NEG_INFINITY, f64::max);
        let auto_ratio = self.cfg.params.habit_auto_ratio;
        let fear_gate = self.cfg.params.fear_mem_mult;
        let fear_cost = self.cfg.params.fear_command_cost;
        let goal_cost = self.cfg.params.goal_break_cost;
        self.cfg
            .species_behaviors(&species)
            .into_iter()
            .map(|(bid, _)| {
                let def = &self.behaviors[&bid];
                match options.iter().find(|o| o.behavior == bid) {
                    Some(o) => {
                        let ratio = if eff_max > 0.0 {
                            (o.eff / eff_max).clamp(0.0, 1.0)
                        } else {
                            1.0
                        };
                        let automated = a.is_automated(&bid, auto_ratio);
                        let feared = o.mem_mult < fear_gate;
                        let goal_avoided = a.active_goal_idx(&bid, tick).is_some();
                        let cost = (if automated {
                            0.0
                        } else {
                            5.0 + 20.0 * (1.0 - ratio)
                        } + if feared { fear_cost } else { 0.0 }
                            + if goal_avoided { goal_cost } else { 0.0 })
                            .round();
                        BehaviorStatus {
                            id: bid.clone(),
                            name: def.display_name().to_string(),
                            available: true,
                            est_cost: cost,
                            gate_note: String::new(),
                            automated,
                            feared,
                            goal_avoided,
                        }
                    }
                    None => BehaviorStatus {
                        id: bid.clone(),
                        name: def.display_name().to_string(),
                        available: false,
                        est_cost: 0.0,
                        gate_note: gate_note(&self.cfg, def),
                        automated: a.is_automated(&bid, auto_ratio),
                        feared: false,
                        goal_avoided: a.active_goal_idx(&bid, tick).is_some(),
                    },
                }
            })
            .collect()
    }

    // ---------- M4 玩家的人生：限期目标 / 习惯报告 / 密友查看 ----------

    /// 给玩家角色设一个戒断目标（设计稿 4.5）：`days` 天内不做 `behavior`。
    /// 只能指挥玩家角色；同一行为同时只有一个目标。
    pub fn set_goal(&mut self, agent_id: u32, behavior: &str, days: u64) -> Result<(), String> {
        let tick = self.tick;
        let a = self
            .agents
            .iter_mut()
            .find(|a| a.id == agent_id && a.is_player)
            .ok_or("只能给玩家角色定目标")?;
        if !a.weights.contains_key(behavior) {
            return Err(format!(
                "它不会做【{behavior}】（不在它的行为树上）"
            ));
        }
        if a.goals.iter().any(|g| g.is_active(tick) && g.behavior == behavior) {
            return Err(format!("【{behavior}】已有进行中的目标"));
        }
        let deadline = tick + days * crate::time::TICKS_PER_DAY;
        a.goals.push(Goal::new(behavior, tick, deadline));
        self.note(LogEvent::GoalSet {
            tick,
            agent: agent_id,
            behavior: behavior.to_string(),
            deadline,
        });
        Ok(())
    }

    /// 中途放弃目标：suppressed 还回权重（旧习惯立刻反弹），返回 (行为, 反弹量)。
    pub fn abandon_goal(&mut self, agent_id: u32, behavior: &str) -> Result<(String, f64), String> {
        let tick = self.tick;
        let a = self
            .agents
            .iter_mut()
            .find(|a| a.id == agent_id)
            .ok_or("没有这个角色")?;
        let gi = a
            .goals
            .iter()
            .position(|g| g.behavior == behavior && g.is_active(tick))
            .ok_or(format!("【{behavior}】没有进行中的目标"))?;
        let rebound = a.goal_rebound(gi);
        let g = a.goals.remove(gi);
        self.note(LogEvent::GoalEnd {
            tick,
            agent: agent_id,
            behavior: g.behavior.clone(),
            success: false,
            abandoned: true,
            violations: g.violations,
            resists: g.resists,
            rebound,
        });
        Ok((g.behavior, rebound))
    }

    /// 目标列表的进度状态（界面显示用）。
    pub fn goal_status(&self, agent_id: u32) -> Vec<GoalStatus> {
        let Some(a) = self.agents.iter().find(|a| a.id == agent_id) else {
            return vec![];
        };
        a.goals
            .iter()
            .filter(|g| g.is_active(self.tick))
            .map(|g| GoalStatus {
                behavior: g.behavior.clone(),
                days_left: ((g.deadline - self.tick) as f64 / 1440.0).ceil() as u64,
                violations: g.violations,
                resists: g.resists,
                suppressed: g.suppressed,
            })
            .collect()
    }

    /// 习惯报告（M4 验收载体）：该角色每个行为的权重强度、是否自动化、
    /// 以及相关的记忆条目——"养成某习惯，因为某经历"的解释与数据一致。
    pub fn habit_report(&self, agent_id: u32) -> Vec<HabitReport> {
        let Some(a) = self.agents.iter().find(|a| a.id == agent_id) else {
            return vec![];
        };
        let auto_ratio = self.cfg.params.habit_auto_ratio;
        let mut out: Vec<HabitReport> = a
            .weights
            .iter()
            .map(|(bid, w)| {
                let base = a.base_weights.get(bid).copied().unwrap_or(*w);
                let tags = self
                    .behaviors
                    .get(bid)
                    .map(|d| d.tags.clone())
                    .unwrap_or_default();
                let mut rel: Vec<(f64, String)> = a
                    .memory
                    .entries
                    .iter()
                    .filter(|m| m.mults.keys().any(|t| tags.contains(t)))
                    .map(|m| {
                        (
                            m.strength,
                            format!(
                                "{}(时段{}，强度{:.2}，{})",
                                m.event,
                                m.quarter,
                                m.strength,
                                if m.valence < 0.0 { "负面" } else { "正面" }
                            ),
                        )
                    })
                    .collect();
                rel.sort_by(|x, y| y.0.total_cmp(&x.0));
                HabitReport {
                    behavior: bid.clone(),
                    weight: *w,
                    base,
                    ratio: w / base.max(1e-9),
                    execs: a.exec_counts.get(bid).copied().unwrap_or(0),
                    automated: a.is_automated(bid, auto_ratio),
                    related_memories: rel.into_iter().take(2).map(|(_, s)| s).collect(),
                }
            })
            .collect();
        out.sort_by(|x, y| y.ratio.total_cmp(&x.ratio));
        out
    }

    /// 密友查看（设计稿 4.7）：亲密值达到 close_friend_bond 才能看见对方的
    /// 部分心理状态——需求与正在生效的负面记忆。不够亲近只能看到表面。
    pub fn inspect_friend(&self, viewer_id: u32, target_id: u32) -> Result<FriendView, String> {
        let (_viewer, t) = match (
            self.agents.iter().find(|a| a.id == viewer_id),
            self.agents.iter().find(|a| a.id == target_id),
        ) {
            (Some(a), Some(t)) => (a, t),
            _ => return Err("没有这个角色".into()),
        };
        if viewer_id == target_id {
            return Err("这是它自己（用状态面板看）".into());
        }
        let bond = self.relations.bond(viewer_id, target_id);
        let threshold = self.cfg.params.close_friend_bond;
        if bond < threshold {
            return Err(format!(
                "你们还不够亲近（亲密 {bond:.2} < {threshold:.2}），看不出它在想什么"
            ));
        }
        let mut fears: Vec<(String, f64)> = t
            .memory
            .entries
            .iter()
            .filter(|m| m.valence < 0.0 && m.strength > 0.1)
            .map(|m| (m.event.clone(), m.strength))
            .collect();
        fears.sort_by(|x, y| y.1.total_cmp(&x.1));
        Ok(FriendView {
            name: t.name.clone(),
            bond,
            needs: t.needs,
            current: t.current.as_ref().map(|(b, _, _)| b.clone()),
            fears: fears.into_iter().take(3).collect(),
        })
    }

    /// 结束时计算汇总（行为分布、与 0 号的总变差距离、主要记忆、亲近关系），
    /// 写入日志并返回。
    pub fn summarize(&mut self) -> Vec<AgentSummary> {
        let dists: Vec<BTreeMap<String, f64>> = self
            .agents
            .iter()
            .map(|a| share_map(&a.exec_counts))
            .collect();
        let name_of = |id: u32| -> String {
            self.agents
                .iter()
                .find(|a| a.id == id)
                .map(|a| a.name.clone())
                .unwrap_or_else(|| format!("#{id}"))
        };
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
            let top_bonds: Vec<(String, f64)> = self
                .relations
                .top_for(a.id, 3)
                .into_iter()
                .map(|(id, b)| (name_of(id), b))
                .collect();
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
                top_bonds,
            });
        }
        let areas: Vec<AreaSummary> = self
            .area_states
            .iter()
            .map(|(id, s)| AreaSummary {
                id: id.clone(),
                reputation: s.reputation,
                cleanliness: s.cleanliness,
                visits: s.visits,
            })
            .collect();
        self.logger.log(&LogEvent::Summary {
            agents: summaries.clone(),
            areas,
        });
        summaries
    }
}

/// 决策核心（自由函数：cfg/behaviors/spots 只读，代理可变）。
/// 返回 [GoalResist/GoalViolation…]、[Decision, ActionStart] 日志事件。
fn decide(
    cfg: &SimConfig,
    behaviors: &BTreeMap<String, BehaviorDef>,
    spots: &BTreeMap<String, SpotDef>,
    area_states: &BTreeMap<String, AreaState>,
    a: &mut Agent,
    tick: Tick,
    comfort: f64,
) -> Vec<LogEvent> {
    let options = compute_options(cfg, behaviors, spots, area_states, a, tick, comfort);
    let auto_ratio = cfg.params.habit_auto_ratio;
    // 限期目标对抗过程中产生的事件（忍住/失守），排在 Decision 之前。
    let mut extra: Vec<LogEvent> = Vec::new();

    let (choice, factors) = if let Some(cmd) = a.pending_command.take() {
        // 第四层：玩家指令覆盖（成本已在 issue_command 结算）。
        let auto = a.is_automated(&cmd, auto_ratio);
        match options.iter().find(|o| o.behavior == cmd) {
            Some(o) => (
                cmd.clone(),
                FactorBreakdown {
                    base: o.base,
                    weight: o.weight,
                    need_mult: o.need_mult,
                    mem_mult: o.mem_mult,
                    area_mult: o.area_mult,
                    comfort,
                    forced: false,
                    will_command: true,
                    habit_auto: auto,
                },
            ),
            None => (
                cmd.clone(),
                FactorBreakdown {
                    base: behaviors.get(&cmd).map(|d| d.base_weight).unwrap_or(1.0),
                    weight: a.weights.get(&cmd).copied().unwrap_or(1.0),
                    need_mult: 1.0,
                    mem_mult: 1.0,
                    area_mult: 1.0,
                    comfort,
                    forced: false,
                    will_command: true,
                    habit_auto: auto,
                },
            ),
        }
    } else if options.is_empty() {
        // 防御：门全关时原地休息（正常配置不会走到）。
        (
            "rest".to_string(),
            FactorBreakdown {
                base: 1.0,
                weight: 1.0,
                need_mult: 1.0,
                mem_mult: 1.0,
                area_mult: 1.0,
                comfort,
                forced: true,
                will_command: false,
                habit_auto: false,
            },
        )
    } else if let Some(o) = forced_choice(cfg, behaviors, &a.needs, &options) {
        // 第一层：生理反射。
        let def = &behaviors[o.as_str()];
        let drive = def.drive_need().unwrap_or("fun");
        let info = options.iter().find(|x| x.behavior == *o).unwrap();
        (
            o.to_string(),
            FactorBreakdown {
                base: info.base,
                weight: info.weight,
                need_mult: urgency(a.needs.get(drive).unwrap_or(100.0)),
                mem_mult: 1.0,
                area_mult: info.area_mult,
                comfort,
                forced: true,
                will_command: false,
                habit_auto: a.is_automated(&o, auto_ratio),
            },
        )
    } else {
        // 第三层：加权随机选择。
        let effs: Vec<f64> = options.iter().map(|o| o.eff).collect();
        let mut idx = softmax_pick(&effs, a.temperature, &mut a.rng_decision);
        // M4 限期目标（戒断）：行为树仍选中戒断对象——玩家角色用意志力忍住
        //（换选次优）；意志力不够则失守（执行 + 违约计数）。
        // 生理反射分支不受此限（上面已 return），不饿死自己。
        if a.is_player {
            let picked = options[idx].behavior.clone();
            if let Some(gi) = a.active_goal_idx(&picked, tick) {
                let cost = cfg.params.goal_resist_cost;
                if a.willpower + 1e-9 >= cost {
                    a.willpower -= cost;
                    a.goals[gi].resists += 1;
                    extra.push(LogEvent::GoalResist {
                        tick,
                        agent: a.id,
                        behavior: picked,
                        cost,
                    });
                    // 忍住：剔除该行为重新选择。
                    let filtered: Vec<usize> = (0..options.len()).filter(|&i| i != idx).collect();
                    if filtered.is_empty() {
                        // 只剩这一个念头：忍住就是原地待一会儿。
                        (
                            "rest".to_string(),
                            FactorBreakdown {
                                base: 1.0,
                                weight: options[idx].weight,
                                need_mult: 1.0,
                                mem_mult: 1.0,
                                area_mult: 1.0,
                                comfort,
                                forced: false,
                                will_command: false,
                                habit_auto: false,
                            },
                        )
                    } else {
                        let effs2: Vec<f64> = filtered.iter().map(|&i| options[i].eff).collect();
                        idx = filtered[softmax_pick(&effs2, a.temperature, &mut a.rng_decision)];
                        let o = &options[idx];
                        (
                            o.behavior.clone(),
                            FactorBreakdown {
                                base: o.base,
                                weight: o.weight,
                                need_mult: o.need_mult,
                                mem_mult: o.mem_mult,
                                area_mult: o.area_mult,
                                comfort,
                                forced: false,
                                will_command: false,
                                habit_auto: a.is_automated(&o.behavior, auto_ratio),
                            },
                        )
                    }
                } else {
                    // 意志力耗尽，失守。
                    a.goals[gi].violations += 1;
                    extra.push(LogEvent::GoalViolation {
                        tick,
                        agent: a.id,
                        behavior: picked,
                        commanded: false,
                    });
                    let o = &options[idx];
                    (
                        o.behavior.clone(),
                        FactorBreakdown {
                            base: o.base,
                            weight: o.weight,
                            need_mult: o.need_mult,
                            mem_mult: o.mem_mult,
                            area_mult: o.area_mult,
                            comfort,
                            forced: false,
                            will_command: false,
                            habit_auto: a.is_automated(&o.behavior, auto_ratio),
                        },
                    )
                }
            } else {
                let o = &options[idx];
                (
                    o.behavior.clone(),
                    FactorBreakdown {
                        base: o.base,
                        weight: o.weight,
                        need_mult: o.need_mult,
                        mem_mult: o.mem_mult,
                        area_mult: o.area_mult,
                        comfort,
                        forced: false,
                        will_command: false,
                        habit_auto: a.is_automated(&o.behavior, auto_ratio),
                    },
                )
            }
        } else {
            let o = &options[idx];
            (
                o.behavior.clone(),
                FactorBreakdown {
                    base: o.base,
                    weight: o.weight,
                    need_mult: o.need_mult,
                    mem_mult: o.mem_mult,
                    area_mult: o.area_mult,
                    comfort,
                    forced: false,
                    will_command: false,
                    habit_auto: a.is_automated(&o.behavior, auto_ratio),
                },
            )
        }
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

    extra.push(LogEvent::Decision {
        tick,
        agent: a.id,
        temperature: a.temperature,
        factors,
        choice: choice.clone(),
    });
    extra.push(LogEvent::ActionStart {
        tick,
        agent: a.id,
        behavior: choice,
        until,
    });
    extra
}

/// 安全感：亲密值达标（comfort_min_bond）的同伴在舒适半径内，
/// 取其中最高的亲密值换算（上限 comfort_max）。无亲近同伴则为 0。
/// 只遍历"安全感来源名单"（达标的少数同伴），不扫描全部邻居。
fn comfort_of(grid: &SpatialGrid, relations: &Relations, params: &Params, a: &Agent) -> f64 {
    let mut best: f64 = 0.0;
    for (sid, bond) in relations.comfort_sources(a.id) {
        if let Some(p) = grid.pos(sid) {
            if crate::space::dist(p, a.pos) <= params.comfort_radius {
                best = best.max(bond);
            }
        }
    }
    (best * params.comfort_max).min(params.comfort_max)
}

/// 计算当前全部可用行为及其权重分解（决策、菜单、指令定价共用）。
/// 物种决定用哪棵树；同一地点的行为共享一次记忆查询与地点乘数
/// （两者都只依赖地点与时段）。
#[allow(clippy::too_many_arguments)]
fn compute_options(
    cfg: &SimConfig,
    behaviors: &BTreeMap<String, BehaviorDef>,
    spots: &BTreeMap<String, SpotDef>,
    area_states: &BTreeMap<String, AreaState>,
    a: &mut Agent,
    tick: Tick,
    comfort: f64,
) -> Vec<ChoiceInfo> {
    let mut avail: Vec<&str> = Vec::new();
    available_behaviors(
        cfg.tree_for(&a.species),
        &a.needs,
        behaviors,
        hour_of(tick) as u32,
        &mut avail,
    );
    let quarter = quarter_of_day(tick);
    // 地点 → (记忆乘数表, 地点声誉乘数)（本决策内缓存；无地点行为共用空表）。
    let mut spot_mems: Vec<(Option<&str>, BTreeMap<String, f64>, f64)> = Vec::new();
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
            let spot_id = def.spot.as_deref();
            let (mem_map, area_mult) = match spot_mems.iter_mut().find(|(s, _, _)| *s == spot_id) {
                Some((_, m, am)) => (&*m, *am),
                None => {
                    let spot_tags: Vec<String> = spot_id
                        .and_then(|s| spots.get(s))
                        .map(|s| s.tags.clone())
                        .unwrap_or_default();
                    let am = spot_id
                        .and_then(|s| spots.get(s))
                        .and_then(|s| s.area.as_deref())
                        .and_then(|aid| area_states.get(aid))
                        .map(|st| {
                            st.behavior_mult(cfg.params.area_mult_min, cfg.params.area_mult_span)
                        })
                        .unwrap_or(1.0);
                    spot_mems.push((spot_id, a.memory.query(quarter, &spot_tags, comfort), am));
                    let (_, m, am) = spot_mems.last_mut().unwrap();
                    (&*m, *am)
                }
            };
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
                area_mult,
                eff: w * nm * mm * area_mult,
            }
        })
        .collect()
}

/// 行为完成对到访地点的体验净值（需求效果求和 /100，钳制 −1..1）：
/// 饿了吃饭是 +0.5 的好体验，纯消耗的闲逛略负。
fn behavior_experience(def: &BehaviorDef) -> f64 {
    let net: f64 = def.effects.values().sum::<f64>() / 70.0;
    net.clamp(-1.0, 1.0)
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
        .map(|k| (a.get(*k).copied().unwrap_or(0.0) - b.get(*k).copied().unwrap_or(0.0)).abs())
        .sum::<f64>()
}

/// 名单转猫群 roster（全部物种 cat）。
pub fn cat_roster(names: Vec<String>) -> Vec<(String, String)> {
    names.into_iter().map(|n| (n, "cat".to_string())).collect()
}

/// 生成人类的默认名字。
pub fn human_names(n: usize) -> Vec<String> {
    const SURNAMES: [&str; 6] = ["陈", "林", "王", "赵", "苏", "何"];
    const GIVEN: [&str; 8] = [
        "晓东", "雨桐", "建国", "小满", "志强", "文静", "大海", "阿福",
    ];
    (0..n)
        .map(|i| format!("{}{}", SURNAMES[i % SURNAMES.len()], GIVEN[i % GIVEN.len()]))
        .collect()
}

/// 生成猫的默认名字。
pub fn cat_names(n: usize) -> Vec<String> {
    const NAMES: [&str; 12] = [
        "小白", "小黑", "小花", "小橘", "小灰", "小黄", "奶牛", "三花", "狸花", "狮子", "布丁",
        "雪球",
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
