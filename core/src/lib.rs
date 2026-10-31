//! living-core ——《活城》模拟核心（M5 一座镇·二阶段：社交对象选择与恋爱、
//! 老龄、流行传播、代际、吃什么变体层、日记与年度总结；承接一阶段的
//! 职业与经济、犯罪与治安、传闻与新闻）。
//!
//! 架构铁律（见《技术选型与架构》）：
//! - 本库不含任何界面代码，只读配置、只写日志；
//! - 确定性：固定种子 + 每代理独立随机数子流，同种子字节级复现；
//! - 内容全部数据驱动（assets/*.json），引擎无内容专属代码。

pub mod agent;
pub mod area;
pub mod config;
pub mod diary;
pub mod goal;
pub mod log;
pub mod memory;
pub mod needs;
pub mod news;
pub mod relation;
pub mod rng;
pub mod save;
pub mod scenario;
pub mod space;
pub mod time;
pub mod tree;
pub mod world;

pub use agent::Agent;
pub use area::AreaState;
pub use config::{load_config, SimConfig};
pub use goal::Goal;
pub use log::{AgentSummary, LogEvent, Logger};
pub use relation::Relations;
pub use rng::WorldSeed;
pub use space::{Pos, SpatialGrid};
pub use time::Tick;
pub use world::{
    cat_names, cat_roster, human_names, tv_distance, BehaviorStatus, ChoiceInfo, CommandOutcome,
    FriendView, GoalStatus, HabitReport, TownStats, World,
};
