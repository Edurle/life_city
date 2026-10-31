#!/usr/bin/env bash
# 《活城》Godot 竖切构建：编译适配层 → 复制动态库到工程。
# 用法：scripts/build_godot.sh [--release]
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ "${1:-}" == "--release" ]]; then
    cargo build --release -p godot_adapter
    cp target/release/libgodot_adapter.so godot/bin/
else
    cargo build -p godot_adapter
    cp target/debug/libgodot_adapter.so godot/bin/
fi
echo "适配层已复制到 godot/bin/。运行："
echo "  ~/app/Godot_v4.7.2-stable_linux.x86_64 --path godot            # 有窗口"
echo "  同上 --headless --quit-after 300                                # 无头冒烟"
