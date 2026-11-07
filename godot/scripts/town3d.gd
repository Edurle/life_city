# 《活城》3D 小镇客户端（M7 一阶段：同构美术场景竖切；二阶段：点选居民 + 声誉染色）。
# 场景资产由 town.json 经 Blender 同构生成导出（assets3d/*.glb，按区域分文件）；
# 卡通外观由 shader 按材质名携带的参数重建（flat|/fx|/stripe|，见 shaders/）。
# 旁观模式（移植自 M6 2D 客户端）：点居民看公开信息，密友门内才见内心（门在核心侧）；
# 楼群材质按区域声誉染色（红低绿高，健康带不上色）。
# 指令模式（设计稿 4.2–4.3 的最小竖切）：0 号居民是"你"（金色），选中自己
# 可下指令——核心按习惯合拍度定价扣意志力，行为树从不否决指令只收代价。
# 模拟全部住在 Rust 核心（SimNode），本脚本只读快照；人类用角色模型（骨骼动画实例，
# 模型缺失时回退胶囊小人），猫用 MultiMesh 批量渲染。移动 = 导航网格寻路绕楼走门口
# + 行人间分离避让（nav_grid.gd；核心"位置由行为决定"语义不变，路上时间不建模）。
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
# 楼体淡出（进楼可见性）：楼里有人 → 该楼立面真·半透明（alpha 混合）到 FADE_OCC，
# 人走空复原。淡出桶 = 有 fx 立面材质的区域（GLB 材质名数据驱动，park/street 这类
# 无楼区域不参与）；桶粒度今天是一区域一楼群，多房间/多楼层后桶键细化到房间/楼层，
# 机制不变（判定端 _occupied_fade_keys 是预留的替换点）。
const FADE_OCC := 0.30        # 楼内有人时的目标不透明度（30% 墙色透出楼内）
const FADE_RATE := 2.5        # 渐变速度（不透明度/秒），进出楼约 0.3 秒
var fade_buckets := {}        # 区域 id → {mats: fx 材质组, meshes: 立面网格组, ghost: bool}
var fade_rects := {}          # 区域 id → Rect2（游戏坐标；占用判定用）
var fade_cur := {}            # 区域 id → 当前不透明度（1 = 实心）
# 走路像人（表现层）：导航网格 A* 寻路（绕楼、走门口，不穿墙）+ 行人间两两分离
# （不叠人）。核心语义不动（位置由行为决定、路上时间不建模），这里只把"直线插值
# 走过去"换成"沿路点绕行"——真·路上时间要动核心，另行立项。
var nav: NavGrid
var nav_path: Array[PackedVector2Array] = []  # 每代理当前路点序列（游戏坐标）
var nav_wp := PackedInt32Array()     # 每代理当前路点下标
var nav_tgt := PackedVector2Array()  # 每代理规划终点（核心位置变了才重规划）
var nav_arrived := PackedByteArray() # 到位闩：站定后分离推挤不回弹
var nav_off := PackedVector2Array()  # 同点多站的占位偏移（到站错开站，别叠人）
var nav_b0 := PackedInt32Array()     # 规划时出发楼/目标楼下标（穿墙统计豁免用）
var nav_b1 := PackedInt32Array()
const WP_R := 0.45                  # 路点切换半径（m）
const ARRIVE_R := 0.60              # 到位死区：进它就算到了（分离推挤不回弹）
const NAV_R_HUMAN := 0.28           # 避让半径（人）
const NAV_R_CAT := 0.18             # 避让半径（猫）
var nav_plans := 0                  # 自检统计：寻路次数
var nav_violations := 0             # 自检统计：走进非豁免楼体的采样次数
var nav_min_dist := INF             # 自检统计：可见代理最小间距（m）
var nav_debug := false              # --nav-debug：障碍/门口/路径调试图层 + 强制全楼淡出
var nav_stuck := {}                 # 自检/让行：持续贴近的代理对 → 连续帧数
var nav_force := {}                 # 贴近超时后强制让行："i,j" → 让行的下标（分不开才升级）
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
const HUMAN_DIR := "res://assets3d/characters"
const HUMAN_FALLBACK := "res://assets3d/characters/ch46.glb"  # 无 VRM 时退回（正面朝 +Z）
const HUMAN_SCALE := 1.0      # ch46 身高 1.48 m；VRM 自带真实身高（1.44–2.00 m）不缩放
const WALK_SPEED := 0.72      # 走路动画的原生速度，米/秒（native_speed_mps）
const WALK_FAST := 2.5        # 赶路最快到原生速度的 2.5 倍
const CATCH_UP_S := 3.0       # 按剩余距离约 3 秒走完定速，再夹在 1–2.5 倍原生速度之间
const ANIM_RATE_MAX := 4.0    # 腿最快 4 倍速（×10/×100 档再快就滑行）
const TURN_RATE := 10.0       # 转身：每秒指数趋近
const PLAYER_TINT := 0.35     # 玩家模型染金色的比例
const Retarget := preload("res://scripts/vrma_retarget.gd")
const VRMA_WALK := "res://assets3d/characters/anims/Walking.vrma"         # VRM 无走/站动画，
const VRMA_IDLE := "res://assets3d/characters/anims/Standing Idle.vrma"   # 运行时重定向补上
var human_scenes: Array[PackedScene] = []  # 居民外观池（characters/*.vrm 按文件名排序）
var human_is_vrm := PackedByteArray()      # 与外观池同序：是否 VRM（决定要不要补动画）
var human_face_off := PackedFloat32Array() # 与外观池同序：模型正面偏航（VRM 朝 -Z → π）
var char_model := PackedByteArray()        # 每位人类代理的外观下标（代理序取模，稳定）
var char_shader: Shader
var blob_mat: ShaderMaterial
var blob_mesh: PlaneMesh
var char_nodes: Array[Node3D] = []           # 与代理同序；猫（及回退时）为 null
var char_anims: Array[AnimationPlayer] = []
var char_walk_native := PackedFloat32Array() # 与代理同序：该模型 walk 动画原生速度
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
		elif arg == "--nav-debug":
			nav_debug = true
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
	_init_fade_rects()
	_resize_nav(p0.size(), p0)
	for i in p0.size():                # 开局同 spot 的按占位偏移错开站（首决策前不叠人）
		if nav_off[i] != Vector2.ZERO:
			visual_pos[i] = g2w(p0[i].x + nav_off[i].x, p0[i].y + nav_off[i].y, GROUND_Y)
	_build_nav_debug()
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
	# 光照与 vrm_test 同配方（暖色低强度环境光 + 微暖主光，Linear 缺省）：楼群 toon shader
	# 是 unshaded 手动光照、不读场景光，外观不受影响；这组光服务 VRM 居民——MToon 对环境
	# 光敏感，蓝天环境光会把肤色染青洗白（vrm_test 双向对照实验证实）。天空仍只作背景。
	env.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	env.ambient_light_color = Color(1.0, 0.96, 0.92)
	env.ambient_light_energy = 0.45
	var we := WorldEnvironment.new()
	we.environment = env
	add_child(we)
	var sun := DirectionalLight3D.new()
	sun.light_color = Color(1.0, 0.95, 0.88)
	sun.light_energy = 0.95
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
	var bldg_rects: Array = []   # 楼体脚印（与 toon 无关，no-toon 模式也要寻路）
	for fname in AREA_FILES:
		var path := "res://assets3d/%s.glb" % fname
		if not FileAccess.file_exists(path):
			push_warning("缺少场景资产 %s" % path)
			continue
		var packed: PackedScene = load(path)
		var node := packed.instantiate()
		node.name = fname
		add_child(node)
		if fname.begins_with("Area_"):
			_collect_building_rects(node, bldg_rects)
		if no_toon:
			continue
		if fname.begins_with("Area_"):
			# 区域文件：材质独立缓存（同名不跨区共享——声誉各染各的），
			# 归集到区域 id 名下供 _apply_area_reputation 统一上色；
			# 有 fx 立面的区域建淡出桶（楼内有人时整桶半透明，见 _update_fades）。
			var aid: String = fname.substr(5)
			var mats: Array[ShaderMaterial] = []
			var fx: Array[ShaderMaterial] = []
			var fxm: Array[MeshInstance3D] = []
			_retint(node, flat_shader, fx_shader, st_shader, {}, mats, fx, fxm)
			area_mats[aid] = mats
			if fx.size() > 0:
				fade_buckets[aid] = {"mats": fx, "meshes": fxm, "ghost": false}
		else:
			_retint(node, flat_shader, fx_shader, st_shader, ground_cache, [])
	# 导航网格：障碍 = 楼体脚印，门口缺口每楼自动开（见 nav_grid.gd）
	nav = NavGrid.new(MAP_W, MAP_H, bldg_rects)

