//! 集成测试：M0 的三条关键性质。
//! 1) 确定性：同种子两次运行，日志字节级一致；
//! 2) 分化：同初始条件的猫，经历不同后行为分布显著分化（验收标准）；
//! 3) 记忆影响分布：恐惧事件后"躲藏"类行为占比上升（系统能报告原因）。

use living_core::{cat_names, load_config, World};
use std::collections::BTreeMap;

fn test_config(noisy: bool) -> String {
    let vacuum = if noisy {
        r#"
    {
      "id": "vacuum_cleaner",
      "tags": ["noise"],
      "interval_min": 120,
      "interval_max": 240,
      "valence": -0.9,
      "intensity": 0.9,
      "behavior_mults": { "hide": 5.0, "explore": 0.3, "play": 0.4 },
      "immune_behavior_tags": ["hide"]
    }"#
    } else {
        r#"
    {
      "id": "quiet_day",
      "tags": ["calm"],
      "interval_min": 120,
      "interval_max": 240,
      "valence": 0.1,
      "intensity": 0.1,
      "behavior_mults": {},
      "immune_behavior_tags": []
    }"#
    };
    format!(
        r#"{{
  "needs": {{"hunger": 4.5, "energy": 3.0, "fun": 5.0, "social": 3.0, "hygiene": 2.0, "bladder": 4.0}},
  "critical_need": 15,
  "params": {{
    "alpha": 0.06, "habit_decay_hourly": 0.02,
    "temperature_min": 0.7, "temperature_max": 1.4,
    "heartbeat_min": 5, "heartbeat_max": 20,
    "memory_cap": 500, "memory_top_k": 8, "weight_jitter": 0.1
  }},
  "spots": [
    {{"id": "bowl", "tags": ["food"]}},
    {{"id": "bed", "tags": ["quiet", "soft"]}},
    {{"id": "windowsill", "tags": ["sun", "view"]}},
    {{"id": "box", "tags": ["hide", "quiet"]}}
  ],
  "behaviors": [
    {{"id": "eat", "base_weight": 3.0, "duration_min": 10, "duration_max": 30,
      "effects": {{"hunger": 50, "energy": 5}}, "tags": ["food"], "spot": "bowl",
      "gate_need": "hunger", "gate_below": 70}},
    {{"id": "sleep", "base_weight": 3.0, "duration_min": 90, "duration_max": 240,
      "effects": {{"energy": 55}}, "tags": ["sleep", "quiet"], "spot": "bed",
      "gate_need": "energy", "gate_below": 45}},
    {{"id": "nap_windowsill", "base_weight": 2.2, "duration_min": 30, "duration_max": 90,
      "effects": {{"energy": 28, "fun": 8}}, "tags": ["sleep", "sun"], "spot": "windowsill",
      "gate_need": "energy", "gate_below": 70}},
    {{"id": "play", "base_weight": 1.6, "duration_min": 15, "duration_max": 45,
      "effects": {{"fun": 32, "energy": -8}}, "tags": ["play"]}},
    {{"id": "hide_box", "base_weight": 1.0, "duration_min": 20, "duration_max": 60,
      "effects": {{"energy": 12, "fun": 4}}, "tags": ["hide", "quiet"], "spot": "box"}},
    {{"id": "explore_room", "base_weight": 1.0, "duration_min": 10, "duration_max": 30,
      "effects": {{"fun": 10, "energy": -4}}, "tags": ["explore"]}}
  ],
  "tree": {{
    "type": "selector",
    "children": [
      {{"type": "gate", "need": "hunger", "below": 70, "child": {{"type": "action", "behavior": "eat"}}}},
      {{"type": "gate", "need": "energy", "below": 45, "child": {{"type": "action", "behavior": "sleep"}}}},
      {{"type": "gate", "need": "energy", "below": 70, "child": {{"type": "action", "behavior": "nap_windowsill"}}}},
      {{"type": "action", "behavior": "play"}},
      {{"type": "action", "behavior": "hide_box"}},
      {{"type": "action", "behavior": "explore_room"}}
    ]
  }},
  "env_events": [{vacuum}]
}}"#
    )
}

