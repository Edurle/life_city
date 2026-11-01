// 两段式卡通着色（平色版）：亮部提亮、暗部偏蓝紫乘数。
// unshaded + 手动太阳方向：不依赖渲染器专属的 light() 内建（兼容模式行为一致）。
// albedo 由脚本按材质名（flat|#hex）注入；COLOR 供 MultiMesh 实例着色。
shader_type spatial;
render_mode unshaded;

uniform vec3 albedo : source_color = vec3(1.0);
uniform vec3 shadow_tint : source_color = vec3(0.42, 0.46, 0.74);
uniform float lit_boost = 1.15;
uniform float cut : hint_range(-1.0, 1.0) = 0.40;
uniform vec3 sun_dir = vec3(0.30, 0.85, 0.43);   // 世界空间，指向太阳

void fragment() {
	vec3 n = normalize(mat3(INV_VIEW_MATRIX) * NORMAL);
	float t = step(cut, dot(n, normalize(sun_dir)));
	ALBEDO = albedo * COLOR.rgb * mix(shadow_tint, vec3(lit_boost), t);
}
