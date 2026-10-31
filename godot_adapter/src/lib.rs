//! 《活城》GDExtension 适配层（M3 表现竖切）。
//!
//! 职责纪律（技术文档 6.2）：
//! - 只做"翻译"：核心 Sim 是普通 Rust 结构体，这里把它包成 Godot 节点；
//! - 不为每个代理建 Godot 对象——批量打包数据一次传出，Godot 侧 MultiMesh 渲染；
//! - 事件不逐条发信号（本竖切直接轮询快照，信号通道留给后续需要 UI 事件时再开）；
//! - 模拟数据全部住在 Rust 结构体里，Godot 只读快照。
//!
//! 构建：cargo build -p godot_adapter && cp target/debug/libgodot_adapter.so godot/bin/

use godot::classes::Node;
use godot::prelude::*;
use living_core::{cat_names, cat_roster, human_names, load_config, World};

/// GDExtension 入口（默认符号 gdext_rust_init，见 living_sim.gdextension）。
#[gdextension]
unsafe impl ExtensionLibrary for SimNode {}

/// 1 现实秒 = 1 模拟分钟（×1 档）；速度档由表现层控制（暂停/×1/×2/×10/×100）。
const TICKS_PER_SECOND: f64 = 1.0;

#[derive(GodotClass)]
#[class(base = Node)]
struct SimNode {
    sim: Option<World>,
    agent_count: usize,
    /// 剩余的零头拍（帧间隔 × 速度常带小数）。
    carry: f64,
    #[var]
    speed: f64,
    #[var]
    paused: bool,
}

#[godot_api]
impl INode for SimNode {
    fn init(_base: Base<Node>) -> Self {
        SimNode {
            sim: None,
            agent_count: 0,
            carry: 0.0,
            speed: 1.0,
            paused: false,
        }
    }

    /// 每帧按 delta × speed 推进整数拍。
    fn process(&mut self, delta: f64) {
        let Some(sim) = self.sim.as_mut() else { return };
        if self.paused {
            return;
        }
        self.carry += delta * self.speed * TICKS_PER_SECOND;
        let n = self.carry.floor() as u64;
        if n > 0 {
            self.carry -= n as f64;
            sim.run(n);
        }
    }
}

#[godot_api]
impl SimNode {
    /// 启动模拟：加载配置、建世界（人类 + 猫）。
    #[func]
    fn start(&mut self, seed: i64, config_path: GString, humans: i32, cats: i32) {
        let json = std::fs::read_to_string(config_path.to_string())
            .expect("读取配置失败（工作目录应为项目根）");
        let cfg = load_config(&json).expect("配置不合法");
        let mut roster: Vec<(String, String)> = human_names(humans.max(0) as usize)
            .into_iter()
            .map(|n| (n, "human".to_string()))
            .collect();
        roster.extend(cat_roster(cat_names(cats.max(0) as usize)));
        let mut world = World::new(cfg, seed as u64, roster, true);
        world.log_meta(seed as u64, 0);
        world.collect_recent = false;
        self.agent_count = world.agents.len();
        self.sim = Some(world);
    }

    /// 速度档：0 = 暂停，1/2/10/100 = 加速倍率。
    #[func]
    fn set_speed_mult(&mut self, mult: f64) {
        if mult <= 0.0 {
            self.paused = true;
        } else {
            self.paused = false;
            self.speed = mult;
        }
    }

    /// 所有代理位置（批量；米坐标 → 像素由 Godot 侧统一缩放）。
    #[func]
    fn agent_positions(&self) -> PackedVector2Array {
        let mut out = PackedVector2Array::new();
        if let Some(sim) = &self.sim {
            for a in &sim.agents {
                out.push(Vector2::new(a.pos.0 as f32, a.pos.1 as f32));
            }
        }
        out
    }

    /// 代理物种（0 = 猫，1 = 人类），与 agent_positions 同序。
    #[func]
    fn agent_species(&self) -> PackedByteArray {
        let mut out = PackedByteArray::new();
        if let Some(sim) = &self.sim {
            for a in &sim.agents {
                out.push(if a.species == "human" { 1 } else { 0 });
            }
        }
        out
    }

