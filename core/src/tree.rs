//! 树结构评估：给定当前需求与时刻，返回可用的行为 ID 列表。
//! 结构只负责"可用性"（需求门、时间门、子树展开），概率与个性全部在数值层（见 world.rs 的决策函数）。

use crate::config::{BehaviorDef, TreeNode};
use crate::needs::Needs;
use std::collections::BTreeMap;

pub fn available_behaviors<'a>(
    node: &'a TreeNode,
    needs: &Needs,
    behaviors: &'a BTreeMap<String, BehaviorDef>,
    subtrees: &'a BTreeMap<String, TreeNode>,
    hour: u32,
    out: &mut Vec<&'a str>,
) {
    match node {
        TreeNode::Selector { children } => {
            for c in children {
                available_behaviors(c, needs, behaviors, subtrees, hour, out);
            }
        }
        TreeNode::Gate { need, below, child } => {
            // 门放行条件：需求当前值低于阈值。
            if let Some(v) = needs.get(need) {
                if v < *below {
                    available_behaviors(child, needs, behaviors, subtrees, hour, out);
                }
            }
        }
        TreeNode::TimeGate { from, to, child } => {
            // 时间门放行条件：当前小时在窗口内（昼夜节律，如睡觉/逛公园）。
            if crate::time::in_hour_window(hour, *from, *to) {
                available_behaviors(child, needs, behaviors, subtrees, hour, out);
            }
        }
        TreeNode::Subtree { tree_ref } => {
            // 子树引用：原地展开（校验已保证存在且无环）。
            if let Some(sub) = subtrees.get(tree_ref) {
                available_behaviors(sub, needs, behaviors, subtrees, hour, out);
            }
        }
        TreeNode::Action { behavior } => {
            // 只有注册表中存在的行为才可用（校验已保证引用合法，此处防御重复/缺失）。
            if behaviors.contains_key(behavior) && !out.contains(&behavior.as_str()) {
                out.push(behavior.as_str());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> BTreeMap<String, BehaviorDef> {
        let mut m = BTreeMap::new();
        m.insert(
            "eat".into(),
            serde_json::from_str(
                r#"{"id":"eat","base_weight":3,"duration_min":10,"duration_max":30,"spot":"bowl"}"#,
            )
            .unwrap(),
        );
        m.insert(
            "play".into(),
            serde_json::from_str(
                r#"{"id":"play","base_weight":1,"duration_min":10,"duration_max":20}"#,
            )
            .unwrap(),
        );
        m
    }

    fn no_subtrees() -> BTreeMap<String, TreeNode> {
        BTreeMap::new()
    }

    #[test]
    fn gate_blocks_when_satisfied() {
        let b = cfg();
        let tree: TreeNode = serde_json::from_str(
            r#"{"type":"selector","children":[
                {"type":"gate","need":"hunger","below":70,"child":{"type":"action","behavior":"eat"}},
                {"type":"action","behavior":"play"}]}"#,
        )
        .unwrap();
        let mut needs = Needs::full(); // hunger = 100
        let mut out = vec![];
        let subs = no_subtrees();
        available_behaviors(&tree, &needs, &b, &subs, 12, &mut out);
        assert_eq!(out, vec!["play"]);

        needs.hunger = 50.0;
        let mut out = vec![];
        let subs = no_subtrees();
        available_behaviors(&tree, &needs, &b, &subs, 12, &mut out);
        assert_eq!(out, vec!["eat", "play"]);
    }

    /// 时间门：夜间行为只在夜间窗口放行（跨零点窗口）。
    #[test]
    fn time_gate_gates_day_and_night() {
        let b = cfg();
        let tree: TreeNode = serde_json::from_str(
            r#"{"type":"selector","children":[
                {"type":"time_gate","from":22,"to":6,"child":{"type":"action","behavior":"eat"}},
                {"type":"action","behavior":"play"}]}"#,
        )
        .unwrap();
        let needs = Needs::full();
        let mut night = vec![];
        let subs = no_subtrees();
        available_behaviors(&tree, &needs, &b, &subs, 23, &mut night);
        assert_eq!(night, vec!["eat", "play"], "夜间放行夜间行为");
        let mut day = vec![];
        available_behaviors(&tree, &needs, &b, &subs, 12, &mut day);
        assert_eq!(day, vec!["play"], "白天不放行夜间行为");
    }

    /// 子树引用（M4.5）：按名展开，与内联写法结果一致。
    #[test]
    fn subtree_reference_expands_inline() {
        let b = cfg();
        let mut subs: BTreeMap<String, TreeNode> = BTreeMap::new();
        subs.insert(
            "meals".into(),
            serde_json::from_str(
                r#"{"type":"gate","need":"hunger","below":70,"child":{"type":"action","behavior":"eat"}}"#,
            )
            .unwrap(),
        );
        let via_ref: TreeNode = serde_json::from_str(
            r#"{"type":"selector","children":[
                {"type":"subtree","ref":"meals"},
                {"type":"action","behavior":"play"}]}"#,
        )
        .unwrap();
        let inline: TreeNode = serde_json::from_str(
            r#"{"type":"selector","children":[
                {"type":"gate","need":"hunger","below":70,"child":{"type":"action","behavior":"eat"}},
                {"type":"action","behavior":"play"}]}"#,
        )
        .unwrap();
        let mut needs = Needs::full();
        needs.hunger = 50.0;
        let mut a = vec![];
        let mut b2 = vec![];
        available_behaviors(&via_ref, &needs, &b, &subs, 12, &mut a);
        available_behaviors(&inline, &needs, &b, &subs, 12, &mut b2);
        assert_eq!(a, b2, "子树引用应与内联等价");
        assert_eq!(a, vec!["eat", "play"]);

        // 门关时子树同样不放行。
        let mut full = vec![];
        available_behaviors(
            &via_ref,
            &Needs::full(),
            &b,
            &subs,
            12,
            &mut full,
        );
        assert_eq!(full, vec!["play"]);
    }

    /// 子树校验：未定义引用与循环引用都被拒绝。
    #[test]
    fn subtree_validation_rejects_missing_and_cycles() {
        let mut cfg_json: serde_json::Value = serde_json::from_str(
            r#"{
              "needs": {"hunger": 4, "energy": 3, "fun": 5, "social": 3, "hygiene": 2, "bladder": 4},
              "critical_need": 15,
              "params": {"alpha": 0.06, "habit_decay_hourly": 0.02,
                "temperature_min": 0.7, "temperature_max": 1.4,
                "heartbeat_min": 5, "heartbeat_max": 20,
                "memory_cap": 500, "memory_top_k": 8, "weight_jitter": 0.1},
              "behaviors": [
                {"id": "eat", "base_weight": 3, "duration_min": 10, "duration_max": 30}
              ],
              "tree": {"type":"subtree","ref":"meals"},
              "subtrees": {
                "meals": {"type":"selector","children":[
                  {"type":"action","behavior":"eat"},
                  {"type":"subtree","ref":"meals"}
                ]}
              },
              "spots": [],
              "env_events": []
            }"#,
        )
        .unwrap();
        let raw = cfg_json.to_string();
        assert!(
            crate::config::load_config(&raw).is_err(),
            "循环子树引用应被拒绝"
        );
        // 换成未定义引用。
        cfg_json["tree"] = serde_json::json!({"type": "subtree", "ref": "nope"});
        assert!(
            crate::config::load_config(&cfg_json.to_string()).is_err(),
            "未定义子树引用应被拒绝"
        );
        // 去掉环后应通过。
        cfg_json["tree"] = serde_json::json!({"type": "subtree", "ref": "meals"});
        cfg_json["subtrees"]["meals"]["children"] =
            serde_json::json!([{"type": "action", "behavior": "eat"}]);
        assert!(
            crate::config::load_config(&cfg_json.to_string()).is_ok(),
            "无环的子树引用应通过"
        );
    }
}
