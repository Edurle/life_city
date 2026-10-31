//! living-play —— M2 交互调试器：玩家的猫与猫群。
//!
//! 上半屏状态面板（时间、当前行为、需求条、意志力、最近记忆、与其他猫的关系），
//! 下方编号菜单：输入数字指挥猫（花意志力）、快进、退出。
//! 每轮把期间发生的的事件打印出来——包括猫群动态（惊吓、目睹、亲密变化）。
//!
//! 运行：cargo run -p living-cli --bin living-play -- --seed 42

use living_core::time::format_clock;
use living_core::{cat_names, load_config, BehaviorStatus, LogEvent, World};
use std::io::{self, BufRead, Write};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let get = |name: &str, default: &str| -> String {
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if a == name {
                return it.next().cloned().unwrap_or_default();
            }
        }
        default.to_string()
    };
    let seed: u64 = get("--seed", "42").parse().expect("种子应为整数");
    let cats: usize = get("--cats", "3").parse().expect("猫数应为整数");
    let assets = get("--assets", "assets/cats.json");
    let load_path = get("--load", "");
    let save_path = get("--save", "logs/play_state.bin");

    let json =
        std::fs::read_to_string(&assets).unwrap_or_else(|e| panic!("读取配置失败 {assets}: {e}"));
    let cfg = load_config(&json).unwrap_or_else(|e| panic!("{e}"));

    let mut world = if load_path.is_empty() {
        let mut w = World::new(cfg, seed, cat_names(cats), true);
        w.set_player(0);
        w.collect_recent = true;
        w.log_meta(seed, 0);
        // 先跑一段，让猫有自己的状态
        w.run(600);
        w
    } else {
        let bytes =
            std::fs::read(&load_path).unwrap_or_else(|e| panic!("读取存档失败 {load_path}: {e}"));
        let mut w = World::from_save(cfg, &bytes).unwrap_or_else(|e| panic!("{e}"));
        w.collect_recent = true;
        println!("（已从存档恢复，从第 {} 天继续）", world_day(&w));
        w
    };

    let player = world.agents[0].name.clone();
    println!("== 《活城》M2 · 玩家的猫与猫群 ==  你指挥 {player}，其余猫自己生活。");
    println!("规则：指令永远可以下，但违背它意愿的指令要花更多意志力（⚡）；");
    println!("      打断正在做的事额外 +8；意志力靠睡觉和慢慢回。输入数字回车。");
    println!("      M2：同处一地的社交时光会拉近猫的关系；亲近的猫会目睹学习彼此的遭遇；");
    println!("      你陪在胆小的猫身边，能安抚它的恐惧。输入数字回车。\n");

    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        draw_panel(&world, &player);
        let status = world.behaviors_status(0);
        print_menu(&status);
        print!("> ");
        io::stdout().flush().ok();
        let line = match lines.next() {
            Some(Ok(l)) => l,
            _ => break,
        };
        let n: Option<u64> = line.trim().parse().ok();
        let before = world.recent_events.len();
        match n {
            Some(0) => {
                world.run(30);
                report(&world, &player, before, "⏩ 30 分钟过去了");
            }
            Some(9) => {
                world.run(240);
                report(&world, &player, before, "⏩ 4 小时过去了");
            }
            Some(99) => break,
            Some(k) if (k as usize) <= status.len() => {
                let s = &status[(k - 1) as usize];
                let out = world.issue_command(0, &s.id);
                if out.accepted {
                    println!(
                        "👤 你让{player}去{}（⚡ -{:.0}）—— {}",
                        s.name, out.cost, out.reason
                    );
                } else {
                    println!("👤 你让{player}去{} —— {}", s.name, out.reason);
                }
                io::stdout().flush().ok();
                world.run(20);
                report(&world, &player, before, "");
            }
            _ => println!("没看懂，请输入菜单里的数字。"),
        }
    }

    let out = get("--out", "logs/play.jsonl");
    let path = std::path::PathBuf::from(&out);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    world.logger.write_to_file(&path).ok();
    let spath = std::path::PathBuf::from(&save_path);
    if let Some(dir) = spath.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    std::fs::write(&spath, world.to_save()).ok();
    println!(
        "\n日志已写入 {}（查看器回放）；进度存档 {}（下次 --load {} 可继续）。",
        path.display(),
        spath.display(),
        spath.display()
    );
}

