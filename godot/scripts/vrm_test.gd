extends Node3D
# VRM 导入渲染自检（Phase 0）：实例化 avatar_test.vrm，打印结构与材质诊断，正面全身+脸部特写两张截图退出。
# 用法：godot --path . res://scenes/vrm_test.tscn -- [--model=...] 
# VRM 规范：模型面朝 -Z。正面视角的相机要放在 -Z 侧。

const DEFAULT_MODEL := "res://assets3d/characters/avatar_test.vrm"
# 光照配方照搬 godot vrm 对照项目（main.tscn，实测肤色正常）：暖色低强度环境光 +
# 微暖主光 + Linear 色调映射。问题从来不在模型与 MToon 材质——不做任何颜色空间转换。
const AMBIENT_COLOR := Color(1.0, 0.96, 0.92)
const AMBIENT_ENERGY := 0.45
const KEY_COLOR := Color(1.0, 0.95, 0.88)
const KEY_ENERGY := 0.95
const KEY_FROM_DIR := Vector3(-0.433013, 0.5, -0.75)   # 单位向量：对照项目 30° 仰角正面偏左（模型朝 -Z 故镜像 Z）
const SHOTS := [
	{"t": 2.0, "file": "godot_vrm_front.png", "pos": Vector3(0, 1.0, -2.9), "look": Vector3(0, 0.85, 0)},
	{"t": 3.5, "file": "godot_vrm_face.png", "pos": Vector3(0.12, 1.38, -1.1), "look": Vector3(0, 1.32, 0)},
]

var model_path := DEFAULT_MODEL
var shot_idx := 0
var timer := SHOTS[0]["t"]
var cam: Camera3D
var interactive := false
var screen_shot := false  # 抓屏幕实际输出（DisplayServer），对照 get_image 读回差异
# 轨道相机参数（交互模式）
var orbit_center := Vector3(-0.15, 0.85, 0)
var orbit_azimuth := -0.5
var orbit_elevation := 0.22
var orbit_dist := 2.6
var auto_spin := true
var dragging := false

func _ready() -> void:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--model="):
			model_path = arg.substr(8)
		elif arg == "--interactive":
			interactive = true
		elif arg == "--screen-shot":
			screen_shot = true

	# 天空只当背景不供光：蓝色天空环境光会把肤色染青并洗掉明暗层次——MToon 受光面
	# ≈ 底色 × 光色，而环境光以"底色 × 环境色"无差别加到包括阴影在内的所有区域，
	# 过强的冷色环境光同时造成"染色"和"洗掉明暗分界"两个问题。
	var sky_mat := ProceduralSkyMaterial.new()
	sky_mat.sky_top_color = Color(0.36, 0.55, 0.78)
	sky_mat.sky_horizon_color = Color(0.83, 0.89, 0.95)
	sky_mat.ground_horizon_color = Color(0.55, 0.57, 0.6)
	sky_mat.ground_bottom_color = Color(0.25, 0.26, 0.28)
	var sky := Sky.new()
	sky.sky_material = sky_mat
	var env := Environment.new()
	env.background_mode = Environment.BG_SKY
	env.sky = sky
	env.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	env.ambient_light_color = AMBIENT_COLOR
	env.ambient_light_energy = AMBIENT_ENERGY
	env.tonemap_mode = Environment.TONE_MAPPER_LINEAR   # 保饱和度（ACES 会把高光区去饱和）
	var we := WorldEnvironment.new()
	we.environment = env
	add_child(we)

	# 主光：与对照项目同角度，暖环境光兼任补光，不再另设第二盏灯
	var key := DirectionalLight3D.new()
	add_child(key)
	key.position = KEY_FROM_DIR * 10.0
	key.look_at(Vector3.ZERO)
	key.light_color = KEY_COLOR
	key.light_energy = KEY_ENERGY
	key.shadow_enabled = true

	cam = Camera3D.new()
	add_child(cam)
	cam.fov = 40
	_place_cam(SHOTS[0]["pos"], SHOTS[0]["look"])

	if not ResourceLoader.exists(model_path):
		push_error("[vrm_test] 找不到模型 %s" % model_path)
		get_tree().quit(1)
		return
	var inst: Node = load(model_path).instantiate()
	add_child(inst)
	_print_diag(inst)

func _place_cam(pos: Vector3, look: Vector3) -> void:
	cam.position = pos
	cam.look_at_from_position(pos, look)

func _update_orbit_cam(delta: float) -> void:
	if auto_spin and not dragging:
		orbit_azimuth += delta * 0.35
	var h := Vector3(cos(orbit_elevation) * sin(orbit_azimuth), sin(orbit_elevation), cos(orbit_elevation) * cos(orbit_azimuth))
	cam.position = orbit_center + h * orbit_dist
	cam.look_at_from_position(cam.position, orbit_center)

