//! JSONL 事件日志：模拟的唯一事实源。
//! 每行一个 JSON 对象，serde 派生序列化；BTreeMap 保证键序确定，日志字节级可比较。

use crate::needs::Needs;
use serde::Serialize;
use std::collections::BTreeMap;

pub const LOG_VERSION: u32 = 2;

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
    /// 安全感（M2）：0 = 身边没有可信任的同伴；越高，负面记忆被压制越多。
    pub comfort: f64,
    /// 第一层生理反射强制执行。
    pub forced: bool,
    /// 玩家意志指令覆盖（M1 意志节点）。
    pub will_command: bool,
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
    Summary {
        agents: Vec<AgentSummary>,
    },
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