fn world_day(w: &World) -> u64 {
    w.tick / 1440 + 1
}

fn bar(v: f64) -> String {
    let filled = ((v.clamp(0.0, 100.0) / 5.0).round() as usize).min(20);
    format!("{}{}", "█".repeat(filled), "·".repeat(20 - filled))
}

fn draw_panel(world: &World, player: &str) {
    let a = &world.agents[0];
    print!("\x1b[2J\x1b[H");
    let now = format_clock(world.tick);
    let action = match &a.current {
        Some((bid, _, end)) => format!(
            "{}（还剩 {} 分钟）",
            behavior_name(world, bid),
            end.saturating_sub(world.tick)
        ),
        None => "闲着（在想做什么）".to_string(),
    };
    println!("──────────────────────────────────────────────");
    println!("🕐 {now}   👤 {player}（你）");
    println!("正在: {action}");
    println!("⚡ 意志力 {} {:>3.0}/100", bar(a.willpower), a.willpower);
    let n = &a.needs;
    println!(
        "🍖 饥饿 {} {:>3.0}    💤 精力 {} {:>3.0}",
        bar(n.hunger),
        n.hunger,
        bar(n.energy),
        n.energy
    );
    println!(
        "🎾 娱乐 {} {:>3.0}    🫂 社交 {} {:>3.0}",
        bar(n.fun),
        n.fun,
        bar(n.social),
        n.social
    );
    println!(
        "🛁 卫生 {} {:>3.0}    🚽 膀胱 {} {:>3.0}",
        bar(n.hygiene),
        n.hygiene,
        bar(n.bladder),
        n.bladder
    );
    let mut mems: Vec<_> = a.memory.entries.iter().collect();
    mems.sort_by(|x, y| y.strength.total_cmp(&x.strength));
    if !mems.is_empty() {
        println!("🧠 记忆:");
        for m in mems.iter().take(3) {
            let kind = if m.vicarious {
                "（目睹所学）"
            } else {
                ""
            };
            println!(
                "   · {}（时段{}，强度{:.2}）{kind}",
                m.event, m.quarter, m.strength
            );
        }
    }
    // 关系：与其他猫的亲密值
    let bonds: Vec<(String, f64)> = {
        let mut v: Vec<_> = world
            .agents
            .iter()
            .skip(1)
            .map(|o| (o.name.clone(), world.bond(0, o.id)))
            .collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        v
    };
    let bond_str = bonds
        .iter()
        .map(|(n, b)| format!("{n} {:.2}", b))
        .collect::<Vec<_>>()
        .join(" · ");
    let comfort = world.agent_comfort(0);
    let comfort_str = if comfort > 0.05 {
        format!("（同伴在身边，安全感 {:.1}）", comfort)
    } else {
        String::new()
    };
    println!("🫂 亲密：{bond_str}{comfort_str}");
    for other in world.agents.iter().skip(1) {
        let act = match &other.current {
            Some((bid, _, _)) => behavior_name(world, bid),
            None => "闲着".to_string(),
        };
        println!("🐾 {}（自主）: {}", other.name, act);
    }
    println!("──────────────────────────────────────────────");
}

fn behavior_name(world: &World, id: &str) -> String {
    world
        .cfg
        .behaviors
        .iter()
        .find(|b| b.id == id)
        .map(|b| b.display_name().to_string())
        .unwrap_or_else(|| id.to_string())
}

