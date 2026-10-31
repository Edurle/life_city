//! 集成测试：M0 的三条关键性质。
//! 1) 确定性：同种子两次运行，日志字节级一致；
//! 2) 分化：同初始条件的猫，经历不同后行为分布显著分化（验收标准）；
//! 3) 记忆影响分布：恐惧事件后"躲藏"类行为占比上升（系统能报告原因）。

use living_core::{cat_names, cat_roster, load_config, World};
use std::collections::BTreeMap;

fn test_config(noisy: bool) -> String {
    let vacuum = if noisy {
        r#"
    {
      "id": "vacuum_cleaner",
      "tags": ["noise"],
      "radius": 2.5,
      "interval_min": 480,
      "interval_max": 960,
      "valence": -0.9,
      "intensity": 0.5,
      "behavior_mults": { "hide": 3.0, "explore": 0.5, "play": 0.6 },
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
    let mut w = World::new(cfg, seed, cat_roster(cat_names(cats)), true);
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
    // M0 验收：同初始条件的猫经历不同事件后行为分布显著分化。
    // 个性吸引子成形有轨迹运气成分，因此对多个种子统计：
    // 每个种子取两两总变差距离的最大值（任意一对猫显著不同即算分化）。
    let json = &test_config(true);
    let mut diverged_seeds = 0;
    for seed in [3u64, 7, 11, 21, 42] {
        let mut w = run_once(json, seed, 6, 10);
        let summaries = w.summarize();
        let mut max_pair_tv = 0.0f64;
        for i in 0..summaries.len() {
            for j in (i + 1)..summaries.len() {
                let d = living_core::tv_distance(
                    &summaries[i].distribution,
                    &summaries[j].distribution,
                );
                max_pair_tv = max_pair_tv.max(d);
            }
        }
        if max_pair_tv > 0.3 {
            diverged_seeds += 1;
        }
        println!("[分化] 种子 {seed}：两两最大 TV={max_pair_tv:.3}");
    }
    assert!(
        diverged_seeds >= 4,
        "5 个种子里应有至少 4 个出现显著分化（实际 {diverged_seeds}）"
    );
}

#[test]
fn frightening_events_raise_hiding_share() {
    let mut noisy = run_once(&test_config(true), 11, 8, 10);
    let mut calm = run_once(&test_config(false), 11, 8, 10);

    fn avg_hide(w: &mut World) -> f64 {
        let s = w.summarize();
        s.iter()
            .map(|x| x.distribution.get("hide_box").copied().unwrap_or(0.0))
            .sum::<f64>()
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
        "cat".into(),
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
    let mut a = living_core::Agent::new(1, 0, "猫".into(), "cat".into(), &[], &cfg.params);
    // 手工写一条强烈恐惧记忆（时段 3 = 夜晚）。
    let mut mults = BTreeMap::new();
    mults.insert("hide".to_string(), 5.0);
    mults.insert("explore".to_string(), 0.3);
    let s = a.memory.write(
        "vacuum",
        &["noise".into()],
        3,
        0.9,
        1.0,
        -0.9,
        false,
        mults,
        100,
    );
    assert!(s > 0.5);

    // 夜晚时段下，hide 相对 explore 的选择概率应大幅上升。
    let mem_map = a.memory.query(3, &[], 0.0);
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
    let mut w = World::new(cfg, 7, cat_roster(cat_names(2)), true);
    w.set_player(0);
    w.log_meta(7, 0);
    w.run(days_run * 1440);
    w
}

#[test]
fn aligned_command_is_cheaper_than_against_one() {
    let mut w = player_world(1);
    let status = w.behaviors_status(0);
    let avail: Vec<_> = status.iter().filter(|s| s.available).cloned().collect();
    assert!(!avail.is_empty());
    let max_cost = avail.iter().map(|s| s.est_cost).fold(f64::MIN, f64::max);
    let min_cost = avail.iter().map(|s| s.est_cost).fold(f64::MAX, f64::min);
    // M4 语义：合拍行为 5 点起步；若一天下来它已把某行为养成自动化习惯
    //（权重 ≥ 基准×1.6），指令免费（习惯成自然）。最逆着来的仍显著更贵。
    let cheapest = avail.iter().find(|s| s.est_cost == min_cost).unwrap();
    if cheapest.automated {
        assert!(
            (min_cost - 0.0).abs() < 1e-9,
            "自动化习惯的指令应免费：{min_cost}"
        );
    } else {
        assert!((min_cost - 5.0).abs() < 0.51, "min {min_cost}");
    }
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
        status
            .iter()
            .find(|s| s.available)
            .map(|s| s.id.clone())
            .unwrap()
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
    let est = status
        .iter()
        .find(|s| s.id == other)
        .map(|s| s.est_cost)
        .unwrap();
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
    let mut a = World::new(
        load_config(json).unwrap(),
        5,
        cat_roster(cat_names(2)),
        true,
    );
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
        b.logger.buf,
        a.logger.buf[offset..],
        "读档继续的日志应与连续运行完全一致（随机数状态必须存进快照）"
    );
}

#[test]
fn snapshot_rejects_version_mismatch() {
    let cfg = load_config(&test_config(false)).unwrap();
    let mut w = World::new(cfg.clone(), 1, cat_roster(cat_names(1)), true);
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
    let mut w = World::new(cfg, 3, cat_roster(cat_names(1)), true);
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

// ---------- M2：猫群——空间、关系、模仿、陪伴 ----------

/// M2 测试配置：带坐标的房间 + 沙发（社交）+ 局部半径的吸尘器。
/// 吸尘器间隔设为极大值——测试用手动触发（trigger_env_event），排除随机排程干扰。
fn m2_config() -> String {
    r#"{
  "needs": {"hunger": 4.5, "energy": 3.0, "fun": 5.0, "social": 3.0, "hygiene": 2.0, "bladder": 4.0},
  "critical_need": 15,
  "params": {
    "alpha": 0.06, "habit_decay_hourly": 0.02,
    "temperature_min": 0.7, "temperature_max": 1.4,
    "heartbeat_min": 5, "heartbeat_max": 20,
    "memory_cap": 500, "memory_top_k": 8, "weight_jitter": 0.1,
    "room_width": 6.0, "room_height": 6.0,
    "witness_radius": 10.0, "comfort_radius": 2.0, "comfort_max": 0.8,
    "imitation_factor": 0.7, "imitation_min_bond": 0.25,
    "startle_min_salience": 0.6,
    "bond_social_gain": 0.08, "bond_player_gain": 0.15, "initial_bond_max": 0.3
  },
  "spots": [
    {"id": "bowl", "tags": ["food"], "x": 1.0, "y": 4.5},
    {"id": "bed", "tags": ["quiet", "soft"], "x": 4.5, "y": 1.0},
    {"id": "windowsill", "tags": ["sun", "view"], "x": 0.3, "y": 3.0},
    {"id": "box", "tags": ["hide", "quiet"], "x": 0.5, "y": 0.5},
    {"id": "sofa", "tags": ["soft", "social"], "x": 3.0, "y": 5.5}
  ],
  "behaviors": [
    {"id": "eat", "base_weight": 3.0, "duration_min": 10, "duration_max": 30,
     "effects": {"hunger": 50, "energy": 5}, "tags": ["food"], "spot": "bowl",
     "gate_need": "hunger", "gate_below": 70},
    {"id": "sleep", "base_weight": 3.0, "duration_min": 90, "duration_max": 240,
     "effects": {"energy": 55}, "tags": ["sleep", "quiet"], "spot": "bed",
     "gate_need": "energy", "gate_below": 45},
    {"id": "nap_windowsill", "base_weight": 2.2, "duration_min": 30, "duration_max": 90,
     "effects": {"energy": 28, "fun": 8}, "tags": ["sleep", "sun"], "spot": "windowsill",
     "gate_need": "energy", "gate_below": 70},
    {"id": "play", "base_weight": 1.6, "duration_min": 15, "duration_max": 45,
     "effects": {"fun": 32, "energy": -8}, "tags": ["play"]},
    {"id": "hide_box", "base_weight": 1.0, "duration_min": 20, "duration_max": 60,
     "effects": {"energy": 12, "fun": 4}, "tags": ["hide", "quiet"], "spot": "box"},
    {"id": "explore_room", "base_weight": 1.0, "duration_min": 10, "duration_max": 30,
     "effects": {"fun": 10, "energy": -4}, "tags": ["explore"]},
    {"id": "cuddle_sofa", "base_weight": 1.5, "duration_min": 10, "duration_max": 30,
     "effects": {"social": 32, "fun": 5}, "tags": ["social"], "spot": "sofa",
     "gate_need": "social", "gate_below": 60}
  ],
  "tree": {
    "type": "selector",
    "children": [
      {"type": "gate", "need": "hunger", "below": 70, "child": {"type": "action", "behavior": "eat"}},
      {"type": "gate", "need": "energy", "below": 45, "child": {"type": "action", "behavior": "sleep"}},
      {"type": "gate", "need": "energy", "below": 70, "child": {"type": "action", "behavior": "nap_windowsill"}},
      {"type": "gate", "need": "social", "below": 60, "child": {"type": "action", "behavior": "cuddle_sofa"}},
      {"type": "action", "behavior": "play"},
      {"type": "action", "behavior": "hide_box"},
      {"type": "action", "behavior": "explore_room"}
    ]
  },
  "env_events": [
    {
      "id": "vacuum_cleaner", "name": "吸尘器",
      "tags": ["noise"], "radius": 2.5,
      "interval_min": 1000000, "interval_max": 1000000,
      "valence": -0.9, "intensity": 0.9,
      "behavior_mults": {"hide": 5.0, "explore": 0.3, "play": 0.4},
      "immune_behavior_tags": ["hide"]
    }
  ]
}"#.to_string()
}

/// 惊吓：足够新颖的强负面事件打断当前行为并立即重新决策。
#[test]
fn startle_interrupts_current_behavior() {
    let cfg = load_config(&m2_config()).unwrap();
    let mut w = World::new(cfg, 5, cat_roster(cat_names(1)), true);
    w.collect_recent = true;
    // 等猫开始做某件事。
    for _ in 0..3000 {
        w.tick();
        if w.agents[0].current.is_some() {
            break;
        }
    }
    assert!(w.agents[0].current.is_some(), "猫应该开始做事了");
    // 在猫的位置触发吸尘器（首次，显著性 0.9 ≥ 惊吓门槛 0.6）。
    let pos = w.agent_pos(0).unwrap();
    w.trigger_env_event(0, pos);
    assert!(w.agents[0].current.is_none(), "惊吓应打断当前行为");
    assert!(
        w.recent_events
            .iter()
            .any(|e| matches!(e, living_core::LogEvent::Startled { .. })),
        "应有惊吓日志"
    );
    // 下一拍立即重新决策。
    w.tick();
    assert!(w.agents[0].current.is_some(), "被吓到的猫应立刻重新决策");
}

/// 躲藏中的猫不受吸尘器影响（免疫），也不会被惊吓。
#[test]
fn hiding_cat_is_immune() {
    let cfg = load_config(&m2_config()).unwrap();
    let mut w = World::new(cfg, 5, cat_roster(cat_names(1)), true);
    w.collect_recent = true;
    w.agents[0].pending_command = Some("hide_box".into());
    w.agents[0].next_heartbeat = w.tick + 1;
    w.run(2);
    assert_eq!(w.agents[0].current.as_ref().unwrap().0, "hide_box");
    let mem_before = w.agents[0].memory.entries.len();
    let pos = w.agent_pos(0).unwrap();
    w.trigger_env_event(0, pos);
    assert!(w.agents[0].current.is_some(), "躲着的猫不应被打断");
    assert_eq!(
        w.agents[0].memory.entries.len(),
        mem_before,
        "免疫 = 不写入记忆"
    );
}

/// 目睹传播（模仿节点）：亲近的猫目睹同伴被吓会学到间接记忆，陌生猫不会。
#[test]
fn vicarious_memory_spread_through_bond() {
    let cfg = load_config(&m2_config()).unwrap();
    let mut w = World::new(cfg, 5, cat_roster(cat_names(3)), true);
    w.collect_recent = true;
    // 0=目击者，1=受害者，2=陌生对照。清除全部初始关系，只留 0-1 亲密。
    w.set_bond(0, 1, 0.9);
    w.set_bond(0, 2, 0.0);
    w.set_bond(1, 2, 0.0);
    // 受害者在纸箱附近；目击者在房间另一头（吸尘器半径外 → 无直接经历，
    // 但目睹半径覆盖全房间 → 能看见受害者的反应）；对照猫也远离。
    w.teleport(1, (0.5, 0.5));
    w.teleport(0, (5.5, 5.0));
    w.teleport(2, (5.0, 1.0));
    w.trigger_env_event(0, (0.5, 0.5));
    // 0 号学到间接记忆，1 号直接经历，2 号什么都没有。
    let has = |id: usize, sub: &str| {
        w.agents[id]
            .memory
            .entries
            .iter()
            .any(|m| m.event.contains(sub))
    };
    assert!(has(1, "vacuum"), "受害者应有直接记忆");
    assert!(has(0, "目睹:vacuum"), "亲密目击者应有间接记忆");
    assert!(
        !has(0, "event")
            || !w.agents[0]
                .memory
                .entries
                .iter()
                .any(|m| m.event == "vacuum_cleaner"),
        "目击者在半径外不应有直接记忆"
    );
    assert!(
        w.agents[0]
            .memory
            .entries
            .iter()
            .all(|m| m.vicarious || m.event != "vacuum_cleaner"),
        "目击者的吸尘器记忆必须是间接的"
    );
    assert!(
        !has(2, "vacuum") && !has(2, "目睹"),
        "陌生对照猫不应学到任何东西"
    );
    // 日志能报告原因（目睹来源）。
    assert!(w.recent_events.iter().any(|e| matches!(
        e,
        living_core::LogEvent::MemoryWrite {
            vicarious: true,
            source: Some(1),
            ..
        }
    )));
}

/// M2 验收一：目睹同伴被吸尘器惊吓后，目击者的躲藏占比显著上升
/// （对比同种子无亲密关系的世界，差异只能来自目睹所学）。
/// 场景：四个时段各目睹一次（不同时段的遭遇写进不同记忆槽）。
#[test]
fn witnessed_fear_raises_hiding_share() {
    let hide_share = |bond: f64, seed: u64| -> f64 {
        let cfg = load_config(&m2_config()).unwrap();
        let mut w = World::new(cfg, seed, cat_roster(cat_names(3)), false);
        w.set_bond(0, 1, bond);
        w.set_bond(0, 2, 0.0);
        w.set_bond(1, 2, 0.0);
        w.teleport(1, (0.5, 0.5));
        w.teleport(0, (5.5, 5.0));
        w.teleport(2, (5.0, 1.0));
        // 先跑一小段让猫安顿下来，再在四个时段各触发一次。
        w.run(600);
        for _ in 0..4 {
            w.run(360);
            w.agents[1].current = None; // 受害者若在躲藏会免疫（无可目睹反应），排除
            w.teleport(1, (0.5, 0.5)); // 受害者可能已移动，固定回事发点
            w.trigger_env_event(0, (0.5, 0.5));
        }
        w.run(3 * 1440);
        let total: u64 = w.agents[0].exec_counts.values().sum();
        w.agents[0]
            .exec_counts
            .get("hide_box")
            .copied()
            .unwrap_or(0) as f64
            / total as f64
    };
    // 多种子取均值，排除单条轨迹的噪声。
    let seeds = [5u64, 9, 13, 17];
    let mut close = 0.0;
    let mut stranger = 0.0;
    for &s in &seeds {
        close += hide_share(0.9, s);
        stranger += hide_share(0.0, s);
    }
    close /= seeds.len() as f64;
    stranger /= seeds.len() as f64;
    println!("[M2 验收] 四时段目睹后的躲藏占比：亲密 {close:.3} vs 陌生 {stranger:.3}");
    assert!(
        close > stranger + 0.05,
        "目睹亲近同伴被惊吓应显著提高躲藏占比：{close:.3} vs {stranger:.3}"
    );
}

/// M2 验收二：玩家的陪伴降低恐惧的影响（安全感压制负面记忆）。
#[test]
fn companionship_reduces_avoidance() {
    let make = || {
        let cfg = load_config(&m2_config()).unwrap();
        let mut w = World::new(cfg, 5, cat_roster(cat_names(2)), true);
        w.set_player(0);
        w.set_bond(0, 1, 1.0);
        w.teleport(1, (3.0, 3.0));
        w
    };
    // 独处：玩家在房间另一头。
    let mut alone = make();
    alone.teleport(0, (0.2, 0.2));
    alone.trigger_env_event(0, (3.0, 3.0));
    // 陪伴：玩家就在身边。
    let mut accompanied = make();
    accompanied.teleport(0, (3.2, 3.0));
    accompanied.trigger_env_event(0, (3.0, 3.0));

    let comfort_alone = alone.agent_comfort(1);
    let comfort_with = accompanied.agent_comfort(1);
    assert!((comfort_alone - 0.0).abs() < 1e-9, "玩家不在身边应无安全感");
    assert!(
        (comfort_with - 0.8).abs() < 1e-9,
        "亲密 1.0 的玩家在身边应给满安全感 0.8"
    );

    let mem_mult = |w: &mut World| -> f64 {
        w.debug_options(1)
            .iter()
            .find(|o| o.behavior == "hide_box")
            .unwrap()
            .mem_mult
    };
    let afraid_alone = mem_mult(&mut alone);
    let afraid_comforted = mem_mult(&mut accompanied);
    println!(
        "[M2 验收] 直接被吸尘器吓到后，躲藏记忆乘数：独处 {afraid_alone:.2} vs 陪伴 {afraid_comforted:.2}（安全感 {comfort_with}）"
    );
    assert!(
        afraid_comforted < afraid_alone * 0.6,
        "玩家陪伴应显著压制恐惧记忆：{afraid_alone:.2} -> {afraid_comforted:.2}"
    );
    // 两只猫都真的写了恐惧记忆（对照有效）。
    assert!(alone.agents[1]
        .memory
        .entries
        .iter()
        .any(|m| m.event == "vacuum_cleaner"));
}

/// 共同度过社交时光（同地点完成社交行为）增长亲密值。
#[test]
fn bond_grows_on_shared_social_behavior() {
    let cfg = load_config(&m2_config()).unwrap();
    let mut w = World::new(cfg, 5, cat_roster(cat_names(2)), true);
    w.collect_recent = true;
    // 两只猫社交需求都低（门开），同一拍开始蹭沙发。
    for a in w.agents.iter_mut() {
        a.needs.social = 30.0;
        a.pending_command = Some("cuddle_sofa".into());
        a.next_heartbeat = w.tick + 1;
    }
    let before = w.bond(0, 1);
    w.run(60);
    let after = w.bond(0, 1);
    assert!(
        after > before,
        "同处沙发的社交行为应增长亲密：{before} -> {after}"
    );
    assert!(
        w.recent_events
            .iter()
            .any(|e| matches!(e, living_core::LogEvent::BondChange { .. })),
        "应有亲密变化日志"
    );
}

/// M2 存档回环：关系与位置进快照，读档后运行与连续运行一致。
#[test]
fn save_load_m2_roundtrip() {
    let json = m2_config();
    let cfg = load_config(&json).unwrap();
    let mut a = World::new(cfg.clone(), 5, cat_roster(cat_names(3)), true);
    a.run(600);
    a.set_bond(0, 1, 0.77);
    a.teleport(2, (1.5, 2.5));
    let bytes = a.to_save();
    let mut b = World::from_save(cfg, &bytes).unwrap();
    assert_eq!(b.bond(0, 1), 0.77, "亲密值应进快照");
    assert_eq!(b.agent_pos(2), Some((1.5, 2.5)), "位置应进快照");
    // 继续运行日志一致（网格从位置重建，不影响确定性）。
    let offset = a.logger.buf.len();
    a.trigger_env_event(0, (3.0, 3.0));
    b.trigger_env_event(0, (3.0, 3.0));
    a.run(500);
    b.run(500);
    assert_eq!(
        b.logger.buf,
        a.logger.buf[offset..],
        "读档继续的日志应与连续运行一致"
    );
}

// ---------- M3：一条街——昼夜、物种、地点声誉 ----------

/// M3 测试配置：公园 + 住宅两个区域，人类居民，公园滋事/音乐会事件
/// （间隔设为极大值，测试手动触发，排除随机排程干扰）。
fn m3_config() -> String {
    r#"{
  "needs": {"hunger": 4.5, "energy": 3.0, "fun": 5.0, "social": 3.0, "hygiene": 2.0, "bladder": 4.0},
  "critical_need": 15,
  "params": {
    "alpha": 0.06, "habit_decay_hourly": 0.02,
    "temperature_min": 0.7, "temperature_max": 1.4,
    "heartbeat_min": 5, "heartbeat_max": 20,
    "memory_cap": 500, "memory_top_k": 8, "weight_jitter": 0.1,
    "map_width": 20.0, "map_height": 10.0,
    "witness_radius": 10.0, "comfort_radius": 2.0, "comfort_max": 0.8,
    "imitation_factor": 0.7, "imitation_min_bond": 0.25, "startle_min_salience": 0.6,
    "bond_social_gain": 0.08, "bond_player_gain": 0.15, "initial_bond_max": 0.3,
    "area_exp_alpha": 0.15, "area_mult_min": 0.35, "area_mult_span": 1.1,
    "visit_litter": 0.004, "clean_daily": 0.35
  },
  "areas": [
    {"id": "park", "name": "公园", "x": 0, "y": 0, "w": 10, "h": 10},
    {"id": "home", "name": "住宅", "x": 10, "y": 0, "w": 10, "h": 10, "litter_scale": 0.1}
  ],
  "spots": [
    {"id": "lawn", "tags": ["park", "outdoor"], "x": 5, "y": 5, "area": "park"},
    {"id": "bench", "tags": ["park", "outdoor"], "x": 3, "y": 3, "area": "park"},
    {"id": "bed_h", "tags": ["home", "sleep"], "x": 15, "y": 5, "area": "home"},
    {"id": "table", "tags": ["home", "food"], "x": 14, "y": 5, "area": "home"},
    {"id": "sofa_h", "tags": ["home"], "x": 12, "y": 2, "area": "home"}
  ],
  "behaviors": [
    {"id": "sleep_home", "name": "在家睡觉", "base_weight": 3.0, "duration_min": 300, "duration_max": 480,
     "effects": {"energy": 55}, "tags": ["sleep", "home"], "spot": "bed_h"},
    {"id": "eat_home", "name": "在家吃饭", "base_weight": 3.0, "duration_min": 20, "duration_max": 40,
     "effects": {"hunger": 50, "fun": 5}, "tags": ["food", "home"], "spot": "table",
     "gate_need": "hunger", "gate_below": 70},
    {"id": "walk_park", "name": "逛公园", "base_weight": 2.2, "duration_min": 30, "duration_max": 90,
     "effects": {"fun": 20, "energy": -5}, "tags": ["park", "outdoor"], "spot": "lawn"},
    {"id": "bench_sit", "name": "长椅小坐", "base_weight": 1.5, "duration_min": 20, "duration_max": 60,
     "effects": {"fun": 8, "energy": 8}, "tags": ["park", "outdoor"], "spot": "bench"},
    {"id": "watch_tv", "name": "看电视", "base_weight": 1.2, "duration_min": 30, "duration_max": 90,
     "effects": {"fun": 15, "social": 5}, "tags": ["relax", "home"], "spot": "sofa_h"}
  ],
  "tree": {
    "type": "selector",
    "children": [
      {"type": "gate", "need": "hunger", "below": 70, "child": {"type": "action", "behavior": "eat_home"}},
      {"type": "time_gate", "from": 22, "to": 6, "child": {"type": "gate", "need": "energy", "below": 80, "child": {"type": "action", "behavior": "sleep_home"}}},
      {"type": "gate", "need": "energy", "below": 45, "child": {"type": "action", "behavior": "sleep_home"}},
      {"type": "time_gate", "from": 6, "to": 21, "child": {"type": "selector", "children": [
        {"type": "action", "behavior": "walk_park"},
        {"type": "action", "behavior": "bench_sit"}
      ]}},
      {"type": "action", "behavior": "watch_tv"}
    ]
  },
  "env_events": [
    {
      "id": "park_rowdy", "name": "公园醉汉滋事",
      "tags": ["scary", "crowd"], "area": "park", "radius": 4.0,
      "interval_min": 1000000000, "interval_max": 1000000000,
      "valence": -0.8, "intensity": 0.75,
      "behavior_mults": {"park": 0.4, "outdoor": 0.7},
      "immune_behavior_tags": []
    },
    {
      "id": "park_concert", "name": "公园音乐会",
      "tags": ["music", "crowd"], "area": "park", "radius": 4.0,
      "interval_min": 1000000000, "interval_max": 1000000000,
      "valence": 0.8, "intensity": 0.6,
      "behavior_mults": {"park": 1.6},
      "immune_behavior_tags": []
    }
  ]
}"#.to_string()
}

fn m3_world(seed: u64, humans: usize) -> World {
    let cfg = load_config(&m3_config()).unwrap();
    let roster = living_core::human_names(humans)
        .into_iter()
        .map(|n| (n, "human".to_string()))
        .collect();
    let mut w = World::new(cfg, seed, roster, false);
    w.log_meta(seed, 0);
    w
}

/// 公园行为的执行次数（逛公园 + 长椅小坐）。
fn park_execs(w: &World) -> u64 {
    w.agents
        .iter()
        .map(|a| {
            a.exec_counts.get("walk_park").copied().unwrap_or(0)
                + a.exec_counts.get("bench_sit").copied().unwrap_or(0)
        })
        .sum()
}

/// M3 验收：公园声誉下降和恢复的完整循环可以复现。
/// 基线（无事件）→ 公园连续滋事 → 声誉跌落、到访减少 → 事件停止 →
/// 到访体验把声誉拉回、到访回升。衰退与恢复由同一套统计机制产生。
#[test]
fn m3_acceptance_park_reputation_cycle() {
    let mut w = m3_world(11, 6);
    // 基线：安定的两天。
    w.run(2 * 1440);
    let rep_base = w.area_states["park"].reputation;
    let execs_base = park_execs(&w);
    assert!(rep_base > 0.5, "基线声誉应处于高位：{rep_base}");
    assert!(execs_base > 0, "居民应该会去公园");

    // 冲击：公园连续三起滋事（受害者被固定在草坪上，确保直接受影响）。
    let rowdy = 0; // env_events 下标
    for _ in 0..3 {
        w.run(360);
        for id in [0u32, 1] {
            w.agents[id as usize].current = None;
            w.teleport(id, (5.0, 5.0));
        }
        w.trigger_env_event(rowdy, (5.0, 5.0));
    }
    let rep_low = w.area_states["park"].reputation;
    println!("[M3 验收] 声誉：基线 {rep_base:.2} → 冲击后 {rep_low:.2}");
    assert!(
        rep_low < rep_base - 0.15,
        "连续滋事应显著压低公园声誉：{rep_base:.2} -> {rep_low:.2}"
    );

    // 事件停止后的第二天：到访量应显著少于基线（记忆抑制 + 声誉乘数双通道）。
    let execs_after_shock_start = park_execs(&w);
    w.run(1440);
    let execs_shock_day = park_execs(&w) - execs_after_shock_start;
    let baseline_per_day = execs_base as f64 / 2.0;
    println!("[M3 验收] 公园到访：基线 {baseline_per_day:.0}/天 → 事件后 {execs_shock_day}/天");
    assert!(
        (execs_shock_day as f64) < baseline_per_day * 0.6,
        "声誉低谷期到访应明显减少：{execs_shock_day} vs {baseline_per_day:.0}"
    );

    // 恢复：连续四天无事发生（外加一场音乐会），口碑被良好的到访体验拉回。
    let concert = 1;
    w.run(720);
    w.trigger_env_event(concert, (5.0, 5.0));
    w.run(4 * 1440);
    let rep_end = w.area_states["park"].reputation;
    let execs_end = park_execs(&w);
    let recovery_per_day = (execs_end - execs_shock_day - execs_after_shock_start) as f64 / 4.0;
    println!("[M3 验收] 恢复期：声誉 {rep_low:.2} → {rep_end:.2}，到访 {recovery_per_day:.1}/天");
    assert!(
        rep_end > rep_low + 0.15,
        "平静与良好体验应恢复声誉：{rep_low:.2} -> {rep_end:.2}"
    );
    assert!(
        recovery_per_day > execs_shock_day as f64,
        "声誉恢复后到访应回升：{recovery_per_day:.1} vs {execs_shock_day}"
    );
}

/// 昼夜：夜间睡觉可用、白天公园行为可用（时间门双向起作用）。
#[test]
fn day_night_gates_flip_behavior_availability() {
    let mut w = m3_world(3, 2);
    // 跑到深夜 23:00。
    while living_core::time::hour_of(w.tick) != 23 {
        w.tick();
    }
    let night_opts = w.debug_options(0);
    assert!(
        night_opts.iter().any(|o| o.behavior == "sleep_home"),
        "夜间应可睡觉"
    );
    assert!(
        !night_opts.iter().any(|o| o.behavior == "walk_park"),
        "夜间不该逛公园"
    );
    // 跑到白天 10:00。
    while living_core::time::hour_of(w.tick) != 10 {
        w.tick();
    }
    let day_opts = w.debug_options(0);
    assert!(
        day_opts.iter().any(|o| o.behavior == "walk_park"),
        "白天应可逛公园"
    );
}

/// 地点声誉反馈进行为权重：声誉跌落 → 公园行为的地点乘数变小。
#[test]
fn reputation_feeds_back_into_behavior_weights() {
    let mut w = m3_world(3, 1);
    w.run(600);
    let before = w
        .debug_options(0)
        .iter()
        .find(|o| o.behavior == "walk_park")
        .unwrap()
        .area_mult;
    w.area_states.get_mut("park").unwrap().experience(-1.0, 1.0); // 声誉打到谷底
    let after = w
        .debug_options(0)
        .iter()
        .find(|o| o.behavior == "walk_park")
        .unwrap()
        .area_mult;
    println!("[M3] 公园行为地点乘数：声誉好 {before:.2} → 声誉差 {after:.2}");
    assert!(
        after < before * 0.6,
        "声誉差应压制公园行为：{before:.2} -> {after:.2}"
    );
}

/// 时间窗事件不在窗内时顺延到下一个窗口起点（夜间施工白天不发生）。
#[test]
fn time_windowed_event_reschedules() {
    let cfg = load_config(&m3_config_with_noise()).unwrap();
    let mut w = World::new(cfg, 5, cat_roster(cat_names(1)), false);
    // 跑到中午 12:00（世界从午夜起算，先离开夜间窗口）。
    w.run(720);
    // 把夜间施工排到当前拍——白天不在窗内，应顺延到今晚 22:00。
    w.schedule_event_at(0, w.tick + 1);
    w.run(3);
    let next = w.event_next_at(0);
    let hour = living_core::time::hour_of(next);
    assert_eq!(hour, 22, "夜间事件应顺延到 22 点：下一拍 {next}");
}

fn m3_config_with_noise() -> String {
    m3_config().replace(
        r#""id": "park_rowdy", "name": "公园醉汉滋事",
      "tags": ["scary", "crowd"], "area": "park", "radius": 4.0,
      "interval_min": 1000000000, "interval_max": 1000000000,"#,
        r#""id": "night_work", "name": "夜间施工",
      "tags": ["noise"], "area": "home", "radius": 4.0,
      "from_hour": 22, "to_hour": 6,
      "interval_min": 1000000000, "interval_max": 1000000000,"#,
    )
}

/// 多物种：人类和猫用不同的树（street 内容冒烟）。
#[test]
fn species_use_different_trees() {
    let json = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../assets/street.json"
    ))
    .expect("需要 assets/street.json");
    let cfg = load_config(&json).unwrap();
    let mut roster: Vec<(String, String)> = living_core::human_names(2)
        .into_iter()
        .map(|n| (n, "human".to_string()))
        .collect();
    roster.extend(cat_roster(cat_names(2)));
    let mut w = World::new(cfg, 7, roster, true);
    w.run(2000);
    // 人类执行过人类行为，猫执行过猫行为，且互不串树。
    let human_behaviors: Vec<&String> = w.agents[0].exec_counts.keys().collect();
    let cat_behaviors: Vec<&String> = w.agents[2].exec_counts.keys().collect();
    assert!(human_behaviors
        .iter()
        .any(|b| b.as_str() == "walk_park" || b.as_str() == "watch_tv"));
    assert!(cat_behaviors
        .iter()
        .any(|b| b.as_str() == "play_tree" || b.as_str() == "sleep_cat"));
    assert!(
        !human_behaviors.iter().any(|b| b.as_str() == "play_tree"),
        "人类不执行猫行为"
    );
    assert!(
        !cat_behaviors.iter().any(|b| b.as_str() == "watch_tv"),
        "猫不执行人类行为"
    );
}