func _collect_building_rects(node: Node, out: Array) -> void:
	# 楼体脚印：带 fx 立面材质的网格的世界 AABB → 游戏坐标矩形（一网格一楼；
	# home 三栋塔 = 三格，各自开门）。
	if node is MeshInstance3D:
		var mi := node as MeshInstance3D
		for i in mi.get_surface_override_material_count():
			var m: Material = mi.get_active_material(i)
			if m != null and _base_name(m.resource_name).begins_with("fx|"):
				var aabb: AABB = mi.get_global_transform() * (mi as VisualInstance3D).get_aabb()
				out.push_back(Rect2(aabb.position.x + MAP_W * 0.5,
					MAP_H * 0.5 - aabb.end.z, aabb.size.x, aabb.size.z))
				break
	for c in node.get_children():
		_collect_building_rects(c, out)

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

func _retint(node: Node, fs: Shader, xs: Shader, ss: Shader, cache: Dictionary, out: Array,
		fx_out: Array = [], fx_meshes: Array = []) -> void:
	# 同名材质（参数相同）整个文件共用一份 ShaderMaterial；解析结果（含 null）
	# 进缓存，未认识的材质保留 GLB 原样。fx_out/fx_meshes 顺带收集立面材质与
	# 网格（淡出桶用：材质去重，网格全收——一材质可被多个网格共用）。
	if node is MeshInstance3D:
		var mi := node as MeshInstance3D
		for i in mi.get_surface_override_material_count():
			var m: Material = mi.get_active_material(i)
			if m == null:
				continue
			var nm := _base_name(m.resource_name)
			var is_fx := nm.begins_with("fx|")
			if not cache.has(nm):
				var made := _toon_mat(nm, fs, xs, ss)
				cache[nm] = made
				if made != null:
					out.push_back(made)
					if is_fx:
						fx_out.push_back(made)
			var sm: ShaderMaterial = cache[nm]
			if sm != null:
				mi.set_surface_override_material(i, sm)
				if is_fx:
					fx_meshes.push_back(mi)
	for c in node.get_children():
		_retint(c, fs, xs, ss, cache, out, fx_out, fx_meshes)

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

