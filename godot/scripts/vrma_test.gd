extends Node3D
# VRMA 动画展示棚：一排 VRM 角色各播一段 .vrma（走 / 站 / 坐×2 / 躺睡），验证 VrmaRetarget
# 的运行时重定向。走路角色原地走 + 根节点按动画原生速度绕圈。
# 落地修正：源动画的髋高是"作者的床/凳高度"（如 Laying Sleeping 髋在 0.76 m），重定向忠实
# 保留；这里按 t=0 全身最低骨高度整体下移，让身体贴地（躺睡再垫上床），凳子顶面贴髋下 9 cm。
# 用法：godot --path godot res://scenes/vrma_test.tscn
#          [-- --screenshot-after=秒 [--shots=N] [--focus=N] [--zoom=距离] [--az=度] [--el=度] [--all=N]]
# 交互：左键拖拽旋转、滚轮缩放、1–5 全员切到该动画、0 各演各的、空格暂停/继续、ESC 退出。
# 光照同 vrm_test 配方（暖环境光 + 微暖主光 + Linear）。

const Retarget := preload("res://scripts/vrma_retarget.gd")
const ANIM_DIR := "res://assets3d/characters/anims"
const CHAR_DIR := "res://assets3d/characters"
const OUT_DIR := "res://../logs"
const AMBIENT_COLOR := Color(1.0, 0.96, 0.92)
const AMBIENT_ENERGY := 0.45
const KEY_COLOR := Color(1.0, 0.95, 0.88)
const KEY_ENERGY := 0.95
const KEY_FROM_DIR := Vector3(-0.433013, 0.5, -0.75)   # 30° 仰角、从角色正面（-Z 侧）偏左打
const SPACING := 1.7
const WALK_RADIUS := 0.6        # 走路角色绕自己站位转圈的半径
const BED_H := 0.24             # 床顶面高度
const GROUND_MARGIN := 0.05     # 最低骨离地留的"皮肉厚度"
const SEAT_DROP := 0.09         # 凳面在髋关节下方的距离（臀部表面）
const SHOWS := [                # 顺序即站位（左→右）
	{"file": "Walking.vrma", "name": "walk", "label": "走路 Walking", "strip": true},
	{"file": "Standing Idle.vrma", "name": "idle", "label": "站立 Standing Idle", "strip": false},
	{"file": "Man Sitting Idle.vrma", "name": "sit_m", "label": "坐姿（男）Man Sitting Idle", "strip": false, "prop": "stool"},
	{"file": "Woman Sitting.vrma", "name": "sit_w", "label": "坐姿（女）Woman Sitting", "strip": false, "prop": "stool"},
	{"file": "Laying Sleeping.vrma", "name": "sleep", "label": "躺睡 Laying Sleeping", "strip": false, "prop": "bed"},
]

var cam: Camera3D
var chars: Array[Node3D] = []
var players: Array[AnimationPlayer] = []
var current: PackedStringArray = []       # 每位角色当前动画名
var walk_speed := PackedFloat32Array()    # 每位角色 walk 的原生速度（米/秒，按各自髋高缩放）
var theta := PackedFloat32Array()         # 绕圈角度
var base_y := PackedFloat32Array()        # 落地修正后的站位高度
var slots: Array[Vector3] = []
var ground_shift: Array[PackedFloat32Array] = []   # 每角色 × 每动画的落地修正量
var t0_poses: Array[Dictionary] = []      # 每角色站位动画 t=0 的髋/头/脚世界位置（摆道具用）
var paused := false
# 轨道相机
var orbit_center := Vector3(0.0, 0.85, 0.0)
var orbit_azimuth := PI                   # 相机在 -Z 侧（角色正面）
var orbit_elevation := 0.16
var orbit_dist := 7.6
var dragging := false
# 自检
var screenshot_after := -1.0
var shots_left := 1
var shot_gap := 0.7
var shot_timer := 0.0
var shot_idx := 0
var shooting := false
var all_anim := -1

