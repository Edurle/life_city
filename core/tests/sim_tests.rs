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
    "memory_cap": 500, "memory_top_k": 8, "weight_jitter": 0.1,
    "novelty_mult": 0.15, "novelty_gain": 0.02
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
        a.habit_boost("sleep", 0.06, 4.0);
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
/// 声誉回升、恐惧记忆消退、到访不锁死。衰退与恢复由同一套统计机制产生。
/// （M5 二阶段起改用多指标断言：完整模拟里到访量随习惯演化有自然漂移，
/// 早期版本拿单日计数比大小，涨落 ±数次就翻脸。）
#[test]
fn m3_acceptance_park_reputation_cycle() {
    let mut w = m3_world(11, 6);
    // 基线：安定的两天。
    w.run(2 * 1440);
    let rep_base = w.area_states["park"].reputation;
    let execs_base = park_execs(&w);
    let baseline_per_day = execs_base as f64 / 2.0;
    assert!(rep_base > 0.5, "基线声誉应处于高位：{rep_base}");
    assert!(
        baseline_per_day > 20.0,
        "居民应该爱去公园：{baseline_per_day}/天"
    );

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
    let fear_peak: f64 = w
        .agents
        .iter()
        .flat_map(|a| {
            a.memory
                .entries
                .iter()
                .filter(|m| m.event.contains("rowdy"))
        })
        .map(|m| m.strength)
        .fold(0.0, f64::max);

    // 事件停止后的第二天：到访量应显著少于基线（记忆抑制 + 声誉乘数双通道）。
    let execs_after_shock_start = park_execs(&w);
    w.run(1440);
    let execs_shock_day = park_execs(&w) - execs_after_shock_start;
    println!(
        "[M3 验收] 公园到访：基线 {baseline_per_day:.0}/天 → 事件后 {execs_shock_day}/天（恐惧峰值 {fear_peak:.2}）"
    );
    assert!(
        (execs_shock_day as f64) < baseline_per_day * 0.5,
        "声誉低谷期到访应明显减少：{execs_shock_day} vs {baseline_per_day:.0}"
    );

    // 恢复：音乐会 + 十天平静，口碑被良好的到访体验拉回。
    let concert = 1;
    w.run(720);
    w.trigger_env_event(concert, (5.0, 5.0));
    w.run(10 * 1440);
    let rep_end = w.area_states["park"].reputation;
    let execs_recovery_start = park_execs(&w);
    w.run(2 * 1440);
    let late_per_day = (park_execs(&w) - execs_recovery_start) as f64 / 2.0;
    let fear_end: f64 = w
        .agents
        .iter()
        .flat_map(|a| {
            a.memory
                .entries
                .iter()
                .filter(|m| m.event.contains("rowdy"))
        })
        .map(|m| m.strength)
        .fold(0.0, f64::max);
    println!(
        "[M3 验收] 恢复期：声誉 {rep_low:.2} → {rep_end:.2}，到访 {late_per_day:.0}/天，恐惧 {fear_peak:.2} → {fear_end:.2}"
    );
    assert!(
        rep_end > rep_low + 0.15,
        "平静与良好体验应恢复声誉：{rep_low:.2} -> {rep_end:.2}"
    );
    assert!(
        fear_end < fear_peak * 0.6 && fear_end < 0.4,
        "恐惧记忆应随时间消退：{fear_peak:.2} -> {fear_end:.2}"
    );
    // 到访不锁死：习惯权重上限与新奇需求保证公园不会从生活里消失
    //（旧引擎无此机制，长跑后公园到访会衰减到零——M5 二阶段的修复项）。
    assert!(
        late_per_day >= 1.5,
        "恢复后期应仍有人去公园：{late_per_day}/天"
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
    assert!(
        (w.agents[1].weights["walk_park"] - w0).abs() < 1e-9,
        "习惯权重保留"
    );
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
    assert!(s.est_cost >= 15.0, "恐惧行为应显著加价：{}", s.est_cost);
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
    let s = status
        .iter()
        .find(|s| s.id == "walk_park")
        .cloned()
        .unwrap();
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
    assert!(!view.fears.is_empty(), "密友应能看到对方正在生效的负面记忆");
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
    w.collect_recent = true;
    let before = w.recent_events.len();
    w.run(2 * 1440);
    let resists = w.recent_events[before..]
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
            living_core::LogEvent::GoalViolation {
                commanded: true,
                ..
            }
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
        living_core::LogEvent::GoalEnd {
            success: false,
            abandoned: false,
            ..
        }
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
    let s = status
        .iter()
        .find(|s| s.id == "walk_park")
        .cloned()
        .unwrap();
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
    let s = status
        .iter()
        .find(|s| s.id == "walk_park")
        .cloned()
        .unwrap();
    assert!(s.feared, "遭遇滋事后逛公园应带恐惧警示");
}

/// M4 存档回环：目标与 LOD 进快照，读档后运行与连续运行一致。
#[test]
fn save_load_m4_roundtrip() {
    let json = m4_config();
    let cfg = load_config(&json).unwrap();
    let mut a = World::new(
        cfg.clone(),
        41,
        living_core::human_names(3)
            .into_iter()
            .map(|n| (n, "human".to_string()))
            .collect(),
        true,
    );
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

// ---------- M4.5：内容热身——吃饭变体、远程社交、最小工作、子树 ----------

fn street_cfg() -> living_core::SimConfig {
    let json = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../assets/street.json"
    ))
    .expect("需要 assets/street.json");
    load_config(&json).unwrap()
}

fn street_world(seed: u64, humans: usize, cats: usize) -> World {
    let mut roster: Vec<(String, String)> = living_core::human_names(humans)
        .into_iter()
        .map(|n| (n, "human".to_string()))
        .collect();
    roster.extend(cat_roster(cat_names(cats)));
    let mut w = World::new(street_cfg(), seed, roster, true);
    w.log_meta(seed, 0);
    w
}

/// 街道内容冒烟：人类会去上班、吃东西有多个变体、猫不碰人类专属行为；
/// 子树引用的树跑起来与直接定义无差别（校验 + 遍历已由单元测试锁定）。
#[test]
fn street_humans_work_and_eat_variely() {
    let mut w = street_world(21, 5, 2);
    w.run(4 * 1440);
    let execs = |id: usize, b: &str| w.agents[id].exec_counts.get(b).copied().unwrap_or(0);
    // 上班：工作时段的人真的去上班（4 天内多名人类执行过）。
    let work_total: u64 = (0..5).map(|i| execs(i, "go_to_work")).sum();
    assert!(work_total >= 6, "四天里应有多次上班记录：{work_total}");
    // 吃饭变体：全体人类至少动用过 3 种吃法（在家/下厨/泡面/外卖/下馆子/夜宵）。
    let foods = [
        "eat_home",
        "cook_fancy",
        "instant_noodles",
        "order_takeout",
        "diner_meal",
        "late_snack",
    ];
    let used: Vec<&str> = foods
        .iter()
        .copied()
        .filter(|b| (0..5).any(|i| execs(i, b) > 0))
        .collect();
    println!("[M4.5] 四天里人类用过的吃法：{used:?}（上班 {work_total} 次）");
    assert!(used.len() >= 3, "吃饭应有真实选择余地：{used:?}");
    // 物种隔离：猫不执行人类的新行为。
    for i in 5..7 {
        for b in ["go_to_work", "cook_fancy", "phone_call", "diner_meal"] {
            assert_eq!(execs(i, b), 0, "猫不应执行 {b}");
        }
    }
}

