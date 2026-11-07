class_name VrmaRetarget
extends RefCounted
# VRMA（VRM Animation 1.0：带 VRMC_vrm_animation 扩展的 GLB）→ 已导入 VRM 骨架的运行时重定向。
#
# 为什么不能直接把 .vrma 喂给 AnimationPlayer：.vrma 自带一副骨架（这批由 Mixamo 转出，66 根、
# 骨名 mixamorig_*），轨道按它的骨名、骨轴、层级存；VRM 插件导入模型时把人形骨改成 Godot 人形
# 规范名（Hips/LeftUpperArm…）并把静止骨轴统一到规范 T-pose。两边骨名对不上一根，骨轴也不同。
#
# 做法：VRMA 规范要求两边静止姿态都是 T-pose，于是每根人形骨取"相对静止姿态的世界空间旋转
# 增量"，乘到目标骨的静止世界旋转上，再换算回目标父骨局部——与骨轴、层级差异（有无 UpperChest
# 之类）无关。髋部位移按两边髋高比例缩放；源骨架朝 +Z（glTF 惯例）而本工程 VRM 导入后朝 -Z，
# 按左臂指向自动判定并绕 Y 转 180°。
#
# 用法：VrmaRetarget.add_to_vrm(vrm_root, "res://assets3d/characters/anims/Walking.vrma", "walk", true)
#       → 动画进 VRM 自带的 AnimationPlayer 默认库，之后 ap.play("walk")。
#       strip_xz=true 剥离水平前进位移（原地走，位置由游戏层驱动）；动画元数据 vrma_speed_mps
#       是剥离前的原生前进速度（目标模型米制），vrma_drift_per_loop 是每循环位移向量。

const SAMPLE_FPS := 30.0
const VRM_HIPS := "hips"

class Source:
	var root: Node              # 生成的源场景（不入树，只做数学；缓存持有防释放）
	var skel: Skeleton3D
	var anim: Animation
	var bones := {}             # VRM 人形骨名（hips/leftUpperArm…）→ 源骨下标
	var rot_tracks := {}        # 源骨下标 → 旋转轨道
	var pos_tracks := {}        # 源骨下标 → 位移轨道

static var _sources := {}      # vrma 路径 → Source
static var _retargeted := {}   # 缓存键 → Animation（同一模型的所有实例共享一份）

# VRM 人形骨名 → Godot 人形规范骨名：首字母大写即可（leftUpperArm → LeftUpperArm）。
static func godot_bone_name(vrm_name: String) -> String:
	return vrm_name.substr(0, 1).to_upper() + vrm_name.substr(1)

static func load_source(vrma_path: String) -> Source:
	if _sources.has(vrma_path):
		return _sources[vrma_path]
	var doc := GLTFDocument.new()
	var state := GLTFState.new()
	state.import_as_skeleton_bones = true      # 无蒙皮的纯骨架文件：全部节点作为一副 Skeleton3D 的骨
	var err := doc.append_from_file(vrma_path, state)
	if err != OK:
		push_error("[vrma] 读取失败 %s（%d）" % [vrma_path, err])
		return null
	var root := doc.generate_scene(state)
	var skels := root.find_children("*", "Skeleton3D", true, false)
	var players := root.find_children("*", "AnimationPlayer", true, false)
	if skels.is_empty() or players.is_empty():
		push_error("[vrma] %s 里没有骨架或动画" % vrma_path)
		root.free()
		return null
	var src := Source.new()
	src.root = root
	src.skel = skels[0]
	var ap: AnimationPlayer = players[0]
	var names := ap.get_animation_list()
	src.anim = ap.get_animation(names[0])
	# 人形骨映射在文件顶层扩展里：节点下标 → 骨名（节点名含 ":" 时导入器已改成 "_"）
	var human: Dictionary = state.json.get("extensions", {}).get("VRMC_vrm_animation", {}).get("humanoid", {}).get("humanBones", {})
	var nodes := state.get_nodes()
	for vrm_name in human:
		var ni: int = human[vrm_name].get("node", -1)
		if ni < 0 or ni >= nodes.size():
			continue
		var gn: GLTFNode = nodes[ni]
		var bi := src.skel.find_bone(gn.resource_name)
		if bi < 0:
			bi = src.skel.find_bone(gn.original_name.replace(":", "_").replace("/", "_"))
		if bi >= 0:
			src.bones[vrm_name] = bi
	for ti in src.anim.get_track_count():
		var p := src.anim.track_get_path(ti)
		if p.get_subname_count() == 0:
			continue
		var bi := src.skel.find_bone(p.get_subname(0))
		if bi < 0:
			continue
		match src.anim.track_get_type(ti):
			Animation.TYPE_ROTATION_3D:
				src.rot_tracks[bi] = ti
			Animation.TYPE_POSITION_3D:
				src.pos_tracks[bi] = ti
	if not src.bones.has(VRM_HIPS):
		push_error("[vrma] %s 的人形骨映射缺 hips（映射 %d 根）" % [vrma_path, src.bones.size()])
		root.free()
		return null
	print("[vrma] %s：源骨 %d 根，人形映射 %d 根，轨道 %d，时长 %.2fs" % [vrma_path.get_file(), src.skel.get_bone_count(), src.bones.size(), src.anim.get_track_count(), src.anim.length])
	_sources[vrma_path] = src
	return src