// ---------- M4：玩家的人生——LOD、习惯自动化、恐惧恢复、密友、限期目标 ----------

/// M4 测试配置：在 M3 配置上显式给小的 LOD 半径（地图 20×10，
/// 默认 t1=25 会把全图装进 T1，测不出分层），事件间隔极大值（手动触发）。
fn m4_config() -> String {
    m3_config().replace(
        r#""area_exp_alpha": 0.15, "area_mult_min": 0.35, "area_mult_span": 1.1,
    "visit_litter": 0.004, "clean_daily": 0.35"#,
        r#""area_exp_alpha": 0.15, "area_mult_min": 0.35, "area_mult_span": 1.1,
    "visit_litter": 0.004, "clean_daily": 0.35,
    "lod_t0_radius": 2.0, "lod_t1_radius": 4.0, "lod_t0_cap": 3, "lod_t1_cap": 3,
    "habit_auto_ratio": 1.6, "fear_mem_mult": 0.75, "fear_command_cost": 10.0,
    "exposure_decay": 0.88, "close_friend_bond": 0.75,
    "goal_resist_cost": 6.0, "goal_break_cost": 15.0, "goal_decay_mult": 4.0"#,
    )
}

fn m4_world(seed: u64, humans: usize) -> World {
    let cfg = load_config(&m4_config()).unwrap();
    let roster = living_core::human_names(humans)
        .into_iter()
        .map(|n| (n, "human".to_string()))
        .collect();
    let mut w = World::new(cfg, seed, roster, true);
    w.set_player(0);
    w.log_meta(seed, 0);
    w
}