fn run_once(json: &str, seed: u64, cats: usize, days: u64) -> World {
    let cfg = load_config(json).unwrap();
    let mut w = World::new(cfg, seed, cat_names(cats), true);
    w.log_meta(seed, days * 1440);
    w.run(days * 1440);
    w
}

#[test]
fn same_seed_produces_identical_logs() {
    let json = &test_config(true);
    let a = run_once(json, 42, 4, 3);
    let b = run_once(json, 42, 4, 3);
    assert_eq!(a.logger.buf, b.logger.buf, "同种子的日志应字节级一致");
    let c = run_once(json, 43, 4, 3);
    assert_ne!(a.logger.buf, c.logger.buf, "不同种子应产生不同日志");
}

#[test]
fn identical_cats_diverge_through_experience() {
    let json = &test_config(true);
    let mut w = run_once(json, 7, 6, 10);
    let summaries = w.summarize();
    // 排除 0 号（TV 恒为 0）；要求至少一半的猫与 0 号分布差异达到可测量水平。
    let diverged = summaries
        .iter()
        .skip(1)
        .filter(|s| s.tv_distance_to_first > 0.15)
        .count();
    assert!(
        diverged >= 2,
        "应有至少 2 只猫与 0 号显著分化（实际 {diverged}），TV: {:?}",
        summaries
            .iter()
            .map(|s| s.tv_distance_to_first)
            .collect::<Vec<_>>()
    );
}

#[test]
fn frightening_events_raise_hiding_share() {
    let mut noisy = run_once(&test_config(true), 11, 8, 10);
    let mut calm = run_once(&test_config(false), 11, 8, 10);

    fn avg_hide(w: &mut World) -> f64 {
        let s = w.summarize();
        s.iter().map(|x| x.distribution.get("hide_box").copied().unwrap_or(0.0)).sum::<f64>()
            / s.len() as f64
    }
    let noisy_share = avg_hide(&mut noisy);
    let calm_share = avg_hide(&mut calm);
    assert!(
        noisy_share > calm_share * 1.5 && noisy_share > 0.08,
        "恐惧环境应显著提高躲藏占比：吵闹 {noisy_share:.3} vs 平静 {calm_share:.3}"
    );
}

#[test]
fn habit_accumulates_and_decays_toward_base() {
    let mut a = living_core::Agent::new(
        1,
        0,
        "测试猫".into(),
        &[("sleep".into(), 3.0)],
        &load_config(&test_config(false)).unwrap().params,
    );
    let w0 = a.weights["sleep"];
    for _ in 0..10 {
        a.habit_boost("sleep", 0.06);
    }
    let w1 = a.weights["sleep"];
    assert!(w1 > w0 * 1.5, "重复执行应累积习惯权重：{w0} -> {w1}");
    for _ in 0..500 {
        a.habit_decay(0.02);
    }
    let w2 = a.weights["sleep"];
    assert!(w2 < w0 * 1.05, "长期不执行应回落到基准附近：{w0} -> {w2}");
}

/// 记忆查询的分布效应（单元级）：写入恐惧后，躲藏行为的选择概率应上升。
#[test]
fn memory_query_shifts_softmax_toward_hide() {
    use living_core::rng::Rng;
    use living_core::world::softmax_pick;

    let cfg = load_config(&test_config(false)).unwrap();
    let mut a = living_core::Agent::new(1, 0, "猫".into(), &[], &cfg.params);
    // 手工写一条强烈恐惧记忆（时段 3 = 夜晚）。
    let mut mults = BTreeMap::new();
    mults.insert("hide".to_string(), 5.0);
    mults.insert("explore".to_string(), 0.3);
    let s = a.memory.write("vacuum", &["noise".into()], 3, 0.9, 1.0, mults, 100);
    assert!(s > 0.5);

    // 夜晚时段下，hide 相对 explore 的选择概率应大幅上升。
    let mem_map = a.memory.query(3, &[]);
    let hide_m = mem_map.get("hide").copied().unwrap_or(1.0);
    let explore_m = mem_map.get("explore").copied().unwrap_or(1.0);
    let w_hide = 1.0 * hide_m;
    let w_explore = 1.0 * explore_m;
    let mut rng = Rng::seeded(5);
    let mut hide_wins = 0;
    for _ in 0..2000 {
        if softmax_pick(&[w_hide, w_explore], 1.0, &mut rng) == 0 {
            hide_wins += 1;
        }
    }
    assert!(
        hide_wins > 1550,
        "恐惧记忆应使躲藏显著占优（{hide_wins}/2000），hide_m={hide_m}, explore_m={explore_m}"
    );
}