func _init_fade_rects() -> void:
	# 淡出桶区域的矩形（游戏坐标）：占用判定用；区域集固定，启动读一次。
	var ids: PackedStringArray = sim.area_ids()
	var rects: PackedFloat32Array = sim.area_rects()
	for i in ids.size():
		if fade_buckets.has(ids[i]) and i * 4 + 3 < rects.size():
			fade_rects[ids[i]] = Rect2(rects[i * 4], rects[i * 4 + 1], rects[i * 4 + 2], rects[i * 4 + 3])

func _occupied_fade_keys(positions: PackedVector2Array) -> Dictionary:
	# 进楼判定（临时近似，预留的整体替换点）：代理核心位置落进有楼区域的矩形
	# 即视为在楼内。今天的临时建筑一区域一楼群，矩形够用；多房间/多楼层后
	# 换成 spot→房间/楼层 元数据判定（等核心"当前行为"批量接口），只改这里，
	# 淡出机制与桶结构不动（桶键届时同步细化）。
	var occ := {}
	for aid in fade_rects:
		var r: Rect2 = fade_rects[aid]
		for p in positions:
			if r.has_point(p):
				occ[aid] = true
				break
	return occ

func _update_fades(positions: PackedVector2Array, delta: float) -> void:
	# 楼体淡出渐变：占用桶 → FADE_OCC，空桶复原；只在值变化时写材质。
	if fade_buckets.is_empty():
		return
	var occ := _occupied_fade_keys(positions)
	if nav_debug:
		# 调试模式：全楼强制淡出——障碍格/门口/路径透过楼体可见
		occ = {}
		for aid in fade_buckets:
			occ[aid] = true
	for aid in fade_buckets:
		var b: Dictionary = fade_buckets[aid]
		var tgt := FADE_OCC if occ.has(aid) else 1.0
		var cur: float = fade_cur.get(aid, 1.0)
		if absf(cur - tgt) < 0.002:
			continue
		cur = move_toward(cur, tgt, FADE_RATE * delta)
		fade_cur[aid] = cur
		for m in b["mats"]:
			m.set_shader_parameter("fade", cur)
		# 半透明楼不投影（半透明楼配实心影子很假）
		var ghost := cur < 0.98
		if ghost != b["ghost"]:
			b["ghost"] = ghost
			var sh := GeometryInstance3D.SHADOW_CASTING_SETTING_OFF if ghost \
					else GeometryInstance3D.SHADOW_CASTING_SETTING_ON
			for mi in b["meshes"]:
				mi.cast_shadow = sh

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
	# 外观池：characters/ 下的 VRM 角色按文件名排序入池；一个都没有退 ch46 GLB；
	# 再没有就胶囊小人。VRM 是 MToon 材质（保持原样不重着色），ch46 才换 toon_tex。
	if DirAccess.dir_exists_absolute(HUMAN_DIR):
		var files := PackedStringArray()
		for f in DirAccess.get_files_at(HUMAN_DIR):
			if f.ends_with(".vrm"):
				files.append(f)
		files.sort()
		for f in files:
			var p := "%s/%s" % [HUMAN_DIR, f]
			if ResourceLoader.exists(p):
				human_scenes.append(load(p))
				human_is_vrm.append(1)
				human_face_off.append(PI)          # VRM 正面朝 -Z
	if human_scenes.is_empty() and ResourceLoader.exists(HUMAN_FALLBACK):
		human_scenes.append(load(HUMAN_FALLBACK))
		human_is_vrm.append(0)
		human_face_off.append(0.0)                # ch46 GLB 正面朝 +Z
	if not human_scenes.is_empty():
		char_shader = load("res://shaders/toon_tex.gdshader")
		blob_mat = ShaderMaterial.new()
		blob_mat.shader = load("res://shaders/blob_shadow.gdshader")
		blob_mesh = PlaneMesh.new()
		blob_mesh.size = Vector2(0.9, 0.9)
	else:
		push_warning("缺少人物模型（%s 下无 VRM 也无 ch46），人类回退为胶囊小人" % HUMAN_DIR)
	_sync_char_nodes()