/// LOD 分配是状态的确定性函数：距离与关系决定层级，同状态两次调用结果一致。
#[test]
fn lod_assignment_by_distance_and_bond() {
    let mut w = m4_world(3, 4);
    // 玩家在公园草坪 (5,5)；1 号在身边（T0），2 号在中距离（T1），3 号很远（T2）。
    w.teleport(0, (5.0, 5.0));
    w.teleport(1, (5.3, 5.0));
    w.teleport(2, (7.5, 5.0));
    w.teleport(3, (18.0, 1.0));
    w.refresh_lod();
    assert_eq!(w.agent_lod(0), 0, "玩家恒为 T0");
    assert_eq!(w.agent_lod(1), 0, "身边的居民完整模拟");
    assert_eq!(w.agent_lod(2), 1, "中距离居民常规模拟");
    assert_eq!(w.agent_lod(3), 2, "远处居民按天批量结算");
    // 关系提升层级：3 号是玩家的密友 → 至少 T0（剧情关联者）。
    w.set_bond(0, 3, 0.9);
    w.refresh_lod();
    assert_eq!(w.agent_lod(3), 0, "密友跨街也保持完整模拟");
    // 确定性：同状态重算，层级不变。
    let before: Vec<u8> = w.agents.iter().map(|a| a.lod).collect();
    w.refresh_lod();
    let after: Vec<u8> = w.agents.iter().map(|a| a.lod).collect();
    assert_eq!(before, after, "LOD 分配应是状态的确定性函数");
}

