//! 空间系统（M2）：位置与空间哈希网格。
//! 代理位置 = 当前行为地点（行为开始时移动过去）；
//! 环境事件按发生位置与影响半径查网格找受影响者，目睹按目击者半径查网格，
//! 复杂度与总代理数无关（技术文档 4.5）。

use std::collections::BTreeMap;

/// 房间坐标（米）。
pub type Pos = (f64, f64);

pub fn dist(a: Pos, b: Pos) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// 均匀网格。格子尺寸取舒适半径量级（约 2 米），
/// 代理移动时增量维护所在格子；查询扫过半径覆盖的格子后按距离过滤。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SpatialGrid {
    cell: f64,
    /// 格子坐标 → 代理 ID（格内按编号升序）。
    cells: BTreeMap<(i32, i32), Vec<u32>>,
    /// 代理当前位置（移动簿记用）。
    positions: BTreeMap<u32, Pos>,
}

impl SpatialGrid {
    pub fn new(cell: f64) -> Self {
        SpatialGrid {
            cell: cell.max(0.5),
            cells: BTreeMap::new(),
            positions: BTreeMap::new(),
        }
    }

    fn cell_of(&self, pos: Pos) -> (i32, i32) {
        (
            (pos.0 / self.cell).floor() as i32,
            (pos.1 / self.cell).floor() as i32,
        )
    }

    /// 移动代理（新代理即插入）。格内不排序——移动是高频操作（每次行为开始），
    /// 排序留给低频的查询路径（查询结果统一排序保证确定性）。
    pub fn move_to(&mut self, id: u32, pos: Pos) {
        if let Some(old) = self.positions.get(&id).copied() {
            let old_cell = self.cell_of(old);
            if let Some(v) = self.cells.get_mut(&old_cell) {
                v.retain(|&x| x != id);
                if v.is_empty() {
                    self.cells.remove(&old_cell);
                }
            }
        }
        let new_cell = self.cell_of(pos);
        self.cells.entry(new_cell).or_default().push(id);
        self.positions.insert(id, pos);
    }

    pub fn pos(&self, id: u32) -> Option<Pos> {
        self.positions.get(&id).copied()
    }

    /// 移出代理（M6 人口退出：离场者不再占据空间——
    /// 事件半径、目睹、社交寻人都查不到他）。再 move_to 即重新插入。
    pub fn remove(&mut self, id: u32) {
        if let Some(old) = self.positions.remove(&id) {
            let cell = self.cell_of(old);
            if let Some(v) = self.cells.get_mut(&cell) {
                v.retain(|&x| x != id);
                if v.is_empty() {
                    self.cells.remove(&cell);
                }
            }
        }
    }

    /// 半径查询：距离 ≤ r 的全部代理（按编号升序，结果顺序确定）。
    pub fn query_radius(&self, center: Pos, r: f64) -> Vec<u32> {
        let r2 = r * r;
        let (cx, cy) = self.cell_of(center);
        let span = (r / self.cell).ceil() as i32;
        let mut out = Vec::new();
        for gx in cx - span..=cx + span {
            for gy in cy - span..=cy + span {
                if let Some(ids) = self.cells.get(&(gx, gy)) {
                    for &id in ids {
                        if let Some(p) = self.positions.get(&id) {
                            let dx = p.0 - center.0;
                            let dy = p.1 - center.1;
                            if dx * dx + dy * dy <= r2 {
                                out.push(id);
                            }
                        }
                    }
                }
            }
        }
        out.sort_unstable();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radius_query_filters_and_sorts() {
        let mut g = SpatialGrid::new(2.0);
        g.move_to(3, (0.0, 0.0));
        g.move_to(1, (0.5, 0.0));
        g.move_to(2, (10.0, 10.0)); // 远处
        let near = g.query_radius((0.0, 0.0), 1.0);
        assert_eq!(near, vec![1, 3], "应只含半径内的代理且按编号排序");
        assert!(g.query_radius((3.0, 3.0), 0.4).is_empty(), "附近没有代理");
    }

    #[test]
    fn move_updates_cells() {
        let mut g = SpatialGrid::new(2.0);
        g.move_to(1, (0.0, 0.0));
        g.move_to(1, (5.0, 5.0));
        assert!(g.query_radius((0.0, 0.0), 1.0).is_empty(), "旧位置应已移除");
        assert_eq!(g.query_radius((5.0, 5.0), 1.0), vec![1]);
        assert_eq!(g.pos(1), Some((5.0, 5.0)));
    }

    #[test]
    fn boundary_distance_inclusive() {
        let mut g = SpatialGrid::new(2.0);
        g.move_to(1, (2.0, 0.0));
        assert_eq!(
            g.query_radius((0.0, 0.0), 2.0),
            vec![1],
            "恰好等于半径应命中"
        );
    }
}