func _sync_char_nodes() -> void:
	# 人物节点数组跟上代理数（新生儿只追加）；猫与回退时留 null。
	var old := char_nodes.size()
	char_nodes.resize(agent_count)
	char_anims.resize(agent_count)
	char_walk_native.resize(agent_count)
	char_walking.resize(agent_count)
	char_yaw.resize(agent_count)
	char_model.resize(agent_count)
	if human_scenes.is_empty():
		return
	for i in range(old, agent_count):
		if i < species_bytes.size() and species_bytes[i] == 1:
			_spawn_char(i)

func _spawn_char(i: int) -> void:
	char_model[i] = i % human_scenes.size()   # 代理序取模选外观：同一代理永远同一张脸
	var node: Node3D = human_scenes[char_model[i]].instantiate()
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
	var ap: AnimationPlayer = null            # VRM 无走/站动画 → VRMA 运行时重定向补 walk/idle
	char_walk_native[i] = WALK_SPEED          # ch46 原生速度（build_character.py 打印值）
	if human_is_vrm[char_model[i]] == 1:
		var wa := Retarget.add_to_vrm(node, VRMA_WALK, "walk", true)   # 剥水平位移：位置由表现层走
		Retarget.add_to_vrm(node, VRMA_IDLE, "idle", false)
		if wa != null:
			char_walk_native[i] = wa.get_meta("vrma_speed_mps", WALK_SPEED)  # 按髋高缩放后的原生速度
	var aps := node.find_children("*", "AnimationPlayer", true, false)
	if aps.size() > 0:
		var cand := aps[0] as AnimationPlayer
		if cand.has_animation("walk") and cand.has_animation("idle"):
			ap = cand
			ap.play("idle")
	char_nodes[i] = node
	char_anims[i] = ap
	char_walking[i] = 0
	char_yaw[i] = fmod(i * 2.39996, TAU)   # 黄金角错开初始朝向，开局不至于全员一个方向
	node.transform = Transform3D(Basis(Vector3.UP, char_yaw[i]).scaled(Vector3.ONE * HUMAN_SCALE), visual_pos[i])

