//! living-play —— M4 交互调试器：玩家的人生。
//!
//! 一条街（默认 street.json）：你指挥自己的人类角色（0 号），其他居民自己生活。
//! 上半屏状态面板（时间、行为、需求、意志力、习惯、恐惧、目标、密友），
//! 下方编号菜单：输入数字指挥（花意志力）、快进、定目标、看密友、读习惯报告。
//!
//! M4 完整意志系统：习惯自动化（指令免费）、恐惧加价与暴露恢复、
//! 限期目标（忍住/失守/反弹）、密友查看。
//!
//! 运行：cargo run -p living-cli --bin living-play -- --seed 42

use living_core::time::format_clock;
use living_core::{
    cat_names, cat_roster, human_names, load_config, BehaviorStatus, LogEvent, World,
};
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
    let humans: usize = get("--humans", "4").parse().expect("人数应为整数");
    let cats: usize = get("--cats", "2").parse().expect("猫数应为整数");
    let assets = get("--assets", "assets/street.json");
    let load_path = get("--load", "");
    let save_path = get("--save", "logs/play_state.bin");
    let scenario_path = get("--scenario", "");

    let json =
        std::fs::read_to_string(&assets).unwrap_or_else(|e| panic!("读取配置失败 {assets}: {e}"));
    let cfg = load_config(&json).unwrap_or_else(|e| panic!("{e}"));

    let mut world = if load_path.is_empty() {
        let mut roster: Vec<(String, String)> = human_names(humans)
            .into_iter()
            .map(|n| (n, "human".to_string()))
            .collect();
        roster.extend(cat_roster(cat_names(cats)));
        let mut w = World::new(cfg, seed, roster, true);
        w.set_player(0);
        w.collect_recent = true;
        w.log_meta(seed, 0);
        // 先跑一段，让街道有自己的生活
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

    // M6：关卡开局。
    if !scenario_path.is_empty() {
        let text = std::fs::read_to_string(&scenario_path)
            .unwrap_or_else(|e| panic!("读取关卡失败 {scenario_path}: {e}"));
        let def = living_core::scenario::load_scenario(&text).unwrap_or_else(|e| panic!("{e}"));
        world
            .start_scenario(def.clone())
            .unwrap_or_else(|e| panic!("{e}"));
        println!(
            "🎯 关卡开始：「{}」—— {}（{} 天）",
            def.name, def.description, def.days
        );
    }
    let mut last_autosave_day = world.tick / 1440;

    let player = world.agents[0].name.clone();
    println!("== 《活城》M6 · 玩家的人生 ==  你扮演 {player}，一条街的其他居民自己生活。");
    println!("规则：指令永远可以下，但违背它意愿的指令要花更多意志力（⚡）；");
    println!("      已养成的习惯（∞）自动执行、指令免费；被恐惧压着的行为（⚠）更贵，");
    println!("      反复去做会慢慢好起来；定目标（🎯）对抗习惯，放弃会反弹。");
    println!("      和邻居处成密友（亲密 ≥ 0.75）就能看懂它的心。输入数字回车。\n");

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
            Some(5) => {
                print_diary(&mut world, &player);
            }
            Some(6) => {
                print_habit_report(&world);
            }
            Some(7) => {
                friend_menu(&mut world, &mut lines);
            }
            Some(8) => {
                goal_menu(&mut world, &mut lines);
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
            Some(4) => {
                print_story(&mut world, &player);
            }
            _ => println!("没看懂，请输入菜单里的数字。"),
        }
        // M6：自动存档（每 3 模拟天一次，槽位 autosave）与关卡结算播报。
        let day = world.tick / 1440;
        if day >= last_autosave_day + 3 {
            last_autosave_day = day;
            let _ = world.save_to_slot(std::path::Path::new("saves"), "autosave");
            println!(
                "💾 已自动存档（第 {} 天，槽位 autosave，--load-slot autosave 续玩）",
                day + 1
            );
        }
        // 关卡结算播报（先把结算事件收集出来，再解开借用去打印故事）。
        let settled: Vec<(String, bool)> = world.recent_events
            [before.min(world.recent_events.len())..]
            .iter()
            .filter_map(|ev| match ev {
                living_core::LogEvent::ScenarioResult { scenario, won, .. } => {
                    Some((scenario.clone(), *won))
                }
                _ => None,
            })
            .collect();
        for (scenario, won) in settled {
            println!(
                "\n🎯 关卡「{scenario}」{}！",
                if won { "达成" } else { "未成" }
            );
            for c in world.scenario_progress() {
                println!(
                    "   {}：{:.2} / {} {}",
                    c.label,
                    c.current,
                    c.target,
                    if c.met { "✓" } else { "" }
                );
            }
            print_story(&mut world, &player);
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
        Some((bid, _, end)) => {
            let shown = match &a.current_variant {
                Some(v) => behavior_name(world, &format!("{bid}#{v}")),
                None => behavior_name(world, bid),
            };
            format!("{shown}（还剩 {} 分钟）", end.saturating_sub(world.tick))
        }
        None => "闲着（在想做什么）".to_string(),
    };
    println!("──────────────────────────────────────────────");
    let job_title = match &a.job {
        Some(j) => world
            .cfg
            .jobs
            .iter()
            .find(|x| x.id == *j)
            .map(|x| x.display_name().to_string())
            .unwrap_or_else(|| j.clone()),
        None => "无业".to_string(),
    };
    println!(
        "🕐 {now}   👤 {player}（你，{job_title}）  💰 {:.0} 元",
        a.wallet
    );
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
    // 习惯（M4）：权重相对基准最突出的行为；自动化打 ∞。
    let habits: Vec<_> = world
        .habit_report(0)
        .into_iter()
        .filter(|r| r.ratio > 1.15 || r.automated)
        .take(4)
        .collect();
    if !habits.is_empty() {
        let parts: Vec<String> = habits
            .iter()
            .map(|h| {
                let name = behavior_name(world, &h.behavior);
                format!(
                    "{} ×{:.2}{}",
                    name,
                    h.ratio,
                    if h.automated { "∞" } else { "" }
                )
            })
            .collect();
        println!("🔁 习惯：{}", parts.join(" · "));
    }
    // 恐惧（M4）：正在生效的负面记忆，作为状态提示（设计稿 4.6）。
    let mut fears: Vec<_> = a
        .memory
        .entries
        .iter()
        .filter(|m| m.valence < 0.0 && m.strength > 0.15)
        .collect();
    fears.sort_by(|x, y| y.strength.total_cmp(&x.strength));
    if !fears.is_empty() {
        let parts: Vec<String> = fears
            .iter()
            .take(2)
            .map(|m| format!("{}（{:.2}）", event_name(world, &m.event), m.strength))
            .collect();
        println!("⚠ 心有余悸：{}", parts.join(" · "));
    }
    // 目标（M4）：进度显示。
    for g in world.goal_status(0) {
        println!(
            "🎯 【{}】还剩 {} 天 · 忍住 {} 次 · 失守 {} 次 · 压制了 {:.1} 点习惯",
            behavior_name(world, &g.behavior),
            g.days_left,
            g.resists,
            g.violations,
            g.suppressed
        );
    }
    // 密友（M4）与恋人（M5 二阶段）。
    if let Some(lid) = world.relations.couple_of(0) {
        if let Some(l) = world.agents.iter().find(|x| x.id == lid) {
            println!(
                "💗 恋人：{}（亲密 {:.2}）",
                l.name,
                world.relations.bond(0, lid)
            );
        }
    }
    let friends = world.close_friends(0);
    if !friends.is_empty() {
        let parts: Vec<String> = friends
            .iter()
            .take(3)
            .map(|(_, name, b)| format!("{name} {:.2}", b))
            .collect();
        println!("💞 密友：{}", parts.join(" · "));
    }
    // 其他居民概览（含 LOD 层级——远处的人过着自己的日子）。
    let lod_name = |l: u8| match l {
        0 => "",
        1 => "（常规）",
        _ => "（背景）",
    };
    for other in world.agents.iter().skip(1).take(5) {
        let act = match &other.current {
            Some((bid, _, _)) => behavior_name(world, bid),
            None => "闲着".to_string(),
        };
        println!("🧍 {}{}: {}", other.name, lod_name(other.lod), act);
    }
    println!("──────────────────────────────────────────────");
}

