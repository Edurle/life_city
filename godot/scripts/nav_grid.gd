# 轻量导航网格（town3d 表现层"走路像人"用）：楼体脚印（游戏坐标矩形）栅格化成
# 障碍，每栋楼在临街侧开一个"门口"缺口，A* 八向寻路 + 视线拉直。
# 不进 Godot 物理系统——拾取是屏幕空间投影、楼体淡出是材质参数，都用不上碰撞体；
# 纯数据结构、零场景节点，坐标系 = 游戏坐标（米，y 北，与核心/area_rects 一致）。
class_name NavGrid
extends RefCounted

const CELL := 1.0            # 栅格 1m（地图 56×36 → 56×36 格）
const STANDOFF := 0.30       # 楼体矩形外扩：别贴墙走
const DOOR_DEPTH := 1.8      # 门口缺口打进楼内的深度（m）
const SQRT2 := 1.4142135623730951

var cols := 0
var rows := 0
var walk := PackedByteArray()          # cols×rows，1=可走
var chan := PackedByteArray()          # 门口通道格：1=仅供进出该楼（A* 不许借道抄近路）
var buildings: Array[Dictionary] = []  # {rect: Rect2 游戏坐标(含外扩), door: Vector2 门口外点}

func _init(map_w: float, map_h: float, bldg_rects: Array) -> void:
	cols = maxi(1, int(ceil(map_w / CELL)))
	rows = maxi(1, int(ceil(map_h / CELL)))
	walk.resize(cols * rows)
	chan.resize(cols * rows)
	walk.fill(1)
	for r in bldg_rects:
		var rect: Rect2 = (r as Rect2).grow(STANDOFF)
		_block(rect)
		var door := _carve_door(rect)
		buildings.push_back({"rect": rect, "door": door})

func cell_at(p: Vector2) -> Vector2i:
	return Vector2i(clampi(floori(p.x / CELL), 0, cols - 1), clampi(floori(p.y / CELL), 0, rows - 1))

func cell_center(c: Vector2i) -> Vector2:
	return Vector2(c.x + 0.5, c.y + 0.5) * CELL

func walkable(c: Vector2i) -> bool:
	return c.x >= 0 and c.y >= 0 and c.x < cols and c.y < rows and walk[c.y * cols + c.x] == 1

func walkable_at(p: Vector2) -> bool:
	return walkable(cell_at(p))

func transit_at(p: Vector2) -> bool:
	# 通行判定（A*/视线用）：可走且不是门口通道格——通道只给进出该楼的直腿用，
	# 别的路不许借门洞抄近路（否则外线路径会斜穿别人家门口进出）。
	var c := cell_at(p)
	return walkable(c) and chan[c.y * cols + c.x] == 0

func containing(p: Vector2) -> int:
	# 点落在哪栋楼（含外扩，与封堵判定一致）；不在任何楼里返回 -1。
	for i in buildings.size():
		if (buildings[i]["rect"] as Rect2).has_point(p):
			return i
	return -1

func find_path(from: Vector2, to: Vector2, seed := 0) -> PackedVector2Array:
	# 路点序列（游戏坐标），末点是 to 本身。楼内出发 → 先直线到门口再 A*；目标在
	# 楼内 → A* 到该楼门口再直线进；同楼内部直达。寻路失败兜底 [to]（退化为直线，
	# 与接入前的旧行为一致，不挡人）。seed=代理下标：门口路点按它加 0.18m 横向
	# 抖动——门点是全图共享的精确坐标，不加抖动对向进出会精确撞在同一点上。
	var bi := containing(from)
	var bj := containing(to)
	if bi >= 0 and bi == bj:
		return PackedVector2Array([to])
	var jit := Vector2.from_angle(seed * 2.39996) * 0.18 if seed != 0 else Vector2.ZERO
	var out: PackedVector2Array = []
	var start := from
	if bi >= 0:
		start = buildings[bi]["door"] + jit
		out.push_back(start)
	var goal := to
	if bj >= 0:
		goal = buildings[bj]["door"] + jit
	var cells := _astar(start, goal)
	if cells.is_empty():
		return PackedVector2Array([to])
	for c in cells:
		out.push_back(c)
	if bj >= 0:
		out.push_back(to)
	_simplify(out)
	return out

func has_line(a: Vector2, b: Vector2) -> bool:
	# 采样视线：每 0.25m 一点，全部可通行才通（门口通道格不许借道）。
	var d := b - a
	var steps := maxi(1, ceili(d.length() / 0.25))
	for k in steps + 1:
		if not transit_at(a + d * (float(k) / float(steps))):
			return false
	return true

func _block(r: Rect2) -> void:
	# 格心落进矩形（含）即封。
	var x0 := maxi(0, floori(r.position.x / CELL))
	var x1 := mini(cols - 1, floori(r.end.x / CELL))
	var y0 := maxi(0, floori(r.position.y / CELL))
	var y1 := mini(rows - 1, floori(r.end.y / CELL))
	for cy in range(y0, y1 + 1):
		for cx in range(x0, x1 + 1):
			walk[cy * cols + cx] = 0

