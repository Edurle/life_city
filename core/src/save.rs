//! 存档：核心状态的二进制快照（bincode）。
//! 存档 = 快照 + 日志尾部（日志本身已持久化，快照负责"从这里继续"）；
//! 随机数生成器状态一并保存，读档后运行与连续运行完全一致（有测试保证）。
//! 配置（assets/*.json）不进快照，读档时重新加载。

use crate::config::{BehaviorDef, SimConfig, SpotDef};
use crate::log::Logger;
use crate::relation::Relations;
use crate::rng::Rng;
use crate::space::SpatialGrid;
use crate::time::Tick;
use crate::world::World;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
pub struct SaveState {
    pub version: u32,
    pub tick: Tick,
    pub agents: Vec<crate::agent::Agent>,
    pub relations: Relations,
    pub grid: SpatialGrid,
    pub area_states: BTreeMap<String, crate::area::AreaState>,
    pub event_schedule: Vec<Tick>,
    pub world_rng: Rng,
    /// 恋爱与生育骰子（M5 二阶段）。
    pub life_rng: Rng,
    /// 世界种子（M5 二阶段：新生儿按种子派生自己的子流）。
    pub world_seed: u64,
    /// 物种索引（M5 二阶段）。
    pub species_index: Vec<String>,
    pub decisions_done: u64,
}

pub const SAVE_VERSION: u32 = 6;

impl World {
    /// 导出二进制快照。
    pub fn to_save(&self) -> Vec<u8> {
        let state = SaveState {
            version: SAVE_VERSION,
            tick: self.tick,
            agents: self.agents.clone(),
            relations: self.relations.clone(),
            grid: self.grid.clone(),
            area_states: self.area_states.clone(),
            event_schedule: self.event_schedule.clone(),
            world_rng: self.world_rng.clone(),
            life_rng: self.life_rng.clone(),
            world_seed: self.world_seed,
            species_index: self.species_index.clone(),
            decisions_done: self.decisions_done,
        };
        bincode::serialize(&state).expect("快照序列化不应失败")
    }

    /// 从快照恢复世界（配置从 JSON 重新加载，日志从空开始——本段新日志）。
    pub fn from_save(cfg: SimConfig, bytes: &[u8]) -> Result<World, String> {
        let state: SaveState =
            bincode::deserialize(bytes).map_err(|e| format!("快照解析失败: {e}"))?;
        if state.version != SAVE_VERSION {
            return Err(format!(
                "快照版本不兼容：文件 {}，当前 {}",
                state.version, SAVE_VERSION
            ));
        }
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
        Ok(World {
            tick: state.tick,
            cfg,
            agents: state.agents,
            relations: state.relations,
            grid: state.grid,
            area_states: state.area_states,
            behaviors,
            spots,
            event_schedule: state.event_schedule,
            world_rng: state.world_rng,
            life_rng: state.life_rng,
            world_seed: state.world_seed,
            species_index: state.species_index,
            logger: Logger::new(true),
            decisions_done: state.decisions_done,
            collect_recent: false,
            recent_events: Vec::new(),
            stats: crate::world::TownStats::default(),
        })
    }
}