/// 数量上限：T0 超额按 (距离, 编号) 降层，不随结算顺序漂移。
#[test]
fn lod_cap_demotes_excess() {
    let mut w = m4_world(5, 7);
    w.teleport(0, (5.0, 5.0));
    for i in 1..7 {
        w.teleport(i, (5.0 + i as f64 * 0.1, 5.0)); // 全部挤进 T0 半径
    }
    w.refresh_lod();
    let t0 = w.agents.iter().filter(|a| a.lod == 0).count();
    assert_eq!(t0, 3, "T0 上限 3 = 玩家 + 最近的两位");
    let t1 = w.agents.iter().filter(|a| a.lod == 1).count();
    assert_eq!(t1, 3, "T1 上限 3（T0 溢出者按距离取前 3）");
    assert_eq!(
        w.agents.iter().filter(|a| a.lod == 2).count(),
        1,
        "其余背景"
    );
}

/// 降层不丢数据：T2 的岁月都在——升回 T0 时记忆、习惯、关系全保留。
#[test]
fn lod_promotion_keeps_history() {
    let mut w = m4_world(7, 2);
    w.teleport(0, (0.5, 0.5));
    w.teleport(1, (19.0, 9.0)); // 远处 → T2
    w.set_bond(0, 1, 0.6);
    // T2 的 1 号被事件触及（记忆写入仍然发生——被事件触及其唤醒数据）。
    w.trigger_env_event(0, (19.0, 9.0));
    assert!(w.agents[1]
        .memory
        .entries
        .iter()
        .any(|m| m.event == "park_rowdy"));
    let mem_n = w.agents[1].memory.entries.len();
    let w0 = w.agents[1].weights["walk_park"];
    // 玩家走过去 → 1 号升入 T0。
    w.teleport(0, (19.2, 9.0));
    w.refresh_lod();
    assert_eq!(w.agent_lod(1), 0, "玩家靠近后升层");
    assert_eq!(w.agents[1].memory.entries.len(), mem_n, "记忆全保留");
    assert!((w.agents[1].weights["walk_park"] - w0).abs() < 1e-9, "习惯权重保留");
    assert_eq!(w.bond(0, 1), 0.6, "关系保留");
}