func _carve_door(r: Rect2) -> Vector2:
	# 选"门外最开阔的一侧"开门口（该侧门外沿线探 7 点，可走格最多 = 临街/临广场），
	# 缺口 = 门外格 → 楼内 DOOR_DEPTH 的格子串置可走。返回门口点（门外格中心）。
	var best_score := -1
	var best_out := r.get_center()
	var c := r.get_center()
	for dir_v in [Vector2(0, -1), Vector2(0, 1), Vector2(-1, 0), Vector2(1, 0)]:
		var dir: Vector2 = dir_v
		var across := Vector2(absf(dir.y), absf(dir.x))
		var side_c: Vector2
		if absf(dir.y) > 0.5:
			side_c = Vector2(c.x, r.position.y if dir.y < 0 else r.end.y)
		else:
			side_c = Vector2(r.position.x if dir.x < 0 else r.end.x, c.y)
		var out_p: Vector2 = side_c + dir * CELL
		var score := 0
		for t in range(-3, 4):
			if walkable_at(out_p + across * (t * CELL)):
				score += 1
		if score > best_score:
			best_score = score
			best_out = cell_center(cell_at(out_p))
	var best_in := best_out + (c - best_out).normalized() * (CELL + DOOR_DEPTH)
	var steps := maxi(1, ceili(best_out.distance_to(best_in) / 0.3))
	for k in steps + 1:
		var cc := cell_at(best_out.lerp(best_in, float(k) / float(steps)))
		walk[cc.y * cols + cc.x] = 1
		# 楼体矩形（含外扩）内的通道格标"仅进出"——A* 不借道，直腿（门口→座位）放行
		if r.has_point(cell_center(cc)):
			chan[cc.y * cols + cc.x] = 1
	return best_out

func _simplify(pts: PackedVector2Array) -> void:
	# 贪心视线拉直：能直达就跳过中间路点（阶梯 → 斜线）。原地改写 pts。
	if pts.size() < 3:
		return
	var out: PackedVector2Array = [pts[0]]
	var i := 0
	while i < pts.size() - 1:
		var j := pts.size() - 1
		while j > i + 1 and not has_line(pts[i], pts[j]):
			j -= 1
		out.push_back(pts[j])
		i = j
	pts.clear()
	pts.append_array(out)

func _near_walkable(c: Vector2i) -> Vector2i:
	# 螺旋找最近可通行格（起点/终点格被封时兜底）；找不到返回 (-1,-1)。
	if transit_at(cell_center(c)):
		return c
	for ring in range(1, 5):
		for dy in range(-ring, ring + 1):
			for dx in range(-ring, ring + 1):
				if maxi(absi(dx), absi(dy)) != ring:
					continue
				var p := Vector2i(c.x + dx, c.y + dy)
				if transit_at(cell_center(p)):
					return p
	return Vector2i(-1, -1)

func _astar(from: Vector2, to: Vector2) -> PackedVector2Array:
	# 八向 A*（禁切角：斜走需两侧直邻格均可走）。网格只有 2000 格出头，开表用
	# 线性取最小即可（不用堆，避免 lambda 捕获 Packed 数组的按值复制陷阱）。
	var s := _near_walkable(cell_at(from))
	var g := _near_walkable(cell_at(to))
	if s.x < 0 or g.x < 0:
		return PackedVector2Array()
	var n := cols * rows
	var gs := PackedFloat32Array(); gs.resize(n); gs.fill(INF)
	var fs := PackedFloat32Array(); fs.resize(n); fs.fill(INF)
	var came := PackedInt32Array(); came.resize(n); came.fill(-1)
	var done := PackedByteArray(); done.resize(n)
	var si := s.y * cols + s.x
	var gi := g.y * cols + g.x
	gs[si] = 0.0
	var open := PackedInt32Array()
	open.push_back(si)
	var found := false
	while open.size() > 0:
		var pick := 0
		for k in range(1, open.size()):
			if fs[open[k]] < fs[open[pick]]:
				pick = k
		var cur := open[pick]
		open.remove_at(pick)
		if done[cur] == 1:
			continue
		done[cur] = 1
		if cur == gi:
			found = true
			break
		var cx := cur % cols
		var cy := cur / cols
		for d in [[1, 0, 1.0], [-1, 0, 1.0], [0, 1, 1.0], [0, -1, 1.0],
				[1, 1, SQRT2], [1, -1, SQRT2], [-1, 1, SQRT2], [-1, -1, SQRT2]]:
			var dxi: int = d[0]
			var dyi: int = d[1]
			var cost: float = d[2]
			var nx := cx + dxi
			var ny := cy + dyi
			if nx < 0 or ny < 0 or nx >= cols or ny >= rows:
				continue
			var ni := ny * cols + nx
			if walk[ni] == 0 or chan[ni] == 1:
				continue
			if dxi != 0 and dyi != 0 and (walk[cy * cols + nx] == 0 or chan[cy * cols + nx] == 1
					or walk[ny * cols + cx] == 0 or chan[ny * cols + cx] == 1):
				continue
			var ng := gs[cur] + cost
			if ng < gs[ni] - 1e-6:
				gs[ni] = ng
				came[ni] = cur
				var dx := absi(nx - g.x)
				var dy := absi(ny - g.y)
				fs[ni] = ng + float(maxi(dx, dy)) + (SQRT2 - 1.0) * float(mini(dx, dy))
				open.push_back(ni)
	if not found:
		return PackedVector2Array()
	var out: PackedVector2Array = []
	var c := gi
	while c != -1:
		out.push_back(cell_center(Vector2i(c % cols, c / cols)))
		c = came[c]
	out.reverse()
	out[out.size() - 1] = to
	return out
