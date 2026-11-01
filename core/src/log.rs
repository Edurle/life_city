//! JSONL 事件日志：模拟的唯一事实源。
//! 每行一个 JSON 对象，serde 派生序列化；BTreeMap 保证键序确定，日志字节级可比较。

use crate::needs::Needs;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const LOG_VERSION: u32 = 8;

/// 决策的因素分解：系统能"报告原因"的载体。
#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        /// 童年实时模仿（M6 二阶段）：true = 这次选择来自"跟着父母做"，
        /// 而非 softmax（决策因素分解照常记录）。
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        imitated: bool,
    },
    ActionStart {
        tick: u64,
        agent: u32,
        behavior: String,
        until: u64,
        /// 选中的变体（M5 二阶段吃什么）：无变体的行为为 None。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        variant: Option<String>,
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
        /// 钱包（M5 经济曲线）。
        #[serde(default)]
        wallet: f64,
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
    /// 案发（M5，设计稿 5.2）：盗窃行为的结算——无论得手还是被抓。
    /// 新闻生成的素材源；因果链（受害者记忆 → 传闻 → 声誉）由后续事件承载。
    Theft {
        tick: u64,
        thief: u32,
        victim: u32,
        amount: f64,
        caught: bool,
        /// 案发区域 ID。
        area: String,
    },
    /// 职业变动（M5，设计稿 5.3）：入职、转行、被捕失业——全部概率驱动。
    JobChange {
        tick: u64,
        agent: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        from: Option<String>,
        to: Option<String>,
        reason: String,
    },
    /// 钱包变动（M5 经济）：工资入账与消费扣款。
    Wallet {
        tick: u64,
        agent: u32,
        delta: f64,
        value: f64,
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
    // ---------- M5 二阶段 ----------
    /// 社交对象选择（"和谁"）：带因素的完整记录——
    /// 能解释"它找了林雨桐（亲密 0.8）而不是王建国（上次吵架）"。
    SocialPick {
        tick: u64,
        agent: u32,
        target: u32,
        /// 四项权重因素的当时值（查看器分解用）。
        bond: f64,
        freshness: f64,
        reach: f64,
        impression: f64,
    },
    /// 恋爱状态机转移：在一起 / 分手。
    Couple {
        tick: u64,
        a: u32,
        b: u32,
        started: bool,
    },
    /// 新生儿出生（代际）：父母双方与新成员。
    Born {
        tick: u64,
        agent: u32,
        mother: u32,
        father: u32,
    },
    /// 行为模仿（M5 二阶段流行传播）：目睹亲近同伴完成某行为，
    /// 自己对该行为的权重上升了一点（习惯的传染）。
    Imitate {
        tick: u64,
        observer: u32,
        from: u32,
        behavior: String,
        /// 变体（吃什么被种草）；行为级模仿为 None。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        variant: Option<String>,
    },
    /// 关卡开始（M6）。
    ScenarioStart {
        tick: u64,
        scenario: String,
        name: String,
        days: u64,
    },
    /// 关卡结算（M6）：到期评估胜负，附每个条件的当时值。
    ScenarioResult {
        tick: u64,
        scenario: String,
        won: bool,
        conditions: Vec<crate::scenario::ConditionState>,
    },
    /// 离场（M6 二阶段，人口退出）：代理离开小镇（年迈/迁出）——
    /// 不是死亡：记忆与关系保留，故事里他还是"去了别的城市的人"。
    Depart {
        tick: u64,
        agent: u32,
        /// 离场原因（年迈 / 外出谋生——数据文案由呈现层映射）。
        reason: String,
        /// 离场时的年龄（模拟日）。
        age_days: f64,
    },
    Summary {
        agents: Vec<AgentSummary>,
        areas: Vec<AreaSummary>,
    },
}

/// 地点终态摘要（M3）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AreaSummary {
    pub id: String,
    pub reputation: f64,
    pub cleanliness: f64,
    pub visits: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// 年龄（模拟日，M5 二阶段）。
    #[serde(default)]
    pub age_days: f64,
    /// 恋人名字（M5 二阶段；无 = None）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lover: Option<String>,
    /// 离场标记（M6 二阶段）：Some(离场拍) = 已离开小镇。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub departed_tick: Option<u64>,
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