func _char_retint(mi: MeshInstance3D, gold: bool) -> void:
	# ch46 的 GLB 原材质（StandardMaterial3D + 漫反射贴图）→ 贴图版卡通 shader，与楼群同一
	# 套光照；VRM（MToon ShaderMaterial）保持原材质不动——forward_plus 暖光下已验证肤色正常。
	# 玩家金色标记：GLB 走 tint 口；MToon 没有 tint，复制材质把 _Color 混向金色。
	for s in mi.get_surface_override_material_count():
		var src: Material = mi.get_active_material(s)
		if src == null:
			continue
		if src is BaseMaterial3D:
			var cache: Dictionary = char_mats_gold if gold else char_mats
			if not cache.has(src):
				var sm := ShaderMaterial.new()
				sm.shader = char_shader
				sm.set_shader_parameter("albedo_tex", (src as BaseMaterial3D).albedo_texture)
				sm.set_shader_parameter("albedo", _v3((src as BaseMaterial3D).albedo_color))
				if gold:
					sm.set_shader_parameter("tint", Vector3(0.98, 0.80, 0.28))
					sm.set_shader_parameter("tint_amt", PLAYER_TINT)
				cache[src] = sm
			mi.set_surface_override_material(s, cache[src])
		elif gold and src is ShaderMaterial:
			if not char_mats_gold.has(src):
				var sm2: ShaderMaterial = (src as ShaderMaterial).duplicate()
				var c = sm2.get_shader_parameter("_Color")
				if c is Color:
					sm2.set_shader_parameter("_Color", (c as Color).lerp(Color(0.98, 0.80, 0.28), PLAYER_TINT))
				char_mats_gold[src] = sm2
			mi.set_surface_override_material(s, char_mats_gold[src])

