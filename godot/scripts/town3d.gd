# 《活城》3D 小镇客户端（M7 一阶段：同构美术场景竖切；二阶段：点选居民 + 声誉染色）。
# 场景资产由 town.json 经 Blender 同构生成导出（assets3d/*.glb，按区域分文件）；
# 卡通外观由 shader 按材质名携带的参数重建（flat|/fx|/stripe|，见 shaders/）。
# 旁观模式（移植自 M6 2D 客户端）：点居民看公开信息，密友门内才见内心（门在核心侧）；
# 楼群材质按区域声誉染色（红低绿高，健康带不上色）。
# 指令模式（设计稿 4.2–4.3 的最小竖切）：0 号居民是"你"（金色），选中自己
# 可下指令——核心按习惯合拍度定价扣意志力，行为树从不否决指令只收代价。
# 模拟全部住在 Rust 核心（SimNode），本脚本只读快照；人类用角色模型（骨骼动画实例，
# 模型缺失时回退胶囊小人），猫用 MultiMesh 批量渲染。
extends Node3D

const MAP_W := 56.0
const MAP_H := 36.0
const AREA_FILES := [
	"Ground", "Wires", "Area_park", "Area_street", "Area_road_e", "Area_road_s",
	"Area_road_n", "Area_diner", "Area_shop", "Area_bakery", "Area_office",
	"Area_police", "Area_home",
]

var sim: SimNode
var mm: MultiMesh
var camera: Camera3D
var dragging := false
# 相机锚定：注视的地面点（y=0）+ 恒定机位偏移（旋转锁死 → 偏移不变）。
# 平移/缩放只动 focus（钳制在小镇附近），机位永远 = focus + 偏移，不会跳变。
var focus := Vector3(0.0, 0.0, 1.0)
var cam_offset := Vector3.ZERO
# 90° 步进旋转（Q/E）：只转机位偏移；光照固定在世界空间。
const BASE_OFFSET := Vector3(44.8, 70.9, 100.6)
var cam_yaw := 0
var cam_offset_tgt := Vector3.ZERO
var fx_mats: Array[ShaderMaterial] = []    # 立面材质（--detail 开窗户）
var st_mats: Array[ShaderMaterial] = []    # 标线材质（--detail 开标线）
var clock_label: Label
var ui_timer := 0.0
var screenshot_after := -1.0
var no_toon := false          # --no-toon：直接用 GLB 自带烘焙贴图（外部查看器视角）
var detail_on := false        # --detail：程序化细节（立面窗户/店面 + 路面标线；默认纯色模式）
var agent_count := 0
var species_bytes := PackedByteArray()
# 区域声誉染色：Area_<区域id> 文件 → 该区域全部卡通材质（各文件独立缓存，不跨区共享）。
var area_mats := {}            # 区域 id → Array[ShaderMaterial]
var area_rep_last := {}        # 区域 id → 上次写入的声誉（变化小于阈值不重设）
# 声誉 → 染色的映射：0.68–0.90 是"健康带"不上色——声誉静息在 0.7 上下，
# 日常波动不打扰画面；跌破下界转砖红（口碑不佳即泛粉，0.38 处到上限），
# 突破上界淡入薄荷绿（口碑极佳）。数值是表现层调参，改这里不改核心。
const REP_FLOOR := 0.68
const REP_CEIL := 0.90
const REP_RED := Color(0.82, 0.30, 0.24)
const REP_GREEN := Color(0.42, 0.78, 0.52)
const REP_RED_AMT := 0.55
const REP_GREEN_AMT := 0.35
# 点选居民（旁观模式，M6 移植）
var selected := -1             # 选中代理下标；-1 = 无
var follow := false            # 跟随镜头
var press_pos := Vector2.ZERO  # 左键按下位置（区分点击与拖拽）
var press_ms := 0
var sel_ring: MeshInstance3D   # 选中者脚下光环
var inspect_panel: PanelContainer
var inspect_title: Label
var inspect_body: RichTextLabel
var follow_btn: Button
var select_arg := -1           # --select=N：启动即选中（截图自检用）
# 指令模式（最小竖切）：0 号代理是玩家（金色小人），意志力与定价全在核心侧。
var player_idx := 0
var player_name := ""
var issue_debug := ""          # --issue=行为id：启动后自动给玩家下一条指令（自检用）
var will_label: Label
var cmd_feedback: Label
var cmd_scroll: ScrollContainer
var cmd_box: VBoxContainer
var last_menu := ""           # 上次菜单内容（没变不重建按钮）
# 小镇日志区（左下角）：居民动作 + 经济 + 治安 + 关系 + 事件（类别着色）。
var feed_text: RichTextLabel
var feed_lines: Array[String] = []
const FEED_MAX := 150         # 滚动保留行数（缓冲区外的事件不追）
const FEED_COLORS := {
	"act": "#c9d4e0", "money": "#a8e6a3", "crime": "#ff8a80", "job": "#8ec9ff",
	"town": "#d9b8ff", "env": "#bfe3ff", "rep": "#ffc37a", "goal": "#e0b0ff",
	"cmd": "#ffe08a",
}
# 视觉移动：核心里"位置由行为决定"（开始行为即就位，路上时间不建模），
# 表现层把跳变走成连续移动（速度规则见下方人物模型段）——核心语义与确定性不动。
var visual_pos := PackedVector3Array()
# 地表高度（GLB 实测：路面 0.16 / 草地 0.17 / 平台 0.15）——代理与光环
# 要放在地表之上（此前 0.02 会把小腿和环埋进地面）。
const GROUND_Y := 0.17
const RING_Y := 0.26          # 光环管底(−0.09)刚好过草面，不埋不飘
const SNAP_SQ := 2500.0   # 距离超过 50m 直接就位（离场哨兵位/初次出现）
# 人物模型：Mixamo 角色 + 动作由 Blender 合并成单个 GLB（~/Documents/blender_scenes/
# build_character.py，下面两个原生数值由它打印）。人口上限 40，逐人一个骨骼动画节点无压力。
# 核心不建模路上时间（开始行为即就位），表现层按步行速度走过去：远了加快到赶路速度，
# 腿同步加速；速度档越高走得越快，追不上的部分滑行。
const HUMAN_MODEL := "res://assets3d/characters/ch46.glb"
const HUMAN_SCALE := 1.0      # 模型身高 1.48 m（height_m）
const WALK_SPEED := 0.72      # 走路动画的原生速度，米/秒（native_speed_mps）
const WALK_FAST := 2.5        # 赶路最快到原生速度的 2.5 倍
const CATCH_UP_S := 3.0       # 按剩余距离约 3 秒走完定速，再夹在 1–2.5 倍原生速度之间
const ANIM_RATE_MAX := 4.0    # 腿最快 4 倍速（×10/×100 档再快就滑行）
const TURN_RATE := 10.0       # 转身：每秒指数趋近
const PLAYER_TINT := 0.35     # 玩家模型染金色的比例
var human_scene: PackedScene  # null = 回退胶囊小人
var char_shader: Shader
var blob_mat: ShaderMaterial
var blob_mesh: PlaneMesh
var char_nodes: Array[Node3D] = []           # 与代理同序；猫（及回退时）为 null
var char_anims: Array[AnimationPlayer] = []
var char_walking := PackedByteArray()
var char_yaw := PackedFloat32Array()
var char_mats := {}           # 源材质 → 卡通材质（全员共用）
var char_mats_gold := {}      # 玩家专用（金色染色）
var hidden_xf := Transform3D(Basis.from_scale(Vector3.ZERO), Vector3(0.0, -100.0, 0.0))
var speed_mult := 1.0         # 当前速度档（走路速度跟着倍率走）
var run_speed := -1.0         # --speed=N（自检）：启动后的速度档
var warmup_s := 0.0           # --warmup=秒（自检）：先 ×100 推进这么多现实秒，再切到 --speed
var zoom_arg := 0.0           # --zoom=N（自检）：相机正交尺寸（默认 42，越小越近）
var follow_arg := false       # --follow（自检）：配合 --select 启动即跟随

