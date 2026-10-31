//! living-play —— M1 交互调试器：玩家的猫。
//!
//! 上半屏状态面板（时间、当前行为、需求条、意志力、最近记忆），
//! 下方编号菜单：输入数字指挥猫（花意志力）、快进、退出。
//! 每轮把期间发生的的事件打印出来——这就是"它想睡，你让它玩"的拉扯现场。
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

    let json = std::fs::read_to_string(&assets)
        .unwrap_or_else(|e| panic!("读取配置失败 {assets}: {e}"));
    let cfg = load_config(&json).unwrap_or_else(|e| panic!("{e}"));

    let mut world = World::new(cfg, seed, cat_names(cats), true);
    world.set_player(0);
    world.collect_recent = true;
    world.log_meta(seed, 0);
    // 先跑一段，让猫有自己的状态
    world.run(600);

    let player = world.agents[0].name.clone();
    println!("== 《活城》M1 · 玩家的猫 ==  你指挥 {player}，其余猫自己生活。");
    println!("规则：指令永远可以下，但违背它意愿的指令要花更多意志力（⚡）；");
    println!("      打断正在做的事额外 +8；意志力靠睡觉和慢慢回。输入数字回车。\n");

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
                    println!("👤 你让{player}去{}（⚡ -{:.0}）—— {}", s.name, out.cost, out.reason);
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
    println!("\n日志已写入 {}，可用 viewer/index.html 回放。", path.display());
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
    println!("🍖 饥饿 {} {:>3.0}    💤 精力 {} {:>3.0}", bar(n.hunger), n.hunger, bar(n.energy), n.energy);
    println!("🎾 娱乐 {} {:>3.0}    🫂 社交 {} {:>3.0}", bar(n.fun), n.fun, bar(n.social), n.social);
    println!("🛁 卫生 {} {:>3.0}    🚽 膀胱 {} {:>3.0}", bar(n.hygiene), n.hygiene, bar(n.bladder), n.bladder);
    let mut mems: Vec<_> = a.memory.entries.iter().collect();
    mems.sort_by(|x, y| y.strength.total_cmp(&x.strength));
    if !mems.is_empty() {
        println!("🧠 记忆:");
        for m in mems.iter().take(3) {
            println!("   · {}（时段{}，强度{:.2}）", m.event, m.quarter, m.strength);
        }
    }
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

/// 打印 before 之后发生的事件（只报玩家的猫与全局环境事件）。
fn report(world: &World, player: &str, before: usize, banner: &str) {
    if !banner.is_empty() {
        println!("{banner}");
    }
    let start = before.min(world.recent_events.len());
    for ev in world.recent_events[start..].iter() {
        match ev {
            LogEvent::EnvEvent { tick, event } => {
                println!("   {} 🌍 发生了【{}】", format_clock(*tick), event);
            }
            LogEvent::PlayerCommand { tick, behavior, cost, accepted, .. } => {
                let mark = if *accepted { "✅" } else { "❌" };
                println!(
                    "   {} 👤 指令【{}】⚡{:.0} {mark}",
                    format_clock(*tick),
                    behavior_name(world, behavior),
                    cost
                );
            }
            LogEvent::Decision { tick, agent, choice, factors, .. } if *agent == 0 => {
                let src = if factors.will_command {
                    "你的指令"
                } else if factors.forced {
                    "生理反射"
                } else {
                    "它自己"
                };
                println!(
                    "   {} 🐾 {player}开始【{}】（{src}｜权重{:.1}×需求{:.1}×记忆{:.1}）",
                    format_clock(*tick),
                    behavior_name(world, choice),
                    factors.weight,
                    factors.need_mult,
                    factors.mem_mult
                );
            }
            LogEvent::ActionEnd { tick, agent, behavior } if *agent == 0 => {
                println!("   {} ■ {}结束了【{}】", format_clock(*tick), player, behavior_name(world, behavior));
            }
            LogEvent::MemoryWrite { tick, agent, event, salience, .. } if *agent == 0 => {
                println!("   {} 🧠 {player}记住了【{}】（显著性{:.2}）", format_clock(*tick), event, salience);
            }
            _ => {}
        }
    }
    println!();
}
