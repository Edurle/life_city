// 人物脚下软阴影：卡通 shader 不接收实时阴影，没有它人物像飘在地面上。
// 一块平放的径向渐变半透明圆片，挂在人物根节点下随行。
shader_type spatial;
render_mode unshaded, blend_mix, depth_draw_never, cull_disabled;

uniform vec3 color : source_color = vec3(0.16, 0.18, 0.30);
uniform float strength : hint_range(0.0, 1.0) = 0.45;

void fragment() {
	float d = length(UV - vec2(0.5)) * 2.0;
	ALBEDO = color;
	ALPHA = strength * (1.0 - smoothstep(0.35, 1.0, d));
}