fn behavior_name(world: &World, id: &str) -> String {
    // M5 二阶段：键可带变体（"行为#变体"），拆开各自查名。
    match id.split_once('#') {
        Some((bid, vid)) => {
            let b = world
                .cfg
                .behaviors
                .iter()
                .find(|x| x.id == bid)
                .map(|x| x.display_name().to_string())
                .unwrap_or_else(|| bid.to_string());
            let v = world
                .cfg
                .behaviors
                .iter()
                .flat_map(|x| x.variants.iter())
                .find(|x| x.id == vid)
                .map(|x| x.display_name().to_string())
                .unwrap_or_else(|| vid.to_string());
            format!("{b}·{v}")
        }
        None => world
            .cfg
            .behaviors
            .iter()
            .find(|b| b.id == id)
            .map(|b| b.display_name().to_string())
            .unwrap_or_else(|| id.to_string()),
    }
}

fn event_name(world: &World, id: &str) -> String {
    let base = id.strip_prefix("目睹:").unwrap_or(id);
    world
        .cfg
        .env_events
        .iter()
        .find(|e| e.id == base)
        .map(|e| e.display_name().to_string())
        .unwrap_or_else(|| id.to_string())
}

fn print_menu(status: &[BehaviorStatus]) {
    println!(
        "  0 ⏩30分钟  9 ⏩4小时  4 📜故事  5 📖日记  6 🔁习惯报告  7 💞密友  8 🎯目标  99 💾存日志退出"
    );
    for (i, s) in status.iter().enumerate() {
        if s.available {
            let marks = format!(
                "{}{}{}",
                if s.automated { "∞" } else { "" },
                if s.feared { "⚠" } else { "" },
                if s.goal_avoided { "🎯" } else { "" },
            );
            let cost = if s.est_cost <= 0.0 {
                "免费".to_string()
            } else {
                format!("≈{:.0}", s.est_cost)
            };
            println!("  {:>2} {}{marks} —— ⚡{cost}", i + 1, s.name);
        } else {
            println!("  {:>2} {} —— {}", i + 1, s.name, s.gate_note);
        }
    }
}

