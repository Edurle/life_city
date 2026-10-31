# 《活城》客户端（M3 竖切 + M6 旁观模式）。
# 区域矩形按声誉着色、MultiMesh 批量渲染代理、速度档按钮、时钟与事件播报。
# 旁观模式：点击任意代理查看——公开信息人人可见；需求与内心只有
# 自己或密友可见（门在核心侧 inspect_friend，客户端不越权）。
# 模拟全部住在 Rust 核心（SimNode），本脚本只读快照——"核心不认识界面"。
extends Node2D

const SCALE := 28.0        # 米 → 像素
const MAP_W := 40.0
const MAP_H := 20.0

var sim: SimNode
var mm: MultiMesh
var area_rects_nodes := []   # ColorRect 数组
var area_labels := []
var clock_label: Label
var feed_label: RichTextLabel
var ui_timer := 0.0
var screenshot_after := -1.0   # > 0 时：到点截图并退出（调试用 --screenshot-after=秒）
var agent_count := 0
var species_bytes := PackedByteArray()
var configured := false
# 旁观模式（M6）
var selected := -1            # 点选的代理下标；-1 = 无
var camera: Camera2D
var inspect_panel: PanelContainer
var inspect_title: Label
var inspect_body: RichTextLabel
var follow_btn: Button
var follow := false

func _ready() -> void:
	# 模拟节点：GDExtension 注册的原生类。
	sim = SimNode.new()
	add_child(sim)
	# res:// = godot/ 工程目录；内容配置在仓库根的 assets/ 下。
	var cfg_path := ProjectSettings.globalize_path("res://../assets/street.json")
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--config="):
			cfg_path = arg.substr(9)
		elif arg.begins_with("--screenshot-after="):
			screenshot_after = arg.substr(19).to_float()
	sim.start(42, cfg_path, 8, 4)
	agent_count = sim.agent_positions().size()
	species_bytes = sim.agent_species()
	_build_camera()
	_build_areas()
	_build_agents()
	_build_ui()
	configured = true
	print("[slice] 启动：%d 名居民，配置 %s，截图模式 %s，用户参数 %s" % [agent_count, cfg_path, screenshot_after, OS.get_cmdline_user_args()])

func _build_camera() -> void:
	camera = Camera2D.new()
	camera.position = Vector2(MAP_W * SCALE / 2, MAP_H * SCALE / 2)
	camera.zoom = Vector2(1.0, 1.0)
	add_child(camera)
	camera.make_current()

func _build_areas() -> void:
	var rects: PackedFloat32Array = sim.area_rects()
	var names: PackedStringArray = sim.area_names()
	for i in names.size():
		var x: float = rects[i * 4]
		var y: float = rects[i * 4 + 1]
		var w: float = rects[i * 4 + 2]
		var h: float = rects[i * 4 + 3]
		var rect := ColorRect.new()
		rect.position = Vector2(x * SCALE, (MAP_H - y - h) * SCALE)
		rect.size = Vector2(w * SCALE, h * SCALE)
		rect.color = Color(0.2, 0.5, 0.25, 0.5)
		add_child(rect)
		area_rects_nodes.push_back(rect)
		var label := Label.new()
		label.position = rect.position + Vector2(8, 6)
		label.add_theme_font_size_override("font_size", 18)
		label.text = names[i]
		add_child(label)
		area_labels.push_back(label)

func _build_agents() -> void:
	# MultiMesh 批量渲染全部代理（不逐个建节点——2,000 代理时的纪律从竖切起守）。
	mm = MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_2D
	mm.use_colors = true
	var mesh := QuadMesh.new()
	mesh.size = Vector2(14, 14)
	mm.mesh = mesh
	mm.instance_count = agent_count
	var mmi := MultiMeshInstance2D.new()
	mmi.multimesh = mm
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
	feed_label = RichTextLabel.new()
	feed_label.position = Vector2(12, 660)
	feed_label.size = Vector2(760, 56)
	feed_label.add_theme_font_size_override("normal_font_size", 14)
	canvas.add_child(feed_label)
	# 旁观检查面板（M6）：点选代理后显示。
	inspect_panel = PanelContainer.new()
	inspect_panel.set_anchors_preset(Control.PRESET_RIGHT_WIDE)
	inspect_panel.position = Vector2(940, 60)
	inspect_panel.size = Vector2(320, 560)
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
	inspect_body.append_text("（旁观模式：任何人都能看到 [b]表面[/b]——在做什么、在哪里。\n需求与内心只有 [b]自己或密友[/b] 能看到。）")
	vb.add_child(inspect_body)
	inspect_panel.add_child(vb)
	canvas.add_child(inspect_panel)
	sim.set_speed_mult(100.0 if screenshot_after > 0.0 else 1.0)