// ---------- M1：意志节点与玩家指令 ----------

fn player_world(days_run: u64) -> World {
    let cfg = load_config(&test_config(false)).unwrap();
    let mut w = World::new(cfg, 7, cat_names(2), true);
    w.set_player(0);
    w.log_meta(7, 0);
    w.run(days_run * 1440);
    w
}

#[test]
fn aligned_command_is_cheaper_than_against_one() {
    let mut w = player_world(1);
    let status = w.behaviors_status(0);
    let avail: Vec<_> = status.iter().filter(|s| s.available).collect();
    assert!(!avail.is_empty());
    let max_cost = avail.iter().map(|s| s.est_cost).fold(f64::MIN, f64::max);
    let min_cost = avail.iter().map(|s| s.est_cost).fold(f64::MAX, f64::min);
    // 与当前意愿最合拍的行为 5 点，最逆着来的 25 点（公式 5 + 20×(1−比值)）。
    assert!((min_cost - 5.0).abs() < 0.51, "min {min_cost}");
    assert!(max_cost > min_cost + 5.0, "max {max_cost} min {min_cost}");
}

#[test]
fn insufficient_willpower_rejects_command() {
    let mut w = player_world(1);
    // 找一个贵的可用行为。
    let status = w.behaviors_status(0);
    let target = status
        .iter()
        .filter(|s| s.available)
        .max_by(|a, b| a.est_cost.total_cmp(&b.est_cost))
        .unwrap()
        .clone();
    w.agents[0].willpower = 1.0;
    let out = w.issue_command(0, &target.id);
    assert!(!out.accepted, "应被拒绝：{:?}", out.reason);
    assert!(out.reason.contains("意志力不足"));
    assert!(w.agents[0].pending_command.is_none());
}

#[test]
fn accepted_command_executes_and_costs_willpower() {
    let mut w = player_world(1);
    let status = w.behaviors_status(0);
    let cheap = status
        .iter()
        .find(|s| s.available)
        .map(|s| s.id.clone())
        .unwrap();
    let before = w.agents[0].willpower;
    let out = w.issue_command(0, &cheap);
    assert!(out.accepted, "{:?}", out.reason);
    assert!(w.agents[0].willpower < before);
    w.run(5);
    let (bid, _, _) = w.agents[0].current.clone().expect("指令应在下一拍执行");
    assert_eq!(bid, cheap);
}

#[test]
fn commands_are_replay_deterministic() {
    let script = |w: &mut World| {
        w.run(300);
        w.issue_command(0, "explore_room");
        w.run(200);
        w.issue_command(0, "play");
        w.run(600);
    };
    let mut a = player_world(0);
    script(&mut a);
    let mut b = player_world(0);
    script(&mut b);
    assert_eq!(
        a.logger.buf, b.logger.buf,
        "含玩家指令的运行应字节级一致（指令本身也是事件）"
    );
}

#[test]
fn interrupting_costs_extra_willpower() {
    let mut w = player_world(1);
    // 空闲时下指令 vs 正做事时下同一指令。
    let idle_cost = {
        let status = w.behaviors_status(0);
        status.iter().find(|s| s.available).map(|s| s.id.clone()).unwrap()
    };
    let out_idle = w.issue_command(0, &idle_cost);
    w.run(3); // 开始执行
    assert!(w.agents[0].current.is_some());
    // 换一个行为打断它。
    let status = w.behaviors_status(0);
    let other = status
        .iter()
        .find(|s| s.available && s.id != idle_cost)
        .map(|s| s.id.clone())
        .unwrap();
    let est = status.iter().find(|s| s.id == other).map(|s| s.est_cost).unwrap();
    let out_interrupt = w.issue_command(0, &other);
    assert!(out_interrupt.accepted);
    // 打断成本 = 基础成本 + 8（四舍五入后允许 1 点误差）。
    assert!(
        out_interrupt.cost >= est + 8.0 - 1.0,
        "打断应加价：基础{est} 实际{}",
        out_interrupt.cost
    );
    assert!(out_idle.accepted);
}

