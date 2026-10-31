//! JSONL 事件日志：模拟的唯一事实源。
//! 每行一个 JSON 对象，serde 派生序列化；BTreeMap 保证键序确定，日志字节级可比较。

use crate::needs::Needs;
use serde::Serialize;
use std::collections::BTreeMap;

pub const LOG_VERSION: u32 = 4;

/// 决策的因素分解：系统能"报告原因"的载体。
#[derive(Debug, Clone, Serialize)]
pub struct FactorBreakdown {
    /// 行为注册表基准权重。
    pub base: f64,
    /// 代理当前权重（含习惯累积与个体抖动）。
    pub weight: f64,
    /// 需求紧迫度乘数。
    pub need_mult: f64,
    /// 情境记忆乘数。
    pub mem_mult: f64,
    /// 地点声誉乘数（M3）：行为所在区域的口碑与整洁度对权重的修正。
    pub area_mult: f64,
    /// 安全感（M2）：0 = 身边没有可信任的同伴；越高，负面记忆被压制越多。
    pub comfort: f64,
    /// 第一层生理反射强制执行。
    pub forced: bool,
    /// 玩家意志指令覆盖（M1 意志节点）。
    pub will_command: bool,
    /// 习惯自动化（M4，设计稿 4.4）：选中行为权重已达自动化阈值——
    /// 重复行为不需要意志参与的"第二天性"。
    pub habit_auto: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum LogEvent {
    Meta {
        version: u32,
        seed: u64,
        agents: usize,
        ticks: u64,
    },
    EnvEvent {
        tick: u64,
        event: String,
        /// 事件发生位置（米）；全房间事件记录房间中心。
        pos: (f64, f64),
    },
    /// 惊吓反应（M2）：强负面事件打断了代理正在进行的行为——
    /// 这是其他代理能"目睹"的公开反应，也是模仿节点的触发源。
    Startled {
        tick: u64,
        agent: u32,
        event: String,
    },
    /// 关系变化（M2）：共同度过社交时光后亲密值增长。
    BondChange {
        tick: u64,
        a: u32,
        b: u32,
        delta: f64,
        value: f64,
    },
    Decision {
        tick: u64,
        agent: u32,
        choice: String,
        factors: FactorBreakdown,
        temperature: f64,
    },
    ActionStart {
        tick: u64,
        agent: u32,
        behavior: String,
        until: u64,
    },
    ActionEnd {
        tick: u64,
        agent: u32,
        behavior: String,
    },
    MemoryWrite {
        tick: u64,
        agent: u32,
        event: String,
        salience: f64,
        mults: BTreeMap<String, f64>,
        /// 间接记忆的来源代理（目睹了谁的遭遇）；直接经历为 None。
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<u32>,
        /// 是否间接记忆（模仿节点写入）。
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        vicarious: bool,
    },
    /// 周期性状态快照（每小时）：查看器只凭日志即可重建任意时刻状态。
    Snapshot {
        tick: u64,
        agent: u32,
        needs: Needs,
        weights: BTreeMap<String, f64>,
        memories: usize,
        willpower: f64,
        /// LOD 层级（M4）：0 完整 / 1 常规 / 2 背景。
        #[serde(default)]
        lod: u8,
    },
    /// 玩家指令（意志节点）：先入日志再由核心下一拍处理，回放包含玩家操作。
    PlayerCommand {
        tick: u64,
        agent: u32,
        behavior: String,
        cost: f64,
        accepted: bool,
        reason: String,
    },
    /// 地点声誉变化（M3）：一次到访体验或事件冲击进声誉统计。
    ReputationChange {
        tick: u64,
        area: String,
        /// 本次体验（−1..1；到访净值或事件效价×强度）。
        experience: f64,
        value: f64,
    },
    /// 地点状态快照（每小时）：声誉、整洁度、累计到访次数。
    LocationState {
        tick: u64,
        area: String,
        reputation: f64,
        cleanliness: f64,
        visits: u64,
    },
    /// 暴露恢复（M4，设计稿 4.6）：完成被负面记忆抑制的行为，
    /// 相关恐惧记忆消退了一点（反复低强度尝试 → 慢慢好起来）。
    Exposure {
        tick: u64,
        agent: u32,
        behavior: String,
        /// 本次总消退量（鼓励在场时更大）。
        faded: f64,
    },
    /// 设立限期目标（M4，设计稿 4.5）。
    GoalSet {
        tick: u64,
        agent: u32,
        behavior: String,
        deadline: u64,
    },
    /// 为守约忍住了戒断行为（消耗意志力，换选次优行为）。
    GoalResist {
        tick: u64,
        agent: u32,
        behavior: String,
        cost: f64,
    },
    /// 失守：行为树选中且没忍住（意志力不足），或玩家主动违约。
    GoalViolation {
        tick: u64,
        agent: u32,
        behavior: String,
        /// true = 玩家主动下令违约；false = 意志力耗尽失守。
        commanded: bool,
    },
    /// 目标结束：到期（成功 = 零违约）或中途放弃（习惯反弹）。
    GoalEnd {
        tick: u64,
        agent: u32,
        behavior: String,
        success: bool,
        abandoned: bool,
        violations: u32,
        resists: u32,
        /// 放弃时还回的权重（反弹幅度）；到期成功时为 0（压制已转化为自然衰减）。
        rebound: f64,
    },
    Summary {
        agents: Vec<AgentSummary>,
        areas: Vec<AreaSummary>,
    },
}

/// 地点终态摘要（M3）。
#[derive(Debug, Clone, Serialize)]
pub struct AreaSummary {
    pub id: String,
    pub reputation: f64,
    pub cleanliness: f64,
    pub visits: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentSummary {
    pub id: u32,
    pub name: String,
    pub temperature: f64,
    /// 行为执行占比（键序确定）。
    pub distribution: BTreeMap<String, f64>,
    /// 与 0 号代理行为分布的总变差距离（0 相同，1 完全不同）。
    pub tv_distance_to_first: f64,
    /// 记忆条数与最强烈的三条记忆（"报告原因"的摘要）。
    pub memory_count: usize,
    pub top_memories: Vec<String>,
    /// 最亲近的关系（M2，名字 + 亲密值）。
    pub top_bonds: Vec<(String, f64)>,
}

/// 日志收集器：开发与常规运行写入内存最后落盘；基准模式关闭以测纯模拟性能。
#[derive(Debug, Default)]
pub struct Logger {
    pub enabled: bool,
    pub buf: String,
}

impl Logger {
    pub fn new(enabled: bool) -> Self {
        Logger {
            enabled,
            buf: String::new(),
        }
    }

    pub fn log(&mut self, ev: &LogEvent) {
        if !self.enabled {
            return;
        }
        if let Ok(line) = serde_json::to_string(ev) {
            self.buf.push_str(&line);
            self.buf.push('\n');
        }
    }

    pub fn write_to_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        std::fs::write(path, &self.buf)
    }
}