/// 习惯报告（M4 验收的载体）："养成某习惯，因为某经历"。
fn print_habit_report(world: &World) {
    println!("\n📖 {} 的习惯报告", world.agents[0].name);
    for h in world.habit_report(0).into_iter().take(8) {
        let name = behavior_name(world, &h.behavior);
        println!(
            "  {}{}：×{:.2}（基准 {:.1}，执行 {} 次）",
            name,
            if h.automated {
                "【已自动化】"
            } else {
                ""
            },
            h.ratio,
            h.base,
            h.execs
        );
        for m in &h.related_memories {
            println!("      因为：{m}");
        }
    }
    // 口味（M5 二阶段变体级习惯）："总点同一家外卖"。
    let tastes: Vec<(&String, &f64)> = world.agents[0]
        .variant_weights
        .iter()
        .filter(|(_, w)| **w > 1.25)
        .collect();
    if !tastes.is_empty() {
        let mut v = tastes;
        v.sort_by(|x, y| y.1.total_cmp(x.1));
        let parts: Vec<String> = v
            .iter()
            .take(4)
            .map(|(id, w)| format!("{} ×{:.2}", behavior_name(world, id), w))
            .collect();
        println!("  🍜 口味：{}", parts.join(" · "));
    }
    println!();
}

/// 你的故事（M6）：玩家角色的经历回放——大事记按时间排成一段。
fn print_story(world: &mut World, player: &str) {
    let name_of = |id: u32| -> String {
        world
            .agents
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| format!("#{id}"))
    };
    let label = |bid: &str, vid: Option<&str>| -> String {
        let b = world
            .cfg
            .behaviors
            .iter()
            .find(|x| x.id == bid)
            .map(|x| x.display_name().to_string())
            .unwrap_or_else(|| bid.to_string());
        match vid {
            Some(v) => format!(
                "{b}·{}",
                world
                    .cfg
                    .behaviors
                    .iter()
                    .flat_map(|x| x.variants.iter())
                    .find(|x| x.id == v)
                    .map(|x| x.display_name().to_string())
                    .unwrap_or_else(|| v.to_string())
            ),
            None => b,
        }
    };
    let all: Vec<living_core::LogEvent> = world
        .logger
        .buf
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let story = living_core::diary::player_story(&all, 0, name_of, &label);
    println!("\n📜 {player} 的故事");
    if story.is_empty() {
        println!("  （还没有值得写下来的大事）");
    }
    for l in &story {
        println!("  {l}");
    }
    println!();
}