func _ready() -> void:
	var focus := -1
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--screenshot-after="):
			screenshot_after = arg.substr(19).to_float()
		elif arg.begins_with("--shots="):
			shots_left = maxi(1, arg.substr(8).to_int())
		elif arg.begins_with("--focus="):
			focus = arg.substr(8).to_int()
		elif arg.begins_with("--zoom="):
			orbit_dist = arg.substr(7).to_float()
		elif arg.begins_with("--az="):
			orbit_azimuth = deg_to_rad(arg.substr(5).to_float())
		elif arg.begins_with("--el="):
			orbit_elevation = deg_to_rad(arg.substr(5).to_float())
		elif arg.begins_with("--all="):
			all_anim = arg.substr(6).to_int()
	shot_timer = screenshot_after            # _ready 中途有 await，先占好再让 _process 跑
	_build_booth()
	var files := PackedStringArray()
	for f in DirAccess.get_files_at(CHAR_DIR):
		if f.ends_with(".vrm") and not f.begins_with("avatar_test"):
			files.append(f)
	files.sort()
	if files.is_empty():
		push_error("[vrma_test] %s 下没有 VRM 角色" % CHAR_DIR)
		get_tree().quit(1)
		return
	for i in SHOWS.size():
		var f: String = files[i % files.size()]
		var slot := Vector3((i - (SHOWS.size() - 1) * 0.5) * SPACING, 0.0, 0.0)
		slots.append(slot)
		_spawn(i, "%s/%s" % [CHAR_DIR, f], slot)
		base_y.append(0.0)
		ground_shift.append(PackedFloat32Array())
		t0_poses.append({})
	if focus >= 0 and focus < slots.size():
		orbit_center = slots[focus] + Vector3(0.0, 0.8, 0.0)
	_update_orbit_cam()
	await get_tree().process_frame
	await _measure_ground()               # 逐角色逐动画量落地修正（内部逐帧推进）
	_place_props()
	for i in chars.size():
		_apply_base_y(i)
	if all_anim >= 0:
		_play_all(all_anim)

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
	var key := DirectionalLight3D.new()
	add_child(key)
	key.position = KEY_FROM_DIR * 10.0
	key.look_at(Vector3.ZERO)
	key.light_color = KEY_COLOR
	key.light_energy = KEY_ENERGY
	key.shadow_enabled = true
	var floor_mi := MeshInstance3D.new()
	var pm := PlaneMesh.new()
	pm.size = Vector2(16, 16)
	floor_mi.mesh = pm
	var fm := StandardMaterial3D.new()
	fm.albedo_color = Color(0.46, 0.48, 0.5)
	fm.roughness = 0.95
	floor_mi.material_override = fm
	add_child(floor_mi)
	cam = Camera3D.new()
	cam.fov = 40
	add_child(cam)
	cam.current = true
	var canvas := CanvasLayer.new()
	add_child(canvas)
	var hint := Label.new()
	hint.position = Vector2(14, 40)
	hint.add_theme_font_size_override("font_size", 18)
	hint.text = "VRMA 动画棚  ·  拖拽旋转 / 滚轮缩放  ·  1–5 全员同一动作，0 各演各的  ·  空格暂停  ·  ESC 退出"
	var hint_bg := PanelContainer.new()
	var hb_style := StyleBoxFlat.new()
	hb_style.bg_color = Color(0.1, 0.1, 0.12, 0.62)
	hint_bg.add_theme_stylebox_override("panel", hb_style)
	hint_bg.position = Vector2(6, 34)
	hint_bg.size = Vector2(980, 34)
	canvas.add_child(hint_bg)
	canvas.add_child(hint)

func _spawn(i: int, model: String, slot: Vector3) -> void:
	var node: Node3D = (load(model) as PackedScene).instantiate()
	add_child(node)
	node.position = slot
	var ap: AnimationPlayer = null
	var speed := 0.0
	for s in SHOWS:
		var anim := Retarget.add_to_vrm(node, "%s/%s" % [ANIM_DIR, s["file"]], s["name"], s["strip"])
		if anim == null:
			continue
		if s["name"] == "walk":
			speed = anim.get_meta("vrma_speed_mps", 0.0)
	var aps := node.find_children("*", "AnimationPlayer", true, false)
	if not aps.is_empty():
		ap = aps[0]
	var show: Dictionary = SHOWS[i]
	if ap != null and ap.has_animation(show["name"]):
		ap.play(show["name"])
	chars.append(node)
	players.append(ap)
	current.append(show["name"])
	walk_speed.append(speed)
	theta.append(0.0)
	var lb := Label3D.new()
	lb.text = "%s\n%s" % [show["label"], model.get_file().get_basename()]
	lb.font_size = 44
	lb.outline_size = 10
	lb.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	lb.position = slot + Vector3(0.0, 1.95, 0.0)
	add_child(lb)
	var skels := node.find_children("*", "Skeleton3D", true, false)
	var hips_y := 0.0
	if not skels.is_empty():
		var sk: Skeleton3D = skels[0]
		hips_y = sk.get_bone_global_rest(sk.find_bone("Hips")).origin.y
	print("[vrma_test] %d %s：髋高 %.3f，walk 原生速度 %.2f m/s，动作 %s" % [i, model.get_file(), hips_y, speed, show["name"]])

