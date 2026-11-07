#!/usr/bin/env bash
# 运行时资产同步：外部资产库（~/city_life_assets/runtime）→ 工程运行时目录。
# 现状（架子）：characters/*.vrm、anims/*.vrma 还留 in git（量小）；等模型超过
# ~30 个或目录超 500MB 时，把它们移入外部库 runtime/ 并在此登记映射，然后
# "克隆代码 → 跑本脚本" 即可还原运行时资产；清单用 scripts/asset_manifest.sh 生成。
# 资产库备份：rsync 到移动盘/NAS/云盘（不走 git）。
set -euo pipefail
cd "$(dirname "$0")/.."

ROOT="${1:-$HOME/city_life_assets}"

# 映射表：外部库相对路径 → 工程内目标目录（将来自行添加行）
declare -A MAP=(
	# ["runtime/characters"]="godot/assets3d/characters"
	# ["runtime/anims"]="godot/assets3d/characters/anims"
)

if [[ ${#MAP[@]} -eq 0 ]]; then
	echo "运行时资产暂留 git（characters/*.vrm、anims/*.vrma），无外部同步项。"
	echo "外部库 $ROOT 就绪状态：$([[ -d $ROOT ]] && echo 是 || echo 否)。"
	echo "迁移时：资产移入 $ROOT/runtime/ → 取消上方 MAP 注释 → 重跑本脚本。"
	exit 0
fi

for src in "${!MAP[@]}"; do
	dst="${MAP[$src]}"
	if [[ ! -d "$ROOT/$src" ]]; then
		echo "跳过：$ROOT/$src 不存在" >&2
		continue
	fi
	mkdir -p "$dst"
	rsync -a --delete "$ROOT/$src/" "$dst/"
	echo "已同步 $src → $dst"
done