# 游戏坐标（米，y 北）→ Godot 世界（glTF 转换：blender(x,y,z) → godot(x,z,-y)）
static func g2w(x: float, y: float, h := 0.0) -> Vector3:
	return Vector3(x - MAP_W * 0.5, h, MAP_H * 0.5 - y)

func _ready() -> void:
	sim = SimNode.new()
	add_child(sim)
	var cfg_path := ProjectSettings.globalize_path("res://../assets/town.json")
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--config="):
			cfg_path = arg.substr(9)
		elif arg.begins_with("--screenshot-after="):
			screenshot_after = arg.substr(19).to_float()
		elif arg == "--no-toon":
			no_toon = true
		elif arg == "--detail":
			detail_on = true
		elif arg.begins_with("--select="):
			select_arg = arg.substr(9).to_int()
		elif arg.begins_with("--issue="):
			issue_debug = arg.substr(8)
		elif arg.begins_with("--speed="):
			run_speed = arg.substr(8).to_float()
		elif arg.begins_with("--warmup="):
			warmup_s = arg.substr(9).to_float()
		elif arg.begins_with("--zoom="):
			zoom_arg = arg.substr(7).to_float()
		elif arg == "--follow":
			follow_arg = true
	if screenshot_after > 0.0:
		get_window().size = Vector2i(1920, 1080)
	sim.start(42, cfg_path, 14, 3)
	sim.set_player(player_idx)
	player_name = sim.agent_names()[player_idx]
	agent_count = sim.agent_positions().size()
	species_bytes = sim.agent_species()
	var p0: PackedVector2Array = sim.agent_positions()
	visual_pos.resize(p0.size())
	for i in p0.size():
		visual_pos[i] = g2w(p0[i].x, p0[i].y, GROUND_Y)
	_build_environment()
	_build_town()
	_build_camera()
	if zoom_arg > 0.0:
		camera.size = clampf(zoom_arg, 10.0, 90.0)
	_build_agents()
	_build_selection_ring()
	_build_ui()
	if select_arg >= 0 and select_arg < agent_count:
		selected = select_arg
		_refresh_inspect()
		if follow_arg:
			_toggle_follow()
	print("[town3d] 启动：%d 名居民，配置 %s" % [agent_count, cfg_path])

func _build_environment() -> void:
	var sky_mat := ProceduralSkyMaterial.new()
	sky_mat.sky_top_color = Color("#2A5FBF")
	sky_mat.sky_horizon_color = Color("#CFE8F7")
	sky_mat.ground_horizon_color = Color("#CFE8F7")
	sky_mat.ground_bottom_color = Color("#454A52")
	var sky := Sky.new()
	sky.sky_material = sky_mat
	var env := Environment.new()
	env.background_mode = Environment.BG_SKY
	env.sky = sky
	env.ambient_light_source = Environment.AMBIENT_SOURCE_SKY
	env.ambient_light_energy = 0.45 if no_toon else 0.55   # no-toon（外部查看器视角）压低曝光防过曝
	var we := WorldEnvironment.new()
	we.environment = env
	add_child(we)
	var sun := DirectionalLight3D.new()
	sun.light_color = Color("#FFF6E8")
	sun.light_energy = 1.05 if no_toon else 1.8
	sun.shadow_enabled = true
	add_child(sun)
	var from_dir := Vector3(0.30, 0.85, 0.43).normalized()
	sun.position = from_dir * 140.0
	sun.look_at(Vector3.ZERO)