/// LOD 的性能意义：背景代理不逐拍决策，决策量随 T0/T1 规模而不是总人口走。
#[test]
fn lod_background_skips_heartbeat_decisions() {
    let make = |near: bool| {
        let mut w = m4_world(11, 8);
        w.teleport(0, (5.0, 5.0));
        for i in 1..8 {
            if near {
                w.teleport(i, (5.2, 5.2));
            } else {
                w.teleport(i, (18.0, 1.0));
            }
        }
        w.refresh_lod();
        w.decisions_done = 0;
        w.run(2 * 1440);
        w
    };
    let far = make(false);
    let near = make(true);
    let t2_count = far.agents.iter().filter(|a| a.lod == 2).count();
    assert!(t2_count >= 6, "远处布置应有大量 T2：{t2_count}");
    println!(
        "[M4] 两天决策量：背景布置 {} 次 vs 全在身边 {} 次",
        far.decisions_done, near.decisions_done
    );
    assert!(
        far.decisions_done < near.decisions_done / 3,
        "背景代理应省下大部分决策：{} vs {}",
        far.decisions_done,
        near.decisions_done
    );
}

/// T2 按天批量结算：需求保持在活人区间、行为与习惯照常累积（统计近似）。
#[test]
fn lod_t2_daily_settle_keeps_agents_alive() {
    let mut w = m4_world(13, 2);
    w.teleport(0, (0.5, 0.5));
    w.teleport(1, (19.0, 9.0));
    w.refresh_lod();
    assert_eq!(w.agent_lod(1), 2);
    w.run(5 * 1440);
    let a = &w.agents[1];
    let execs: u64 = a.exec_counts.values().sum();
    assert!(execs >= 10, "批量结算应累积行为：{execs}");
    assert!(
        a.needs.energy > 5.0 && a.needs.hunger > 5.0,
        "背景代理的需求应保持活人区间：精力 {} 饥饿 {}",
        a.needs.energy,
        a.needs.hunger
    );
    // 去访留下的痕迹：背景居民也参与地点声誉统计。
    assert!(w.area_states["home"].visits > 0 || w.area_states["park"].visits > 0);
}

