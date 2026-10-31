# city_life_simulator

概率行为树驱动的第一人称生活模拟游戏《活城》。设计文档在 Obsidian：`Obsidian Vault/city_life_simulator/`。

## 构建（M3：一条街——人类角色、街道地图、昼夜、地点声誉）

```bash
cargo build --workspace
cargo test --workspace
```

## 运行

```bash
# 一条街：8 名人类居民 + 4 只猫同街生活 14 天（地点声誉、昼夜节律）
cargo run -p living-cli -- --assets assets/street.json --seed 42 --humans 8 --cats 4 --days 14 --out logs/street.jsonl

# 单房间猫群（M2 场景）：6 只猫 7 天
cargo run -p living-cli -- --seed 42 --cats 6 --days 7 --assets assets/cats.json --out logs/demo.jsonl

# 中途存档 / 读档继续
cargo run -p living-cli -- --seed 42 --cats 6 --days 3 --save logs/state.bin
cargo run -p living-cli -- --load logs/state.bin --days 4 --out logs/demo2.jsonl

# 交互调试器（玩家的猫与猫群：输数字指挥，实时反馈；退出自动存档，--load 续玩）
cargo run -p living-cli --bin living-play -- --seed 42
cargo run -p living-cli --bin living-play -- --load logs/play_state.bin

# 性能校准（决策耗时、模拟一年耗时）
cargo run -p living-cli --release -- --bench
```

## 查看日志

浏览器打开 `viewer/index.html`，拖入 `logs/*.jsonl`：时间轴回放、需求曲线、权重变化、记忆表（含目睹所学）、猫群动态（事件/惊吓/亲密变化）、亲近关系、地点声誉曲线、决策因素分解（含安全感）。

## Godot 客户端（M3 表现竖切）

```bash
scripts/build_godot.sh                      # 编译适配层并复制到 godot/bin/
~/app/Godot_v4.7.2-stable_linux.x86_64 --path godot   # 打开街道视图
#   窗口内：⏸/×1/×2/×10/×100 速度档，区域按声誉着色，代理实时移动
# 自动截图（可视验证）：-- --screenshot-after=6
```

架构：`core/`（纯 Rust 模拟）+ `godot_adapter/`（GDExtension 适配层，只做翻译）+ `godot/`（场景与脚本）。godot-rust 0.5（API 4.7）绑定层验证通过。

## 工程结构

- `core/` 模拟核心（纯 Rust 库，无界面）
- `cli/` 命令行运行器与基准
- `godot_adapter/` + `godot/` Godot 客户端（GDExtension 竖切）
- `scripts/` 构建脚本
- `assets/` 数据驱动的内容（cats.json 单房间 / street.json 一条街）
- `viewer/` 日志回放查看器（静态网页）