/// 远程社交（打电话）：不见面也能维系关系——完成通话与认识名单里的人增长亲密。
#[test]
fn phone_call_grows_bond_without_meeting() {
    let mut w = street_world(23, 3, 0);
    w.collect_recent = true;
    // 清空初始熟悉度、三人分处地图三个角落（绝无同地点可能）。
    for i in 0..3 {
        for j in 0..3 {
            if i != j {
                w.set_bond(i, j, 0.0);
            }
        }
    }
    w.teleport(0, (2.0, 2.0));
    w.teleport(1, (36.0, 18.0));
    w.teleport(2, (20.0, 10.0));
    // 强制 0 号打两通电话。
    for _ in 0..2 {
        w.agents[0].pending_command = Some("phone_call".into());
        w.agents[0].next_heartbeat = w.tick + 1;
        w.run(40);
    }
    let bonds: Vec<f64> = vec![w.bond(0, 1), w.bond(0, 2)];
    println!("[M4.5] 两通电话后的亲密：{bonds:?}");
    assert!(
        bonds.iter().any(|b| *b > 0.0),
        "远程社交应与认识的人增长亲密（哪怕不见面）"
    );
    assert!(
        w.recent_events
            .iter()
            .any(|e| matches!(e, living_core::LogEvent::BondChange { a: 0, .. })),
        "应有亲密变化日志"
    );
}

/// 下馆子是社交场合：两人同桌吃饭 → 亲密增长（约饭，机制为内容配置）。
#[test]
fn dining_together_grows_bond() {
    let mut w = street_world(27, 2, 0);
    w.set_bond(0, 1, 0.0);
    // 两人同时被派去饭馆。
    for i in 0..2 {
        w.agents[i].pending_command = Some("diner_meal".into());
        w.agents[i].next_heartbeat = w.tick + 1;
    }
    let before = w.bond(0, 1);
    w.run(60);
    let after = w.bond(0, 1);
    println!("[M4.5] 同桌下馆子：亲密 {before:.3} → {after:.3}");
    assert!(after > before, "同桌吃饭应拉近关系（社交标签 + 同地点）");
}

/// 上班的职业习惯雏形：高频执行的上班行为权重升高（设计稿 5.3 的机制基础）。
#[test]
fn work_becomes_habit() {
    let mut w = street_world(29, 3, 0);
    w.run(6 * 1440);
    let a = &w.agents[0];
    let ratio = a.weights["go_to_work"] / a.base_weights["go_to_work"];
    println!(
        "[M4.5] 六天后上班权重倍数：×{ratio:.2}（执行 {} 次）",
        a.exec_counts.get("go_to_work").copied().unwrap_or(0)
    );
    assert!(
        ratio > 1.05,
        "天天上班应自然累积职业习惯（当前 ×{ratio:.2}）"
    );
}

// ---------- M5 一座镇（一阶段）：职业与经济、犯罪与治安、传闻与新闻 ----------

fn town_cfg() -> living_core::SimConfig {
    let json = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/town.json"))
        .expect("需要 assets/town.json");
    load_config(&json).unwrap()
}

fn town_world(seed: u64, humans: usize) -> World {
    let roster: Vec<(String, String)> = living_core::human_names(humans)
        .into_iter()
        .map(|n| (n, "human".to_string()))
        .collect();
    let mut w = World::new(town_cfg(), seed, roster, true);
    w.set_player(0);
    w.log_meta(seed, 0);
    w
}