func _skel(i: int) -> Skeleton3D:
	var skels := chars[i].find_children("*", "Skeleton3D", true, false)
	return skels[0] if not skels.is_empty() else null

func _bone_world(i: int, bone: String) -> Vector3:
	var sk := _skel(i)
	var bi := sk.find_bone(bone)
	return chars[i].global_transform * sk.get_bone_global_pose(bi).origin

func _bone_min_y(i: int) -> float:
	var sk := _skel(i)
	var xf := chars[i].global_transform
	var lowest := 1e9
	for b in sk.get_bone_count():
		if sk.get_bone_parent(b) < 0:
			continue          # 根骨（VRM 的 Root）钉在原点，不代表身体
		lowest = minf(lowest, (xf * sk.get_bone_global_pose(b).origin).y)
	return lowest

# 每角色 × 每动画：播到 t=0 量全身最低骨，算"落到地面"的整体下移；顺带记录站位动画的首帧
# 髋/头/脚位置给道具摆位。
func _measure_ground() -> void:
	for i in chars.size():
		var keep: String = current[i]
		var shifts := PackedFloat32Array()
		for s in SHOWS:
			var nm: String = s["name"]
			if players[i] == null or not players[i].has_animation(nm):
				shifts.append(0.0)
				continue
			players[i].play(nm)
			players[i].seek(0.0, true)
			await get_tree().process_frame
			await get_tree().process_frame   # 第二帧确保混合器已把姿态写进骨架
			var shift := GROUND_MARGIN - _bone_min_y(i)
			shifts.append(shift)
			chars[i].position.y = shift
			if nm == SHOWS[i]["name"]:
				t0_poses[i] = {"hips": _bone_world(i, "Hips"), "head": _bone_world(i, "Head"),
						"lfoot": _bone_world(i, "LeftFoot")}
		players[i].play(keep)
		ground_shift[i] = shifts
		var shift_txt := ""
		for j in SHOWS.size():
			shift_txt += "%s:%.2f  " % [SHOWS[j]["name"], shifts[j]]
		print("[vrma_test] %d 落地修正  %s" % [i, shift_txt])

# 站位道具：凳子顶面贴到（修正后的）髋下方，床顺着身体方向垫在最低点下面。
func _place_props() -> void:
	for i in chars.size():
		var show: Dictionary = SHOWS[i]
		if not show.has("prop") or t0_poses[i].is_empty():
			continue
		var j := _anim_index(show["name"])
		var shift: float = ground_shift[i][j]
		var hips: Vector3 = t0_poses[i]["hips"]
		var mi := MeshInstance3D.new()
		var box := BoxMesh.new()
		var mat := StandardMaterial3D.new()
		mat.roughness = 0.85
		if show["prop"] == "stool":
			var top := maxf(hips.y + shift - SEAT_DROP, 0.12)
			box.size = Vector3(0.42, top, 0.42)
			mat.albedo_color = Color(0.62, 0.45, 0.3)
			mi.position = Vector3(hips.x, top * 0.5, hips.z)
		else:
			# 床沿身体方向（头→脚）摆，中心取髋/头/脚的中间；角色最低骨落在床顶面
			var head: Vector3 = t0_poses[i]["head"]
			var foot: Vector3 = t0_poses[i]["lfoot"]
			var axis := Vector3(head.x - foot.x, 0.0, head.z - foot.z)
			var center := Vector3((head.x + hips.x + foot.x) / 3.0, 0.0, (head.z + hips.z + foot.z) / 3.0)
			box.size = Vector3(0.95, BED_H, 2.05)
			mat.albedo_color = Color(0.55, 0.62, 0.78)
			mi.position = Vector3(center.x, BED_H * 0.5, center.z)
			if axis.length() > 0.01:
				mi.look_at_from_position(mi.position, mi.position + axis.normalized())
			ground_shift[i][j] += BED_H - GROUND_MARGIN   # 最低骨落在床顶面（再沉一点像压着床垫）
		mi.mesh = box
		mi.material_override = mat
		add_child(mi)
		print("[vrma_test] 道具 %s @ %s" % [show["prop"], mi.position])

