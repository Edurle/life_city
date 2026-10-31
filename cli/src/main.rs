//! living-cli —— M0 运行器。
//!
//! 常规模式：cargo run -p living-cli -- --seed 42 --cats 6 --days 7 --out logs/demo.jsonl
//! 基准模式：cargo run -p living-cli --release -- --bench
//!   校准技术文档第 1 节的性能预算（决策耗时、模拟一年耗时）。

use living_core::{cat_names, cat_roster, human_names, load_config, World};
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--bench") {
        bench();
        return;
    }

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
    let cats: usize = get("--cats", "6").parse().expect("猫数应为整数");
    let humans: usize = get("--humans", "0").parse().expect("人数应为整数");
    let days: u64 = get("--days", "7").parse().expect("天数应为整数");
    let assets = get("--assets", "assets/cats.json");
    let out = get("--out", "");
    let save_path = get("--save", "");
    let load_path = get("--load", "");

    let json =
        std::fs::read_to_string(&assets).unwrap_or_else(|e| panic!("读取配置失败 {assets}: {e}"));
    let cfg = load_config(&json).unwrap_or_else(|e| panic!("{e}"));

    let ticks = days * 1440;
    let roster = {
        let mut r: Vec<(String, String)> = human_names(humans)
            .into_iter()
            .map(|n| (n, "human".to_string()))
            .collect();
        r.extend(cat_roster(cat_names(cats)));
        r
    };
    let mut world = if load_path.is_empty() {
        let mut w = World::new(cfg, seed, roster, true);
        w.log_meta(seed, ticks);
        w
    } else {
        let bytes =
            std::fs::read(&load_path).unwrap_or_else(|e| panic!("读取存档失败 {load_path}: {e}"));
        let w = World::from_save(cfg, &bytes).unwrap_or_else(|e| panic!("{e}"));
        println!(
            "已从存档 {load_path} 恢复：第 {} 天起继续 {} 天",
            w.tick / 1440 + 1,
            days
        );
        w
    };
    let t0 = Instant::now();
    world.run(ticks);
    let elapsed = t0.elapsed();
    let summaries = world.summarize();

    println!(
        "模拟完成：{} 名居民 × {} 天（{} 拍，{} 次决策），耗时 {:.2}s",
        humans + cats,
        days,
        ticks,
        world.decisions_done,
        elapsed.as_secs_f64()
    );
    println!();

    // M0 验收：同初始条件的猫分化 + 系统报告原因。
    println!("=== 行为分布（与 0 号猫的总变差距离） ===");
    for s in &summaries {
        let mut top: Vec<(String, f64)> = s
            .distribution
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        top.sort_by(|a, b| b.1.total_cmp(&a.1));
        let top3: Vec<String> = top
            .iter()
            .take(3)
            .map(|(k, v)| format!("{} {:.0}%", k, v * 100.0))
            .collect();
        println!(
            "{:<4} 温度{:.2} TV={:.3} | {} | 记忆{}条",
            s.name,
            s.temperature,
            s.tv_distance_to_first,
            top3.join("，"),
            s.memory_count
        );
        for m in &s.top_memories {
            println!("        记忆: {m}");
        }
    }

    // M5 小镇快报：从日志生成的新闻 + 年度统计。
    let news = world.news_digest(8);
    if !news.is_empty() {
        println!("\n=== 小镇快报（最近） ===");
        for n in &news {
            println!("  {n}");
        }
    }
    if world.stats.thefts > 0 || world.stats.job_changes > 0 {
        println!(
            "\n=== 统计 === 盗窃 {} 起（被抓 {}）· 职业变动 {} 次",
            world.stats.thefts, world.stats.thefts_caught, world.stats.job_changes
        );
    }

    if !save_path.is_empty() {
        let path = PathBuf::from(&save_path);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        std::fs::write(&path, world.to_save()).unwrap_or_else(|e| panic!("写存档失败: {e}"));
        println!("存档已写入 {}", path.display());
    }

    if !out.is_empty() {
        let path = PathBuf::from(&out);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        world
            .logger
            .write_to_file(&path)
            .unwrap_or_else(|e| panic!("写日志失败: {e}"));
        println!(
            "\n日志已写入 {}（用 viewer/index.html 打开查看）",
            path.display()
        );
    }
}

/// 性能校准：对应《技术选型与架构》第 1 节的预算。
/// 1) T0 等价负载：128 只猫全模拟，测每次决策耗时（预算 ≤ 20µs）；
/// 2) 全镇等价负载：2000 只猫模拟 30 天，外推一年耗时（预算：单核 ≤ 5 分钟）。
fn bench() {
    let json = std::fs::read_to_string("assets/cats.json").expect("需要 assets/cats.json");
    let cfg = load_config(&json).expect("配置应合法");

    println!("== 基准 1：T0 等价（128 只猫，完整模拟 30 天，含日志关闭） ==");
    let t0 = Instant::now();
    let mut w = World::new(cfg.clone(), 99, cat_roster(cat_names(128)), false);
    w.run(30 * 1440);
    let d1 = t0.elapsed();
    let per_decision = d1.as_secs_f64() / w.decisions_done as f64 * 1e6;
    println!(
        "  决策 {} 次，耗时 {:.2}s → {:.2} µs/决策（预算 ≤ 20）",
        w.decisions_done,
        d1.as_secs_f64(),
        per_decision
    );
    println!(
        "  快进等价：×100 速度下模拟负载占单核 {:.1}%",
        per_decision * 128.0 * 100.0 / 1e4
    );

    println!("== 基准 2：全镇等价（2000 只猫模拟 30 天 → 外推一年） ==");
    let t0 = Instant::now();
    let mut w = World::new(cfg, 99, cat_roster(cat_names(2000)), false);
    w.run(30 * 1440);
    let d2 = t0.elapsed();
    let year = d2.as_secs_f64() * 365.0 / 30.0;
    let prev_decisions = w.decisions_done;
    println!(
        "  30 天耗时 {:.2}s（{} 次决策）→ 外推一年 ≈ {:.1} 分钟（预算 ≤ 5）",
        d2.as_secs_f64(),
        w.decisions_done,
        year / 60.0
    );

    println!("== 基准 3：LOD 分层（2000 只猫 + 玩家，T0/T1/T2 各司其职） ==");
    let json = std::fs::read_to_string("assets/cats.json").expect("需要 assets/cats.json");
    let cfg = load_config(&json).expect("配置应合法");
    let t0 = Instant::now();
    let mut w = World::new(cfg, 99, cat_roster(cat_names(2000)), false);
    w.set_player(0); // 有玩家才启用 LOD；玩家身边 T0，远处降层
    w.run(30 * 1440);
    let d3 = t0.elapsed();
    let year3 = d3.as_secs_f64() * 365.0 / 30.0;
    let tier_counts = w
        .agents
        .iter()
        .fold([0usize; 3], |mut c, a| {
            c[a.lod as usize] += 1;
            c
        });
    println!(
        "  30 天耗时 {:.2}s（{} 次决策，T0={} T1={} T2={}）→ 外推一年 ≈ {:.1} 分钟",
        d3.as_secs_f64(),
        w.decisions_done,
        tier_counts[0],
        tier_counts[1],
        tier_counts[2],
        year3 / 60.0
    );
    println!(
        "  对比基准 2：决策量降到 {:.0}%，耗时 {:.0}%",
        100.0 * w.decisions_done as f64 / prev_decisions as f64,
        100.0 * d3.as_secs_f64() / d2.as_secs_f64()
    );
}
