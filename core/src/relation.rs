//! 关系表（M2）：代理之间的亲密程度，稀疏存储。
//! 技术文档 4.5：不建 N×N 矩阵——按代理各自的认识对象存邻接表。
//! 双向对称（M2 的猫群互相熟悉；M5 人类角色再考虑单向好感）。

use std::collections::BTreeMap;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Relations {
    /// 邻接表：代理 → (认识的代理 → 亲密程度 0..1)。只存有关系的对（双向各一份）。
    adj: BTreeMap<u32, BTreeMap<u32, f64>>,
    /// 每个代理当前的最大亲密值缓存（comfort 判定的 O(1) 短路）。
    max_bond: BTreeMap<u32, f64>,
    /// 安全感来源名单：与该代理亲密值达标（≥ 门槛）的少数同伴。
    /// 跨越门槛时登记——决策时的安全感检查只对名单内的猫查距离，
    /// 不再扫描全部邻居（密集世界里邻居成百上千，来源只有寥寥几个）。
    comfort_sources: BTreeMap<u32, BTreeMap<u32, f64>>,
    /// 安全感生效的亲密值门槛。
    comfort_min_bond: f64,
}

impl Default for Relations {
    fn default() -> Self {
        Relations::new(0.35)
    }
}

impl Relations {
    pub fn new(comfort_min_bond: f64) -> Self {
        Relations {
            adj: BTreeMap::new(),
            max_bond: BTreeMap::new(),
            comfort_sources: BTreeMap::new(),
            comfort_min_bond,
        }
    }

    pub fn bond(&self, a: u32, b: u32) -> f64 {
        if a == b {
            return 0.0;
        }
        self.adj
            .get(&a)
            .and_then(|m| m.get(&b))
            .copied()
            .unwrap_or(0.0)
    }

    pub fn set_bond(&mut self, a: u32, b: u32, v: f64) {
        let v = v.clamp(0.0, 1.0);
        let old = self.adj.entry(a).or_default().insert(b, v);
        self.adj.entry(b).or_default().insert(a, v);
        self.update_max(a, old, v);
        self.update_max(b, old, v); // 双向对称存储，两侧旧值相同
                                    // 安全感来源名单：跨越门槛时登记/除名（双向）。
        let (was, now) = (
            old.unwrap_or(0.0) >= self.comfort_min_bond,
            v >= self.comfort_min_bond,
        );
        if now && !was {
            self.comfort_sources.entry(a).or_default().insert(b, v);
            self.comfort_sources.entry(b).or_default().insert(a, v);
        } else if !now && was {
            if let Some(m) = self.comfort_sources.get_mut(&a) {
                m.remove(&b);
            }
            if let Some(m) = self.comfort_sources.get_mut(&b) {
                m.remove(&a);
            }
        } else if now {
            if let Some(m) = self.comfort_sources.get_mut(&a) {
                m.insert(b, v);
            }
            if let Some(m) = self.comfort_sources.get_mut(&b) {
                m.insert(a, v);
            }
        }
    }

    /// 增长亲密值，返回新值。增长不会让陌生变疏远（下限 0）。
    pub fn add_bond(&mut self, a: u32, b: u32, delta: f64) -> f64 {
        let v = (self.bond(a, b) + delta).clamp(0.0, 1.0);
        self.set_bond(a, b, v);
        v
    }

    /// 该代理的最大亲密值（没有任何关系时为 0）。
    pub fn max_bond(&self, a: u32) -> f64 {
        self.max_bond.get(&a).copied().unwrap_or(0.0)
    }

    /// 安全感来源名单（代理 → 亲密值），决策时逐个查距离。
    pub fn comfort_sources(&self, a: u32) -> impl Iterator<Item = (u32, f64)> + '_ {
        self.comfort_sources
            .get(&a)
            .map(|m| m.iter().map(|(id, b)| (*id, *b)))
            .into_iter()
            .flatten()
    }

    /// 与指定代理最亲近的若干对象（按亲密值降序），供汇总与查看器展示。
    pub fn top_for(&self, a: u32, k: usize) -> Vec<(u32, f64)> {
        let mut v: Vec<(u32, f64)> = self
            .adj
            .get(&a)
            .map(|m| m.iter().map(|(id, b)| (*id, *b)).collect())
            .unwrap_or_default();
        v.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
        v.truncate(k);
        v
    }

    /// 增量维护每代理最大亲密值（comfort 判定的 O(1) 短路）：
    /// 新值更高 → 直接抬高；被调低且旧值恰为最大值 → 全量重算（罕见路径）。
    fn update_max(&mut self, x: u32, old: Option<f64>, v: f64) {
        let cur = self.max_bond.get(&x).copied().unwrap_or(0.0);
        if v >= cur {
            self.max_bond.insert(x, v);
        } else if old == Some(cur) {
            self.refresh_max(x);
        }
    }

    fn refresh_max(&mut self, a: u32) {
        let m = self
            .adj
            .get(&a)
            .and_then(|m| {
                m.values().cloned().fold(None::<f64>, |acc, x| {
                    Some(match acc {
                        Some(v) if v >= x => v,
                        _ => x,
                    })
                })
            })
            .unwrap_or(0.0);
        self.max_bond.insert(a, m);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bond_is_symmetric_and_clamped() {
        let mut r = Relations::default();
        r.add_bond(1, 2, 0.5);
        assert_eq!(r.bond(1, 2), 0.5);
        assert_eq!(r.bond(2, 1), 0.5, "亲密值应双向对称");
        r.add_bond(1, 2, 0.9);
        assert_eq!(r.bond(1, 2), 1.0, "上限 1");
        assert_eq!(r.bond(1, 3), 0.0, "陌生为 0");
        assert_eq!(r.bond(1, 1), 0.0, "自己与自己无意义");
    }

    #[test]
    fn max_bond_short_circuit() {
        let mut r = Relations::default();
        assert_eq!(r.max_bond(7), 0.0);
        r.set_bond(7, 1, 0.4);
        r.set_bond(7, 2, 0.9);
        assert_eq!(r.max_bond(7), 0.9);
        assert_eq!(r.top_for(7, 2)[0], (2, 0.9));
    }

    /// 安全感来源名单：跨越门槛登记、跌破门槛除名。
    #[test]
    fn comfort_sources_track_threshold() {
        let mut r = Relations::new(0.35);
        assert_eq!(r.comfort_sources(1).count(), 0);
        r.add_bond(1, 2, 0.3);
        assert_eq!(r.comfort_sources(1).count(), 0, "门槛下不算来源");
        r.add_bond(1, 2, 0.1); // 0.4，跨越门槛
        let srcs: Vec<_> = r.comfort_sources(1).collect();
        assert_eq!(srcs, vec![(2, 0.4)], "跨门槛后登记");
        let back: Vec<_> = r.comfort_sources(2).collect();
        assert_eq!(back, vec![(1, 0.4)], "双向登记");
        // 跌回门槛下 → 除名。
        r.set_bond(1, 2, 0.2);
        assert_eq!(r.comfort_sources(1).count(), 0);
        assert_eq!(r.comfort_sources(2).count(), 0);
    }
}