func _update_char(i: int, to: Vector3, dist: float, speed: float, delta: float) -> void:
	# to = 本帧世界位移向量（朝向用它；步长近零时不动朝向，防 atan2(0,0) 猛回头）。
	var node := char_nodes[i]
	var vp := visual_pos[i]
	node.visible = vp.x > -500.0          # 离场者在地图外哨兵位
	if not node.visible:
		return
	var ap := char_anims[i]
	if speed > 0.0 and to.length_squared() > 1e-8:
		# ch46 正面朝 +Z（yaw = atan2(x,z) 即面向移动方向）；VRM 正面朝 -Z，按外观偏移 π 校正
		char_yaw[i] = lerp_angle(char_yaw[i], atan2(to.x, to.z) + human_face_off[char_model[i]], minf(delta * TURN_RATE, 1.0))
		if ap != null:
			ap.speed_scale = minf(speed / (char_walk_native[i] * HUMAN_SCALE), ANIM_RATE_MAX)
			if char_walking[i] == 0:
				char_walking[i] = 1
				ap.play("walk", 0.25)
	elif char_walking[i] == 1:
		char_walking[i] = 0
		if ap != null:
			ap.speed_scale = 1.0
			ap.play("idle", 0.3)
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
	var n := positions.size()
	# —— 阶段 1：沿各自路径算"期望新位置"（游戏坐标）——
	var want := PackedVector2Array(); want.resize(n)
	var from_pos := PackedVector2Array(); from_pos.resize(n)
	var speeds := PackedFloat32Array(); speeds.resize(n)
	var held := PackedByteArray(); held.resize(n)
	for i in n:
		var tgt := positions[i]
		var goal := tgt + nav_off[i]       # 占位偏移后的实际站位（同点多站错开）
		var vp := Vector2(visual_pos[i].x + MAP_W * 0.5, MAP_H * 0.5 - visual_pos[i].z)
		from_pos[i] = vp
		var straight := vp.distance_to(goal)
		var np := vp
		var spd := 0.0
		var fresh := nav_tgt[i] != tgt     # 核心位置变了（新行为/新去处）
		if fresh:
			nav_arrived[i] = 0
			nav_off[i] = _spot_offset(i, tgt)
			goal = tgt + nav_off[i]
			straight = vp.distance_to(goal)
		if straight * straight > SNAP_SQ:
			np = tgt                       # 离场哨兵位/超长跳变：直接就位，不寻路
			nav_path[i] = PackedVector2Array()
			nav_wp[i] = 0
			nav_tgt[i] = tgt
			nav_arrived[i] = 1
			nav_off[i] = Vector2.ZERO
			nav_b0[i] = -1
			nav_b1[i] = -1
		elif nav_arrived[i] == 1:
			pass                           # 站定中：只受阶段 2 的分离推挤
		elif straight <= ARRIVE_R:
			nav_arrived[i] = 1             # 到位：闩上，推挤不回弹
		else:
			if fresh:
				nav_tgt[i] = tgt
				nav_plans += 1
				if nav != null:
					nav_b0[i] = nav.containing(vp)
					nav_b1[i] = nav.containing(goal)
					nav_path[i] = nav.find_path(vp, goal, i)
				else:
					nav_path[i] = PackedVector2Array([goal])
					nav_b0[i] = -1
					nav_b1[i] = -1
				nav_wp[i] = 0
			var path := nav_path[i]
			var wp := goal if path.is_empty() else path[nav_wp[i]]
			if path.size() > 0 and nav_wp[i] < path.size() - 1 and vp.distance_to(wp) < WP_R:
				nav_wp[i] += 1
				wp = path[nav_wp[i]]
			var remaining := straight if path.is_empty() else vp.distance_to(wp)
			var k := nav_wp[i] + 1
			while k < path.size():
				remaining += path[k - 1].distance_to(path[k])
				k += 1
			spd = clampf(remaining / CATCH_UP_S, v_walk, v_walk * WALK_FAST) * eff
			var dirv := wp - vp
			np = vp + dirv / maxf(dirv.length(), 0.001) * minf(spd * delta, dirv.length())
		want[i] = np
		speeds[i] = spd
	# —— 阶段 2：两两分离（不叠人；走动者让站定者；推不动的——门口窄道对向
	# 会车——就地等待让行，小下标先行防死锁）——
	for it in 2:
		for i in n:
			if want[i].x < -300.0:
				continue
			for j in range(i + 1, n):
				if want[j].x < -300.0:
					continue
				var ri := NAV_R_CAT if i < species_bytes.size() and species_bytes[i] == 0 else NAV_R_HUMAN
				var rj := NAV_R_CAT if j < species_bytes.size() and species_bytes[j] == 0 else NAV_R_HUMAN
				var min_d := ri + rj
				var d := want[i].distance_to(want[j])
				if d >= min_d:
					continue
				var idle_i := speeds[i] <= 0.001
				var idle_j := speeds[j] <= 0.001
				# 强制让行（贴近 >0.5s 才升级到这）：高位者退路边当站定者，低位者
				# 照常从旁绕行（推挤/排队）；分开 1m 或超时 3s 解除。
				var fkey := "%d,%d" % [i, j]
				if nav_force.has(fkey):
					var f: Dictionary = nav_force[fkey]
					f["n"] = int(f["n"]) + 1
					if d >= 1.0 or idle_i or idle_j or int(f["n"]) > 180:
						nav_force.erase(fkey)
					elif int(f["y"]) == j:
						held[j] = 1
						_yield_away(j, want, i, from_pos)
						idle_j = true
					else:
						held[i] = 1
						_yield_away(i, want, j, from_pos)
						idle_i = true
				if d < 0.0001:
					# 完全重合（出生同 spot）：除法没方向，按下标角度确定性挤开
					var nudge := Vector2.from_angle(i * 2.39996) * min_d * 0.5
					_sep_push(i, want, nudge, idle_i)
					_sep_push(j, want, -nudge, idle_j)
					continue
				var push := (want[i] - want[j]) / d * (min_d - d)
				var wi := 0.5 if not idle_i else (0.0 if not idle_j else 0.5)
				var wj := 0.5 if not idle_j else (0.0 if not idle_i else 0.5)
				var ok_i := wi <= 0.001 or _sep_push(i, want, push * wi, idle_i)
				var ok_j := wj <= 0.001 or _sep_push(j, want, -push * wj, idle_j)
				if not ok_i and not idle_i and not ok_j and not idle_j:
					# 双方都推不进（门口窄道对向）：低位者继续走，高位者退让到路边
					if j > i:
						held[j] = 1
						_yield_away(j, want, i, from_pos)
					else:
						held[i] = 1
						_yield_away(i, want, j, from_pos)
				elif not ok_i and not idle_i:
					held[i] = 1
					want[i] = from_pos[i]
				elif not ok_j and not idle_j:
					held[j] = 1
					want[j] = from_pos[j]
	# 让行的人站住不播走路动画
	for i in n:
		if held[i] == 1:
			speeds[i] = 0.0
	# 间距统计放分离之后（反映最终画面）；×10/×100 档步长几米/帧是瞬移追赶，
	# 中途穿插不代表画面，只在常速档统计。同一对贴近持续 >1.5s 才算真死锁——
	# 门口错身一人等一人过（1-2 秒）是正常行人行为。
	if eff <= 2.0:
		var close_now := {}
		for i in n:
			if want[i].x < -300.0:
				continue
			for j in range(i + 1, n):
				if want[j].x < -300.0:
					continue
				var d := want[i].distance_to(want[j])
				if d > 0.0 and d < nav_min_dist:
					nav_min_dist = d
				var fkey_r := "%d,%d" % [i, j]
				if nav_force.has(fkey_r) and d >= 1.0:
					nav_force.erase(fkey_r)   # 分开够了才解除强制让行
				if d < 0.25:
					var key := "%d,%d" % [i, j]
					close_now[key] = true
					nav_stuck[key] = int(nav_stuck.get(key, 0)) + 1
					if nav_stuck[key] == 90:
						print("[nav] 警告：代理 %s 贴近 %.2fm 持续 1.5s+ @%.1f,%.1f" % [
							key, d, want[i].x, want[i].y])
					elif nav_stuck[key] == 30 and not nav_force.has(key):
						# 互相推得动但分不开（门口对向弹回）：高位者强制让行
						nav_force[key] = {"y": (j if j > i else i), "n": 0}
		for key in nav_stuck.keys():
			if not close_now.has(key):
				nav_stuck.erase(key)
	# —— 阶段 3：写回视觉位置（穿墙统计在写前做，位移向量喂朝向/动画）——
	for i in n:
		var np := want[i]
		var old_g := Vector2(visual_pos[i].x + MAP_W * 0.5, MAP_H * 0.5 - visual_pos[i].z)
		if speeds[i] > 0.001 and nav != null:
			var steps := maxi(1, ceili(old_g.distance_to(np) / 0.3))
			for k in range(1, steps + 1):
				var b := nav.containing(old_g.lerp(np, float(k) / float(steps)))
				if b >= 0 and b != nav_b0[i] and b != nav_b1[i]:
					nav_violations += 1
					break
		var step := g2w(np.x, np.y, GROUND_Y) - visual_pos[i]
		visual_pos[i] = g2w(np.x, np.y, GROUND_Y)
		if char_nodes[i] != null:
			_update_char(i, step, step.length(), speeds[i], delta)
			mm.set_instance_transform(i, hidden_xf)
			continue
		var sp := species_bytes[i] if i < species_bytes.size() else 0
		var sc := 1.0 if sp == 1 else 0.6
		mm.set_instance_transform(i, Transform3D(Basis().scaled(Vector3(sc, sc, sc)), visual_pos[i]))
		if i == player_idx:
			mm.set_instance_color(i, Color(0.98, 0.80, 0.28))   # 你 = 金色
		else:
			mm.set_instance_color(i, Color(0.35, 0.45, 0.65) if sp == 1 else Color(1.0, 0.7, 0.3))
	# 楼体淡出：按占用桶渐变（判定见 _occupied_fade_keys）
	_update_fades(positions, delta)
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
		_refresh_nav_debug()
		if issue_debug != "" and warmup_s <= 0.0:
			# --issue=行为id（自检）：给玩家自动下一条指令（有 --warmup 时等预热结束），看反馈与意志力结算。
			_issue(issue_debug)
			issue_debug = ""