func _build_town() -> void:
	var flat_shader: Shader = load("res://shaders/toon_flat.gdshader")
	var fx_shader: Shader = load("res://shaders/toon_facade.gdshader")
	var st_shader: Shader = load("res://shaders/toon_stripe.gdshader")
	var ground_cache := {}   # Ground/Wires 共用缓存（不染色，仅同名材质去重）
	for fname in AREA_FILES:
		var path := "res://assets3d/%s.glb" % fname
		if not FileAccess.file_exists(path):
			push_warning("缺少场景资产 %s" % path)
			continue
		var packed: PackedScene = load(path)
		var node := packed.instantiate()
		node.name = fname
		add_child(node)
		if no_toon:
			continue
		if fname.begins_with("Area_"):
			# 区域文件：材质独立缓存（同名不跨区共享——声誉各染各的），
			# 归集到区域 id 名下供 _apply_area_reputation 统一上色。
			var mats: Array[ShaderMaterial] = []
			_retint(node, flat_shader, fx_shader, st_shader, {}, mats)
			area_mats[fname.substr(5)] = mats
		else:
			_retint(node, flat_shader, fx_shader, st_shader, ground_cache, [])

# 材质名 → 卡通 shader 参数（名字是 Blender 侧唯一可靠的数据通道）
func _base_name(n: String) -> String:
	var i := n.rfind(".")
	var j := n.rfind("|")
	if i > j and i != -1:
		n = n.substr(0, i)
	return n

func _v3(c: Color) -> Vector3:
	return Vector3(c.r, c.g, c.b)

func _toon_mat(nm: String, fs: Shader, xs: Shader, ss: Shader) -> ShaderMaterial:
	if nm.begins_with("flat|#"):
		var sm := ShaderMaterial.new()
		sm.shader = fs
		sm.set_shader_parameter("albedo", _v3(Color(nm.get_slice("|", 1))))
		return sm
	if nm.begins_with("fx|#"):
		var p := nm.split("|")
		if p.size() < 5:
			return null
		var sm := ShaderMaterial.new()
		sm.shader = xs
		sm.set_shader_parameter("wall_color", _v3(Color(p[1])))
		sm.set_shader_parameter("win_color", _v3(Color(p[2])))
		sm.set_shader_parameter("accent_color", _v3(Color(p[3])))
		sm.set_shader_parameter("bldg_h", p[4].to_float())
		fx_mats.push_back(sm)
		return sm
	if nm.begins_with("stripe|"):
		var p := nm.split("|")
		if p.size() < 6:
			return null
		var sm := ShaderMaterial.new()
		sm.shader = ss
		sm.set_shader_parameter("axis_id", 0 if p[1] == "X" else 1)
		sm.set_shader_parameter("period", p[2].to_float())
		sm.set_shader_parameter("duty", p[3].to_float())
		sm.set_shader_parameter("on_color", _v3(Color(p[4])))
		sm.set_shader_parameter("off_color", _v3(Color(p[5])))
		st_mats.push_back(sm)
		return sm
	return null

func _retint(node: Node, fs: Shader, xs: Shader, ss: Shader, cache: Dictionary, out: Array) -> void:
	# 同名材质（参数相同）整个文件共用一份 ShaderMaterial；解析结果（含 null）
	# 进缓存，未认识的材质保留 GLB 原样。
	if node is MeshInstance3D:
		var mi := node as MeshInstance3D
		for i in mi.get_surface_override_material_count():
			var m: Material = mi.get_active_material(i)
			if m == null:
				continue
			var nm := _base_name(m.resource_name)
			if not cache.has(nm):
				var made := _toon_mat(nm, fs, xs, ss)
				cache[nm] = made
				if made != null:
					out.push_back(made)
			var sm: ShaderMaterial = cache[nm]
			if sm != null:
				mi.set_surface_override_material(i, sm)
	for c in node.get_children():
		_retint(c, fs, xs, ss, cache, out)

func _build_camera() -> void:
	# 正交 31° 斜俯视、方位角斜转 25°——与 Blender 验证机位一致（旋转锁定）。
	# 平移/缩放见 _unhandled_input：拖拽抓地平移、滚轮锚定光标缩放、WASD/方向键。
	camera = Camera3D.new()
	camera.projection = Camera3D.PROJECTION_ORTHOGONAL
	camera.size = 42.0
	add_child(camera)
	camera.position = Vector3(44.8, 70.9, 101.6)
	camera.look_at(Vector3(0.0, 4.0, 1.0))
	camera.make_current()
	focus = Vector3(0.0, 0.0, 1.0)
	cam_offset = BASE_OFFSET
	cam_offset_tgt = BASE_OFFSET

func _clamp_focus() -> void:
	# 注视点钳制在地图（56×36，中心原点）外扩 8 米内
	focus.x = clampf(focus.x, -36.0, 36.0)
	focus.z = clampf(focus.z, -26.0, 26.0)

