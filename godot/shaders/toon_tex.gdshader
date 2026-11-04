// 两段式卡通着色（贴图版）：人物模型用——漫反射贴图 × 与 toon_flat 同一套两段光照。
// albedo_tex/albedo 由脚本从 GLB 原材质拷贝；tint/tint_amt 给玩家染金色。
// 不读 COLOR：Mixamo 等外部模型的顶点色不可靠（有的是全黑）。
shader_type spatial;
render_mode unshaded;

uniform sampler2D albedo_tex : source_color, hint_default_white, filter_linear_mipmap_anisotropic;
uniform vec3 albedo : source_color = vec3(1.0);
uniform vec3 tint : source_color = vec3(1.0);
uniform float tint_amt : hint_range(0.0, 1.0) = 0.0;
uniform vec3 shadow_tint : source_color = vec3(0.42, 0.46, 0.74);
uniform float lit_boost = 1.15;
uniform float cut : hint_range(-1.0, 1.0) = 0.40;
uniform vec3 sun_dir = vec3(0.30, 0.85, 0.43);   // 世界空间，指向太阳

void fragment() {
	vec3 n = normalize(mat3(INV_VIEW_MATRIX) * NORMAL);
	float t = step(cut, dot(n, normalize(sun_dir)));
	vec3 base = texture(albedo_tex, UV).rgb * albedo;
	ALBEDO = mix(base, tint, tint_amt) * mix(shadow_tint, vec3(lit_boost), t);
}
