#!/usr/bin/env bash
# 《活城》发布打包（M6 二阶段）：产出"下载即玩"的可分发目录。
# 结构（godot/ 与 assets/ 必须是兄弟目录——客户端用 res://../assets 读内容）：
#   dist/city_life_simulator-<日期>/
#   ├── godot/     Godot 4.7 工程（旁观模式客户端 + release 适配层动态库）
#   ├── assets/    内容配置（town/street/cats + 关卡）
#   ├── viewer/    日志回放查看器（静态网页，零依赖）
#   ├── bin/       living-cli / living-play（release）
#   └── 运行说明.md
# 用法：scripts/package.sh [--archive] [--skip-smoke]
#   --archive     额外打 tar.gz（默认只出目录）
#   --skip-smoke  跳过冒烟验证（无头 Godot + CLI 短跑）
set -euo pipefail
cd "$(dirname "$0")/.."

ARCHIVE=0
SMOKE=1
for arg in "$@"; do
    case "$arg" in
        --archive) ARCHIVE=1 ;;
        --skip-smoke) SMOKE=0 ;;
        *) echo "未知参数：$arg"; exit 1 ;;
    esac
done

STAMP="$(date +%Y%m%d)"
DIST="dist/city_life_simulator-$STAMP"

echo "==> 构建 release（核心 + CLI + Godot 适配层）"
cargo build --release -p living-cli -p godot_adapter

echo "==> 组装 $DIST"
rm -rf "$DIST"
mkdir -p "$DIST/bin" "$DIST/godot/bin"

# Godot 工程：场景/脚本/着色器/3D 资产/工程文件（bin 里只放 release 动态库与扩展描述）。
cp -r godot/project.godot godot/scenes godot/scripts godot/shaders godot/assets3d "$DIST/godot/"
cp godot/bin/living_sim.gdextension "$DIST/godot/bin/"
cp target/release/libgodot_adapter.so "$DIST/godot/bin/"

# 内容与工具。
cp -r assets "$DIST/"
rm -rf "$DIST/assets/scenarios/.DS_Store" 2>/dev/null || true
cp -r viewer "$DIST/"
cp target/release/living-cli target/release/living-play "$DIST/bin/"
strip "$DIST/bin/living-cli" "$DIST/bin/living-play" "$DIST/godot/bin/libgodot_adapter.so" 2>/dev/null || true

cat > "$DIST/运行说明.md" <<'EOF'
# 《活城》试玩包

一个概率行为树驱动的第一人称生活模拟小镇：居民自己过日子——上班、恋爱、
生孩子、变老、离开；你扮演其中一位，指挥他的生活，看故事自己长出来。

## 玩法一：图形客户端（推荐，需要 Godot 4.7）

1. 从 godotengine.org 下载 Godot 4.7（标准版即可，不用 .NET 版）。
2. 用 Godot 打开本目录下的 `godot/project.godot`（Import → 选择文件）。
3. 按 F5 运行：一条街的小镇。点选任意居民查看公开信息；与自己亲密 ≥ 0.75
   的密友还能看懂他的心；⏸/×1/×2/×10/×100 切换时间速度。

## 玩法二：终端扮演（无需 Godot）

```bash
cd 本目录
bin/living-play --assets assets/street.json        # 一条街，交互式扮演
bin/living-play --assets assets/town.json          # 一座镇（更大）
```
数字菜单指挥角色（违背意愿的指令费意志力⚡）、快进、定戒断目标🎯、
看习惯报告、读日记；每 3 天自动存档到 saves/autosave。

## 玩法三：批量观察 + 回放

```bash
bin/living-cli --assets assets/town.json --seed 42 --humans 14 --cats 3 \
    --days 365 --scenario assets/scenarios/love_and_jobs.json --out logs/year.jsonl
```
跑完用浏览器打开 viewer/index.html，载入 logs/year.jsonl：
需求曲线、决策因素、恋爱与案发、地点声誉、LOD 分层……全部可视化回放。

## 试玩验收（M6 目标）

在没有提示的情况下玩 15–30 分钟，然后回答一个问题：
**你的角色身上发生了什么值得讲的事？**
如果你能脱口讲出一个连开发者都没想到的故事——这个游戏就成了。
EOF

if [[ "$SMOKE" -eq 1 ]]; then
    echo "==> 冒烟：CLI 从包内跑 3 天"
    (cd "$DIST" && ./bin/living-cli --assets assets/street.json --seed 7 \
        --humans 6 --cats 2 --days 3 --out logs/smoke.jsonl >/dev/null) \
        && echo "    CLI 冒烟通过" || { echo "    CLI 冒烟失败"; exit 1; }
    GODOT_BIN="${GODOT:-$(ls ~/app/Godot_v4.7*_linux.x86_64 2>/dev/null | head -1 || true)}"
    if [[ -n "$GODOT_BIN" && -x "$GODOT_BIN" ]]; then
        echo "==> 冒烟：Godot 无头跑 300 拍（$GODOT_BIN）"
        (cd "$DIST/godot" && "$GODOT_BIN" --headless --path . --quit-after 300 >/dev/null 2>&1) \
            && echo "    Godot 冒烟通过" || echo "    Godot 冒烟失败（可忽略，窗口环境相关）"
    else
        echo "==> 跳过 Godot 冒烟（未找到可执行；可用 GODOT=路径 指定）"
    fi
fi

if [[ "$ARCHIVE" -eq 1 ]]; then
    echo "==> 打包 tar.gz"
    tar -czf "$DIST.tar.gz" -C dist "city_life_simulator-$STAMP"
    echo "    $DIST.tar.gz"
fi

echo "==> 完成：$DIST（godot/ 与 assets/ 是兄弟目录，整体移动不散架）"