func _apply_camera() -> void:
	camera.position = focus + cam_offset
	camera.look_at(focus + Vector3(0.0, 4.0, 0.0))

func _rotate_view(dir: int) -> void:
	# 光照固定在世界空间（不随视角转）——转到背面看到的就是真实的背光面。
	_stop_follow()
	cam_yaw += dir
	cam_offset_tgt = BASE_OFFSET.rotated(Vector3.UP, cam_yaw * (PI / 2.0))

func _ground_point(screen_px: Vector2) -> Vector3:
	# 视线与地面(y=0)交点；指向天空时返回 INF
	var o := camera.project_ray_origin(screen_px)
	var d := camera.project_ray_normal(screen_px)
	if absf(d.y) < 1e-4:
		return Vector3.INF
	return o + d * (-o.y / d.y)

func _pan(rel: Vector2) -> void:
	_stop_follow()
	# 像素位移 → 世界位移（抓地：内容跟手，相机反向/纵向移动）
	var spp := camera.size / get_viewport().get_visible_rect().size.y
	var basis := camera.global_transform.basis
	var fwd := -basis.z
	fwd.y = 0.0
	fwd = fwd.normalized()
	focus += -basis.x * rel.x * spp + fwd * rel.y * spp
	_clamp_focus()
	_apply_camera()

func _zoom_at(screen_px: Vector2, factor: float) -> void:
	var anchor := _ground_point(screen_px)
	camera.size = clampf(camera.size * factor, 10.0, 90.0)
	if anchor.is_finite():
		focus += anchor - _ground_point(screen_px)
	_clamp_focus()
	_apply_camera()

func _unhandled_input(event: InputEvent) -> void:
	if camera == null:
		return
	if event is InputEventMouseButton:
		var mb := event as InputEventMouseButton
		if mb.button_index == MOUSE_BUTTON_LEFT:
			if mb.pressed:
				dragging = true
				press_pos = mb.position
				press_ms = Time.get_ticks_msec()
			else:
				dragging = false
				# 位移小、按得短 = 点击点选（否则是拖拽平移）
				if mb.position.distance_squared_to(press_pos) < 36.0 \
						and Time.get_ticks_msec() - press_ms < 400:
					_click_select(mb.position)
		elif mb.button_index == MOUSE_BUTTON_MIDDLE:
			dragging = mb.pressed
		elif mb.pressed and mb.button_index == MOUSE_BUTTON_WHEEL_UP:
			_zoom_at(mb.position, 0.85)
		elif mb.pressed and mb.button_index == MOUSE_BUTTON_WHEEL_DOWN:
			_zoom_at(mb.position, 1.0 / 0.85)
	elif event is InputEventMouseMotion and dragging:
		_pan((event as InputEventMouseMotion).relative)
	elif event is InputEventKey and event.pressed and not event.echo:
		if event.keycode == KEY_Q:
			_rotate_view(1)
		elif event.keycode == KEY_E:
			_rotate_view(-1)
		elif event.keycode == KEY_ESCAPE:
			_deselect()

func _pick_agent(screen_px: Vector2) -> int:
	# 屏幕空间最近拾取：正交相机下投影位置即所见位置（场景无碰撞体，
	# 楼后的代理同样可选——与 2D 客户端一致的近似）。几位代理几乎重叠时
	# 取离相机最近的（看起来在前面的那位）。
	var best := -1
	var best_d := INF        # 屏幕距离²
	var best_depth := INF    # 相机距离²
	const NEAR_SQ := 144.0   # 12px 内视为重叠
	const TOL_SQ := 1024.0   # 32px 内算点中
	for i in visual_pos.size():
		var vp: Vector3 = visual_pos[i]
		if camera.is_position_behind(vp):
			continue
		# 瞄身体中段：人物模型胸口 / 胶囊小人 / 猫（0.6 倍胶囊）
		var mid := 0.75 * HUMAN_SCALE if char_nodes[i] != null else 0.55
		if i < species_bytes.size() and species_bytes[i] == 0:
			mid = 0.33
		var sp := camera.unproject_position(vp + Vector3(0, mid, 0))
		var d: float = sp.distance_squared_to(screen_px)
		if d > TOL_SQ:
			continue
		var depth: float = camera.global_position.distance_squared_to(vp)
		if d < best_d - NEAR_SQ or (d <= best_d + NEAR_SQ and depth < best_depth):
			best = i
			best_d = minf(best_d, d)
			best_depth = depth
	return best

func _click_select(screen_px: Vector2) -> void:
	var idx := _pick_agent(screen_px)
	if idx < 0:
		_deselect()
		return
	selected = idx
	_refresh_inspect()
	if idx == player_idx:
		_refresh_commands()

func _deselect() -> void:
	selected = -1
	_stop_follow()
	_set_command_ui_visible(false)
	inspect_title.text = "点击一位居民看看"
	inspect_body.clear()
	inspect_body.append_text("（旁观模式：[b]表面[/b]人人可见——在做什么、在哪里；需求与内心只有 [b]自己或密友[/b] 能看到。）")

func _stop_follow() -> void:
	follow = false
	if follow_btn != null:
		follow_btn.text = "跟随镜头：关"