// ---------- M1 收尾：存档与验收 ----------

#[test]
fn save_load_resumes_identically() {
    let json = &test_config(true);
    let mut a = World::new(load_config(json).unwrap(), 5, cat_names(2), true);
    a.set_player(0);
    a.run(900);
    a.issue_command(0, "explore_room");
    a.run(300);
    let bytes = a.to_save();
    let offset = a.logger.buf.len();
    a.run(600);
    // 读档继续的另一条时间线：从快照恢复后走完全相同的剩余路程。
    let mut b = World::from_save(load_config(json).unwrap(), &bytes).unwrap();
    b.run(600);
    assert_eq!(
        b.logger.buf, a.logger.buf[offset..],
        "读档继续的日志应与连续运行完全一致（随机数状态必须存进快照）"
    );
}

#[test]
fn snapshot_rejects_version_mismatch() {
    let cfg = load_config(&test_config(false)).unwrap();
    let mut w = World::new(cfg.clone(), 1, cat_names(1), true);
    w.run(10);
    let mut bytes = w.to_save();
    bytes[0] = 99; // 破坏版本号
    assert!(World::from_save(cfg, &bytes).is_err());
}

/// M1 验收场景：它想睡，你让它做别的——量化的意志拉扯。
/// 合心的指令 5 点起步；打断睡觉去做它最不想做的事，明显更贵且必须执行成功。
#[test]
fn m1_acceptance_sleepy_cat_story() {
    let cfg = load_config(&test_config(false)).unwrap();
    let mut w = World::new(cfg, 3, cat_names(1), true);
    w.set_player(0);
    // 等到猫睡着（睡大觉或窗台打盹；能量衰减到门槛后必然发生）。
    let mut sleeping = false;
    for _ in 0..2500 {
        w.tick();
        if let Some((bid, _, _)) = &w.agents[0].current {
            if bid == "sleep" || bid == "nap_windowsill" {
                sleeping = true;
                break;
            }
        }
    }
    assert!(sleeping, "猫应该在两天内入睡（睡大觉或打盹）");

    let status = w.behaviors_status(0);
    let avail: Vec<_> = status.iter().filter(|s| s.available).cloned().collect();
    assert!(!avail.is_empty());
    let top = avail
        .iter()
        .min_by(|a, b| a.est_cost.total_cmp(&b.est_cost))
        .unwrap()
        .clone();
    let against = avail
        .iter()
        .max_by(|a, b| a.est_cost.total_cmp(&b.est_cost))
        .unwrap()
        .clone();
    let play = status.iter().find(|s| s.id == "play").cloned().unwrap();

    // 它睡着时下令"去做它最不想做的事"。
    let before = w.agents[0].willpower;
    let out = w.issue_command(0, &against.id);
    assert!(out.accepted, "{:?}", out.reason);
    let premium = out.cost - against.est_cost;

    println!(
        "[M1 验收] 它睡着的时刻：合心『{}』≈{:.0} 点；玩球『{}』≈{:.0} 点；最逆『{}』≈{:.0} 点；打断睡去做最逆的事共 {:.0} 点（打断费 {:.0}）",
        top.name, top.est_cost, play.name, play.est_cost, against.name, against.est_cost, out.cost, premium
    );
    println!(
        "[M1 验收] 意志力 {:.0} → {:.0}，反应：{}",
        before, w.agents[0].willpower, out.reason
    );
    assert!(
        (7.0..=9.0).contains(&premium),
        "打断加价应约为 8：{premium}"
    );
    assert!(
        out.cost >= top.est_cost + 10.0,
        "打断+逆着意愿应显著贵于合心指令：{} vs {:.0}",
        out.cost,
        top.est_cost
    );
    assert!(w.agents[0].willpower < before, "应扣除意志力");
    // 下一拍真的去做了。
    w.run(3);
    assert_eq!(w.agents[0].current.as_ref().unwrap().0, against.id);
    // 意志力耗尽则拒绝。
    w.agents[0].willpower = 0.0;
    let refused = w.issue_command(0, "explore_room");
    assert!(!refused.accepted);
    println!("[M1 验收] 意志力归零后：{}", refused.reason);
}