/// 日记（M5 二阶段，设计稿 6.4）：从日志自动生成的今天。
fn print_diary(world: &mut World, player: &str) {
    let day = world.tick.saturating_sub(1) / 1440;
    let events = living_core::diary::events_of_day(world.logger.buf.lines(), day);
    println!("\n📖 {player} 的日记（第 {} 天）", day + 1);
    if events.is_empty() {
        println!("  （今天还没发生什么值得写的）");
        return;
    }
    let name_of = |id: u32| -> String {
        world
            .agents
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| format!("#{id}"))
    };
    let label = |bid: &str, vid: Option<&str>| -> String {
        match vid {
            Some(v) => behavior_name(world, &format!("{bid}#{v}")),
            None => behavior_name(world, bid),
        }
    };
    for l in living_core::diary::day_diary(&events, 0, name_of, &label) {
        println!("  {l}");
    }
    println!();
}

/// 目标子菜单：选行为、定期限（设计稿 4.5：戒断目标）。
fn goal_menu(world: &mut World, lines: &mut io::Lines<io::StdinLock>) {
    let active = world.goal_status(0);
    if !active.is_empty() {
        println!("\n🎯 进行中的目标：");
        for (i, g) in active.iter().enumerate() {
            println!(
                "  {} 【{}】还剩 {} 天 · 忍住 {} · 失守 {} —— 输入 9 放弃（习惯会反弹）",
                i + 1,
                behavior_name(world, &g.behavior),
                g.days_left,
                g.resists,
                g.violations
            );
        }
    } else {
        println!("\n🎯 没有进行中的目标。想对抗哪个习惯？（输入菜单行为的数字，0 返回）");
    }
    let report = world.habit_report(0);
    let mut candidates: Vec<String> = report
        .iter()
        .filter(|r| r.ratio > 1.15)
        .take(4)
        .map(|r| r.behavior.clone())
        .collect();
    // 口味级目标（M5 二阶段："这个月不点麻辣烫"）。
    for (vid, w) in world.agents[0].variant_weights.iter() {
        if *w > 1.35 && candidates.len() < 7 {
            if let Some(owner) = world
                .cfg
                .behaviors
                .iter()
                .find(|b| b.variants.iter().any(|v| v.id == *vid))
            {
                candidates.push(format!("{}#{vid}", owner.id));
            }
        }
    }
    if !candidates.is_empty() {
        let parts: Vec<String> = candidates
            .iter()
            .enumerate()
            .map(|(i, b)| format!("{}={}", i + 1, behavior_name(world, b)))
            .collect();
        println!("  候选（较强的习惯/口味）：{}", parts.join(" "));
    } else {
        println!("  （目前没有明显的习惯——先反复做点什么吧）");
    }
    print!("> ");
    io::stdout().flush().ok();
    let line = match lines.next() {
        Some(Ok(l)) => l,
        _ => return,
    };
    let n: Option<u64> = line.trim().parse().ok();
    match n {
        Some(9) => {
            // 放弃当前第一个目标。
            if let Some(g) = world.goal_status(0).first() {
                let bid = g.behavior.clone();
                match world.abandon_goal(0, &bid) {
                    Ok((name, rebound)) => println!(
                        "💔 放弃了【{}】的目标——旧习惯反弹 +{:.2}（它又想做了）",
                        behavior_name(world, &name),
                        rebound
                    ),
                    Err(e) => println!("放弃失败：{e}"),
                }
            }
        }
        Some(k) if k >= 1 && (k as usize) <= candidates.len() => {
            let bid = candidates[(k - 1) as usize].clone();
            println!("定期限几天？（1–7，直接回车 = 3 天）");
            print!("> ");
            io::stdout().flush().ok();
            let days: u64 = lines
                .next()
                .and_then(|l| l.ok())
                .and_then(|l| l.trim().parse().ok())
                .unwrap_or(3)
                .clamp(1, 7);
            match world.set_goal(0, &bid, days) {
                Ok(()) => println!(
                    "🎯 定下目标：{} 天不做【{}】。它还想做的时候会靠意志力忍住。",
                    days,
                    behavior_name(world, &bid)
                ),
                Err(e) => println!("定不了：{e}"),
            }
        }
        _ => {}
    }
}