/// 习惯自动化（设计稿 4.4）：反复执行的行为权重过阈值 → 指令零消耗。
#[test]
fn automated_habit_command_is_free() {
    let mut w = m4_world(17, 1);
    w.run(60);
    // 走到白天让时间门放行，再把行为练成习惯（之后不再衰减扰动）。
    while living_core::time::hour_of(w.tick) < 8 {
        w.tick();
    }
    let base = w.agents[0].base_weights["walk_park"];
    w.agents[0].weights.insert("walk_park".into(), base * 1.6);
    w.agents[0].current = None;
    w.agents[0].next_heartbeat = w.tick + 9999; // 保持空闲，别让它自己先动
    let status = w.behaviors_status(0);
    let s = status.iter().find(|s| s.id == "walk_park").unwrap();
    assert!(s.automated, "权重过阈值应标记为自动化习惯");
    assert_eq!(s.est_cost, 0.0, "自动化习惯的指令免费");
    let before = w.agents[0].willpower;
    let out = w.issue_command(0, "walk_park");
    assert!(out.accepted, "{:?}", out.reason);
    assert_eq!(out.cost, 0.0, "执行不应扣意志力");
    assert_eq!(w.agents[0].willpower, before);
    w.run(3);
    assert_eq!(w.agents[0].current.as_ref().unwrap().0, "walk_park");
}

