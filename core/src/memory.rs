//! 记忆与学习系统：经历按显著性写入，查询按情境相似度部分匹配（泛化）。
//! M2 完整版（技术文档 4.4）：
//! - 两级检索——一级按量化特征（时段、标签）做桶索引取候选，二级在候选内算精确相似度；
//! - 聚合结果按（时段、标签、安全感分档）缓存，本代理新记忆写入时失效；
//! - 间接记忆（目睹同伴的遭遇，模仿节点写入）与直接经历分开习惯化；
//! - 安全感（comfort）：信任的同伴在身边时，负面记忆的影响力被压低。

use crate::time::Tick;
use std::collections::BTreeMap;

/// 间接记忆（目睹）的事件名前缀。
pub const VICARIOUS_PREFIX: &str = "目睹:";

/// 事件的基础名（去掉目睹前缀）：同一基础事件在同一时段的记忆相互竞争，
/// 直接经历与传闻不叠加——亲历更强时覆盖传闻，反之传闻保留。
pub fn base_event(e: &str) -> &str {
    e.strip_prefix(VICARIOUS_PREFIX).unwrap_or(e)
}

/// 构造目睹版事件名（模仿节点写入用）。
pub fn vicarious_event(id: &str) -> String {
    format!("{VICARIOUS_PREFIX}{id}")
}

/// 记忆条目。键 = (事件类型, 事件特征标签, 时段)，值 = 对行为标签的权重乘数。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryEntry {
    pub event: String,
    pub tags: Vec<String>,
    /// 0 凌晨 / 1 上午 / 2 下午 / 3 夜晚。
    pub quarter: u8,
    /// 写入强度 = 显著性，钳制在 0..1。
    pub strength: f64,
    /// 情绪效价（-1..1）：负面记忆会被安全感压制，正面记忆不受影响。
    pub valence: f64,
    /// 间接记忆：目睹同伴的遭遇学来的（模仿节点），不是亲身经历。
    pub vicarious: bool,
    /// 对行为标签的乘数（>1 促进，<1 抑制）。
    pub mults: BTreeMap<String, f64>,
    pub tick: Tick,
}

/// 一级检索的桶键：记忆按（时段、每个标签）入桶，查询取各键桶的并集为候选。
/// 并集恰好覆盖所有可能过相似度门槛的条目（标签重叠或时段匹配至少其一）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
enum Bucket {
    Quarter(u8),
    Tag(String),
}

/// 缓存键：(时段, 安全感分档, 标签集合)。安全感按 1/8 分档量化。
type CacheKey = (u8, u8, String);

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct MemoryTable {
    pub entries: Vec<MemoryEntry>,
    /// 事件类型的习惯化程度（0..0.9）：同一事件反复发生，新颖度下降。
    /// 键为事件字符串——间接记忆（"目睹:x"）与直接经历分别习惯化。
    habituation: BTreeMap<String, f64>,
    cap: usize,
    top_k: usize,
    /// 一级桶索引（派生数据，随条目结构变化时重建）。
    index: BTreeMap<Bucket, Vec<u32>>,
    /// 二级聚合缓存（派生数据，写入或强度变化时清空）。
    cache: BTreeMap<CacheKey, BTreeMap<String, f64>>,
}

impl MemoryTable {
    pub fn new(cap: usize, top_k: usize) -> Self {
        MemoryTable {
            entries: Vec::new(),
            habituation: BTreeMap::new(),
            cap,
            top_k,
            index: BTreeMap::new(),
            cache: BTreeMap::new(),
        }
    }