/// 密友子菜单（设计稿 4.7）：亲密达标才能看对方内心。
fn friend_menu(world: &mut World, lines: &mut io::Lines<io::StdinLock>) {
    let friends = world.close_friends(0);
    if friends.is_empty() {
        println!("\n💞 还没有密友（亲密 ≥ 0.75）。多和邻居共处（同地点的社交时光）会拉近距离。");
        return;
    }
    println!("\n💞 密友：");
    for (i, (_, name, b)) in friends.iter().enumerate() {
        println!("  {} {}（亲密 {:.2}）", i + 1, name, b);
    }
    println!("  输入编号查看，0 返回。");
    print!("> ");
    io::stdout().flush().ok();
    let line = match lines.next() {
        Some(Ok(l)) => l,
        _ => return,
    };
    let n: Option<u64> = line.trim().parse().ok();
    if let Some(k) = n.filter(|k| *k >= 1 && (*k as usize) <= friends.len()) {
        let (fid, _, _) = &friends[(k - 1) as usize];
        match world.inspect_friend(0, *fid) {
            Ok(v) => {
                let cur = v
                    .current
                    .as_ref()
                    .map(|b| behavior_name(world, b))
                    .unwrap_or_else(|| "闲着".into());
                println!("\n👀 {}（亲密 {:.2}）正在【{}】", v.name, v.bond, cur);
                let nn = &v.needs;
                println!(
                    "   需求：饥饿 {:.0} 精力 {:.0} 娱乐 {:.0} 社交 {:.0} 卫生 {:.0}",
                    nn.hunger, nn.energy, nn.fun, nn.social, nn.hygiene
                );
                if v.fears.is_empty() {
                    println!("   心事：看起来没什么阴影");
                } else {
                    let parts: Vec<String> = v
                        .fears
                        .iter()
                        .map(|(e, s)| format!("{}（{:.2}）", event_name(world, e), s))
                        .collect();
                    println!("   心事：还在怕 —— {}", parts.join(" · "));
                }
            }
            Err(e) => println!("看不了：{e}"),
        }
    }
}