func _unhandled_input(event: InputEvent) -> void:
	if not interactive:
		return
	if event is InputEventMouseButton:
		if event.button_index == MOUSE_BUTTON_LEFT:
			dragging = event.pressed
			if event.pressed:
				auto_spin = false
		elif event.button_index == MOUSE_BUTTON_WHEEL_UP and event.pressed:
			orbit_dist = maxf(0.8, orbit_dist - 0.25)
		elif event.button_index == MOUSE_BUTTON_WHEEL_DOWN and event.pressed:
			orbit_dist = minf(6.0, orbit_dist + 0.25)
	elif event is InputEventMouseMotion and dragging:
		orbit_azimuth -= event.relative.x * 0.006
		orbit_elevation = clampf(orbit_elevation + event.relative.y * 0.004, -0.35, 1.2)
	elif event is InputEventKey and event.pressed and event.keycode == KEY_ESCAPE:
		get_tree().quit()

func _print_diag(root: Node) -> void:
	var meshes := root.find_children("*", "MeshInstance3D", true, false)
	var skel := root.find_children("*", "Skeleton3D", true, false)
	var anims := root.find_children("*", "AnimationPlayer", true, false)
	print("[vrm_test] MeshInstance3D × %d，AnimationPlayer × %d" % [meshes.size(), anims.size()])
	if skel.size() > 0:
		var s: Skeleton3D = skel[0]
		print("[vrm_test] 骨骼 %d 根" % s.get_bone_count())
		var head := s.find_bone("Head")
		if head >= 0:
			var hp: Vector3 = s.get_global_transform() * s.get_bone_global_pose(head).origin
			print("[vrm_test] 头骨全局位置 %s" % [hp])
	# 模型整体高度
	var box := AABB()
	var first := true
	for mi: MeshInstance3D in meshes:
		var b: AABB = mi.global_transform * mi.mesh.get_aabb()
		if first:
			box = b
			first = false
		else:
			box = box.merge(b)
	print("[vrm_test] 包围盒 中心 %s 尺寸 %s（高 %.2f m）" % [box.position + box.size * 0.5, box.size, box.size.y])
	# MToon 材质贴图绑定与平均色（对照渲染颜色查色彩空间/光照问题）
	for mi in meshes:
		for idx in mi.get_surface_override_material_count():
			var mat: Material = mi.get_active_material(idx)
			if not (mat is ShaderMaterial):
				continue
			var info := "%s[%d]" % [mi.name, idx]
			for p in mat.get_property_list():
				var pname: String = p["name"]
				if not pname.begins_with("shader_parameter/"):
					continue
				if pname.trim_prefix("shader_parameter/") != "_Color":
					continue
				var v = mat.get(pname)
				if v is Color:
					print("[vrm_test] %s _Color = %s" % [info, v])
				break  # 每表面只看 _Color，全量列出

func _tex_avg(tex: Texture2D) -> String:
	var img := tex.get_image()
	if img == null or img.is_empty():
		return "(无法读取)"
	img = img.duplicate()
	var n := 0
	var r := 0.0
	var g := 0.0
	var b := 0.0
	var step := maxi(1, (img.get_width() * img.get_height()) / 4096)
	for y in range(0, img.get_height(), step):
		for x in range(0, img.get_width(), step):
			var c := img.get_pixel(x, y)
			if c.a < 0.05:
				continue
			r += c.r8
			g += c.g8
			b += c.b8
			n += 1
	if n == 0:
		return "(全透明)"
	return "(%d,%d,%d)" % [r / n, g / n, b / n]

func _process(delta: float) -> void:
	if screen_shot:
		timer -= delta
		if timer <= 0.0:
			var simg := DisplayServer.screen_get_image(DisplayServer.get_primary_screen())
			if simg != null and not simg.is_empty():
				var dir := ProjectSettings.globalize_path("res://../logs")
				DirAccess.make_dir_recursive_absolute(dir)
				simg.save_png(dir + "/godot_vrm_screen.png")
				print("[vrm_test] 屏幕实拍已存 logs/godot_vrm_screen.png %dx%d" % [simg.get_width(), simg.get_height()])
			else:
				print("[vrm_test] DisplayServer.screen_get_image 不可用（空图像）")
			get_tree().quit()
		return
	if interactive:
		_update_orbit_cam(delta)
		return
	timer -= delta
	if timer > 0.0:
		return
	var shot: Dictionary = SHOTS[shot_idx]
	var img := get_viewport().get_texture().get_image()
	var dir := ProjectSettings.globalize_path("res://../logs")
	DirAccess.make_dir_recursive_absolute(dir)
	img.save_png(dir + "/" + shot["file"])
	print("[vrm_test] 截图 logs/%s" % shot["file"])
	shot_idx += 1
	if shot_idx >= SHOTS.size():
		get_tree().quit()
		return
	timer = SHOTS[shot_idx]["t"] - SHOTS[shot_idx - 1]["t"]
	_place_cam(SHOTS[shot_idx]["pos"], SHOTS[shot_idx]["look"])
