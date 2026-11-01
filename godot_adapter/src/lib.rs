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

    /// 区域 id（与 area_rects/area_states 同序）。3D 客户端用 Area_<id> 场景文件
    /// 名把区域状态关联到对应楼群的材质上。
    #[func]
    fn area_ids(&self) -> PackedStringArray {
        let mut out = PackedStringArray::new();
        if let Some(sim) = &self.sim {
            for a in &sim.cfg.areas {
                out.push(a.id.as_str());
            }
        }
        out
    }

    /// 区域是否承载小镇生活（有 spot 或时间窗事件引用它）：纯装饰道路为 0。
    /// 声誉染色只上有生活的区域——无人到访的道路没有口碑可言。
    #[func]
    fn area_active(&self) -> PackedByteArray {
        let mut out = PackedByteArray::new();
        if let Some(sim) = &self.sim {
            let mut living = std::collections::BTreeSet::new();
            for s in &sim.cfg.spots {
                if let Some(a) = &s.area {
                    living.insert(a.as_str());
                }
            }
            for e in &sim.cfg.env_events {
                if let Some(a) = &e.area {
                    living.insert(a.as_str());
                }
            }
            for a in &sim.cfg.areas {
                out.push(u8::from(living.contains(a.id.as_str())));
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

    /// 玩家角色编号（旁观模式的"自己"参照）。
    #[func]
    fn player_index(&self) -> i64 {
        self.sim
            .as_ref()
            .and_then(|s| s.agents.iter().position(|a| a.is_player))
            .map(|i| i as i64)
            .unwrap_or(-1)
    }

    /// 代理的公开信息（旁观点选）：名字/物种/职业/正在做什么/所在区域。
    /// 心理状态默认不可见（设计稿 6.x 信息分层）。
    #[func]
    fn agent_brief(&self, idx: i64) -> PackedStringArray {
        let mut out = PackedStringArray::new();
        let Some(sim) = &self.sim else { return out };
        let Some(a) = sim.agents.get(idx as usize) else {
            return out;
        };
        // 离场者（M6 人口退出）：只报姓名与去向，其余字段无意义。
        if let Some(d) = a.departed_tick {
            out.push(a.name.as_str());
            out.push("已离开小镇");
            out.push(format!("（第 {} 天送行）", d / 1440 + 1).as_str());
            return out;
        }
        let species = if a.species == "human" {
            "人类"
        } else {
            "猫"
        };
        let job = a
            .job
            .as_deref()
            .map(|j| {
                sim.cfg
                    .jobs
                    .iter()
                    .find(|x| x.id == j)
                    .map(|x| x.display_name().to_string())
                    .unwrap_or_else(|| j.to_string())
            })
            .unwrap_or_else(|| "无业".to_string());
        let current = a
            .current
            .as_ref()
            .map(|(b, _, end)| {
                let label = sim
                    .cfg
                    .behaviors
                    .iter()
                    .find(|x| x.id == *b)
                    .map(|x| x.display_name().to_string())
                    .unwrap_or_else(|| b.clone());
                format!("{label}（还剩 {} 分钟）", end.saturating_sub(sim.tick))
            })
            .unwrap_or_else(|| "闲着".to_string());
        let area = sim
            .cfg
            .areas
            .iter()
            .find(|ar| ar.contains(a.pos))
            .map(|ar| ar.display_name().to_string())
            .unwrap_or_else(|| "街上".to_string());
        out.push(a.name.as_str());
        out.push(species);
        out.push(job.as_str());
        out.push(current.as_str());
        out.push(area.as_str());
        out
    }

    /// 代理的内心状态（密友门后）：需求与在意的负面记忆。
    /// 只有玩家自己或亲密达标的对象可见（门在核心侧，客户端不越权）。
    #[func]
    fn agent_deep(&self, idx: i64) -> PackedStringArray {
        let mut out = PackedStringArray::new();
        let Some(sim) = &self.sim else { return out };
        let Some(a) = sim.agents.get(idx as usize) else {
            return out;
        };
        let player = sim.agents.iter().position(|p| p.is_player).unwrap_or(0) as u32;
        if a.id == player {
            // 自己完全可见。
            out.push("（这是你自己）");
            let needs = [
                ("饥饿", a.needs.hunger),
                ("精力", a.needs.energy),
                ("娱乐", a.needs.fun),
                ("社交", a.needs.social),
                ("卫生", a.needs.hygiene),
                ("膀胱", a.needs.bladder),
            ];
            for (k, v) in needs {
                out.push(format!("{k} {:.0}", v).as_str());
            }
            for m in a.memory.entries.iter().filter(|m| m.valence < 0.0).take(3) {
                out.push(format!("在意「{}」（强度 {:.2}）", m.event, m.strength).as_str());
            }
            return out;
        }
        // 其他人：密友门（设计稿 4.7）。
        match sim.inspect_friend(player, a.id) {
            Ok(v) => {
                let needs = [
                    ("饥饿", v.needs.hunger),
                    ("精力", v.needs.energy),
                    ("娱乐", v.needs.fun),
                    ("社交", v.needs.social),
                    ("卫生", v.needs.hygiene),
                    ("膀胱", v.needs.bladder),
                ];
                out.push(format!("（密友 · 亲密 {:.2}）", v.bond).as_str());
                for (k, val) in needs {
                    out.push(format!("{k} {:.0}", val).as_str());
                }
                for (ev, s) in &v.fears {
                    out.push(format!("在意「{ev}」（强度 {s:.2}）").as_str());
                }
            }
            Err(e) => {
                out.push(e.as_str());
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
