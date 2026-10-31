//! 记忆与学习系统：经历按显著性写入，查询按情境相似度部分匹配。
//! M0 实现基础版（特征重叠相似度 + 前 K 聚合）；泛化检索的完整版（分桶索引 + 缓存）按技术文档安排在 M2。

use crate::time::Tick;
use std::collections::BTreeMap;

/// 记忆条目。键 = (事件类型, 事件特征标签, 时段)，值 = 对行为标签的权重乘数。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryEntry {
    pub event: String,
    pub tags: Vec<String>,
    /// 0 凌晨 / 1 上午 / 2 下午 / 3 夜晚。
    pub quarter: u8,
    /// 写入强度 = 显著性，钳制在 0..1。
    pub strength: f64,
    /// 对行为标签的乘数（>1 促进，<1 抑制）。
    pub mults: BTreeMap<String, f64>,
    pub tick: Tick,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct MemoryTable {
    pub entries: Vec<MemoryEntry>,
    /// 事件类型的习惯化程度（0..0.9）：同一事件反复发生，新颖度下降。
    habituation: BTreeMap<String, f64>,
    cap: usize,
    top_k: usize,
}

impl MemoryTable {
    pub fn new(cap: usize, top_k: usize) -> Self {
        MemoryTable {
            entries: Vec::new(),
            habituation: BTreeMap::new(),
            cap,
            top_k,
        }
    }

    /// 写入一条记忆，返回显著性（供日志报告原因）。
    ///
    /// 显著性 = 强度 × 新颖度 × 可塑性（M0 猫的可塑性恒为 1，老龄化参数留待 M5）。
    /// 相同（事件, 时段）的记忆做竞争性覆盖：保留更强者。
    pub fn write(
        &mut self,
        event: &str,
        tags: &[String],
        quarter: u8,
        intensity: f64,
        plasticity: f64,
        mults: BTreeMap<String, f64>,
        tick: Tick,
    ) -> f64 {
        let hab = self.habituation.get(event).copied().unwrap_or(0.0);
        let novelty = 1.0 - hab;
        self.habituation.insert(
            event.to_string(),
            (hab + 0.25).min(0.9),
        );
        let salience = (intensity * novelty * plasticity).clamp(0.0, 1.0);
        if salience < 0.05 {
            return salience; // 太弱的事件不写入
        }
        // 竞争性覆盖：同事件同时段的旧记忆被更强的取代，条目不无限增长。
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|e| e.event == event && e.quarter == quarter)
        {
            if salience >= existing.strength {
                existing.strength = salience;
                existing.mults = mults;
                existing.tick = tick;
                existing.tags = tags.to_vec();
            }
        } else {
            self.entries.push(MemoryEntry {
                event: event.to_string(),
                tags: tags.to_vec(),
                quarter,
                strength: salience,
                mults,
                tick,
            });
            // 淘汰最弱者
            if self.entries.len() > self.cap {
                if let Some(weakest) = self
                    .entries
                    .iter()
                    .enumerate()
                    .min_by(|a, b| a.1.strength.total_cmp(&b.1.strength))
                    .map(|(i, _)| i)
                {
                    self.entries.remove(weakest);
                }
            }
        }
        salience
    }

    /// 情境查询：给定当前时段与地点特征标签，返回对行为标签的聚合乘数。
    /// 相似度 = 0.6 × 标签重叠率 + 0.4 × 时段匹配；取相似度前 K 条，
    /// 每条的有效乘数 = 1 + (m − 1) × 强度 × 相似度（强度和相似度越高越接近声明值）。
    pub fn query(&self, quarter: u8, spot_tags: &[String]) -> BTreeMap<String, f64> {
        let mut scored: Vec<(f64, &MemoryEntry)> = self
            .entries
            .iter()
            .map(|e| {
                let overlap = if e.tags.is_empty() {
                    0.0
                } else {
                    e.tags
                        .iter()
                        .filter(|t| spot_tags.contains(t))
                        .count() as f64
                        / e.tags.len() as f64
                };
                let time_match = if e.quarter == quarter { 1.0 } else { 0.0 };
                let score = 0.6 * overlap + 0.4 * time_match;
                (score, e)
            })
            .filter(|(s, _)| *s > 0.15)
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut combined: BTreeMap<String, f64> = BTreeMap::new();
        for (score, e) in scored.into_iter().take(self.top_k) {
            let weight = (e.strength * score).clamp(0.0, 1.0);
            for (tag, m) in &e.mults {
                let eff = 1.0 + (m - 1.0) * weight;
                *combined.entry(tag.clone()).or_insert(1.0) *= eff;
            }
        }
        combined
    }

    /// 每日例行衰减：习惯化程度回落（事件恢复新颖度），
    /// 记忆强度轻微消退（×0.95/天，设计稿的"习惯/偏好缓慢消退"；
    /// 强烈记忆靠重演维持，长期不重演则失去对权重的影响力）。
    pub fn daily_decay(&mut self) {
        for v in self.habituation.values_mut() {
            *v = (*v - 0.05).max(0.0);
        }
        for e in &mut self.entries {
            e.strength *= 0.95;
        }
        self.entries.retain(|e| e.strength > 0.02);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mults(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), *v))
            .collect()
    }

    #[test]
    fn write_then_query_boosts_tag() {
        let mut mem = MemoryTable::new(100, 8);
        let tags = vec!["noise".to_string()];
        let s = mem.write(
            "vacuum",
            &tags,
            3,
            0.9,
            1.0,
            mults(&[("hide", 5.0), ("explore", 0.3)]),
            100,
        );
        assert!(s > 0.8);
        // 同时段查询：hide 提升到 1 和 5 之间（按强度×相似度打折）
        let q = mem.query(3, &["noise".to_string()]);
        let hide = *q.get("hide").unwrap();
        assert!(hide > 1.5 && hide <= 5.0, "hide = {hide}");
        assert!(*q.get("explore").unwrap() < 1.0);
        // 无关情境不受影响
        let q0 = mem.query(0, &["food".to_string()]);
        assert!(q0.get("hide").copied().unwrap_or(1.0) > 0.99
            && q0.get("hide").copied().unwrap_or(1.0) < 1.05);
    }

    #[test]
    fn repetition_reduces_salience() {
        let mut mem = MemoryTable::new(100, 8);
        let tags = vec!["noise".to_string()];
        let s1 = mem.write("vacuum", &tags, 2, 0.9, 1.0, BTreeMap::new(), 1);
        let s2 = mem.write("vacuum", &tags, 2, 0.9, 1.0, BTreeMap::new(), 2);
        assert!(s2 < s1, "重复事件显著性应下降: {s1} -> {s2}");
    }

    #[test]
    fn competitive_overwrite_keeps_strongest() {
        let mut mem = MemoryTable::new(100, 8);
        let tags = vec!["t".to_string()];
        mem.write("e", &tags, 1, 0.9, 1.0, BTreeMap::new(), 1);
        mem.write("e", &tags, 1, 0.5, 1.0, BTreeMap::new(), 2);
        assert_eq!(mem.entries.len(), 1);
    }
}