    /// 写入一条记忆，返回显著性（供日志报告原因）。
    ///
    /// 显著性 = 强度 × 新颖度 × 可塑性（M0 猫的可塑性恒为 1，老龄化参数留待 M5）。
    /// 相同（事件, 时段）的记忆做竞争性覆盖：保留更强者。
    #[allow(clippy::too_many_arguments)]
    pub fn write(
        &mut self,
        event: &str,
        tags: &[String],
        quarter: u8,
        intensity: f64,
        plasticity: f64,
        valence: f64,
        vicarious: bool,
        mults: BTreeMap<String, f64>,
        tick: Tick,
    ) -> f64 {
        let hab = self.habituation.get(event).copied().unwrap_or(0.0);
        let novelty = 1.0 - hab;
        self.habituation
            .insert(event.to_string(), (hab + 0.25).min(0.9));
        let salience = (intensity * novelty * plasticity).clamp(0.0, 1.0);
        if salience < 0.05 {
            return salience; // 太弱的事件不写入
        }
        // 竞争性覆盖：同一基础事件同时段的旧记忆被更强的取代（条目不无限增长，
        // 直接经历与目睹传闻竞争同一槽位而不是叠加放大）。
        let base = base_event(event);
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|e| base_event(&e.event) == base && e.quarter == quarter)
        {
            if salience >= existing.strength {
                existing.strength = salience;
                existing.mults = mults;
                existing.tick = tick;
                existing.tags = tags.to_vec();
                existing.valence = valence;
                existing.vicarious = vicarious;
                existing.event = event.to_string();
            }
        } else {
            self.entries.push(MemoryEntry {
                event: event.to_string(),
                tags: tags.to_vec(),
                quarter,
                strength: salience,
                valence,
                vicarious,
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
        // 任何结构或强度变化都可能改变后续查询结果：重建索引、清空缓存。
        self.rebuild_index();
        self.cache.clear();
        salience
    }

    /// 情境查询（泛化检索）：给定当前时段与地点特征标签，返回对行为标签的聚合乘数。
    /// 相似度 = 0.6 × 标签重叠率 + 0.4 × 时段匹配；取相似度前 K 条，
    /// 每条的有效乘数 = 1 + (m − 1) × 强度 × 相似度（强度和相似度越高越接近声明值）。
    ///
    /// comfort（0..1）：安全感——负面效价的记忆条目有效强度 ×(1−comfort)，
    /// 对应"信任的同伴在身边，恐惧没那么支配行为"；正面记忆不受影响。
    pub fn query(
        &mut self,
        quarter: u8,
        spot_tags: &[String],
        comfort: f64,
    ) -> BTreeMap<String, f64> {
        let comfort = comfort.clamp(0.0, 1.0);
        let bucketed = (comfort * 8.0).floor() as u8;
        let mut tag_key: Vec<&str> = spot_tags.iter().map(|s| s.as_str()).collect();
        tag_key.sort_unstable();
        let cache_key: CacheKey = (quarter, bucketed, tag_key.join("\u{1f}"));
        if let Some(hit) = self.cache.get(&cache_key) {
            return hit.clone();
        }

        // 一级：桶索引取候选（等价于"标签重叠或时段匹配"的全量扫描）。
        let mut cand: Vec<u32> = self
            .index
            .get(&Bucket::Quarter(quarter))
            .cloned()
            .unwrap_or_default();
        for t in spot_tags {
            if let Some(ids) = self.index.get(&Bucket::Tag(t.clone())) {
                for id in ids {
                    if !cand.contains(id) {
                        cand.push(*id);
                    }
                }
            }
        }
        // 二级：候选内精确相似度。
        let mut scored: Vec<(f64, &MemoryEntry)> = cand
            .iter()
            .filter_map(|i| self.entries.get(*i as usize))
            .map(|e| {
                let overlap = if e.tags.is_empty() {
                    0.0
                } else {
                    e.tags.iter().filter(|t| spot_tags.contains(t)).count() as f64
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
            let fear_damp = if e.valence < 0.0 { 1.0 - comfort } else { 1.0 };
            let weight = (e.strength * score * fear_damp).clamp(0.0, 1.0);
            for (tag, m) in &e.mults {
                let eff = 1.0 + (m - 1.0) * weight;
                *combined.entry(tag.clone()).or_insert(1.0) *= eff;
            }
        }
        self.cache.insert(cache_key, combined.clone());
        combined
    }

    /// 每日例行衰减：习惯化程度回落（事件恢复新颖度），
    /// 记忆强度轻微消退（×0.95/天；强烈记忆靠重演维持）。
    pub fn daily_decay(&mut self) {
        for v in self.habituation.values_mut() {
            *v = (*v - 0.05).max(0.0);
        }
        for e in &mut self.entries {
            e.strength *= 0.95;
        }
        self.entries.retain(|e| e.strength > 0.02);
        self.rebuild_index();
        self.cache.clear();
    }

    /// 重建一级索引（写入、淘汰、覆盖、衰减后调用；结构变化罕见，代价可忽略）。
    fn rebuild_index(&mut self) {
        self.index.clear();
        for (i, e) in self.entries.iter().enumerate() {
            self.index
                .entry(Bucket::Quarter(e.quarter))
                .or_default()
                .push(i as u32);
            for t in &e.tags {
                self.index
                    .entry(Bucket::Tag(t.clone()))
                    .or_default()
                    .push(i as u32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mults(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    fn tags(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn write_then_query_boosts_tag() {
        let mut mem = MemoryTable::new(100, 8);
        let s = mem.write(
            "vacuum",
            &tags(&["noise"]),
            3,
            0.9,
            1.0,
            -0.9,
            false,
            mults(&[("hide", 5.0), ("explore", 0.3)]),
            100,
        );
        assert!(s > 0.8);
        // 同时段查询：hide 提升到 1 和 5 之间（按强度×相似度打折）
        let q = mem.query(3, &tags(&["noise"]), 0.0);
        let hide = *q.get("hide").unwrap();
        assert!(hide > 1.5 && hide <= 5.0, "hide = {hide}");
        assert!(*q.get("explore").unwrap() < 1.0);
        // 无关情境不受影响
        let q0 = mem.query(0, &tags(&["food"]), 0.0);
        assert!(
            q0.get("hide").copied().unwrap_or(1.0) > 0.99
                && q0.get("hide").copied().unwrap_or(1.0) < 1.05
        );
    }

    #[test]
    fn repetition_reduces_salience() {
        let mut mem = MemoryTable::new(100, 8);
        let t = tags(&["noise"]);
        let s1 = mem.write("vacuum", &t, 2, 0.9, 1.0, -0.9, false, BTreeMap::new(), 1);
        let s2 = mem.write("vacuum", &t, 2, 0.9, 1.0, -0.9, false, BTreeMap::new(), 2);
        assert!(s2 < s1, "重复事件显著性应下降: {s1} -> {s2}");
    }

    #[test]
    fn competitive_overwrite_keeps_strongest() {
        let mut mem = MemoryTable::new(100, 8);
        let t = tags(&["t"]);
        mem.write("e", &t, 1, 0.9, 1.0, -0.5, false, BTreeMap::new(), 1);
        mem.write("e", &t, 1, 0.5, 1.0, -0.5, false, BTreeMap::new(), 2);
        assert_eq!(mem.entries.len(), 1);
    }

    /// 桶索引检索与全量线性扫描结果完全一致（两级检索的正确性）。
    #[test]
    fn bucket_index_matches_linear_scan() {
        let mut mem = MemoryTable::new(500, 8);
        let tag_pool = ["noise", "food", "sun", "social", "quiet", "hide"];
        for i in 0..40u32 {
            let ev = format!("e{i}");
            let t = vec![
                tag_pool[(i % 6) as usize].to_string(),
                tag_pool[((i + 3) % 6) as usize].to_string(),
            ];
            mem.write(
                &ev,
                &t,
                (i % 4) as u8,
                0.3 + (i % 5) as f64 * 0.15,
                1.0,
                if i % 2 == 0 { -0.7 } else { 0.6 },
                i % 3 == 0,
                mults(&[("hide", 5.0), ("food", 3.0)]),
                i as u64,
            );
        }
        for quarter in 0..4u8 {
            for combo in [
                vec![],
                vec!["noise".to_string()],
                vec!["food".to_string(), "sun".to_string()],
                vec!["hide".to_string(), "quiet".to_string()],
            ] {
                for comfort in [0.0, 0.4, 0.8] {
                    let via_index = mem.query(quarter, &combo, comfort);
                    // 暴力对照：M0 的全量扫描逻辑。
                    let mut scored: Vec<(f64, &MemoryEntry)> = mem
                        .entries
                        .iter()
                        .map(|e| {
                            let overlap = if e.tags.is_empty() {
                                0.0
                            } else {
                                e.tags.iter().filter(|t| combo.contains(t)).count() as f64
                                    / e.tags.len() as f64
                            };
                            let time_match = if e.quarter == quarter { 1.0 } else { 0.0 };
                            (0.6 * overlap + 0.4 * time_match, e)
                        })
                        .filter(|(s, _)| *s > 0.15)
                        .collect();
                    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
                    let mut expect: BTreeMap<String, f64> = BTreeMap::new();
                    for (score, e) in scored.iter().take(mem.top_k) {
                        let damp = if e.valence < 0.0 { 1.0 - comfort } else { 1.0 };
                        let weight = (e.strength * score * damp).clamp(0.0, 1.0);
                        for (tag, m) in &e.mults {
                            let eff = 1.0 + (m - 1.0) * weight;
                            *expect.entry(tag.clone()).or_insert(1.0) *= eff;
                        }
                    }
                    assert_eq!(
                        via_index, expect,
                        "桶索引结果应与全量扫描一致 quarter={quarter} combo={combo:?} comfort={comfort}"
                    );
                }
            }
        }
    }

    /// 安全感只压制负面记忆，正面记忆不受影响。
    #[test]
    fn comfort_damps_negative_only() {
        let mut mem = MemoryTable::new(100, 8);
        mem.write(
            "vacuum",
            &tags(&["noise"]),
            2,
            0.9,
            1.0,
            -0.9,
            false,
            mults(&[("hide", 5.0)]),
            10,
        );
        mem.write(
            "treat",
            &tags(&["food"]),
            2,
            0.9,
            1.0,
            0.8,
            false,
            mults(&[("food", 3.0)]),
            20,
        );
        let afraid = mem.query(2, &tags(&["noise", "food"]), 0.0);
        let comforted = mem.query(2, &tags(&["noise", "food"]), 0.75);
        let hide_plain = *afraid.get("hide").unwrap();
        let hide_comfort = *comforted.get("hide").unwrap();
        assert!(
            hide_comfort < hide_plain * 0.5,
            "高安全感应显著压制恐惧记忆：{hide_plain} -> {hide_comfort}"
        );
        let food_plain = *afraid.get("food").unwrap();
        let food_comfort = *comforted.get("food").unwrap();
        assert!(
            (food_plain - food_comfort).abs() < 1e-9,
            "正面记忆不应受安全感影响：{food_plain} vs {food_comfort}"
        );
    }

    /// 直接经历与目睹传闻竞争同一槽位：不叠加，留更强者。
    #[test]
    fn direct_and_vicarious_compete_not_stack() {
        let mut mem = MemoryTable::new(100, 8);
        let t = tags(&["noise"]);
        // 先听闻（目睹，弱）。
        let hearsay = mem.write(
            "目睹:vacuum",
            &t,
            2,
            0.2,
            1.0,
            -0.9,
            true,
            mults(&[("hide", 5.0)]),
            10,
        );
        // 后亲历（直接，强）——应覆盖传闻。
        let direct = mem.write(
            "vacuum",
            &t,
            2,
            0.9,
            1.0,
            -0.9,
            false,
            mults(&[("hide", 5.0)]),
            20,
        );
        assert!(direct >= hearsay);
        assert_eq!(mem.entries.len(), 1, "同槽位只有一条");
        assert!(!mem.entries[0].vicarious, "亲历覆盖传闻");
        // 反向：已有强亲历后，弱传闻不覆盖。
        mem.write(
            "目睹:vacuum",
            &t,
            2,
            0.3,
            1.0,
            -0.9,
            true,
            mults(&[("hide", 5.0)]),
            30,
        );
        assert_eq!(mem.entries.len(), 1);
        assert!(!mem.entries[0].vicarious, "弱传闻不应覆盖强亲历");
        // 查询只有一条记忆的效果（两条叠加会得到 ≈ 4.6² > 20）。
        let q = mem.query(2, &t, 0.0);
        let hide = *q.get("hide").unwrap();
        assert!(hide < 5.0, "单条 hide×5 记忆不应超过声明值：{hide}");
    }

    /// 新记忆写入后缓存必须失效（查询结果更新）。
    #[test]
    fn cache_invalidated_on_write() {
        let mut mem = MemoryTable::new(100, 8);
        let before = mem.query(2, &tags(&["noise"]), 0.0);
        assert_eq!(before.get("hide"), None, "初始无恐惧记忆");
        mem.write(
            "vacuum",
            &tags(&["noise"]),
            2,
            0.9,
            1.0,
            -0.9,
            false,
            mults(&[("hide", 5.0)]),
            100,
        );
        let after = mem.query(2, &tags(&["noise"]), 0.0);
        assert!(
            *after.get("hide").unwrap() > 1.5,
            "写入后的查询应看到新记忆（缓存已失效）"
        );
    }
}
