# 《活城》3D 小镇客户端（M7 一阶段：同构美术场景竖切）。
# 场景资产由 town.json 经 Blender 同构生成导出（assets3d/*.glb，按区域分文件）；
# 卡通外观由 shader 按材质名携带的参数重建（flat|/fx|/stripe|，见 shaders/）。
# 模拟全部住在 Rust 核心（SimNode），本脚本只读快照；代理用 MultiMesh 批量渲染。
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
var clock_label: Label
var ui_timer := 0.0
var screenshot_after := -1.0
var no_toon := false          # --no-toon：直接用 GLB 自带烘焙贴图（外部查看器视角）
var agent_count := 0
var species_bytes := PackedByteArray()
# 视觉平滑：核心里"位置由行为决定"（开始行为即就位，路上时间不建模），
# 表现层用指数趋近把跳变抹成连续滑动——核心语义与确定性不动。
var visual_pos := PackedVector3Array()
const GLIDE := 3.0        # 每秒收敛约 95%
const SNAP_SQ := 2500.0   # 距离超过 50m 直接就位（离场哨兵位/初次出现）

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
	if screenshot_after > 0.0:
		get_window().size = Vector2i(1920, 1080)
	sim.start(42, cfg_path, 14, 3)
	agent_count = sim.agent_positions().size()
	species_bytes = sim.agent_species()
	var p0: PackedVector2Array = sim.agent_positions()
	visual_pos.resize(p0.size())
	for i in p0.size():
		visual_pos[i] = g2w(p0[i].x, p0[i].y, 0.02)
	_build_environment()
	_build_town()
	_build_camera()
	_build_agents()
	_build_ui()
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
	for fname in AREA_FILES:
		var path := "res://assets3d/%s.glb" % fname
		if not FileAccess.file_exists(path):
			push_warning("缺少场景资产 %s" % path)
			continue
		var packed: PackedScene = load(path)
		var node := packed.instantiate()
		node.name = fname
		add_child(node)
		if not no_toon:
			_retint(node, flat_shader, fx_shader, st_shader)

# 材质名 → 卡通 shader 参数（名字是 Blender 侧唯一可靠的数据通道）
func _base_name(n: String) -> String:
	var i := n.rfind(".")
	var j := n.rfind("|")
	if i > j and i != -1:
		n = n.substr(0, i)
	return n

func _v3(c: Color) -> Vector3:
	return Vector3(c.r, c.g, c.b)

func _toon_mat(m: Material, fs: Shader, xs: Shader, ss: Shader) -> ShaderMaterial:
	var nm := _base_name(m.resource_name)
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
		return sm
	return null

func _retint(node: Node, fs: Shader, xs: Shader, ss: Shader) -> void:
	if node is MeshInstance3D:
		var mi := node as MeshInstance3D
		for i in mi.get_surface_override_material_count():
			var m: Material = mi.get_active_material(i)
			if m == null:
				continue
			var sm := _toon_mat(m, fs, xs, ss)
			if sm != null:
				mi.set_surface_override_material(i, sm)
	for c in node.get_children():
		_retint(c, fs, xs, ss)

func _build_camera() -> void:
	# 正交 31° 斜俯视、方位角斜转 25°——与 Blender 验证机位一致（锁旋转，平移缩放自由）。
	var cam := Camera3D.new()
	cam.projection = Camera3D.PROJECTION_ORTHOGONAL
	cam.size = 42.0
	add_child(cam)
	cam.position = Vector3(44.8, 70.9, 101.6)
	cam.look_at(Vector3(0.0, 4.0, 1.0))
	cam.make_current()

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
		btn.pressed.connect(func(): sim.set_speed_mult(float(speed)))
		hbox.add_child(btn)
	sim.set_speed_mult(100.0 if screenshot_after > 0.0 else 1.0)

func _process(delta: float) -> void:
	if screenshot_after > 0.0:
		screenshot_after -= delta
		if screenshot_after <= 0.0:
			_capture_and_quit()
			return
	var positions: PackedVector2Array = sim.agent_positions()
	if positions.size() != agent_count:
		_resize_agents(positions.size())
	for i in positions.size():
		var sp := 1 if i < species_bytes.size() else 0
		var sc := 1.0 if sp == 1 else 0.6
		var target := g2w(positions[i].x, positions[i].y, 0.02)
		var vp: Vector3 = visual_pos[i]
		if vp.distance_squared_to(target) > SNAP_SQ:
			vp = target
		else:
			vp = vp.lerp(target, minf(delta * GLIDE, 1.0))
		visual_pos[i] = vp
		var basis := Basis().scaled(Vector3(sc, sc, sc))
		mm.set_instance_transform(i, Transform3D(basis, vp))
		mm.set_instance_color(i, Color(0.35, 0.45, 0.65) if sp == 1 else Color(1.0, 0.7, 0.3))
	ui_timer += delta
	if ui_timer >= 0.5:
		ui_timer = 0.0
		clock_label.text = sim.clock_text()

func _resize_agents(n: int) -> void:
	# 居民数变化（新生儿/离场后重进）：重建实例，新增者直接就位（不全场滑動）。
	agent_count = n
	species_bytes = sim.agent_species()
	var positions: PackedVector2Array = sim.agent_positions()
	var old_visual := visual_pos
	visual_pos = PackedVector3Array()
	visual_pos.resize(n)
	for i in n:
		var target := g2w(positions[i].x, positions[i].y, 0.02)
		visual_pos[i] = old_visual[i] if i < old_visual.size() else target
	mm.instance_count = n

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