/// 打印 before 之后发生的事件（玩家、街道动态与目标对抗过程）。
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
                println!(
                    "   {} 🌍 发生了【{}】",
                    format_clock(*tick),
                    event_name(world, event)
                );
            }
            LogEvent::Startled { tick, agent, event } => {
                println!(
                    "   {} 😱 {}被【{}】吓了一跳",
                    format_clock(*tick),
                    name_of(*agent),
                    event_name(world, event)
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
                let auto = if factors.habit_auto {
                    "（习惯）"
                } else {
                    ""
                };
                let comfort = if factors.comfort > 0.05 {
                    format!("×安全感{:.1}", factors.comfort)
                } else {
                    String::new()
                };
                println!(
                    "   {} 🐾 {player}开始【{}】{auto}（{src}｜权重{:.1}×需求{:.1}×记忆{:.1}{}）",
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
                    let who = name_of(*agent);
                    let from = source.map(name_of).unwrap_or_default();
                    println!(
                        "   {} 🧠 {who}目睹了{from}的遭遇，也记住了【{}】（显著性{:.2}）",
                        format_clock(*tick),
                        event_name(world, event),
                        salience
                    );
                } else if *agent == 0 {
                    println!(
                        "   {} 🧠 {player}记住了【{}】（显著性{:.2}）",
                        format_clock(*tick),
                        event_name(world, event),
                        salience
                    );
                }
            }
            LogEvent::Theft {
                tick,
                thief,
                victim,
                amount,
                caught,
                ..
            } => {
                let who = name_of(*thief);
                let target = name_of(*victim);
                if *caught {
                    println!(
                        "   {} 🚨 {who}行窃被当场抓获，{amount:.0} 已归还 {target}，将做社区服务",
                        format_clock(*tick)
                    );
                } else {
                    println!(
                        "   {} 🚨 {target}被盗 {amount:.0}（邻居们议论纷纷）",
                        format_clock(*tick)
                    );
                }
            }
            LogEvent::JobChange {
                tick,
                agent,
                from,
                to,
                reason,
            } => {
                let who = name_of(*agent);
                let job_name = |j: &Option<String>| -> String {
                    match j {
                        Some(id) => world
                            .cfg
                            .jobs
                            .iter()
                            .find(|x| x.id == *id)
                            .map(|x| x.display_name().to_string())
                            .unwrap_or_else(|| id.clone()),
                        None => "无业".to_string(),
                    }
                };
                println!(
                    "   {} 📰 {who}从{}变成了{}（{reason}）",
                    format_clock(*tick),
                    job_name(from),
                    job_name(to)
                );
            }
            LogEvent::Exposure {
                tick,
                agent,
                behavior,
                faded,
            } if *agent == 0 => {
                println!(
                    "   {} 💪 {player}硬着头皮做完了【{}】，恐惧淡了一点（-{:.2}）",
                    format_clock(*tick),
                    behavior_name(world, behavior),
                    faded
                );
            }
            LogEvent::GoalSet {
                tick,
                behavior,
                deadline,
                ..
            } => {
                println!(
                    "   {} 🎯 定下目标：【{}】戒到 {}",
                    format_clock(*tick),
                    behavior_name(world, behavior),
                    format_clock(*deadline)
                );
            }
            LogEvent::GoalResist {
                tick,
                behavior,
                cost,
                ..
            } => {
                println!(
                    "   {} 🎯 它想做【{}】，你定了目标——它忍住了（⚡-{:.0}）",
                    format_clock(*tick),
                    behavior_name(world, behavior),
                    cost
                );
            }
            LogEvent::GoalViolation {
                tick,
                behavior,
                commanded,
                ..
            } => {
                let why = if *commanded {
                    "你主动违约"
                } else {
                    "它没忍住"
                };
                println!(
                    "   {} 🎯 【{}】失守了（{why}）",
                    format_clock(*tick),
                    behavior_name(world, behavior)
                );
            }
            LogEvent::GoalEnd {
                tick,
                behavior,
                success,
                abandoned,
                violations,
                resists,
                rebound,
                ..
            } => {
                let verdict = if *abandoned {
                    format!("中途放弃，旧习惯反弹 +{rebound:.2}")
                } else if *success {
                    "成功了！习惯真的改掉了".to_string()
                } else {
                    format!("到期但有 {violations} 次失守")
                };
                println!(
                    "   {} 🎯 【{}】目标结束：{verdict}（忍住 {resists} 次）",
                    format_clock(*tick),
                    behavior_name(world, behavior)
                );
            }
            // M5 二阶段：社交对象选择、恋爱、新生儿、模仿种草。
            LogEvent::SocialPick {
                tick,
                agent,
                target,
                bond,
                freshness,
                impression,
                ..
            } => {
                let who = name_of(*agent);
                let tgt = name_of(*target);
                let why = if *impression < 0.6 {
                    "（其实心里有点膈应）".to_string()
                } else if *freshness < 0.5 {
                    "（刚见过面，又想聊）".to_string()
                } else {
                    format!("（亲密{bond:.2}）")
                };
                println!("   {} 🚶 {who}特意去找{tgt}{why}", format_clock(*tick));
            }
            LogEvent::Couple {
                tick,
                a,
                b,
                started,
            } => {
                if *started {
                    println!(
                        "   {} 💗 {}和{}在一起了！",
                        format_clock(*tick),
                        name_of(*a),
                        name_of(*b)
                    );
                } else {
                    println!(
                        "   {} 💔 {}和{}分开了",
                        format_clock(*tick),
                        name_of(*a),
                        name_of(*b)
                    );
                }
            }
            LogEvent::Born {
                tick,
                agent,
                mother,
                father,
            } => {
                println!(
                    "   {} 👶 {}和{}的家添了新成员{}",
                    format_clock(*tick),
                    name_of(*mother),
                    name_of(*father),
                    name_of(*agent)
                );
            }
            LogEvent::Imitate {
                tick,
                observer,
                from,
                behavior,
                variant,
                ..
            } if *observer == 0 => {
                let key = match variant {
                    Some(v) => format!("{behavior}#{v}"),
                    None => behavior.clone(),
                };
                println!(
                    "   {} 👀 {player}看{}做了【{}】，有点想试",
                    format_clock(*tick),
                    name_of(*from),
                    behavior_name(world, &key)
                );
            }
            _ => {}
        }
    }
    println!();
}
