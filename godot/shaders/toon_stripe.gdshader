// 道路标线（斑马线/中心虚线）：局部坐标取模条纹，off 色贴路面隐形。
// 参数由材质名（stripe|轴|周期|占空|#on|#off）注入；轴 0=X，1=Z（原 Blender Y）。
shader_type spatial;
render_mode unshaded;

uniform int axis_id = 0;
uniform float period = 3.5;
uniform float duty = 1.3;
uniform vec3 on_color : source_color = vec3(1.0);
// 纯色模式（默认）：标线隐藏（off 色 = 路面色）；--detail 开回。
uniform bool stripes_on = false;
// 区域声誉染色（M7 旁观二阶段）：与 flat/facade 同一对参数（老街这类带摊位的路也染）。
uniform vec3 rep_color : source_color = vec3(1.0);
uniform float rep_amt : hint_range(0.0, 1.0) = 0.0;
uniform vec3 off_color : source_color = vec3(0.45);
uniform vec3 shadow_tint : source_color = vec3(0.42, 0.46, 0.74);
uniform float lit_boost = 1.15;
uniform float cut : hint_range(-1.0, 1.0) = 0.40;
uniform vec3 sun_dir = vec3(0.30, 0.85, 0.43);

void fragment() {
	float c = (axis_id == 0) ? VERTEX.x : VERTEX.z;
	float m = mod(c, period);
	vec3 base = mix(off_color, on_color, float(m < duty) * float(stripes_on));
	base = mix(base, rep_color, rep_amt);
	vec3 n = normalize(mat3(INV_VIEW_MATRIX) * NORMAL);
	float t = step(cut, dot(n, normalize(sun_dir)));
	ALBEDO = base * mix(shadow_tint, vec3(lit_boost), t);
}