/// 恐惧标记与暴露恢复（设计稿 4.6）：恐惧行为指令加价并警示；
/// 反复硬着头皮去做 → 恐惧消退到不再碍事的水平（乘数回门槛以上）。
#[test]
fn fear_markup_then_exposure_recovers() {
    let mut w = m4_world(19, 2);
    w.collect_recent = true;
    while living_core::time::hour_of(w.tick) < 9 {
        w.tick();
    }
    // 玩家在草坪遭遇滋事 → 公园行为被负面记忆抑制。
    w.teleport(0, (5.0, 5.0));
    w.agents[0].current = None;
    w.trigger_env_event(0, (5.0, 5.0));
    let status = w.behaviors_status(0);
    let s = status
        .iter()
        .find(|s| s.id == "walk_park")
        .cloned()
        .unwrap();
    assert!(s.feared, "被恐惧抑制的行为应带警示标记");
    assert!(
        s.est_cost >= 15.0,
        "恐惧行为应显著加价：{}",
        s.est_cost
    );
    let cost_before = s.est_cost;
    // 暴露恢复：反复下令逛公园（每次完成都硬着头皮）。
    let mut exposures = 0;
    for _ in 0..40 {
        w.agents[0].willpower = 100.0; // 测试聚焦恐惧消退，排除意志力耗尽
        let out = w.issue_command(0, "walk_park");
        if out.accepted {
            w.run(120);
            exposures += w
                .recent_events
                .iter()
                .filter(|e| matches!(e, living_core::LogEvent::Exposure { .. }))
                .count();
        } else {
            w.run(30);
        }
    }
    println!("[M4] 暴露恢复：{exposures} 次消退事件，加价 {cost_before:.0} → 现价");
    let status = w.behaviors_status(0);
    let s = status.iter().find(|s| s.id == "walk_park").cloned().unwrap();
    assert!(exposures > 0, "反复尝试应产生暴露消退");
    assert!(
        !s.feared,
        "反复尝试后恐惧应退到不再碍事的水平（当前价 {:.0}）",
        s.est_cost
    );
    assert!(s.est_cost < cost_before, "指令应明显变便宜");
}

/// 密友查看（设计稿 4.7）：亲密值达标才能看见对方的需求与负面记忆。
#[test]
fn friend_inspection_gated_by_bond() {
    let mut w = m4_world(23, 2);
    w.set_bond(0, 1, 0.5);
    let err = w.inspect_friend(0, 1).unwrap_err();
    assert!(err.contains("不够亲近"), "{err}");
    // 升为密友，并让 1 号带着一条负面记忆。
    w.set_bond(0, 1, 0.8);
    w.teleport(1, (5.0, 5.0));
    w.trigger_env_event(0, (5.0, 5.0));
    let view = w.inspect_friend(0, 1).unwrap();
    assert_eq!(view.name, "林雨桐");
    assert!(view.bond >= 0.75);
    assert!(
        !view.fears.is_empty(),
        "密友应能看到对方正在生效的负面记忆"
    );
    assert!(view.fears[0].0.contains("rowdy"));
    // 密友名单与跨街完整模拟（LOD 的"剧情关联者"）。
    let friends = w.close_friends(0);
    assert_eq!(friends.len(), 1, "密友名单只有达标者");
    w.teleport(1, (19.0, 9.0));
    w.refresh_lod();
    assert_eq!(w.agent_lod(1), 0, "密友即使远处也保持 T0");
}

/// 限期目标完整周期（设计稿 4.5）：设定 → 压制 → 忍住/失守 → 放弃反弹。
#[test]
fn goal_resist_suppress_and_rebound() {
    let mut w = m4_world(29, 1);
    w.run(60);
    // 把逛公园练成强习惯（目标对抗的对象）。
    let base = w.agents[0].base_weights["walk_park"];
    w.agents[0].weights.insert("walk_park".into(), base * 2.0);
    w.set_goal(0, "walk_park", 5).unwrap();
    assert!(!w.goal_status(0).is_empty(), "目标应进行中");
    let w_start = w.agents[0].weights["walk_park"];
    // 跑两天：权重被目标压制加速回落（对比自然衰减）。
    let will_start = w.agents[0].willpower;
    let mut resists = 0u32;
    w.collect_recent = true;
    let before = w.recent_events.len();
    w.run(2 * 1440);
    resists = w.recent_events[before..]
        .iter()
        .filter(|e| matches!(e, living_core::LogEvent::GoalResist { .. }))
        .count() as u32;
    let w_now = w.agents[0].weights["walk_park"];
    println!(
        "[M4] 目标压制：权重 {w_start:.2} → {w_now:.2}（基准 {base:.2}），忍住 {resists} 次，意志 {will_start:.0} → {:.0}",
        w.agents[0].willpower
    );
    assert!(
        w_now < w_start * 0.9,
        "目标期间习惯权重应被加速压制：{w_start:.2} -> {w_now:.2}"
    );
    let st = &w.goal_status(0)[0];
    assert!(st.suppressed > 0.0, "应记录被压制的量");
    if resists == 0 {
        // 行为树这两天没选它也可能（时间门/需求门），但压制与反弹仍须成立。
        println!("[M4]（这两天树没有选中戒断对象，无忍住事件）");
    }
    // 中途放弃 → 旧习惯立刻反弹。
    let (behavior, rebound) = w.abandon_goal(0, "walk_park").unwrap();
    assert_eq!(behavior, "walk_park");
    assert!(rebound > 0.0, "放弃应触发反弹：{rebound}");
    let w_after = w.agents[0].weights["walk_park"];
    assert!(
        w_after > w_now,
        "反弹后权重应回升：{w_now:.2} -> {w_after:.2}"
    );
    assert!(w.goal_status(0).is_empty(), "目标已结束");
    // 放弃后再设同一行为的目标应允许。
    w.set_goal(0, "walk_park", 2).unwrap();
}

/// 目标到期：零违约成功（压制已转化为自然衰减）；主动违约会计数。
#[test]
fn goal_deadline_success_and_commanded_violation() {
    let mut w = m4_world(31, 1);
    w.run(60);
    // 选一个夜里做不了的行为设 1 天目标（白天树也未必选它），
    // 到期零违约 → GoalEnd success。
    w.set_goal(0, "bench_sit", 1).unwrap();
    // 主动违约一次：把意志力灌满后下令（时间门内才能下令）。
    while living_core::time::hour_of(w.tick) < 8 {
        w.tick();
    }
    w.agents[0].willpower = 100.0;
    w.collect_recent = true;
    let before = w.recent_events.len();
    let out = w.issue_command(0, "bench_sit");
    assert!(out.accepted, "{:?}", out.reason);
    assert!(
        w.recent_events[before..].iter().any(|e| matches!(
            e,
            living_core::LogEvent::GoalViolation { commanded: true, .. }
        )),
        "主动下令戒断行为应记违约"
    );
    let st = &w.goal_status(0)[0];
    assert_eq!(st.violations, 1);
    // 跑过期限 → 目标结束（有违约 → 不算成功）。
    w.run(26 * 60);
    assert!(w.goal_status(0).is_empty(), "到期后目标应结束");
    assert!(w.recent_events.iter().any(|e| matches!(
        e,
        living_core::LogEvent::GoalEnd { success: false, abandoned: false, .. }
    )));
}

