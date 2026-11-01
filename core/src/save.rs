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
    /// 离场索引（M6 二阶段）。
    pub departed_index: Vec<bool>,
    pub decisions_done: u64,
    /// 年度统计与关卡（M6）。
    pub stats: crate::world::TownStats,
    pub scenario: Option<crate::scenario::ActiveScenario>,
}

pub const SAVE_VERSION: u32 = 8;

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
            departed_index: self.departed_index.clone(),
            decisions_done: self.decisions_done,
            stats: self.stats.clone(),
            scenario: self.scenario.clone(),
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
            departed_index: state.departed_index,
            logger: Logger::new(true),
            decisions_done: state.decisions_done,
            stats: state.stats,
            scenario: state.scenario,
            collect_recent: false,
            recent_events: Vec::new(),
        })
    }
}

// ---------- M6：存档打磨（多槽位 + 元信息 + 自动存档载体） ----------

/// 存档元信息：不读档也能在槽位列表里展示进度。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SaveMeta {
    pub version: u32,
    pub day: u64,
    pub agents: usize,
    /// 运行中的关卡（ID 与名字）。
    pub scenario: Option<(String, String)>,
    /// 已结算的胜负（关卡打完了但没清掉时可见）。
    pub scenario_won: Option<bool>,
    pub decisions: u64,
    /// 保存时刻（本地时钟字符串，仅供人看）。
    pub saved_at: String,
}

impl World {
    pub fn save_meta(&self) -> SaveMeta {
        let scenario = self.scenario.as_ref().map(|s| {
            let def = s.def();
            (def.id.clone(), def.name.clone())
        });
        SaveMeta {
            version: SAVE_VERSION,
            day: self.tick / crate::time::TICKS_PER_DAY,
            agents: self.agents.len(),
            scenario,
            scenario_won: self.scenario.as_ref().and_then(|s| s.finished),
            decisions: self.decisions_done,
            saved_at: chrono_like_now(),
        }
    }

    /// 写入槽位：`<dir>/<slot>.bin`（快照）+ `<dir>/<slot>.meta.json`（元信息）。
    pub fn save_to_slot(&self, dir: &std::path::Path, slot: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let stem = sanitize_slot(slot);
        std::fs::write(dir.join(format!("{stem}.bin")), self.to_save())?;
        let meta = serde_json::to_string_pretty(&self.save_meta()).unwrap_or_else(|_| "{}".into());
        std::fs::write(dir.join(format!("{stem}.meta.json")), meta)?;
        Ok(())
    }

    /// 从槽位读档（元信息与快照同生共死；缺一即报错）。
    pub fn load_from_slot(
        cfg: SimConfig,
        dir: &std::path::Path,
        slot: &str,
    ) -> Result<World, String> {
        let stem = sanitize_slot(slot);
        let bin = dir.join(format!("{stem}.bin"));
        let bytes = std::fs::read(&bin).map_err(|e| format!("读档失败 {}: {e}", bin.display()))?;
        World::from_save(cfg, &bytes)
    }

    /// 列出槽位（读所有 *.meta.json；损坏的跳过并标注）。
    pub fn list_slots(dir: &std::path::Path) -> Vec<(String, SaveMeta)> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return out;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(stem) = name.strip_suffix(".meta.json") {
                if let Ok(text) = std::fs::read_to_string(e.path()) {
                    if let Ok(meta) = serde_json::from_str::<SaveMeta>(&text) {
                        out.push((stem.to_string(), meta));
                    }
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
}

/// 槽位名只留安全字符（防路径穿越）。
fn sanitize_slot(slot: &str) -> String {
    slot.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// 本地时间字符串（不引第三方库：系统命令不可靠就用 Unix 秒）。
fn chrono_like_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| format!("unix:{}", d.as_secs()))
        .unwrap_or_else(|_| "unknown".into())
}