func _refresh_inspect() -> void:
	if selected < 0:
		return
	var brief: PackedStringArray = sim.agent_brief(selected)
	if brief.size() >= 5:
		inspect_title.text = "%s（%s · %s）" % [brief[0], brief[1], brief[2]]
		inspect_body.clear()
		inspect_body.append_text("正在：%s\n地点：%s\n钱包：¥%.0f\n" % [brief[3], brief[4],
			sim.agent_wallet(selected)])
		for line in sim.agent_deep(selected):
			inspect_body.append_text(line + "\n")
	elif brief.size() >= 3:
		# 离场者（M6 人口退出）：名册保留可查，人已不在镇上——跟随自然结束。
		inspect_title.text = "%s（已离开小镇）" % brief[0]
		inspect_body.clear()
		inspect_body.append_text(brief[2] + "\n")
		_stop_follow()

func _toggle_follow() -> void:
	if selected < 0:
		return
	follow = not follow
	follow_btn.text = "跟随镜头：开" if follow else "跟随镜头：关"

# ---------- 指令模式（最小竖切） ----------

func _set_command_ui_visible(on: bool) -> void:
	will_label.visible = on
	cmd_feedback.visible = on
	cmd_scroll.visible = on

func _refresh_commands() -> void:
	# 指令菜单（仅玩家）：数据与定价全在核心侧（behaviors_status），
	# 这里只渲染。按钮仅在菜单内容变化时重建（重建会吃掉正在进行的点击）。
	if selected != player_idx:
		_set_command_ui_visible(false)
		return
	_set_command_ui_visible(true)
	var wp: float = sim.agent_willpower(player_idx)
	will_label.text = "意志力 %.0f / 100 · 钱包 ¥%.0f" % [wp, sim.agent_wallet(player_idx)]
	var flat: PackedStringArray = sim.behaviors_status(player_idx)
	var joined := "|".join(flat)
	if joined == last_menu:
		return
	last_menu = joined
	for c in cmd_box.get_children():
		c.queue_free()
	for i in flat.size() / 8:
		var id := flat[i * 8]
		var disp := flat[i * 8 + 1]
		var avail := flat[i * 8 + 2] == "1"
		var cost := flat[i * 8 + 3].to_float()
		var tags := ""
		if flat[i * 8 + 5] == "1":
			tags += " 自动"
		if flat[i * 8 + 6] == "1":
			tags += " 怕"
		if flat[i * 8 + 7] == "1":
			tags += " 违约"
		var btn := Button.new()
		btn.text = "%s ·%s −%.0f" % [disp, tags, cost]
		btn.disabled = not avail or cost > wp
		btn.tooltip_text = flat[i * 8 + 4]
		btn.pressed.connect(_issue.bind(id))
		cmd_box.add_child(btn)

func _select_player() -> void:
	# 顶栏"我：xxx"按钮：与点击金色小人同效（选中 + 面板 + 指令区）。
	selected = player_idx
	_refresh_inspect()
	_refresh_commands()

func _poll_town_feed() -> void:
	# 小镇日志：核心动态流（"类别\t文字"）→ 着色滚动区，只保留末尾 FEED_MAX 行。
	var feed: PackedStringArray = sim.drain_town_feed()
	if feed.size() == 0:
		return
	for line in feed:
		var tab := line.find("\t")
		var cat := line.substr(0, tab) if tab > 0 else "act"
		var body := line.substr(tab + 1) if tab > 0 else line
		feed_lines.push_back("[color=%s]%s[/color]" % [FEED_COLORS.get(cat, "#c9d4e0"), body])
	while feed_lines.size() > FEED_MAX:
		feed_lines.pop_front()
	feed_text.clear()
	feed_text.append_text("\n".join(feed_lines))

func _issue(behavior_id: String) -> void:
	var r: PackedStringArray = sim.issue_command(player_idx, behavior_id)
	if r.size() < 3:
		return
	var accepted := r[0] == "1"
	var cost := r[1].to_float()
	cmd_feedback.text = r[2]
	if accepted:
		cmd_feedback.text += "（意志力 −%.0f）" % cost
		cmd_feedback.modulate = Color(0.7, 1.0, 0.75)
	else:
		cmd_feedback.modulate = Color(1.0, 0.72, 0.68)
	_refresh_inspect()
	_refresh_commands()

func _rep_tint(rep: float) -> Array:
	# 声誉 → [染色, 强度]；健康带内 [白, 0] 不上色（见常量注释）。
	if rep < REP_FLOOR:
		return [REP_RED, clampf((REP_FLOOR - rep) / (REP_FLOOR - 0.38), 0.0, 1.0) * REP_RED_AMT]
	if rep > REP_CEIL:
		return [REP_GREEN, clampf((rep - REP_CEIL) / (1.0 - REP_CEIL), 0.0, 1.0) * REP_GREEN_AMT]
	return [Color.WHITE, 0.0]

func _apply_area_reputation() -> void:
	# 区域声誉 → 楼群材质染色；纯装饰道路（area_active = 0）跳过。
	var ids: PackedStringArray = sim.area_ids()
	var active: PackedByteArray = sim.area_active()
	var states: PackedFloat32Array = sim.area_states()
	for i in ids.size():
		if i < active.size() and active[i] == 0:
			continue
		var rep: float = states[i * 2]
		if absf(rep - float(area_rep_last.get(ids[i], -1.0))) < 0.005:
			continue
		area_rep_last[ids[i]] = rep
		var mats: Array = area_mats.get(ids[i], [])
		var tint := _rep_tint(rep)
		var col := _v3(tint[0])
		for m in mats:
			m.set_shader_parameter("rep_color", col)
			m.set_shader_parameter("rep_amt", tint[1])