func _anim_index(nm: String) -> int:
	for j in SHOWS.size():
		if SHOWS[j]["name"] == nm:
			return j
	return -1

func _apply_base_y(i: int) -> void:
	var j := _anim_index(current[i])
	base_y[i] = ground_shift[i][j] if j >= 0 else 0.0
	chars[i].position.y = base_y[i]

func _play_all(idx: int) -> void:
	for i in chars.size():
		var nm: String = SHOWS[idx]["name"] if idx >= 0 and idx < SHOWS.size() else SHOWS[i]["name"]
		if players[i] != null and players[i].has_animation(nm) and current[i] != nm:
			players[i].play(nm, 0.3)
			current[i] = nm
		_apply_base_y(i)

func _update_orbit_cam() -> void:
	var h := Vector3(cos(orbit_elevation) * sin(orbit_azimuth), sin(orbit_elevation), cos(orbit_elevation) * cos(orbit_azimuth))
	cam.position = orbit_center + h * orbit_dist
	cam.look_at_from_position(cam.position, orbit_center)

func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		if event.button_index == MOUSE_BUTTON_LEFT:
			dragging = event.pressed
		elif event.button_index == MOUSE_BUTTON_WHEEL_UP and event.pressed:
			orbit_dist = maxf(1.0, orbit_dist - 0.4)
		elif event.button_index == MOUSE_BUTTON_WHEEL_DOWN and event.pressed:
			orbit_dist = minf(14.0, orbit_dist + 0.4)
		_update_orbit_cam()
	elif event is InputEventMouseMotion and dragging:
		orbit_azimuth -= event.relative.x * 0.006
		orbit_elevation = clampf(orbit_elevation + event.relative.y * 0.004, -0.2, 1.3)
		_update_orbit_cam()
	elif event is InputEventKey and event.pressed:
		if event.keycode == KEY_ESCAPE:
			get_tree().quit()
		elif event.keycode >= KEY_1 and event.keycode <= KEY_5:
			_play_all(event.keycode - KEY_1)
		elif event.keycode == KEY_0:
			_play_all(-1)
		elif event.keycode == KEY_SPACE:
			paused = not paused
			for ap in players:
				if ap != null:
					ap.speed_scale = 0.0 if paused else 1.0

func _process(delta: float) -> void:
	if not paused:
		for i in chars.size():
			if current[i] != "walk" or walk_speed[i] <= 0.0:
				continue
			# 原地走的角色绕自己站位转圈：圈速 = 动画原生前进速度，模型正面朝 -Z 用默认 look_at
			theta[i] += delta * walk_speed[i] / WALK_RADIUS
			var c := slots[i]
			var pos := c + Vector3(sin(theta[i]), 0.0, cos(theta[i])) * WALK_RADIUS
			var tangent := Vector3(cos(theta[i]), 0.0, -sin(theta[i]))
			chars[i].position = Vector3(pos.x, base_y[i], pos.z)
			chars[i].look_at(pos + tangent)
	if screenshot_after < 0.0 or shooting:
		return
	shot_timer -= delta
	if shot_timer > 0.0:
		return
	shooting = true
	await RenderingServer.frame_post_draw
	var img := get_viewport().get_texture().get_image()
	var dir := ProjectSettings.globalize_path(OUT_DIR)
	DirAccess.make_dir_recursive_absolute(dir)
	shot_idx += 1
	var file := "godot_vrma.png" if shots_left == 1 and shot_idx == 1 else "godot_vrma_%d.png" % shot_idx
	img.save_png(dir + "/" + file)
	print("[vrma_test] 截图 logs/%s" % file)
	shots_left -= 1
	if shots_left <= 0:
		get_tree().quit()
		return
	shot_timer = shot_gap
	shooting = false
