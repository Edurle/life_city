#!/usr/bin/env bash
# 运行时资产同步：外部资产库（~/city_life_assets/runtime）→ 工程运行时目录。
# VRM 模型与 VRMA 动画不进 git（.gitignore），正本住外部库；克隆代码后：
#   scripts/sync_assets.sh && godot --headless --import   # .import 参数已在 git，import 只生成 .godot 产物
# （anims/*.vrma 不走 import，拷回即用；ch46.glb 兜底小人留在 git，未同步也能跑。）
# 不用 --delete：工程目录里还有 .import 等跟踪文件；外部库删文件后手动清理对应工程文件即可。
# 资产库备份：rsync 到移动盘/NAS/云盘（不走 git）。
set -euo pipefail
cd "$(dirname "$0")/.."

ROOT="${1:-$HOME/city_life_assets}"

# 映射表：外部库相对路径 → 工程内目标目录
declare -A MAP=(
	["runtime/characters"]="godot/assets3d/characters"
	["runtime/anims"]="godot/assets3d/characters/anims"
)

if [[ ! -d "$ROOT/runtime" ]]; then
	echo "缺少 $ROOT/runtime（外部资产库未同步？清单见 assets/资产清单.csv）" >&2
	exit 1
fi

for src in "${!MAP[@]}"; do
	dst="${MAP[$src]}"
	if [[ ! -d "$ROOT/$src" ]]; then
		echo "跳过：$ROOT/$src 不存在" >&2
		continue
	fi
	mkdir -p "$dst"
	rsync -a "$ROOT/$src/" "$dst/"
	echo "已同步 $src → $dst（$(ls "$ROOT/$src" | wc -l) 个文件）"
done