func _build_selection_ring() -> void:
	# 选中者脚下光环（亮黄圆环），每帧贴视觉位置。
	var torus := TorusMesh.new()
	torus.inner_radius = 0.34
	torus.outer_radius = 0.52
	var mat := ShaderMaterial.new()
	mat.shader = load("res://shaders/toon_flat.gdshader")
	mat.set_shader_parameter("albedo", Vector3(1.0, 0.8, 0.25))
	sel_ring = MeshInstance3D.new()
	sel_ring.mesh = torus
	sel_ring.material_override = mat
	sel_ring.visible = false
	add_child(sel_ring)

func _person_mesh() -> ArrayMesh:
	# 小人 = 胶囊身体 + 球头（一个 ArrayMesh，MultiMesh 单网格纪律）。
	var am := ArrayMesh.new()
	var st := SurfaceTool.new()
	var body := CapsuleMesh.new()
	body.radius = 0.16
	body.height = 0.9
	var head := SphereMesh.new()
	head.radius = 0.13
	head.height = 0.26
	st.append_from(body, 0, Transform3D(Basis(), Vector3(0, 0.45, 0)))
	st.append_from(head, 0, Transform3D(Basis(), Vector3(0, 1.05, 0)))
	st.commit(am)
	return am

func _build_agents() -> void:
	# MultiMesh 每位代理留一个槽（同序好管）：画猫与回退小人，有模型的人类槽位隐藏。
	mm = MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.use_colors = true
	mm.mesh = _person_mesh()
	mm.instance_count = agent_count
	var mmi := MultiMeshInstance3D.new()
	mmi.multimesh = mm
	var mat := ShaderMaterial.new()
	mat.shader = load("res://shaders/toon_flat.gdshader")
	mat.set_shader_parameter("albedo", Vector3.ONE)
	mmi.material_override = mat
	add_child(mmi)
	if ResourceLoader.exists(HUMAN_MODEL):
		human_scene = load(HUMAN_MODEL)
		char_shader = load("res://shaders/toon_tex.gdshader")
		blob_mat = ShaderMaterial.new()
		blob_mat.shader = load("res://shaders/blob_shadow.gdshader")
		blob_mesh = PlaneMesh.new()
		blob_mesh.size = Vector2(0.9, 0.9)
	else:
		push_warning("缺少人物模型 %s，人类回退为胶囊小人" % HUMAN_MODEL)
	_sync_char_nodes()

func _sync_char_nodes() -> void:
	# 人物节点数组跟上代理数（新生儿只追加）；猫与回退时留 null。
	var old := char_nodes.size()
	char_nodes.resize(agent_count)
	char_anims.resize(agent_count)
	char_walking.resize(agent_count)
	char_yaw.resize(agent_count)
	if human_scene == null:
		return
	for i in range(old, agent_count):
		if i < species_bytes.size() and species_bytes[i] == 1:
			_spawn_char(i)

func _spawn_char(i: int) -> void:
	var node: Node3D = human_scene.instantiate()
	add_child(node)
	for mi: MeshInstance3D in node.find_children("*", "MeshInstance3D", true, false):
		mi.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
		if not no_toon:
			_char_retint(mi, i == player_idx)
	var blob := MeshInstance3D.new()
	blob.mesh = blob_mesh
	blob.material_override = blob_mat
	blob.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	blob.position.y = 0.015
	node.add_child(blob)
	var ap: AnimationPlayer = node.find_children("*", "AnimationPlayer", true, false)[0]
	ap.play("idle")
	char_nodes[i] = node
	char_anims[i] = ap
	char_walking[i] = 0
	char_yaw[i] = fmod(i * 2.39996, TAU)   # 黄金角错开初始朝向，开局不至于全员一个方向
	node.transform = Transform3D(Basis(Vector3.UP, char_yaw[i]).scaled(Vector3.ONE * HUMAN_SCALE), visual_pos[i])

func _char_retint(mi: MeshInstance3D, gold: bool) -> void:
	# GLB 原材质（StandardMaterial3D + 漫反射贴图）→ 贴图版卡通 shader，与楼群同一套光照；
	# 全员共用一份，玩家另一份染金色（"你"在人群里一眼可辨）。
	var cache: Dictionary = char_mats_gold if gold else char_mats
	for s in mi.get_surface_override_material_count():
		var src: Material = mi.get_active_material(s)
		if src == null:
			continue
		if not cache.has(src):
			var sm := ShaderMaterial.new()
			sm.shader = char_shader
			if src is BaseMaterial3D:
				sm.set_shader_parameter("albedo_tex", (src as BaseMaterial3D).albedo_texture)
				sm.set_shader_parameter("albedo", _v3((src as BaseMaterial3D).albedo_color))
			if gold:
				sm.set_shader_parameter("tint", Vector3(0.98, 0.80, 0.28))
				sm.set_shader_parameter("tint_amt", PLAYER_TINT)
			cache[src] = sm
		mi.set_surface_override_material(s, cache[src])

func _update_char(i: int, to: Vector3, dist: float, speed: float, delta: float) -> void:
	var node := char_nodes[i]
	var vp := visual_pos[i]
	node.visible = vp.x > -500.0          # 离场者在地图外哨兵位
	if not node.visible:
		return
	var ap := char_anims[i]
	if speed > 0.0:
		char_yaw[i] = lerp_angle(char_yaw[i], atan2(to.x, to.z), minf(delta * TURN_RATE, 1.0))
		ap.speed_scale = minf(speed / (WALK_SPEED * HUMAN_SCALE), ANIM_RATE_MAX)
		if char_walking[i] == 0:
			char_walking[i] = 1
			ap.play("walk", 0.25)
	elif char_walking[i] == 1:
		char_walking[i] = 0
		ap.speed_scale = 1.0
		ap.play("idle", 0.3)
	# 模型正面朝 +Z（Mixamo/Blender 前视图朝向经 glTF 换算），yaw = atan2(x, z) 即面向移动方向
	node.transform = Transform3D(Basis(Vector3.UP, char_yaw[i]).scaled(Vector3.ONE * HUMAN_SCALE), vp)