func _yield_away(i: int, want: PackedVector2Array, other: int, from_pos: PackedVector2Array) -> void:
	# 退让到路边（held 由调用方置位）：沿远离对方方向小步试最多 0.6m，站到能
	# 通过的最远可通行点；一点都让不开就退回本帧起点原地等。
	var dir := want[i] - want[other]
	if dir.length_squared() < 0.0001:
		dir = Vector2.from_angle(i * 2.39996)
	dir = dir.normalized()
	var best := from_pos[i]
	for t in range(1, 5):
		var q := from_pos[i] + dir * (0.15 * t)
		if nav == null or nav.transit_at(q):
			best = q
		else:
			break
	want[i] = best

func _spot_offset(i: int, tgt: Vector2) -> Vector2:
	# 同点多站的占位偏移：先到的站正点，后来的绕正点错开 0.45m 站（方向按
	# 下标+占位序散开；偏进墙/出界就转 90° 再试）。多人挤同一张沙发/餐桌不再叠人。
	var k := 0
	for j in mini(nav_tgt.size(), agent_count):
		if j != i and nav_tgt[j] == tgt and nav_arrived[j] == 1:
			k += 1
	if k == 0:
		return Vector2.ZERO
	var base := i * 2.39996 + k * 1.7
	for t in 4:
		var off := Vector2.from_angle(base + t * (PI / 2.0)) * 0.45
		var q := tgt + off
		if nav == null or not nav.walkable_at(tgt) or nav.walkable_at(q) or nav.containing(q) == nav.containing(tgt):
			return off      # 室外点不进墙；室内点（本来非可走格）只求不换楼
	return Vector2.ZERO