# 源骨在 t 时刻的局部变换：有轨道取轨道，否则取静止姿态。
static func _src_local(src: Source, b: int, t: float) -> Transform3D:
	var rest := src.skel.get_bone_rest(b)
	var q := rest.basis.get_rotation_quaternion()
	if src.rot_tracks.has(b):
		q = src.anim.rotation_track_interpolate(src.rot_tracks[b], t)
	var o := rest.origin
	if src.pos_tracks.has(b):
		o = src.anim.position_track_interpolate(src.pos_tracks[b], t)
	return Transform3D(Basis(q), o)

static func _src_global(src: Source, b: int, t: float, memo: Dictionary) -> Transform3D:
	if memo.has(b):
		return memo[b]
	var g := _src_local(src, b, t)
	var p := src.skel.get_bone_parent(b)
	if p >= 0:
		g = _src_global(src, p, t, memo) * g
	memo[b] = g
	return g

# 目标骨在 t 时刻的全局变换（ctx 见 retarget）：映射骨按世界增量算，其余骨保持静止姿态。
static func _tgt_global(ctx: Dictionary, b: int) -> Transform3D:
	var memo: Dictionary = ctx.memo
	if memo.has(b):
		return memo[b]
	var skel: Skeleton3D = ctx.skel
	var p := skel.get_bone_parent(b)
	var parent_g := Transform3D.IDENTITY if p < 0 else _tgt_global(ctx, p)
	var local := skel.get_bone_rest(b)
	if ctx.pairs.has(b):
		var src: Source = ctx.src
		var sb: int = ctx.pairs[b]
		var rfix: Quaternion = ctx.rfix
		var sg: Transform3D = _src_global(src, sb, ctx.t, ctx.smemo)
		var sr: Transform3D = src.skel.get_bone_global_rest(sb)
		var delta := rfix * sg.basis.get_rotation_quaternion() * sr.basis.get_rotation_quaternion().inverse() * rfix.inverse()
		var g_rot := delta * skel.get_bone_global_rest(b).basis.get_rotation_quaternion()
		local.basis = Basis(parent_g.basis.get_rotation_quaternion().inverse() * g_rot)
		if b == ctx.hips:
			var dp: Vector3 = rfix * (sg.origin - sr.origin) * ctx.scale
			if ctx.strip:
				dp -= ctx.drift_xz * (ctx.t / ctx.length)
			local.origin = parent_g.affine_inverse() * (skel.get_bone_global_rest(b).origin + dp)
	var g := parent_g * local
	memo[b] = g
	return g

# 源与目标是否朝向相反：比较左上臂→左前臂的世界 X 指向（T-pose 下左臂水平外伸）。
static func _needs_flip(src: Source, skel: Skeleton3D) -> bool:
	if not (src.bones.has("leftUpperArm") and src.bones.has("leftLowerArm")):
		return false
	var tu := skel.find_bone("LeftUpperArm")
	var tl := skel.find_bone("LeftLowerArm")
	if tu < 0 or tl < 0:
		return false
	var sx := src.skel.get_bone_global_rest(src.bones["leftLowerArm"]).origin.x - src.skel.get_bone_global_rest(src.bones["leftUpperArm"]).origin.x
	var tx := skel.get_bone_global_rest(tl).origin.x - skel.get_bone_global_rest(tu).origin.x
	return sx * tx < 0.0

