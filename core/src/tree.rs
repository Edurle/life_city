//! 树结构评估：给定当前需求与时刻，返回可用的行为 ID 列表。
//! 结构只负责"可用性"（需求门、时间门），概率与个性全部在数值层（见 world.rs 的决策函数）。

use crate::config::{BehaviorDef, TreeNode};
use crate::needs::Needs;
use std::collections::BTreeMap;

pub fn available_behaviors<'a>(
    node: &'a TreeNode,
    needs: &Needs,
    behaviors: &'a BTreeMap<String, BehaviorDef>,
    hour: u32,
    out: &mut Vec<&'a str>,
) {
    match node {
        TreeNode::Selector { children } => {
            for c in children {
                available_behaviors(c, needs, behaviors, hour, out);
            }
        }
        TreeNode::Gate { need, below, child } => {
            // 门放行条件：需求当前值低于阈值。
            if let Some(v) = needs.get(need) {
                if v < *below {
                    available_behaviors(child, needs, behaviors, hour, out);
                }
            }
        }
        TreeNode::TimeGate { from, to, child } => {
            // 时间门放行条件：当前小时在窗口内（昼夜节律，如睡觉/逛公园）。
            if crate::time::in_hour_window(hour, *from, *to) {
                available_behaviors(child, needs, behaviors, hour, out);
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
        available_behaviors(&tree, &needs, &b, 12, &mut out);
        assert_eq!(out, vec!["play"]);

        needs.hunger = 50.0;
        let mut out = vec![];
        available_behaviors(&tree, &needs, &b, 12, &mut out);
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
        available_behaviors(&tree, &needs, &b, 23, &mut night);
        assert_eq!(night, vec!["eat", "play"], "夜间放行夜间行为");
        let mut day = vec![];
        available_behaviors(&tree, &needs, &b, 12, &mut day);
        assert_eq!(day, vec!["play"], "白天不放行夜间行为");
    }
}