/// 世界生成器：岗位按序分配、非法行当不被指派、猫不参加工作。
#[test]
fn town_job_assignment_and_economy_flow() {
    let mut w = town_world(3, 26);
    let employed = w
        .agents
        .iter()
        .filter(|a| a.species == "human" && a.job.is_some())
        .count();
    assert_eq!(employed, 23, "23 个岗位给 23 人（小偷不指派，3 人待业）");
    // 跑五天：工作者挣到钱（钱包高于初始），且镇上有消费发生。
    let start: Vec<f64> = w.agents.iter().map(|a| a.wallet).collect();
    w.run(5 * 1440);
    let workers_gained = w
        .agents
        .iter()
        .zip(&start)
        .filter(|(a, s)| a.job.is_some() && a.species == "human" && a.wallet > *s + 50.0)
        .count();
    assert!(
        workers_gained >= 10,
        "五天后多数工作者应攒下钱：{workers_gained}"
    );
    let spending = w
        .logger
        .buf
        .lines()
        .filter(|l| l.contains(r#""t":"wallet""#) && l.contains(r#""delta":-"#))
        .count();
    assert!(spending > 10, "应有消费流水：{spending}");
}

/// 资源门槛：付不起的行为不进候选（泡面自由是贫困线以下的尊严）。
#[test]
fn resource_gate_blocks_unaffordable_food() {
    let mut w = town_world(5, 6);
    while living_core::time::hour_of(w.tick) < 9 {
        w.tick();
    }
    w.agents[0].wallet = 12.0;
    w.agents[0].needs.hunger = 40.0; // 饿了，门开
    let opts = w.debug_options(0);
    let has = |b: &str| opts.iter().any(|o| o.behavior == b);
    assert!(!has("diner_meal"), "下馆子（90）不该出现在候选里");
    assert!(!has("order_takeout"), "外卖（65）不该出现");
    assert!(has("instant_noodles"), "泡面（10）应可负担");
    // 有钱之后选项回来。
    w.agents[0].wallet = 500.0;
    let opts = w.debug_options(0);
    assert!(opts.iter().any(|o| o.behavior == "diner_meal"));
}

/// 犯罪得手路径：财物转移、受害者记忆（关于小偷本人）、区域声誉冲击。
#[test]
fn crime_success_transfers_and_memorializes() {
    let mut w = town_world(7, 4);
    while living_core::time::hour_of(w.tick) != 22 {
        w.tick();
    }
    // 环境概率有稀释（照明/巡逻/人流），循环作案直到至少一起得手——
    // 固定种子下确定性收敛，断言对"得手的那起"做。
    let rep_before = w.area_states["street"].reputation;
    let start_wallet = w.agents[0].wallet;
    let mut got_away = false;
    for _ in 0..10 {
        w.agents[1].wallet = 300.0;
        w.agents[0].job = Some("thief".into());
        for i in [0usize, 1] {
            w.agents[i].needs = living_core::needs::Needs::full();
            w.agents[i].current = None;
        }
        w.agents[1].next_heartbeat = w.tick + 500; // 钉在案发现场
        // 其他居民钉去家里：案发 8 米内只剩这位钱包最鼓的受害者。
        for i in 2..w.agents.len() {
            w.teleport(i as u32, (50.0, 30.0));
            w.agents[i].current = None;
            w.agents[i].next_heartbeat = w.tick + 500;
        }
        w.teleport(0, (18.5, 4.0));
        w.teleport(1, (18.7, 4.0));
        w.agents[0].pending_command = Some("steal".into());
        w.agents[0].next_heartbeat = w.tick + 1;
        w.run(90);
        if w.stats.thefts > w.stats.thefts_caught {
            got_away = true;
            break;
        }
    }
    assert!(got_away, "十次作案应至少一起得手");
    assert!(w.agents[0].wallet > start_wallet, "得手的小偷应更有钱");
    // 受害者写入了关于小偷本人的负面记忆。
    let vm = w.agents[1]
        .memory
        .entries
        .iter()
        .find(|m| m.event == "被偷")
        .expect("受害者应有被偷记忆");
    assert_eq!(vm.person, Some(0), "记忆应指向小偷本人");
    assert!(
        w.area_states["street"].reputation < rep_before,
        "案发街区声誉应受冲击"
    );
    let has_theft_log = w.logger.buf.contains("\"t\":\"theft\"");
    assert!(has_theft_log, "应有案发日志");
}

/// 犯罪被抓路径：归还、开除、社区服务排程、被捕记忆压制再犯。
#[test]
fn crime_caught_full_consequences() {
    // 被抓基础概率拉满（环境乘数仍会稀释，循环作案直到被抓——确定性收敛）。
    let json = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/town.json"))
        .unwrap();
    let cfg = load_config(&json.replace(r#""caught_base": 0.5"#, r#""caught_base": 1.0"#)).unwrap();
    let roster: Vec<(String, String)> = living_core::human_names(4)
        .into_iter()
        .map(|n| (n, "human".to_string()))
        .collect();
    let mut w = World::new(cfg, 11, roster, true);
    w.set_player(0);
    while living_core::time::hour_of(w.tick) != 22 {
        w.tick();
    }
    let mut caught = false;
    for _ in 0..10 {
        w.agents[1].wallet = 300.0;
        w.agents[0].job = Some("thief".into());
        for i in [0usize, 1] {
            w.agents[i].needs = living_core::needs::Needs::full();
            w.agents[i].current = None;
        }
        w.agents[1].next_heartbeat = w.tick + 500;
        w.teleport(0, (18.5, 4.0));
        w.teleport(1, (18.7, 4.0));
        w.agents[0].pending_command = Some("steal".into());
        w.agents[0].next_heartbeat = w.tick + 1;
        w.run(90);
        if w.agents[0].job.is_none() {
            caught = true;
            break;
        }
    }
    assert!(caught, "必抓环境下十次作案应至少一次被抓");
    // 社区服务已排入（可能已在执行——两种都算排上）。
    let served = w.agents[0]
        .exec_counts
        .get("community_service")
        .copied()
        .unwrap_or(0);
    let pending_or_doing = w.agents[0]
        .current
        .as_ref()
        .map(|c| c.0 == "community_service")
        .unwrap_or(false)
        || w.agents[0].pending_command.as_deref() == Some("community_service");
    assert!(
        served > 0 || pending_or_doing,
        "被抓后应去打扫公园（执行 {served} 次）"
    );
    // 被捕记忆：压制再犯（crime 标签乘数 < 1）。
    assert!(w.agents[0]
        .memory
        .entries
        .iter()
        .any(|m| m.event == "被捕" && m.mults.get("crime").copied().unwrap_or(1.0) < 1.0));
    // 日志能查到被抓的案底。
    assert!(w.logger.buf.contains(r#""caught":true"#));
}

/// 传闻（技术文档 4.5）：受害者把被偷经历讲给第三者——负面记忆扩散。
#[test]
fn hearsay_spreads_crime_memory() {
    let mut w = town_world(13, 4);
    // 先让 1 号被 0 号偷一次（走真实结算）。
    while living_core::time::hour_of(w.tick) != 22 {
        w.tick();
    }
    w.agents[0].job = Some("thief".into());
    for i in [0usize, 1] {
        w.agents[i].needs = living_core::needs::Needs::full();
        w.agents[i].current = None;
    }
    w.agents[1].wallet = 300.0;
    w.teleport(0, (18.5, 4.0));
    w.teleport(1, (18.7, 4.0));
    w.agents[1].next_heartbeat = w.tick + 500; // 钉在案发现场
    w.agents[0].pending_command = Some("steal".into());
    w.agents[0].next_heartbeat = w.tick + 1;
    for _ in 0..240 {
        if w.agents[1].memory.entries.iter().any(|m| m.event == "被偷") {
            break;
        }
        w.tick();
    }
    assert!(w.agents[1].memory.entries.iter().any(|m| m.event == "被偷"));
    // 受害者与 2 号电话聊一次（M5 二阶段：对象按
    // 亲密×新鲜×可及×印象 选——把其他认识人压到 0，电话必打给 2 号）。
    while living_core::time::hour_of(w.tick) < 10 {
        w.tick();
    }
    w.set_bond(1, 0, 0.0);
    w.set_bond(1, 3, 0.0);
    w.set_bond(1, 2, 0.9);
    // 对象选择有权重分布（≈九成打给 2 号）——固定种子下循环拨打直到接通，
    // 断言对"接通 2 号的那次"做。
    let mut spread = false;
    for _ in 0..8 {
        while living_core::time::hour_of(w.tick) < 10 && living_core::time::day_of(w.tick) < 3 {
            w.tick();
        }
        w.agents[1].current = None;
        w.agents[1].current_variant = None;
        w.agents[1].pending_command = Some("phone_call".into());
        w.agents[1].next_heartbeat = w.tick + 1;
        w.run(90);
        if w.agents[2]
            .memory
            .entries
            .iter()
            .any(|m| m.event == "传闻:被偷")
        {
            spread = true;
            break;
        }
        // 这一小时没聊成（对方不在线/时序），跳到下一小时的窗口。
        while living_core::time::minute_of(w.tick) < 59 {
            w.tick();
        }
        w.tick();
    }
    assert!(
        spread,
        "听过受害者倾诉的人应获得传闻记忆，实际：{:?}",
        w.agents[2]
            .memory
            .entries
            .iter()
            .map(|m| m.event.clone())
            .collect::<Vec<_>>()
    );
}

/// 职业转变（验收核心）：诚实度低 + 经济压力大的失业者会"走上歪路"；
/// 就业充足时多数人一生不碰非法行当（对照同种子有岗位的世界）。
#[test]
fn job_market_driven_career_changes() {
    // 岗位 21 < 人口 26：存在长期失业者——非法行当才有生存空间
    //（就业充足时它被挤出，这正是设计稿"就业机会充足则多数人
    // 一生不考虑非法职业"的机制表达）。
    let mut w = town_world(17, 26);
    // 布置主角处境：失业、不诚实、见底（其余居民自然生活、竞争岗位）。
    w.agents[0].job = None;
    w.agents[0].honesty = 0.08;
    w.agents[0].wallet = 10.0;
    let mut went_wrong = false;
    for _ in 0..40 * 1440 {
        w.tick();
        if w.agents[0].job.as_deref() == Some("thief") {
            went_wrong = true;
            break;
        }
    }
    assert!(
        went_wrong,
        "失业压力下不诚实者应在四十天内走上歪路（概率驱动，无剧本）"
    );
    // 对照：在岗、钱包健康的人不辞职去偷。
    let stable = w
        .agents
        .iter()
        .filter(|a| a.species == "human" && a.job.is_some() && a.job.as_deref() != Some("thief"))
        .count();
    assert!(stable >= 15, "在岗者应保持正当职业：{stable}");
}

/// 新闻生成：案发与职业变动都能转成小镇新闻（设计稿 6.3）。
#[test]
fn news_generated_from_town_events() {
    let mut w = town_world(5, 26);
    w.run(60 * 1440);
    let name_of = |id: u32| -> String {
        w.agents
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| format!("#{id}"))
    };
    let job_name_of = |j: &str| -> String {
        w.cfg
            .jobs
            .iter()
            .find(|x| x.id == j)
            .map(|x| x.display_name().to_string())
            .unwrap_or_else(|| j.to_string())
    };
    let mut news = Vec::new();
    for line in w.logger.buf.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            let ev = match v["t"].as_str().unwrap_or("") {
                "theft" => Some(living_core::LogEvent::Theft {
                    tick: v["tick"].as_u64().unwrap(),
                    thief: v["thief"].as_u64().unwrap() as u32,
                    victim: v["victim"].as_u64().unwrap() as u32,
                    amount: v["amount"].as_f64().unwrap(),
                    caught: v["caught"].as_bool().unwrap(),
                    area: v["area"].as_str().unwrap().to_string(),
                }),
                _ => None,
            };
            if let Some(ev) = ev {
                if let Some(t) = living_core::news::news_text(&ev, name_of, job_name_of) {
                    news.push(t);
                }
            }
        }
    }
    println!("[M5] 60 天新闻 {} 条，例如：", news.len());
    for n in news.iter().take(3) {
        println!("  {n}");
    }
    assert!(!news.is_empty(), "30 天的镇子应有新闻可播");
}

/// M5 验收：固定种子模拟一年——盗窃案发生、有人被抓、职业转变出现，
/// 案发区域声誉有据可查（受控测试覆盖声誉链，这里锁总量分布）。
#[test]
fn town_year_statistical_acceptance() {
    let mut w = town_world(5, 26);
    w.logger.enabled = false; // 统计走 TownStats，不为断言跑巨型日志
    w.run(365 * 1440);
    let st = &w.stats;
    println!(
        "[M5 验收] 一年：盗窃 {} 起（被抓 {}），职业变动 {} 次，成对 {} 对，新生儿 {} 名",
        st.thefts, st.thefts_caught, st.job_changes, st.couples_formed, st.births
    );
    assert!(st.thefts >= 10, "一年应有多起盗窃：{}", st.thefts);
    assert!(st.thefts_caught >= 3, "应有人被抓：{}", st.thefts_caught);
    assert!(st.job_changes >= 5, "应有职业转变：{}", st.job_changes);
    // M5 二阶段：恋爱与代际在年度尺度真实发生。
    assert!(
        st.couples_formed >= 3,
        "一年应有多对恋人：{}",
        st.couples_formed
    );
    assert!(st.births >= 1, "一年应有新生儿：{}", st.births);
    // 经济没有崩溃：工作者的钱包在年末普遍为正，且没有全体赤贫。
    let humans: Vec<&living_core::Agent> =
        w.agents.iter().filter(|a| a.species == "human").collect();
    let solvent = humans.iter().filter(|a| a.wallet > 50.0).count();
    assert!(solvent >= 12, "多数居民应保持偿付能力：{solvent}/26");
}

/// 一座镇的回放确定性（含经济、犯罪、职业流动的完整机制）。
#[test]
fn town_window_replay_deterministic() {
    let script = |w: &mut World| {
        w.run(45 * 1440);
    };
    let mut a = town_world(23, 12);
    a.logger.enabled = false;
    script(&mut a);
    let mut b = town_world(23, 12);
    b.logger.enabled = false;
    script(&mut b);
    assert_eq!(a.stats.thefts, b.stats.thefts, "同种子的盗窃数应一致");
    assert_eq!(a.stats.job_changes, b.stats.job_changes);
    let wa: Vec<f64> = a.agents.iter().map(|x| x.wallet.round()).collect();
    let wb: Vec<f64> = b.agents.iter().map(|x| x.wallet.round()).collect();
    assert_eq!(wa, wb, "同种子的钱包终态应一致");
}

// ---------- M5 二阶段：社交对象选择、恋爱、老龄、代际、变体、流行、日记 ----------

/// 强制某代理立即执行一个行为（测试用：清状态、排下一拍）。
fn force_action(w: &mut World, id: u32, behavior: &str) {
    let tick = w.tick;
    if let Some(a) = w.agents.iter_mut().find(|a| a.id == id) {
        a.current = None;
        a.current_variant = None;
        a.pending_command = Some(behavior.to_string());
        a.next_heartbeat = tick + 1;
    }
}

/// 从日志里取某代理的 SocialPick 事件。
fn social_picks(w: &World, id: u32) -> Vec<(u32, f64, f64)> {
    let mut out = Vec::new();
    for line in w.logger.buf.lines() {
        if let Ok(living_core::LogEvent::SocialPick {
            agent,
            target,
            bond,
            impression,
            ..
        }) = serde_json::from_str(line)
        {
            if agent == id {
                out.push((target, bond, impression));
            }
        }
    }
    out
}

/// 社交对象选择（"和谁"）：亲密高的人常被选中；关于某人的负面记忆
///（被偷过）把他挤出对象名单——人物记忆进入社交决策（头脑风暴清单 3）。
#[test]
fn social_pick_prefers_friend_avoids_wrongdoer() {
    let mut w = town_world(7, 4);
    // 1 号与 2、3 号 equally 亲近；3 号偷过 1 号（写入人物记忆）。
    w.set_bond(1, 2, 0.8);
    w.set_bond(1, 3, 0.8);
    let mut mults = BTreeMap::new();
    mults.insert("crime".to_string(), 0.3);
    let s = w.agents[1].memory.write(
        "被偷",
        &["crime".to_string()],
        2,
        0.9,
        1.0,
        -0.8,
        false,
        mults,
        10,
    );
    assert!(s > 0.3);
    w.agents[1].memory.set_person("被偷", 2, 3);
    let imp = w.agents[1].memory.person_impressions();
    assert!(imp[&3] < 0.5, "对作案者的印象应强烈负面：{:?}", imp);
    // 反复强制 1 号街头聊天，统计对象分布。
    while living_core::time::hour_of(w.tick) < 9 {
        w.tick();
    }
    let mut picks_2 = 0;
    let mut picks_3 = 0;
    for _ in 0..10 {
        force_action(&mut w, 1, "chat_street");
        w.run(80);
        for _ in 0..30 {
            if w.agents[1].current.is_none() {
                break;
            }
            w.tick();
        }
        for (t, _, _) in social_picks(&w, 1).iter().rev().take(1) {
            if *t == 2 {
                picks_2 += 1;
            }
            if *t == 3 {
                picks_3 += 1;
            }
        }
        while living_core::time::hour_of(w.tick) != 12 {
            w.tick();
            if living_core::time::hour_of(w.tick) == 12 {
                break;
            }
        }
        // 聊完跳过两小时让新鲜度回落一些再聊。
        w.run(120);
        if picks_2 + picks_3 >= 8 {
            break;
        }
    }
    println!(
        "[M5.2] 对象选择：2 号 {} 次 vs 3 号（结仇）{} 次",
        picks_2, picks_3
    );
    assert!(picks_2 >= 6, "多数社交应找没结仇的 2 号：{picks_2}");
    assert!(picks_3 <= 1, "结了仇的 3 号基本不该被主动找上门：{picks_3}");
}

/// 寻人移动（头脑风暴清单 4）：面对面社交开始时，发起者走到对象身边。
#[test]
fn seek_move_puts_agent_beside_target() {
    let mut w = town_world(9, 4);
    w.set_bond(0, 2, 0.7);
    w.teleport(0, (30.0, 30.0)); // 玩家在住宅区
    w.teleport(2, (5.0, 5.0)); // 2 号在公园
    while living_core::time::hour_of(w.tick) < 10 {
        w.tick();
    }
    force_action(&mut w, 0, "chat_street");
    w.run(10);
    let d = living_core::space::dist(w.agent_pos(0).unwrap(), w.agent_pos(2).unwrap());
    assert!(d <= 1.0, "寻人移动应把发起者带到对象身边：距离 {d:.1} 米");
    let picks = social_picks(&w, 0);
    assert!(!picks.is_empty(), "应记录对象选择（可解释的'和谁'）");
}

/// 恋爱状态机：亲密达标的互动概率成为恋人；伴侣门放行约会行为；
/// 亲密跌破分手线 → 分手。全部概率与规则驱动，无剧本。
#[test]
fn couple_forms_unlocks_dates_and_breaks_up() {
    // 撮合概率拉高（参数是数据）：恋爱机制本身由概率覆盖，这里验证状态机。
    let json = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/town.json"))
        .unwrap();
    let cfg =
        load_config(&json.replace(r#""couple_form_prob": 0.08"#, r#""couple_form_prob": 0.5"#))
            .unwrap();
    let roster: Vec<(String, String)> = living_core::human_names(6)
        .into_iter()
        .map(|n| (n, "human".to_string()))
        .collect();
    let mut w = World::new(cfg, 11, roster, true);
    w.set_player(0);
    w.log_meta(11, 0);
    w.set_bond(1, 2, 0.88);
    for oid in [0u32, 3, 4, 5] {
        w.set_bond(1, oid, 0.0);
        w.set_bond(2, oid, 0.0);
    }
    // 反复让两人聊天（对象选择会互相找到对方——都住在镇上）。
    while living_core::time::hour_of(w.tick) < 9 {
        w.tick();
    }
    let mut coupled = false;
    for _ in 0..30 {
        force_action(&mut w, 1, "chat_street");
        w.run(90);
        if w.relations.are_couple(1, 2) {
            coupled = true;
            break;
        }
        while living_core::time::hour_of(w.tick) < 11 {
            w.tick();
        }
        w.run(60);
    }
    assert!(coupled, "亲密达标的两人反复互动应成为恋人（概率驱动）");
    assert!(w.logger.buf.contains(r#""t":"couple""#), "恋情应进日志");
    // 伴侣门：恋爱中的 1 号解锁约会行为（先压社交需求打开社交门）。
    w.agents[1].needs.social = 40.0;
    w.agents[3].needs.social = 40.0;
    let opts = w.debug_options(1);
    assert!(
        opts.iter().any(|o| o.behavior == "date_walk"),
        "恋爱中应解锁【和对象逛逛】：{:?}",
        opts.iter().map(|o| o.behavior.clone()).collect::<Vec<_>>()
    );
    // 单身者不解锁。
    let opts3 = w.debug_options(3);
    assert!(!opts3.iter().any(|o| o.behavior == "date_walk"));
    // 分手：亲密跌破分手线，日界评估。
    w.set_bond(1, 2, 0.2);
    for _ in 0..(2 * 1440) {
        w.tick();
        if !w.relations.are_couple(1, 2) {
            break;
        }
    }
    assert!(!w.relations.are_couple(1, 2), "亲密耗尽应分手");
}

/// 老龄（设计稿 4.8）：可塑性下降 + 怀旧（正面记忆相对上升）。
#[test]
fn aging_lowers_plasticity_and_biases_positive() {
    let cfg = town_cfg();
    let p = &cfg.params;
    let mut adult = living_core::Agent::new(1, 0, "成年".into(), "human".into(), &[], p);
    let mut elder = living_core::Agent::new(1, 1, "老年".into(), "human".into(), &[], p);
    elder.birth_tick = -((p.old_age_days as i64) * 1440 + 1440); // 已过老龄线
    let tick = 100;
    assert!((adult.plasticity(p, tick) - 1.0).abs() < 1e-9);
    assert!(elder.plasticity(p, tick) < 0.8, "老年可塑性应显著下降");
    assert!(elder.nostalgia(p, tick) > 0.3, "老年应怀旧");
    // 同一事件的写入强度：老年 < 成年。
    let s_adult = adult.memory.write(
        "事件",
        &[],
        1,
        0.8,
        adult.plasticity(p, tick),
        -0.5,
        false,
        BTreeMap::new(),
        tick,
    );
    let s_elder = elder.memory.write(
        "事件",
        &[],
        1,
        0.8,
        elder.plasticity(p, tick),
        -0.5,
        false,
        BTreeMap::new(),
        tick,
    );
    assert!(
        s_elder < s_adult * 0.8,
        "老年新记忆写入应更慢：{s_elder} vs {s_adult}"
    );
    // 怀旧消退：同样强度的正负记忆，老年人的正负比更高。
    let mut a_mem = living_core::memory::MemoryTable::new(100, 8);
    let mut e_mem = living_core::memory::MemoryTable::new(100, 8);
    for (mem, plast) in [(&mut a_mem, 1.0), (&mut e_mem, 0.45)] {
        mem.write("坏事", &[], 1, 0.9, plast, -0.8, false, BTreeMap::new(), 1);
        mem.write("好事", &[], 1, 0.9, plast, 0.8, false, BTreeMap::new(), 1);
    }
    for _ in 0..20 {
        a_mem.daily_decay(0.0);
        e_mem.daily_decay(0.5);
    }
    let strength = |m: &living_core::memory::MemoryTable, ev: &str| {
        m.entries
            .iter()
            .find(|e| e.event == ev)
            .map(|e| e.strength)
            .unwrap()
    };
    let adult_ratio = strength(&a_mem, "好事") / strength(&a_mem, "坏事");
    let elder_ratio = strength(&e_mem, "好事") / strength(&e_mem, "坏事");
    assert!(
        elder_ratio > adult_ratio * 1.5,
        "怀旧使正面记忆相对更重：{elder_ratio:.2} vs {adult_ratio:.2}"
    );
}

/// 建镇元老会自然变老：一年模拟的中后段出现老年代理（怀旧真实发生）。
#[test]
fn founders_grow_old_within_a_year() {
    let mut w = town_world(15, 8);
    w.logger.enabled = false;
    w.run(240 * 1440); // old_age_days=240：元老恰好步入老年
    let elders = w
        .agents
        .iter()
        .filter(|a| a.is_elder(&w.cfg.params, w.tick))
        .count();
    assert!(elders >= 6, "八个月后元老应全部步入老年：{elders}/8");
}

/// 代际（设计稿 5.6）：恋人生育 → 新生儿继承父母习惯权重与口味、
/// 童年高可塑、与父母天生亲近、不参加工作。
#[test]
fn birth_inherits_parents_and_grows_as_child() {
    // 生育概率拉满、冷静期归零（参数是数据——测试改数据不改引擎）。
    let json = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/town.json"))
        .unwrap();
    let cfg = load_config(
        &json
            .replace(r#""birth_prob": 0.02"#, r#""birth_prob": 1.0"#)
            .replace(
                r#""birth_min_couple_days": 20"#,
                r#""birth_min_couple_days": 0"#,
            ),
    )
    .unwrap();
    let roster: Vec<(String, String)> = living_core::human_names(6)
        .into_iter()
        .map(|n| (n, "human".to_string()))
        .collect();
    let mut w = World::new(cfg, 19, roster, true);
    w.set_player(0);
    // 直接撮合一对（关系表白已由上一测试覆盖，这里测代际）。
    w.set_bond(1, 2, 0.95);
    w.relations.set_couple(1, 2, w.tick);
    // 父母先把某个行为的权重做出个人特色（孩子应继承这个倾向）。
    for _ in 0..12 {
        w.agents[1].habit_boost("walk_park", 0.06, w.cfg.params.habit_weight_cap);
        w.agents[2].habit_boost("walk_park", 0.06, w.cfg.params.habit_weight_cap);
    }
    let parent_avg = (w.agents[1].weights["walk_park"] + w.agents[2].weights["walk_park"]) / 2.0;
    let before = w.agents.len();
    // 跑过恰好一个日界：必生一胎。
    w.run(1440 + 60);
    assert_eq!(w.agents.len(), before + 1, "第二天应添一名新生儿");
    assert!(w.logger.buf.contains(r#""t":"born""#));
    let baby = w.agents.last().unwrap();
    assert!(baby.age_days(w.tick) < 3.0, "新生儿年龄应按天计");
    assert_eq!(baby.species, "human");
    // 继承：孩子对 walk_park 的初始倾向 ≈ 父母均值（±个体抖动 10%）。
    let inherited = baby.weights.get("walk_park").copied().unwrap_or(0.0);
    assert!(
        (inherited / parent_avg - 1.0).abs() < 0.35,
        "孩子的初始权重应接近父母均值：{inherited:.2} vs {parent_avg:.2}"
    );
    // 与父母天生亲近；童年高可塑。
    assert!(w.relations.bond(baby.id, 1) >= 0.8);
    assert!((baby.plasticity(&w.cfg.params, w.tick) - w.cfg.params.child_plasticity).abs() < 1e-9);
    // 童年不工作：过两个日界的职业市场也不上岗。
    w.run(3 * 1440);
    assert!(w.agents.last().unwrap().job.is_none(), "孩子不该去上班");
}

/// 变体层（方案 B）：付得起的变体才进候选；菜单挂地点；
/// 变体级习惯累积（"总点那一家"）。
#[test]
fn variant_layer_menu_price_and_habit() {
    let mut w = town_world(21, 4);
    while living_core::time::hour_of(w.tick) < 9 {
        w.tick();
    }
    // 钱包 12：泡面只买得起红烧牛肉面（10），豪华海鲜面（16）出局。
    w.agents[0].wallet = 12.0;
    w.agents[0].needs.hunger = 30.0;
    force_action(&mut w, 0, "instant_noodles");
    w.run(3); // 决策发生在下一拍，动作还在进行中
    assert_eq!(w.agents[0].current_variant.as_deref(), Some("noodle_beef"));
    // 菜单挂地点：下馆子只能在饭馆菜单里选（两个变体都在菜单——验证机制走通：
    // 把钱包加回去后变体应在菜单集合内）。
    w.agents[0].wallet = 500.0;
    w.agents[0].needs.hunger = 30.0;
    force_action(&mut w, 0, "diner_meal");
    w.run(20);
    let v = w.agents[0].current_variant.clone();
    assert!(
        v.as_deref() == Some("diner_set") || v.as_deref() == Some("diner_signature"),
        "变体应来自饭馆菜单：{v:?}"
    );
    // 变体级习惯：反复在家吃饭，选中的口味权重上升。
    let before: f64 = w.agents[0]
        .variant_weights
        .values()
        .cloned()
        .fold(0.0, f64::max);
    for _ in 0..6 {
        while w.agents[0].current.is_some() {
            w.tick();
        }
        w.agents[0].needs.hunger = 30.0;
        w.agents[0].wallet = 500.0;
        force_action(&mut w, 0, "eat_home");
        w.run(60);
    }
    let after: f64 = w.agents[0]
        .variant_weights
        .values()
        .cloned()
        .fold(0.0, f64::max);
    assert!(
        after > before.max(1.0) * 1.15,
        "总点的口味应养成习惯：{before:.2} → {after:.2}"
    );
}

/// 变体级目标（"这个月不吃大鱼大肉"）：只压制那个变体，
/// 同行为的其他口味照常（设计稿 4.5 精确到变体）。
#[test]
fn variant_goal_targets_single_variant() {
    let mut w = town_world(23, 4);
    w.agents[0].willpower = 100.0;
    w.set_goal(0, "eat_home#home_feast", 3)
        .expect("变体级目标应可设立");
    let mut saw_other = false;
    let mut resisted_or_avoided = false;
    for _ in 0..10 {
        while living_core::time::hour_of(w.tick) < 12 {
            w.tick();
        }
        while w.agents[0].current.is_some() {
            w.tick();
        }
        w.agents[0].needs.hunger = 30.0;
        w.agents[0].wallet = 500.0;
        force_action(&mut w, 0, "eat_home");
        w.run(3);
        match w.agents[0].current_variant.as_deref() {
            Some("home_feast") => {} // 忍住或失守都会在下面日志里查
            Some(_) => saw_other = true,
            None => {}
        }
        w.run(80); // 等这顿饭结束
        if w.logger.buf.contains(r#""behavior":"eat_home#home_feast""#) {
            resisted_or_avoided = true; // GoalResist/Violation 带变体键
            break;
        }
    }
    assert!(saw_other, "家常菜（非戒断变体）应照常可吃");
    assert!(
        resisted_or_avoided
            || w.agents[0]
                .variant_weights
                .get("home_feast")
                .copied()
                .unwrap_or(1.0)
                < 1.05,
        "目标应压制大鱼大肉的口味（忍住换口味，或权重被压）"
    );
}

/// 流行传播（设计稿 5.4，统计回归观察）：新活动（奶茶）经新奇需求与
/// 行为模仿扩散，随后随习惯化与新颖度消退而起落。
#[test]
fn fad_rises_then_falls() {
    let mut w = town_world(29, 16);
    w.logger.enabled = false;
    let mut monthly: Vec<u64> = Vec::new();
    let mut prev_tea = 0u64;
    for _ in 0..8 {
        w.run(30 * 1440);
        let tea: u64 = w
            .agents
            .iter()
            .map(|a| a.exec_counts.get("bubble_tea").copied().unwrap_or(0))
            .sum();
        monthly.push((tea - prev_tea).max(1));
        prev_tea = tea;
    }
    let peak = monthly.iter().copied().max().unwrap();
    let first = monthly[0];
    let last = *monthly.last().unwrap();
    println!(
        "[M5.2] 奶茶月度执行：{:?}（峰 {peak} 首 {first} 尾 {last}）",
        monthly
    );
    assert!(
        peak > first * 2 && peak >= 8,
        "流行应起：首月 {first} → 峰值 {peak}，{monthly:?}"
    );
    assert!(
        (last as f64) < peak as f64 * 0.75,
        "流行应落：峰值 {peak} → 末月 {last}，{monthly:?}"
    );
}

/// 日记与年度总结（设计稿 6.4）：从日志自动生成，内容与模拟数据一致。
#[test]
fn diary_and_annual_summary_from_log() {
    let mut w = town_world(31, 10);
    w.run(3 * 1440);
    let day = w.tick.saturating_sub(1) / 1440;
    let events = living_core::diary::events_of_day(w.logger.buf.lines(), day);
    let name_of = |id: u32| -> String {
        w.agents
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| format!("#{id}"))
    };
    let label = |bid: &str, vid: Option<&str>| -> String {
        let b = w
            .cfg
            .behaviors
            .iter()
            .find(|x| x.id == bid)
            .map(|x| x.display_name().to_string())
            .unwrap_or_else(|| bid.to_string());
        match vid {
            Some(v) => format!("{b}·{v}"),
            None => b,
        }
    };
    let diary = living_core::diary::day_diary(&events, 0, name_of, &label);
    println!("[M5.2] 日记样例：");
    for l in diary.iter().take(5) {
        println!("  {l}");
    }
    assert!(!diary.is_empty(), "一天过去应写得出日记");
    assert!(
        diary.iter().any(|l| l.contains("今天主要在")),
        "日记应总结一天的行为分布"
    );
    // 年度总结（数据从全年日志统计）。
    let all: Vec<living_core::LogEvent> = w
        .logger
        .buf
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let annual = living_core::diary::annual_summary(&all, 0, name_of, &label);
    assert!(
        annual.iter().any(|l| l.contains("最常做的事")),
        "年度总结应点名本年最常做的事"
    );
}

/// M5 二阶段回放确定性：恋爱、出生、变体、流行全套机制下双跑一致。
#[test]
fn m5_2_replay_deterministic() {
    let script = |w: &mut World| {
        w.set_bond(1, 2, 0.9);
        w.run(30 * 1440);
    };
    let mut a = town_world(37, 10);
    a.logger.enabled = false;
    script(&mut a);
    let mut b = town_world(37, 10);
    b.logger.enabled = false;
    script(&mut b);
    assert_eq!(a.stats.thefts, b.stats.thefts);
    assert_eq!(a.stats.couples_formed, b.stats.couples_formed);
    assert_eq!(a.stats.births, b.stats.births);
    let wa: Vec<f64> = a.agents.iter().map(|x| x.wallet.round()).collect();
    let wb: Vec<f64> = b.agents.iter().map(|x| x.wallet.round()).collect();
    assert_eq!(wa, wb, "同种子的钱包终态应一致");
    let ea: Vec<u64> = a
        .agents
        .iter()
        .map(|x| x.exec_counts.get("bubble_tea").copied().unwrap_or(0))
        .collect();
    let eb: Vec<u64> = b
        .agents
        .iter()
        .map(|x| x.exec_counts.get("bubble_tea").copied().unwrap_or(0))
        .collect();
    assert_eq!(ea, eb, "同种子的流行轨迹应一致");
}

/// M5 二阶段存档往返：恋爱表、变体权重、新奇、年龄随快照往返。
#[test]
fn save_load_m5_2_roundtrip() {
    let mut a = town_world(41, 8);
    a.set_bond(1, 2, 0.9);
    a.relations.set_couple(1, 2, a.tick);
    a.run(3 * 1440);
    let bytes = a.to_save();
    let offset = a.logger.buf.len();
    a.run(2 * 1440);
    let mut b = World::from_save(town_cfg(), &bytes).unwrap();
    assert!(b.relations.are_couple(1, 2), "恋情应随存档往返");
    b.run(2 * 1440);
    assert_eq!(
        b.logger.buf,
        a.logger.buf[offset..],
        "读档继续的日志应与连续运行完全一致"
    );
}

// ---------- M6：目标关卡、你的故事、存档槽 ----------

/// M6 验收链：目标关卡的进度查询、开局事件注入、到期结算与日志。
#[test]
fn scenario_start_progress_settle() {
    // 关卡断言要看日志，这里开日志建世界（m3_world 为省内存默认关）。
    let cfg = load_config(&m3_config()).unwrap();
    let roster: Vec<(String, String)> = living_core::human_names(6)
        .into_iter()
        .map(|n| (n, "human".to_string()))
        .collect();
    let mut w = World::new(cfg, 11, roster, true);
    w.set_player(0);
    let def = living_core::scenario::load_scenario(
        r#"{"id":"t","name":"测试","description":"","days":2,
            "win":[{"kind":"area_reputation","area":"park","min":0.5}],
            "inject_events":[{"day":0,"event_index":0}]}"#,
    )
    .unwrap();
    w.start_scenario(def).unwrap();
    // 进度可查：声誉条件当前值 = 公园初始声誉。
    let prog = w.scenario_progress();
    assert_eq!(prog.len(), 1);
    assert!((prog[0].current - 1.0).abs() < 1e-9, "开局公园满声誉");
    // 跑两天：第 0 天注入一次滋事事件（写日志），到期结算。
    w.run(2 * 1440);
    assert!(
        w.logger.buf.contains("\"t\":\"scenario_result\""),
        "应有关卡结算日志"
    );
    let active = w.scenario.as_ref().unwrap();
    assert!(active.finished.is_some(), "到期后应已结算");
    assert!(
        w.logger.buf.contains("\"t\":\"env_event\"") || w.logger.buf.contains("park_rowdy"),
        "开局注入的事件应已触发"
    );
    // 已结算后进度仍可查，且不再重复结算。
    let tick_now = w.tick;
    w.run(1440);
    let results = w.logger.buf.matches("\"t\":\"scenario_result\"").count();
    assert_eq!(results, 1, "结算只发生一次");
    assert!(w.tick >= tick_now + 1440);
}

/// 胜利路径：条件天然满足时到期判胜。
#[test]
fn scenario_wins_when_conditions_met() {
    let mut w = m3_world(5, 2);
    w.start_scenario(
        living_core::scenario::load_scenario(
            r#"{"id":"easy","name":"白送","description":"","days":1,
            "win":[{"kind":"wallet","min":0.0}]}"#,
        )
        .unwrap(),
    )
    .unwrap();
    w.run(1440);
    assert!(
        w.scenario.as_ref().unwrap().finished == Some(true),
        "钱包≥0 应判胜"
    );
}

/// 你的故事：从日志事件里挑出值得讲述的大事（M6 验收的载体）。
#[test]
fn player_story_collects_notables() {
    use living_core::LogEvent;
    let events = vec![
        LogEvent::Couple {
            tick: 600,
            a: 0,
            b: 3,
            started: true,
        },
        LogEvent::Theft {
            tick: 1200,
            thief: 2,
            victim: 0,
            amount: 70.0,
            caught: false,
            area: "park".into(),
        },
        LogEvent::JobChange {
            tick: 2000,
            agent: 0,
            from: Some("baker".into()),
            to: Some("courier".into()),
            reason: "迫于生计".into(),
        },
        LogEvent::Born {
            tick: 3000,
            agent: 9,
            mother: 0,
            father: 3,
        },
        LogEvent::Startled {
            tick: 3400,
            agent: 0,
            event: "park_rowdy".into(),
        },
        LogEvent::MemoryWrite {
            tick: 3500,
            agent: 0,
            event: "目睹:park_rowdy".into(),
            salience: 0.6,
            mults: Default::default(),
            source: Some(2),
            vicarious: true,
        },
    ];
    let name_of = |id: u32| -> String { format!("#{id}") };
    let label = |b: &str, _: Option<&str>| -> String { b.to_string() };
    let story = living_core::diary::player_story(&events, 0, name_of, &label);
    let joined = story.join("\n");
    assert!(joined.contains("在一起了"), "应讲述恋爱：{joined}");
    assert!(joined.contains("被#2偷走"), "应讲述被偷：{joined}");
    assert!(joined.contains("转行"), "应讲述转行：{joined}");
    assert!(joined.contains("孩子"), "应讲述孩子出生：{joined}");
    assert!(joined.contains("吓得不轻"), "应讲述被吓：{joined}");
    assert!(joined.contains("目睹"), "应讲述目睹所学：{joined}");
    // 按时间排序：第一行是恋爱（tick 600）。
    assert!(story[0].contains("在一起了"), "故事按时间排序");
}

/// 存档槽：写入 → 列表元信息 → 读档续跑一致。
#[test]
fn save_slots_roundtrip_and_meta() {
    let dir = std::env::temp_dir().join(format!("living_slots_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let json = m3_config();
    let mut a = World::new(
        load_config(&json).unwrap(),
        5,
        cat_roster(cat_names(2)),
        true,
    );
    a.set_player(0);
    a.run(900);
    a.start_scenario(
        living_core::scenario::load_scenario(
            r#"{"id":"s","name":"槽位关卡","description":"","days":5,"win":[{"kind":"wallet","min":0.0}]}"#,
        )
        .unwrap(),
    )
    .unwrap();
    a.save_to_slot(&dir, "slot-1").unwrap();

    // 元信息列表。
    let slots = World::list_slots(&dir);
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].0, "slot-1");
    assert_eq!(slots[0].1.agents, 2);
    assert_eq!(slots[0].1.scenario.as_ref().unwrap().1, "槽位关卡");

    // 读档续跑 = 连续运行。
    let mut b = World::load_from_slot(load_config(&json).unwrap(), &dir, "slot-1").unwrap();
    let offset = a.logger.buf.len();
    a.run(300);
    b.run(300);
    assert_eq!(
        b.logger.buf,
        a.logger.buf[offset..],
        "槽位读档续跑应与连续运行一致"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// 盗窃调平回归（M6 内容调参，M5 观察项）：年盗窃量低于调参前的 435。
#[test]
fn theft_rate_tuned_down() {
    let mut w = town_world(5, 26);
    w.logger.enabled = false;
    w.run(365 * 1440);
    let st = &w.stats;
    println!("[M6 调平] 一年盗窃 {} 起（原 M5 记录 435）", st.thefts);
    assert!(
        st.thefts < 400,
        "调参后年盗窃应低于 400（目标继续逼近 <200，靠后续内容量）：{}",
        st.thefts
    );
}

// ---------- M6 二阶段：童年实时模仿、人口退出 ----------

/// 改参数后的 town 配置（参数是数据——测试改数据不改引擎）。
fn town_cfg_patched(repls: &[(&str, &str)]) -> living_core::SimConfig {
    let json = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/town.json"))
        .expect("需要 assets/town.json");
    let mut json = json;
    for (from, to) in repls {
        json = json.replace(from, to);
    }
    load_config(&json).unwrap()
}

/// 生一个孩子并等待其落地（生育概率拉满 + 直接撮合一对），随后推进到
/// 上午 9 点（公园等白天行为的时间门打开）。返回孩子编号。
fn bear_a_child(w: &mut World) -> u32 {
    w.set_bond(1, 2, 0.95);
    w.relations.set_couple(1, 2, w.tick);
    let before = w.agents.len();
    w.run(1440 + 60);
    assert_eq!(w.agents.len(), before + 1, "第二天应添一名新生儿");
    while living_core::time::hour_of(w.tick) < 9 {
        w.tick();
    }
    w.agents.last().unwrap().id
}

/// 童年实时模仿（M6 二阶段）：父母做什么，孩子跟在身边做同样的事。
/// 跟随选择来自父母而非 softmax（Decision.imitated），门把照常把关。
#[test]
fn childhood_realtime_imitation_follows_parent() {
    let cfg = town_cfg_patched(&[
        (r#""birth_prob": 0.02"#, r#""birth_prob": 1.0"#),
        (r#""birth_min_couple_days": 20"#, r#""birth_min_couple_days": 0"#),
        (r#""child_imitate_prob": 0.5"#, r#""child_imitate_prob": 1.0"#),
    ]);
    let roster: Vec<(String, String)> = living_core::human_names(6)
        .into_iter()
        .map(|n| (n, "human".to_string()))
        .collect();
    let mut w = World::new(cfg, 23, roster, true);
    w.set_player(0);
    let baby = bear_a_child(&mut w);
    // 需求调舒适：生理反射不抢先（饿着的孩子先吃饭，不跟着散步——
    // 这是设计；这里验证的是"无饥无忧时的跟随"）。
    {
        let b = &mut w.agents[baby as usize];
        b.needs.hunger = 80.0;
        b.needs.energy = 80.0;
        b.needs.fun = 80.0;
        b.needs.social = 80.0;
        b.needs.hygiene = 80.0;
        b.needs.bladder = 80.0;
    }
    // 父母去公园散步（免费、无需求门——孩子做得了）。
    force_action(&mut w, 1, "walk_park");
    w.agents[baby as usize].lod = 0; // 跟随发生在逐拍模拟层（LOD）
    w.agents[baby as usize].current = None;
    w.agents[baby as usize].next_heartbeat = w.tick + 1;
    w.run(2);
    assert_eq!(
        w.agents[1].current.as_ref().map(|(b, _, _)| b.as_str()),
        Some("walk_park")
    );
    let baby_doing = w.agents[baby as usize]
        .current
        .as_ref()
        .map(|(b, _, _)| b.clone());
    assert_eq!(
        baby_doing.as_deref(),
        Some("walk_park"),
        "孩子应跟着父母做同样的事"
    );
    assert_eq!(w.agents[baby as usize].imitate_from, Some(1));
    assert!(
        w.logger.buf.contains(r#""imitated":true"#),
        "跟随选择应有日志标记"
    );
    // 同地点（跟在父母身边）：两人都在公园草坪。
    let pp = w.agents[1].pos;
    let bp = w.agents[baby as usize].pos;
    let d = ((pp.0 - bp.0).powi(2) + (pp.1 - bp.1).powi(2)).sqrt();
    assert!(d < 0.5, "应在同一地点：{pp:?} vs {bp:?}");
}

/// 门把把关：父母做的事孩子做不了（需求门未开），不跟随。
#[test]
fn childhood_imitation_respects_gates() {
    let cfg = town_cfg_patched(&[
        (r#""birth_prob": 0.02"#, r#""birth_prob": 1.0"#),
        (r#""birth_min_couple_days": 20"#, r#""birth_min_couple_days": 0"#),
        (r#""child_imitate_prob": 0.5"#, r#""child_imitate_prob": 1.0"#),
    ]);
    let roster: Vec<(String, String)> = living_core::human_names(6)
        .into_iter()
        .map(|n| (n, "human".to_string()))
        .collect();
    let mut w = World::new(cfg, 29, roster, true);
    w.set_player(0);
    let baby = bear_a_child(&mut w);
    // 孩子不饿（hunger 100）：吃饭的门不开。
    w.agents[baby as usize].needs.hunger = 100.0;
    w.agents[baby as usize].lod = 0;
    w.agents[baby as usize].current = None;
    w.agents[baby as usize].next_heartbeat = w.tick + 1;
    // 父方要真饿（否则 eat_home 起不来，线索会落到另一位家长身上）；
    // 另一位家长钉住不做事，保证孩子的模仿线索只有 eat_home。
    w.agents[1].needs.hunger = 40.0;
    w.agents[2].current = None;
    w.agents[2].next_heartbeat = w.tick + 500;
    let log_mark = w.logger.buf.len(); // 只看本阶段日志（出生当天可能已有合法跟随）
    force_action(&mut w, 1, "eat_home");
    w.run(2);
    assert!(
        !w.logger.buf[log_mark..].contains(r#""imitated":true"#),
        "门未开的行为不该被跟随"
    );
    assert_ne!(
        w.agents[baby as usize]
            .current
            .as_ref()
            .map(|(b, _, _)| b.as_str()),
        Some("eat_home")
    );
}

/// 概率关到 0：跟随机制整体关闭（不产生跟随决策）。
#[test]
fn childhood_imitation_disabled_by_prob() {
    let cfg = town_cfg_patched(&[
        (r#""birth_prob": 0.02"#, r#""birth_prob": 1.0"#),
        (r#""birth_min_couple_days": 20"#, r#""birth_min_couple_days": 0"#),
        (r#""child_imitate_prob": 0.5"#, r#""child_imitate_prob": 0.0"#),
    ]);
    let roster: Vec<(String, String)> = living_core::human_names(6)
        .into_iter()
        .map(|n| (n, "human".to_string()))
        .collect();
    let mut w = World::new(cfg, 31, roster, true);
    w.set_player(0);
    let baby = bear_a_child(&mut w);
    force_action(&mut w, 1, "walk_park");
    w.agents[baby as usize].lod = 0;
    w.agents[baby as usize].current = None;
    w.agents[baby as usize].next_heartbeat = w.tick + 1;
    w.run(2);
    assert!(
        !w.logger.buf.contains(r#""imitated":true"#),
        "概率为 0 时不该有跟随决策"
    );
}

/// 人口退出（M6 二阶段）：年迈离场——概率拉满后除玩家外全体离开，
/// 恋人自动分手、岗位腾空、日志有 Depart；名册保留（id 与记忆都在）。
#[test]
fn elders_depart_and_leave_cleanly() {
    let cfg = town_cfg_patched(&[
        (r#""depart_age_days": 300"#, r#""depart_age_days": 61"#),
        (r#""depart_prob": 0.0025"#, r#""depart_prob": 1.0"#),
    ]);
    let roster: Vec<(String, String)> = living_core::human_names(6)
        .into_iter()
        .map(|n| (n, "human".to_string()))
        .collect();
    let mut w = World::new(cfg, 27, roster, true);
    w.set_player(0);
    // 撮合一对：离场时应自动分手（另一半回到单身）。
    w.set_bond(1, 2, 0.95);
    w.relations.set_couple(1, 2, w.tick);
    w.run(2 * 1440);
    assert_eq!(w.stats.departures, 5, "除玩家外 5 位元老应全部离场");
    assert_eq!(w.agents.len(), 6, "名册保留（id=下标不变量）");
    for a in &w.agents {
        if a.is_player {
            assert!(a.present(), "玩家永不离场");
        } else {
            assert!(!a.present(), "{} 应已离场", a.name);
            assert!(a.job.is_none(), "离场应腾出岗位");
        }
    }
    assert!(w.relations.couple_pairs().is_empty(), "离场应自动分手");
    assert!(w.logger.buf.contains(r#""t":"depart"#));
    assert!(w.logger.buf.contains(r#""reason":"年迈""#));
    // 离场者的社交不应再被找到（给离场者打电话就过了）。
    let news = w.news_digest(10);
    assert!(
        news.iter().any(|n| n.contains("离开小镇")),
        "离场应上新闻：{news:?}"
    );
}

/// 一年统计（M6 调参）：离场温和发生——老镇有出有进，不会被掏空。
#[test]
fn town_year_departure_statistics() {
    let mut w = town_world(5, 26);
    w.logger.enabled = false;
    w.run(365 * 1440);
    let present = w.agents.iter().filter(|a| a.present()).count();
    println!(
        "[M6 二阶段] 一年：离场 {} 位 · 新生儿 {} 名 · 年末在场 {}/{}",
        w.stats.departures,
        w.stats.births,
        present,
        w.agents.len()
    );
    assert!(
        (1..=12).contains(&w.stats.departures),
        "离场应温和发生（首年约四分之一元老）：{}",
        w.stats.departures
    );
    assert!(present >= 18, "镇子不该被掏空：{present}/26");
    assert!(w.stats.births >= 1, "出生照常（离场腾出人口额度）");
}

/// 离场后的存档往返：读档续跑与连续运行完全一致。
#[test]
fn save_load_m6_departure_roundtrip() {
    let json = {
        let base = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/town.json"
        ))
        .unwrap();
        base.replace(r#""depart_age_days": 300"#, r#""depart_age_days": 61"#)
            .replace(r#""depart_prob": 0.0025"#, r#""depart_prob": 1.0"#)
    };
    let mk = || {
        let cfg = load_config(&json).unwrap();
        let roster: Vec<(String, String)> = living_core::human_names(6)
            .into_iter()
            .map(|n| (n, "human".to_string()))
            .collect();
        let mut w = World::new(cfg, 33, roster, true);
        w.set_player(0);
        w
    };
    let mut a = mk();
    a.run(2 * 1440); // 全员离场后的世界
    let bytes = a.to_save();
    let offset = a.logger.buf.len();
    a.run(2 * 1440);
    let mut b = World::from_save(load_config(&json).unwrap(), &bytes).unwrap();
    b.run(2 * 1440);
    assert_eq!(b.stats.departures, a.stats.departures);
    assert_eq!(
        b.logger.buf,
        a.logger.buf[offset..],
        "离场世界的读档续跑应与连续运行一致"
    );
}

/// 职业门回归（M6 二阶段内容回填）：无业者不执行工作行为——
/// 街道岗位 6 个，9 人里挤不进岗位的 3 人不该出现在写字间。
#[test]
fn jobless_never_perform_work_behavior() {
    let mut w = street_world(43, 9, 0);
    let employed = w.agents.iter().filter(|a| a.job.is_some()).count();
    assert_eq!(employed, 6, "6 个岗位给 9 人中的前 6 位");
    w.run(5 * 1440);
    for a in &w.agents {
        let worked = a.exec_counts.get("go_to_work").copied().unwrap_or(0);
        if a.job.is_none() {
            assert_eq!(worked, 0, "无业的 {} 不应上班", a.name);
        }
    }
    let total: u64 = w.agents.iter().map(|a| a.exec_counts.get("go_to_work").copied().unwrap_or(0)).sum();
    assert!(total > 0, "在岗者照常上班：{total}");
}
