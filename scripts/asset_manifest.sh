#!/usr/bin/env bash
# 资产清单生成：外部资产库（~/city_life_assets）→ assets/资产清单.csv。
# 资产版本不进 git，靠清单里的 sha256 锁定可复现性；资产库更新后重跑本脚本并提交清单。
# 用法：scripts/asset_manifest.sh [资产库根目录]（默认 ~/city_life_assets）
set -euo pipefail
cd "$(dirname "$0")/.."

ROOT="${1:-$HOME/city_life_assets}"
OUT="assets/资产清单.csv"

if [[ ! -d "$ROOT/raw" ]]; then
	echo "缺少 $ROOT/raw（外部资产库未同步？）" >&2
	exit 1
fi

echo "相对路径,大小字节,sha256,类型" > "$OUT"
cd "$ROOT"
find raw runtime -type f ! -name ".*" 2>/dev/null | sort | while read -r f; do
	printf '%s,%s,%s,%s\n' \
		"$f" \
		"$(stat -c%s "$f")" \
		"$(sha256sum "$f" | cut -d' ' -f1)" \
		"${f##*.}"
done >> "$OLDPWD/$OUT"
echo "清单已生成 $OUT（$(($(wc -l < "$OLDPWD/$OUT") - 1)) 个文件）"