# 把 vrma 重定向到 skel，轨道路径以 skel_path（相对 AnimationPlayer 根节点）为前缀。
static func retarget(vrma_path: String, skel: Skeleton3D, skel_path: String, strip_xz := false) -> Animation:
	var src := load_source(vrma_path)
	if src == null:
		return null
	var pairs := {}                             # 目标骨下标 → 源骨下标
	for vrm_name in src.bones:
		var tb := skel.find_bone(godot_bone_name(vrm_name))
		if tb >= 0:
			pairs[tb] = src.bones[vrm_name]
	var hips := skel.find_bone("Hips")
	if hips < 0 or not pairs.has(hips):
		push_error("[vrma] 目标骨架没有 Hips，无法重定向 %s" % vrma_path)
		return null
	var length := src.anim.length
	var flip := _needs_flip(src, skel)
	var rfix := Quaternion(Vector3.UP, PI) if flip else Quaternion.IDENTITY
	var src_hips: int = src.bones[VRM_HIPS]
	var src_hips_y := absf(src.skel.get_bone_global_rest(src_hips).origin.y)
	var scale := absf(skel.get_bone_global_rest(hips).origin.y) / maxf(src_hips_y, 0.001)
	var p0 := _src_global(src, src_hips, 0.0, {}).origin
	var p1 := _src_global(src, src_hips, length, {}).origin
	var drift: Vector3 = rfix * (p1 - p0) * scale
	var drift_xz := Vector3(drift.x, 0.0, drift.z)

	var ctx := {"skel": skel, "src": src, "pairs": pairs, "hips": hips, "rfix": rfix, "scale": scale,
			"strip": strip_xz, "drift_xz": drift_xz, "length": length, "t": 0.0, "memo": {}, "smemo": {}}
	var n := int(ceil(length * SAMPLE_FPS))
	var times := PackedFloat32Array()
	var rot_keys := {}                          # 目标骨 → Array[Quaternion]
	var pos_keys: Array[Vector3] = []
	for tb in pairs:
		rot_keys[tb] = []
	for k in n + 1:
		var t := minf(k / SAMPLE_FPS, length)
		times.append(t)
		ctx.t = t
		ctx.memo = {}
		ctx.smemo = {}
		for tb in pairs:
			_tgt_global(ctx, tb)
			var local_rot: Quaternion = (ctx.memo[tb] as Transform3D).basis.get_rotation_quaternion()
			var pg: Transform3D = Transform3D.IDENTITY if skel.get_bone_parent(tb) < 0 else ctx.memo[skel.get_bone_parent(tb)]
			local_rot = pg.basis.get_rotation_quaternion().inverse() * local_rot
			rot_keys[tb].append(local_rot.normalized())
		var hg: Transform3D = ctx.memo[hips]
		var hp: Transform3D = Transform3D.IDENTITY if skel.get_bone_parent(hips) < 0 else ctx.memo[skel.get_bone_parent(hips)]
		pos_keys.append(hp.affine_inverse() * hg.origin)

	var anim := Animation.new()
	anim.length = length
	anim.loop_mode = Animation.LOOP_LINEAR
	for tb in pairs:
		var keys: Array = rot_keys[tb]
		var constant := true
		for q: Quaternion in keys:
			if not q.is_equal_approx(keys[0]) and not (-q).is_equal_approx(keys[0]):
				constant = false
				break
		var ti := anim.add_track(Animation.TYPE_ROTATION_3D)
		anim.track_set_path(ti, NodePath("%s:%s" % [skel_path, skel.get_bone_name(tb)]))
		# 常量轨道也保留（单键）：切换动画时这些骨要回到本动画的姿态，而不是停在上一段的末帧
		var count: int = 1 if constant else keys.size()
		for k in count:
			var q: Quaternion = keys[k]
			if k > 0 and q.dot(keys[k - 1]) < 0.0:
				q = -q
			anim.rotation_track_insert_key(ti, times[k], q)
	# 髋位移：AnimationMixer 播放时会乘 Skeleton3D.motion_scale（导入器设为髋高），写入前先除掉
	var ms := skel.motion_scale if skel.motion_scale > 0.0001 else 1.0
	var pt := anim.add_track(Animation.TYPE_POSITION_3D)
	anim.track_set_path(pt, NodePath("%s:%s" % [skel_path, skel.get_bone_name(hips)]))
	for k in pos_keys.size():
		anim.position_track_insert_key(pt, times[k], pos_keys[k] / ms)
	anim.set_meta("vrma_drift_per_loop", drift)
	anim.set_meta("vrma_speed_mps", drift_xz.length() / maxf(length, 0.001))
	anim.set_meta("vrma_flipped", flip)
	return anim

static func _find_or_make_player(vrm_root: Node) -> AnimationPlayer:
	var ap := vrm_root.get_node_or_null("AnimationPlayer") as AnimationPlayer
	if ap == null:
		var aps := vrm_root.find_children("*", "AnimationPlayer", true, false)
		if not aps.is_empty():
			ap = aps[0]
	if ap == null:
		ap = AnimationPlayer.new()
		ap.name = "AnimationPlayer"
		vrm_root.add_child(ap)
	return ap

# 把 vrma 重定向后加进 VRM 实例自带的 AnimationPlayer（默认库），名为 anim_name；返回动画。
static func add_to_vrm(vrm_root: Node, vrma_path: String, anim_name: String, strip_xz := false) -> Animation:
	var skels := vrm_root.find_children("*", "Skeleton3D", true, false)
	if skels.is_empty():
		push_error("[vrma] %s 下没有 Skeleton3D" % vrm_root.name)
		return null
	var skel: Skeleton3D = skels[0]
	var ap := _find_or_make_player(vrm_root)
	var base: Node = ap.get_node_or_null(ap.root_node)
	if base == null:
		base = vrm_root
	var skel_path := str(base.get_path_to(skel))
	var key := "%s|%s|%d|%s|%s" % [vrma_path, vrm_root.scene_file_path, skel.get_bone_count(), skel_path, strip_xz]
	var anim: Animation = _retargeted.get(key)
	if anim == null:
		anim = retarget(vrma_path, skel, skel_path, strip_xz)
		if anim == null:
			return null
		_retargeted[key] = anim
	var lib: AnimationLibrary
	if ap.has_animation_library(""):
		lib = ap.get_animation_library("")
	else:
		lib = AnimationLibrary.new()
		ap.add_animation_library("", lib)
	if lib.has_animation(anim_name):
		lib.remove_animation(anim_name)
	lib.add_animation(anim_name, anim)
	return anim