func _build_ui() -> void:
	var canvas := CanvasLayer.new()
	add_child(canvas)
	var panel := PanelContainer.new()
	panel.set_anchors_preset(Control.PRESET_TOP_WIDE)
	canvas.add_child(panel)
	var hbox := HBoxContainer.new()
	hbox.add_theme_constant_override("separation", 8)
	panel.add_child(hbox)
	clock_label = Label.new()
	clock_label.add_theme_font_size_override("font_size", 20)
	clock_label.text = "第1天 00:00"
	hbox.add_child(clock_label)
	for speed in [0, 1, 2, 10, 100]:
		var btn := Button.new()
		btn.text = ("⏸" if speed == 0 else "×%d" % speed)
		btn.pressed.connect(func(): _set_speed(float(speed)))
		hbox.add_child(btn)
	# 一键选中自己（与点击金色小人同效）——玩家在楼里上班点不到时也能下指令。
	var me_btn := Button.new()
	me_btn.text = "我：%s" % player_name
	me_btn.pressed.connect(_select_player)
	hbox.add_child(me_btn)
	var hint := Label.new()
	hint.add_theme_font_size_override("font_size", 14)
	hint.modulate = Color(1.0, 1.0, 1.0, 0.62)
	hint.text = "  你是：%s（金色） · 点自己下指令 · 楼色随区域声誉" % player_name
	hbox.add_child(hint)
	# 旁观检查面板（M6 移植）：右侧常驻，点选后显示公开信息 + 密友内心。
	inspect_panel = PanelContainer.new()
	inspect_panel.anchor_left = 1.0
	inspect_panel.anchor_right = 1.0
	inspect_panel.anchor_top = 0.0
	inspect_panel.anchor_bottom = 1.0
	inspect_panel.offset_left = -352.0
	inspect_panel.offset_right = -16.0
	inspect_panel.offset_top = 72.0
	inspect_panel.offset_bottom = -16.0
	var vb := VBoxContainer.new()
	inspect_title = Label.new()
	inspect_title.add_theme_font_size_override("font_size", 18)
	inspect_title.text = "点击一位居民看看"
	vb.add_child(inspect_title)
	follow_btn = Button.new()
	follow_btn.text = "跟随镜头：关"
	follow_btn.pressed.connect(_toggle_follow)
	vb.add_child(follow_btn)
	inspect_body = RichTextLabel.new()
	inspect_body.size_flags_vertical = Control.SIZE_EXPAND_FILL
	inspect_body.add_theme_font_size_override("normal_font_size", 15)
	inspect_body.append_text("（旁观模式：[b]表面[/b]人人可见——在做什么、在哪里；需求与内心只有 [b]自己或密友[/b] 能看到。）")
	vb.add_child(inspect_body)
	# 指令区（选中自己时显示）：意志力 + 行为菜单（核心定价）+ 指令反馈。
	will_label = Label.new()
	will_label.add_theme_font_size_override("font_size", 15)
	vb.add_child(will_label)
	cmd_feedback = Label.new()
	cmd_feedback.add_theme_font_size_override("font_size", 14)
	cmd_feedback.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	cmd_feedback.custom_minimum_size = Vector2(0, 42)
	vb.add_child(cmd_feedback)
	cmd_scroll = ScrollContainer.new()
	cmd_scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	cmd_scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	cmd_box = VBoxContainer.new()
	cmd_box.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	cmd_scroll.add_child(cmd_box)
	vb.add_child(cmd_scroll)
	_set_command_ui_visible(false)
	inspect_panel.add_child(vb)
	canvas.add_child(inspect_panel)
	# 小镇日志区（左下角）：滚动跟随最新；上翻即停止跟随（RichTextLabel 自带）。
	var fpanel := PanelContainer.new()
	fpanel.anchor_left = 0.0
	fpanel.anchor_top = 1.0
	fpanel.anchor_right = 0.0
	fpanel.anchor_bottom = 1.0
	fpanel.offset_left = 12.0
	fpanel.offset_top = -252.0
	fpanel.offset_right = 648.0
	fpanel.offset_bottom = -12.0
	var fv := VBoxContainer.new()
	var fhead := Label.new()
	fhead.add_theme_font_size_override("font_size", 15)
	fhead.text = "小镇日志（动作 · 经济 · 事件）"
	fv.add_child(fhead)
	feed_text = RichTextLabel.new()
	feed_text.bbcode_enabled = true
	feed_text.scroll_following = true
	feed_text.size_flags_vertical = Control.SIZE_EXPAND_FILL
	feed_text.add_theme_font_size_override("normal_font_size", 13)
	fv.add_child(feed_text)
	fpanel.add_child(fv)
	canvas.add_child(fpanel)
	if detail_on:
		for m in fx_mats:
			m.set_shader_parameter("windows_on", true)
		for m in st_mats:
			m.set_shader_parameter("stripes_on", true)
	if warmup_s > 0.0:
		_set_speed(100.0)
	elif run_speed >= 0.0:
		_set_speed(run_speed)
	else:
		_set_speed(100.0 if screenshot_after > 0.0 else 1.0)

