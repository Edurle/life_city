# city_life_simulator

概率行为树驱动的第一人称生活模拟游戏《活城》。设计文档在 Obsidian：`Obsidian Vault/city_life_simulator/`。

## 构建（M0：一群猫）

```bash
cargo build --workspace
cargo test --workspace
```

## 运行

```bash
# 模拟 6 只猫 7 天，写日志并用命令行打印行为分布
cargo run -p living-cli -- --seed 42 --cats 6 --days 7 --assets assets/cats.json --out logs/demo.jsonl

# 性能校准（决策耗时、模拟一年耗时）
cargo run -p living-cli --release -- --bench
```

## 查看日志

浏览器打开 `viewer/index.html`，拖入 `logs/*.jsonl`：时间轴回放、需求曲线、权重变化、记忆表、决策因素分解。

## 工程结构

- `core/` 模拟核心（纯 Rust 库，无界面）
- `cli/` 命令行运行器与基准
- `assets/` 数据驱动的内容（行为、树、事件）
- `viewer/` 日志回放查看器（静态网页）
