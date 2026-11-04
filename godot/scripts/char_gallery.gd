extends Node3D
# 角色外貌识别棚：--model=res://... 指定模型，四视角截图（±Z 全身 + ±Z 头部特写）到
# logs/char_gallery/<文件名>_<视角>.png；文件名与视角烙在截图左上角，截图自描述。
# 光照与 vrm_test 同一套配方（暖环境光 + 微暖主光 + Linear，MToon/贴图肤色已验证）；
# 主光每个视角转到相机一侧、同样 30° 仰角打。模型正面朝向不定（VRM 常见 -Z、
# Mixamo 导出的 GLB 为 +Z），两侧都拍，识别时取有脸的那张。
# 用法：godot --path godot res://scenes/char_gallery.tscn -- --model=res://gallery_models/Ch46_nonPBR.fbx
# （模型须在工程内且先 godot --headless --import 过；gallery_models/ 是临时副本目录，不进版本库）

const AMBIENT_COLOR := Color(1.0, 0.96, 0.92)
const AMBIENT_ENERGY := 0.45
const KEY_COLOR := Color(1.0, 0.95, 0.88)
const KEY_ENERGY := 0.95
const OUT_DIR := "res://../logs/char_gallery"
const FULL_DIST := 1.9        # 全身机位距离 = 身高 × 此值（fov 40° 下画面高约 1.38 倍身高）
const FACE_Y_RATIO := 0.93    # 特写目标高度 = 包围盒底部 + 身高 × 此值（约在眼线）
const FACE_DIST := 0.9        # 特写机位距离 = 身高 × 此值

var model_path := ""
var cam: Camera3D
var key: DirectionalLight3D
var floor_mi: MeshInstance3D
var label: Label
var box := AABB()
var shots: Array[Dictionary] = []
var shot_idx := 0
var timer := 0.0

func _ready() -> void:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--model="):
			model_path = arg.substr(8)
	if model_path == "" or not ResourceLoader.exists(model_path):
		push_error("[gallery] 需 --model=res://...（工程内路径，先 --headless --import）：%s" % model_path)
		get_tree().quit(1)
		return
	_build_booth()
	var inst: Node = (load(model_path) as PackedScene).instantiate()
	add_child(inst)
	await get_tree().process_frame          # 让导入场景的变换生效
	if not _measure():
		get_tree().quit(1)
		return
	_print_materials(inst)
	_plan_shots()
	_apply_shot(0)

func _build_booth() -> void:
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
	env.tonemap_mode = Environment.TONE_MAPPER_LINEAR
	var we := WorldEnvironment.new()
	we.environment = env
	add_child(we)
	key = DirectionalLight3D.new()
	key.light_color = KEY_COLOR
	key.light_energy = KEY_ENERGY
	key.shadow_enabled = true
	add_child(key)
	floor_mi = MeshInstance3D.new()
	var pm := PlaneMesh.new()
	pm.size = Vector2(12, 12)
	floor_mi.mesh = pm
	var fm := StandardMaterial3D.new()
	fm.albedo_color = Color(0.42, 0.44, 0.47)
	fm.roughness = 0.9
	floor_mi.material_override = fm
	add_child(floor_mi)
	cam = Camera3D.new()
	cam.fov = 40
	add_child(cam)
	cam.current = true
	var canvas := CanvasLayer.new()
	add_child(canvas)
	label = Label.new()
	label.position = Vector2(16, 10)
	label.add_theme_font_size_override("font_size", 28)
	canvas.add_child(label)

func _measure() -> bool:
	var first := true
	for mi: MeshInstance3D in find_children("*", "MeshInstance3D", true, false):
		var b: AABB = mi.global_transform * mi.mesh.get_aabb()
		if first:
			box = b
			first = false
		else:
			box = box.merge(b)
	if first or box.size.y <= 0.0:
		push_error("[gallery] 没有网格，无法取包围盒")
		return false
	floor_mi.position.y = box.position.y      # 地板贴脚底，接住影子
	print("[gallery] 包围盒 中心 %s 尺寸 %s（高 %.2f m）" % [box.position + box.size * 0.5, box.size, box.size.y])
	return true

func _print_materials(inst: Node) -> void:
	var seen := {}
	for mi: MeshInstance3D in inst.find_children("*", "MeshInstance3D", true, false):
		for idx in mi.get_surface_override_material_count():
			var mat: Material = mi.get_active_material(idx)
			if mat == null or seen.has(mat.get_instance_id()):
				continue
			seen[mat.get_instance_id()] = true
			if mat is BaseMaterial3D:
				var bm := mat as BaseMaterial3D
				var tex_name: String = bm.albedo_texture.resource_name if bm.albedo_texture else "-"
				print("[gallery] 材质 %s[%d] albedo=%s 贴图=%s" % [bm.resource_name, idx, bm.albedo_color, tex_name])
			elif mat is ShaderMaterial:
				var v = (mat as ShaderMaterial).get_shader_parameter("_Color")
				print("[gallery] 材质 %s[%d] _Color=%s" % [mat.resource_name, idx, v])

func _plan_shots() -> void:
	var stem: String = model_path.get_file().get_basename()
	var c := box.position + box.size * 0.5
	var h := box.size.y
	for v in [["pz", Vector3(0, 0, 1)], ["nz", Vector3(0, 0, -1)]]:
		var d: Vector3 = v[1]
		shots.append({"t": 1.5 + shots.size() * 1.3, "file": "%s_%s_full.png" % [stem, v[0]],
				"dir": d, "target": c, "dist": h * FULL_DIST})
		shots.append({"t": 1.5 + shots.size() * 1.3, "file": "%s_%s_face.png" % [stem, v[0]],
				"dir": d, "target": Vector3(c.x, box.position.y + h * FACE_Y_RATIO, c.z), "dist": h * FACE_DIST})

func _apply_shot(i: int) -> void:
	var s: Dictionary = shots[i]
	var dir_h: Vector3 = (s["dir"] as Vector3).normalized()
	cam.position = (s["target"] as Vector3) + dir_h * (s["dist"] as float)
	cam.look_at(s["target"])
	key.position = Vector3(dir_h.x, 0.5, dir_h.z).normalized() * 10.0
	key.look_at(Vector3.ZERO)
	label.text = "%s  |  %s" % [model_path.get_file(), s["file"]]

func _process(delta: float) -> void:
	if shot_idx >= shots.size():
		return
	timer += delta
	if timer < shots[shot_idx]["t"]:
		return
	var s: Dictionary = shots[shot_idx]
	await RenderingServer.frame_post_draw
	var img := get_viewport().get_texture().get_image()
	var dir := ProjectSettings.globalize_path(OUT_DIR)
	DirAccess.make_dir_recursive_absolute(dir)
	img.save_png(dir + "/" + s["file"])
	print("[gallery] 截图 %s" % s["file"])
	shot_idx += 1
	if shot_idx >= shots.size():
		get_tree().quit()
		return
	_apply_shot(shot_idx)