func _unhandled_input(event: InputEvent) -> void:
	if not configured:
		return
	if event is InputEventMouseButton and event.pressed and event.button_index == MOUSE_BUTTON_LEFT:
		var world := get_global_mouse_position()
		var positions: PackedVector2Array = sim.agent_positions()
		var best := -1
		var best_d := 400.0   # 点击容差（像素平方）
		for i in positions.size():
			var p := Vector2(positions[i].x * SCALE, (MAP_H - positions[i].y) * SCALE)
			var d: float = p.distance_squared_to(world)
			if d < best_d:
				best_d = d
				best = i
		if best >= 0:
			selected = best
			_refresh_inspect()

func _toggle_follow() -> void:
	follow = not follow
	follow_btn.text = "跟随镜头：开" if follow else "跟随镜头：关"

func _refresh_inspect() -> void:
	if selected < 0:
		return
	var brief: PackedStringArray = sim.agent_brief(selected)
	if brief.size() < 5:
		return
	inspect_title.text = "%s（%s · %s）" % [brief[0], brief[1], brief[2]]
	inspect_body.clear()
	inspect_body.append_text("正在：%s\n地点：%s\n" % [brief[3], brief[4]])
	var deep: PackedStringArray = sim.agent_deep(selected)
	for line in deep:
		inspect_body.append_text(line + "\n")

func _process(delta: float) -> void:
	if not configured:
		return
	if screenshot_after > 0.0:
		screenshot_after -= delta
		if screenshot_after <= 0.0:
			_capture_and_quit()
			return
	# 每帧：代理位置 → MultiMesh 变换（世界 y 向上，Godot y 向下 → 翻转）。
	var positions: PackedVector2Array = sim.agent_positions()
	for i in positions.size():
		var t := Transform2D(0.0, Vector2(positions[i].x * SCALE, (MAP_H - positions[i].y) * SCALE))
		mm.set_instance_transform_2d(i, t)
		var species := species_bytes[i] if i < species_bytes.size() else 0
		mm.set_instance_color(i, Color(0.45, 0.7, 1.0) if species == 1 else Color(1.0, 0.7, 0.3))
	# 每半秒：时钟、区域声誉着色、事件播报。
	ui_timer += delta
	if ui_timer >= 0.5:
		ui_timer = 0.0
		clock_label.text = sim.clock_text()
		var states: PackedFloat32Array = sim.area_states()
		for i in area_rects_nodes.size():
			var rep: float = states[i * 2]
			var clean: float = states[i * 2 + 1]
			(area_rects_nodes[i] as ColorRect).color = Color(
				0.75 - 0.55 * rep, 0.25 + 0.5 * rep, 0.3, 0.35 + 0.25 * clean)
			(area_labels[i] as Label).text = "%s  声誉 %.0f%%" % [sim.area_names()[i], rep * 100]
		_refresh_inspect()
		var feed: PackedStringArray = sim.drain_recent_events()
		if feed.size() > 0:
			var lines := []
			for line in feed:
				lines.push_back(line)
			feed_label.append_text("\n".join(lines) + "\n")
	# 跟随镜头（M6 旁观）：平滑贴向选中代理。
	if follow and selected >= 0:
		var follow_pos: PackedVector2Array = sim.agent_positions()
		if selected < follow_pos.size():
			var target := Vector2(follow_pos[selected].x * SCALE, (MAP_H - follow_pos[selected].y) * SCALE)
			camera.position = camera.position.lerp(target, minf(delta * 4.0, 1.0))

func _capture_and_quit() -> void:
	# 调试：把视口存成 PNG 后退出（自动化可视验证用；需要真渲染，不能 headless）。
	await RenderingServer.frame_post_draw
	var img := get_viewport().get_texture().get_image()
	img.save_png(ProjectSettings.globalize_path("res://../logs/godot_slice.png"))
	print("[slice] 截图已保存 logs/godot_slice.png，%s" % sim.clock_text())
	get_tree().quit()

func _exit_tree() -> void:
	if configured:
		print("[slice] 退出：%s，%d 次决策" % [sim.clock_text(), sim.decisions_done()])
