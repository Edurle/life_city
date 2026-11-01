// 两段式卡通 + 程序化立面：楼层横带窗、底层店面（玻璃+彩色门楣）、屋顶压暗。
// 参数由材质名（fx|#墙|#窗|#门楣|楼高）注入；网格顶点即建模时的物体局部坐标，
// 窗户图案用局部坐标取模，零贴图零 UV。unshaded + 手动太阳方向（跨渲染器一致）。
shader_type spatial;
render_mode unshaded;

uniform vec3 wall_color : source_color = vec3(0.9);
uniform vec3 win_color : source_color = vec3(0.35, 0.45, 0.55);
uniform vec3 accent_color : source_color = vec3(0.8);
uniform vec3 glass_color : source_color = vec3(0.43, 0.53, 0.63);
uniform vec3 roof_color : source_color = vec3(0.45, 0.44, 0.42);
uniform float bldg_h = 6.0;
// 区域声誉染色（M7 旁观二阶段）：整楼 wash（墙/窗/屋顶统一压色），rep_amt = 0 时无感。
uniform vec3 rep_color : source_color = vec3(1.0);
uniform float rep_amt : hint_range(0.0, 1.0) = 0.0;
// 纯色模式（默认）：窗户/店面图案全关，楼 = 墙色 + 深屋顶。
// 游戏成型后上真贴图前，可先用 --windows 开回程序化细节。
uniform bool windows_on = false;
uniform vec3 shadow_tint : source_color = vec3(0.42, 0.46, 0.74);
uniform float lit_boost = 1.15;
uniform float cut : hint_range(-1.0, 1.0) = 0.40;
uniform vec3 sun_dir = vec3(0.30, 0.85, 0.43);

void fragment() {
	vec3 p = VERTEX;
	vec3 n = normalize(mat3(INV_VIEW_MATRIX) * NORMAL);
	float zb = p.y + bldg_h * 0.5;                    // 距地高度
	float side = float(abs(n.y) < 0.55);
	float fz = mod(zb, 3.0);                          // 每层 3m
	float band = step(0.9, fz) * (1.0 - step(2.35, fz)) * step(3.0, zb);
	float wx = step(0.6, mod(p.x, 2.8)) * (1.0 - step(2.1, mod(p.x, 2.8)));
	float wz = step(0.6, mod(p.z, 2.8)) * (1.0 - step(2.1, mod(p.z, 2.8)));
	float won = float(windows_on);
	float win = band * max(wx, wz) * side * won;
	float gz = step(0.5, zb) * (1.0 - step(2.4, zb)) * side * won;
	float ground = (1.0 - step(3.0, zb)) * side * won;
	float roof = step(0.55, n.y);
	vec3 base = mix(wall_color, accent_color, ground);
	base = mix(base, glass_color, gz);
	base = mix(base, win_color, win);
	base = mix(base, roof_color, roof);
	base = mix(base, rep_color, rep_amt);
	float t = step(cut, dot(n, normalize(sun_dir)));
	ALBEDO = base * mix(shadow_tint, vec3(lit_boost), t);
}