func _sep_push(i: int, want: PackedVector2Array, p: Vector2, idle: bool) -> bool:
	# 分离位移落地：不许进障碍格，也不许被推进"非本程出发/目标楼"的外扩带
	# （门口通道是可走格、且必属本程目标楼，天然放行）。推不进返回 false，
	# 调用方让走动者就地等待。站定者在楼内座位（非可走格）时放行小位移。
	var q := want[i] + p
	if nav != null:
		var b := nav.containing(q)
		if b >= 0 and b != nav_b0[i] and b != nav_b1[i]:
			return false
	if nav == null or nav.walkable_at(q) or (idle and not nav.walkable_at(want[i])):
		want[i] = q
		return true
	return false

func _resize_nav(n: int, positions: PackedVector2Array) -> void:
	# 导航状态数组随人口伸缩（Packed resize 保旧值）；新增者（新生儿）直接站定。
	var old_n := nav_arrived.size()
	nav_path.resize(n)
	nav_wp.resize(n)
	nav_arrived.resize(n)
	nav_off.resize(n)
	nav_b0.resize(n)
	nav_b1.resize(n)
	nav_tgt.resize(n)
	for i in range(old_n, n):
		nav_tgt[i] = positions[i]
		nav_arrived[i] = 1
		nav_wp[i] = 0
		nav_off[i] = _spot_offset(i, positions[i]) if nav != null else Vector2.ZERO
		nav_b0[i] = -1
		nav_b1[i] = -1

# —— 导航调试图层（--nav-debug）：障碍格红点 / 门口绿点 / 各代理路径黄点 ——
var nav_cell_mm: MultiMesh
var nav_path_mm: MultiMesh

func _build_nav_debug() -> void:
	if not nav_debug or nav == null:
		return
	var blocked := 0
	var cells: Array[Transform3D] = []
	for cy in nav.rows:
		for cx in nav.cols:
			if nav.walk[cy * nav.cols + cx] == 0:
				blocked += 1
				var p := nav.cell_center(Vector2i(cx, cy))
				cells.push_back(Transform3D(Basis(), g2w(p.x, p.y, 0.20)))
	nav_cell_mm = _debug_dot_layer(cells, Vector2(0.62, 0.62), Vector3(0.95, 0.25, 0.2))
	var doors: Array[Transform3D] = []
	for b in nav.buildings:
		var d: Vector2 = b["door"]
		doors.push_back(Transform3D(Basis().scaled(Vector3(1.4, 1.4, 1.4)), g2w(d.x, d.y, 0.24)))
	_debug_dot_layer(doors, Vector2(0.8, 0.8), Vector3(0.3, 0.95, 0.4))
	nav_path_mm = _debug_dot_layer([], Vector2(0.3, 0.3), Vector3(1.0, 0.85, 0.2))
	print("[nav] 调试图层：楼 %d 栋，障碍格 %d" % [nav.buildings.size(), blocked])

func _debug_dot_layer(tfs: Array[Transform3D], sz: Vector2, col: Vector3) -> MultiMesh:
	var plane := PlaneMesh.new()
	plane.size = sz
	var mat := ShaderMaterial.new()
	mat.shader = load("res://shaders/toon_flat.gdshader")
	mat.set_shader_parameter("albedo", col)
	var mmi := MultiMeshInstance3D.new()
	var m := MultiMesh.new()
	m.transform_format = MultiMesh.TRANSFORM_3D
	m.mesh = plane
	m.instance_count = maxi(1, tfs.size())
	for k in tfs.size():
		m.set_instance_transform(k, tfs[k])
	mmi.multimesh = m
	mmi.material_override = mat
	mmi.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	add_child(mmi)
	return m

func _refresh_nav_debug() -> void:
	# 路径黄点：所有代理的当前路点序列（0.5s 一刷，走完的显示最后一次的路径）。
	if not nav_debug or nav_path_mm == null:
		return
	var tfs: Array[Transform3D] = []
	for i in mini(agent_count, nav_path.size()):
		for wp in nav_path[i]:
			tfs.push_back(Transform3D(Basis().scaled(Vector3(1.6, 1.6, 1.6)), g2w(wp.x, wp.y, 0.7)))
	nav_path_mm.instance_count = maxi(1, tfs.size())
	for k in tfs.size():
		nav_path_mm.set_instance_transform(k, tfs[k])

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
	_resize_nav(n, positions)
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
	if nav != null:
		print("[nav] 寻路 %d 次，穿墙违规 %d 次，最小间距 %s m" % [
			nav_plans, nav_violations,
			("%.2f" % nav_min_dist) if nav_min_dist < INF else "n/a"])