func _set_speed(mult: float) -> void:
	speed_mult = mult
	sim.set_speed_mult(mult)

func _process(delta: float) -> void:
	# 键盘平移（WASD / 方向键）
	if camera != null:
		var rel := Vector2(
			(1.0 if Input.is_key_pressed(KEY_A) or Input.is_key_pressed(KEY_LEFT) else 0.0)
				- (1.0 if Input.is_key_pressed(KEY_D) or Input.is_key_pressed(KEY_RIGHT) else 0.0),
			(1.0 if Input.is_key_pressed(KEY_W) or Input.is_key_pressed(KEY_UP) else 0.0)
				- (1.0 if Input.is_key_pressed(KEY_S) or Input.is_key_pressed(KEY_DOWN) else 0.0))
		if rel != Vector2.ZERO:
			_pan(rel.normalized() * 420.0 * delta)
	# 90° 步进旋转动画
	if cam_offset.distance_squared_to(cam_offset_tgt) > 0.01:
		cam_offset = cam_offset.slerp(cam_offset_tgt, minf(delta * 8.0, 1.0))
		if cam_offset.distance_squared_to(cam_offset_tgt) < 0.01:
			cam_offset = cam_offset_tgt
		_apply_camera()
	if screenshot_after > 0.0:
		screenshot_after -= delta
		if screenshot_after <= 0.0:
			_capture_and_quit()
			return
	if warmup_s > 0.0:
		warmup_s -= delta
		if warmup_s <= 0.0:
			_set_speed(run_speed if run_speed >= 0.0 else 1.0)
	var positions: PackedVector2Array = sim.agent_positions()
	if positions.size() != agent_count:
		_resize_agents(positions.size())
	var v_walk := WALK_SPEED * HUMAN_SCALE
	var eff := maxf(speed_mult, 1.0)       # 暂停时没走完的照常走完
	for i in positions.size():
		var sp := species_bytes[i] if i < species_bytes.size() else 0
		var target := g2w(positions[i].x, positions[i].y, GROUND_Y)
		var vp: Vector3 = visual_pos[i]
		var to := target - vp
		var dist := to.length()
		var speed := 0.0
		if dist * dist > SNAP_SQ:
			vp = target
		elif dist > 0.0:
			speed = clampf(dist / CATCH_UP_S, v_walk, v_walk * WALK_FAST) * eff
			vp = vp.move_toward(target, speed * delta)
		visual_pos[i] = vp
		if char_nodes[i] != null:
			_update_char(i, to, dist, speed, delta)
			mm.set_instance_transform(i, hidden_xf)
			continue
		var sc := 1.0 if sp == 1 else 0.6
		mm.set_instance_transform(i, Transform3D(Basis().scaled(Vector3(sc, sc, sc)), vp))
		if i == player_idx:
			mm.set_instance_color(i, Color(0.98, 0.80, 0.28))   # 你 = 金色
		else:
			mm.set_instance_color(i, Color(0.35, 0.45, 0.65) if sp == 1 else Color(1.0, 0.7, 0.3))
	# 选中光环贴住选中者的视觉位置（相机不管怎么转都在脚下）
	if selected >= 0 and selected < visual_pos.size():
		sel_ring.visible = true
		sel_ring.position = Vector3(visual_pos[selected].x, RING_Y, visual_pos[selected].z)
	elif sel_ring != null:
		sel_ring.visible = false
	# 跟随镜头：注视点贴向选中者的地面位置（平移/旋转手动操作会结束跟随）
	if follow and selected >= 0 and selected < visual_pos.size():
		focus = focus.lerp(
			Vector3(visual_pos[selected].x, 0.0, visual_pos[selected].z), minf(delta * 3.0, 1.0))
		_clamp_focus()
		_apply_camera()
	ui_timer += delta
	if ui_timer >= 0.5:
		ui_timer = 0.0
		clock_label.text = sim.clock_text()
		_apply_area_reputation()
		_refresh_inspect()
		_refresh_commands()
		_poll_town_feed()
		if issue_debug != "" and warmup_s <= 0.0:
			# --issue=行为id（自检）：给玩家自动下一条指令（有 --warmup 时等预热结束），看反馈与意志力结算。
			_issue(issue_debug)
			issue_debug = ""

func _resize_agents(n: int) -> void:
	# 居民数变化（新生儿/离场后重进）：重建实例，新增者直接就位（不全场滑動）。
	agent_count = n
	species_bytes = sim.agent_species()
	var positions: PackedVector2Array = sim.agent_positions()
	var old_visual := visual_pos
	visual_pos = PackedVector3Array()
	visual_pos.resize(n)
	for i in n:
		var target := g2w(positions[i].x, positions[i].y, GROUND_Y)
		visual_pos[i] = old_visual[i] if i < old_visual.size() else target
	mm.instance_count = n
	_sync_char_nodes()

func _capture_and_quit() -> void:
	await RenderingServer.frame_post_draw
	var img := get_viewport().get_texture().get_image()
	var dir := ProjectSettings.globalize_path("res://../logs")
	DirAccess.make_dir_recursive_absolute(dir)
	img.save_png(dir + "/godot_town3d.png")
	print("[town3d] 截图已保存 logs/godot_town3d.png，%s" % sim.clock_text())
	get_tree().quit()

func _exit_tree() -> void:
	print("[town3d] 退出：%s" % sim.clock_text())