    /// 代理显示名（调试/UI 悬浮用）。
    #[func]
    fn agent_names(&self) -> PackedStringArray {
        let mut out = PackedStringArray::new();
        if let Some(sim) = &self.sim {
            for a in &sim.agents {
                out.push(a.name.as_str());
            }
        }
        out
    }

    /// 地点矩形 [x, y, w, h] × N（静态，启动后取一次）。
    #[func]
    fn area_rects(&self) -> PackedFloat32Array {
        let mut out = PackedFloat32Array::new();
        if let Some(sim) = &self.sim {
            for a in &sim.cfg.areas {
                out.push(a.x as f32);
                out.push(a.y as f32);
                out.push(a.w as f32);
                out.push(a.h as f32);
            }
        }
        out
    }

    /// 地点名（与 area_rects 同序）。
    #[func]
    fn area_names(&self) -> PackedStringArray {
        let mut out = PackedStringArray::new();
        if let Some(sim) = &self.sim {
            for a in &sim.cfg.areas {
                out.push(a.display_name());
            }
        }
        out
    }

    /// 地点状态 [声誉, 整洁度] × N（每秒轮询一次足够）。
    #[func]
    fn area_states(&self) -> PackedFloat32Array {
        let mut out = PackedFloat32Array::new();
        if let Some(sim) = &self.sim {
            for a in &sim.cfg.areas {
                let st = sim.area_states.get(&a.id);
                out.push(st.map(|s| s.reputation as f32).unwrap_or(1.0));
                out.push(st.map(|s| s.cleanliness as f32).unwrap_or(1.0));
            }
        }
        out
    }

    /// 地点矩形的像素坐标（Godot 的 y 向下；世界 y 向上 → 翻转）。
    #[func]
    fn spot_positions(&self) -> PackedVector2Array {
        let mut out = PackedVector2Array::new();
        if let Some(sim) = &self.sim {
            for s in sim.cfg.spots.iter() {
                out.push(Vector2::new(s.x as f32, s.y as f32));
            }
        }
        out
    }

    /// 当前模拟时钟（人类可读）。
    #[func]
    fn clock_text(&self) -> GString {
        match &self.sim {
            Some(sim) => GString::from(living_core::time::format_clock(sim.tick).as_str()),
            None => GString::from("未启动"),
        }
    }

    /// 最近发生的事件文本（UI 播报；每次调用消费缓冲）。
    #[func]
    fn drain_recent_events(&mut self) -> PackedStringArray {
        let mut out = PackedStringArray::new();
        if let Some(sim) = self.sim.as_mut() {
            sim.collect_recent = true;
            for ev in sim.recent_events.drain(..) {
                let text = match ev {
                    living_core::LogEvent::EnvEvent { tick, event, .. } => {
                        Some(format!("🌍 {}", describe(tick, &event)))
                    }
                    living_core::LogEvent::Startled { tick, agent, event } => Some(format!(
                        "😱 #{} 被吓了一跳（{}）",
                        agent,
                        describe(tick, &event)
                    )),
                    living_core::LogEvent::ReputationChange {
                        tick,
                        area,
                        experience,
                        value,
                    } => Some(format!(
                        "{} {} 口碑 {} → {:.2}",
                        living_core::time::format_clock(tick),
                        area,
                        if experience >= 0.0 {
                            "回升"
                        } else {
                            "受损"
                        },
                        value
                    )),
                    _ => None,
                };
                if let Some(t) = text {
                    out.push(t.as_str());
                }
            }
        }
        out
    }

    /// 累计决策数（性能自检）。
    #[func]
    fn decisions_done(&self) -> i64 {
        self.sim
            .as_ref()
            .map(|s| s.decisions_done as i64)
            .unwrap_or(0)
    }
}

fn describe(tick: living_core::Tick, event: &str) -> String {
    format!("{}【{}】", living_core::time::format_clock(tick), event)
}