/// M4 验收故事（用真实内容 street.json）：反复下令逛公园 → 养成自动化习惯
/// 且系统能报告原因；随后设目标对抗 → 压制与忍住可见；放弃 → 反弹；
/// 遭遇滋事后习惯报告能把经历关联进来。解释与数据一致。
#[test]
fn m4_acceptance_habit_story() {
    let json = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../assets/street.json"
    ))
    .expect("需要 assets/street.json");
    let cfg = load_config(&json).unwrap();
    let roster = living_core::human_names(3)
        .into_iter()
        .map(|n| (n, "human".to_string()))
        .collect();
    let mut w = World::new(cfg, 37, roster, true);
    w.set_player(0);
    w.collect_recent = true;
    // 三天里玩家坚持每天白天都去逛公园（可用就下令）。
    for _ in 0..3 * 1440 {
        if w.agents[0].current.is_none() {
            let ok = w.issue_command(0, "walk_park");
            if !ok.accepted {
                w.agents[0].willpower = 100.0; // 睡醒回满，排除意志力噪声
            }
        }
        w.tick();
    }
    let report = w.habit_report(0);
    let walk = report.iter().find(|r| r.behavior == "walk_park").unwrap();
    println!(
        "[M4 验收] 逛公园：权重 {:.2}（基准 {:.2}，×{:.2}），执行 {} 次，自动化 {}",
        walk.weight, walk.base, walk.ratio, walk.execs, walk.automated
    );
    assert!(walk.execs >= 5, "三天坚持应多次执行：{}", walk.execs);
    assert!(
        walk.automated,
        "三天反复执行应养成自动化习惯（当前 ×{:.2}）",
        walk.ratio
    );
    // 习惯报告能给出解释（相关记忆或执行史），与模拟数据一致。
    let status = w.behaviors_status(0);
    let s = status.iter().find(|s| s.id == "walk_park").cloned().unwrap();
    assert!(s.automated && s.est_cost == 0.0);
    // 对抗：给逛公园设 3 天目标，期间树选中时玩家角色要靠意志力忍。
    w.set_goal(0, "walk_park", 3).unwrap();
    let w0 = w.agents[0].weights["walk_park"];
    let log_len_before = w.logger.buf.lines().count();
    w.run(2 * 1440);
    // 从完整日志计数（recent_events 是 512 条的环形缓冲，长跑下切片不可靠）。
    let tail: Vec<&str> = w.logger.buf.lines().skip(log_len_before).collect();
    let resists = tail
        .iter()
        .filter(|l| l.contains(r#""t":"goal_resist""#))
        .count();
    let broke = tail
        .iter()
        .filter(|l| l.contains(r#""t":"goal_violation""#))
        .count();
    let w1 = w.agents[0].weights["walk_park"];
    let st = &w.goal_status(0)[0];
    println!(
        "[M4 验收] 目标对抗两天：权重 {w0:.2} → {w1:.2}（基准 {:.2}），忍住 {} 次、失守 {} 次",
        walk.base, resists, broke
    );
    assert!(w1 < w0, "目标期间习惯应被压制");
    assert!(resists >= 1, "强习惯被树选中时应出现忍住事件：{resists}");
    assert_eq!(st.resists as usize, resists, "目标计数与日志一致");
    // 放弃 → 反弹，完整闭环。
    let (_, rebound) = w.abandon_goal(0, "walk_park").unwrap();
    let w2 = w.agents[0].weights["walk_park"];
    println!("[M4 验收] 放弃目标：反弹 +{rebound:.2} → {w2:.2}");
    assert!(w2 > w1, "放弃后旧习惯应立刻反弹");
    // 最后一段经历：白天在草坪遭遇滋事——习惯报告能把这段经历关联进来
    //（"养成某习惯，因为某经历"的解释与模拟数据一致）。
    // 清空记忆排除之前随机事件的干扰，聚焦这一次经历的效果。
    while living_core::time::hour_of(w.tick) != 10 {
        w.tick();
    }
    w.agents[0].memory = living_core::memory::MemoryTable::new(500, 8);
    w.teleport(0, (5.0, 5.0));
    w.agents[0].current = None;
    w.trigger_env_event(0, (5.0, 5.0));
    let report = w.habit_report(0);
    let walk = report.iter().find(|r| r.behavior == "walk_park").unwrap();
    assert!(
        walk.related_memories.iter().any(|m| m.contains("rowdy")),
        "习惯报告应能关联到相关经历：{:?}",
        walk.related_memories
    );
    let status = w.behaviors_status(0);
    let s = status.iter().find(|s| s.id == "walk_park").cloned().unwrap();
    assert!(s.feared, "遭遇滋事后逛公园应带恐惧警示");
}

/// M4 存档回环：目标与 LOD 进快照，读档后运行与连续运行一致。
#[test]
fn save_load_m4_roundtrip() {
    let json = m4_config();
    let cfg = load_config(&json).unwrap();
    let mut a = World::new(cfg.clone(), 41, living_core::human_names(3)
        .into_iter().map(|n| (n, "human".to_string())).collect(), true);
    a.set_player(0);
    a.run(900);
    a.set_goal(0, "walk_park", 2).unwrap();
    a.teleport(2, (19.0, 9.0)); // 2 号进入 T2
    a.run(300);
    let bytes = a.to_save();
    let offset = a.logger.buf.len();
    a.run(1440);
    let mut b = World::from_save(load_config(&json).unwrap(), &bytes).unwrap();
    assert_eq!(b.goal_status(0).len(), 1, "目标应进快照");
    assert_eq!(b.agent_lod(2), 2, "LOD 层级应进快照");
    b.run(1440);
    assert_eq!(
        b.logger.buf,
        a.logger.buf[offset..],
        "读档继续的日志应与连续运行一致"
    );
}

/// 含目标操作的回放确定性。
#[test]
fn m4_script_replay_deterministic() {
    let script = |w: &mut World| {
        w.run(600);
        w.set_goal(0, "walk_park", 2).unwrap();
        w.run(500);
        let _ = w.issue_command(0, "walk_park");
        w.run(300);
        let _ = w.abandon_goal(0, "walk_park");
        w.run(400);
    };
    let mut a = m4_world(43, 2);
    script(&mut a);
    let mut b = m4_world(43, 2);
    script(&mut b);
    assert_eq!(a.logger.buf, b.logger.buf, "含目标操作的运行应字节级一致");
}
