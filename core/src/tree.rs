//! 树结构评估：给定当前需求，返回可用的行为 ID 列表。
//! 结构只负责"可用性"（需求门），概率与个性全部在数值层（见 world.rs 的决策函数）。

use crate::config::{BehaviorDef, TreeNode};
use crate::needs::Needs;
use std::collections::BTreeMap;

pub fn available_behaviors<'a>(
    node: &'a TreeNode,
    needs: &Needs,
    behaviors: &'a BTreeMap<String, BehaviorDef>,
    out: &mut Vec<&'a str>,
) {
    match node {
        TreeNode::Selector { children } => {
            for c in children {
                available_behaviors(c, needs, behaviors, out);
            }
        }
        TreeNode::Gate { need, below, child } => {
            // 门放行条件：需求当前值低于阈值。
            if let Some(v) = needs.get(need) {
                if v < *below {
                    available_behaviors(child, needs, behaviors, out);
                }
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
            serde_json::from_str(r#"{"id":"eat","base_weight":3,"duration_min":10,"duration_max":30,"spot":"bowl"}"#)
                .unwrap(),
        );
        m.insert(
            "play".into(),
            serde_json::from_str(r#"{"id":"play","base_weight":1,"duration_min":10,"duration_max":20}"#)
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
        available_behaviors(&tree, &needs, &b, &mut out);
        assert_eq!(out, vec!["play"]);

        needs.hunger = 50.0;
        let mut out = vec![];
        available_behaviors(&tree, &needs, &b, &mut out);
        assert_eq!(out, vec!["eat", "play"]);
    }
}