fn print_menu(status: &[BehaviorStatus]) {
    println!("  0 ⏩ 快进30分钟    9 ⏩ 快进4小时    99 💾 存日志退出");
    for (i, s) in status.iter().enumerate() {
        if s.available {
            println!("  {:>2} {} —— 意志≈{:.0}", i + 1, s.name, s.est_cost);
        } else {
            println!("  {:>2} {} —— {}", i + 1, s.name, s.gate_note);
        }
    }
}

/// 打印 before 之后发生的事件（玩家的猫、猫群动态与全局环境事件）。
fn report(world: &World, player: &str, before: usize, banner: &str) {
    if !banner.is_empty() {
        println!("{banner}");
    }
    let start = before.min(world.recent_events.len());
    let name_of = |id: u32| -> String {
        world
            .agents
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| format!("#{id}"))
    };
    for ev in world.recent_events[start..].iter() {
        match ev {
            LogEvent::EnvEvent { tick, event, .. } => {
                let label = world
                    .cfg
                    .env_events
                    .iter()
                    .find(|e| e.id == *event)
                    .map(|e| e.display_name().to_string())
                    .unwrap_or_else(|| event.clone());
                println!("   {} 🌍 发生了【{label}】", format_clock(*tick));
            }
            LogEvent::Startled { tick, agent, event } => {
                let who = name_of(*agent);
                let label = world
                    .cfg
                    .env_events
                    .iter()
                    .find(|e| e.id == *event)
                    .map(|e| e.display_name().to_string())
                    .unwrap_or_else(|| event.clone());
                println!(
                    "   {} 😱 {who}被【{label}】吓了一跳，丢下了正在做的事",
                    format_clock(*tick)
                );
            }
            LogEvent::BondChange {
                tick, a, b, value, ..
            } => {
                println!(
                    "   {} 🫂 {}和{}更亲近了（现在 {:.2}）",
                    format_clock(*tick),
                    name_of(*a),
                    name_of(*b),
                    value
                );
            }
            LogEvent::PlayerCommand {
                tick,
                behavior,
                cost,
                accepted,
                ..
            } => {
                let mark = if *accepted { "✅" } else { "❌" };
                println!(
                    "   {} 👤 指令【{}】⚡{:.0} {mark}",
                    format_clock(*tick),
                    behavior_name(world, behavior),
                    cost
                );
            }
            LogEvent::Decision {
                tick,
                agent,
                choice,
                factors,
                ..
            } if *agent == 0 => {
                let src = if factors.will_command {
                    "你的指令"
                } else if factors.forced {
                    "生理反射"
                } else {
                    "它自己"
                };
                let comfort = if factors.comfort > 0.05 {
                    format!("×安全感{:.1}", factors.comfort)
                } else {
                    String::new()
                };
                println!(
                    "   {} 🐾 {player}开始【{}】（{src}｜权重{:.1}×需求{:.1}×记忆{:.1}{}）",
                    format_clock(*tick),
                    behavior_name(world, choice),
                    factors.weight,
                    factors.need_mult,
                    factors.mem_mult,
                    comfort
                );
            }
            LogEvent::ActionEnd {
                tick,
                agent,
                behavior,
            } if *agent == 0 => {
                println!(
                    "   {} ■ {}结束了【{}】",
                    format_clock(*tick),
                    player,
                    behavior_name(world, behavior)
                );
            }
            LogEvent::MemoryWrite {
                tick,
                agent,
                event,
                salience,
                source,
                vicarious,
                ..
            } => {
                if *vicarious {
                    // 目睹学习：谁看着谁的遭遇学到了什么（M2 猫群故事线）
                    let who = name_of(*agent);
                    let from = source.map(name_of).unwrap_or_default();
                    println!(
                        "   {} 🧠 {who}目睹了{from}的遭遇，也记住了【{event}】（显著性{:.2}）",
                        format_clock(*tick),
                        salience
                    );
                } else if *agent == 0 {
                    println!(
                        "   {} 🧠 {player}记住了【{event}】（显著性{:.2}）",
                        format_clock(*tick),
                        salience
                    );
                }
            }
            _ => {}
        }
    }
    println!();
}
