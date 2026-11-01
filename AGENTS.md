# 项目说明

city_life_simulator — 概率行为树驱动的第一人称生活模拟游戏（轻松基调，参考经典《模拟人生》）。

## 文档规则
- 设计稿、计划文档等一律放在 Obsidian：`/home/dzj/Documents/Obsidian Vault/city_life_simulator/`，不要放在本项目目录内。
- 当前文档：《游戏整体构思.md》（v0.9，设计）、《技术选型与架构.md》（v1.2，Rust 无界面核心 + LOD 分层 + 确定性模拟）。
- 文档语言规范：通俗语言描述，专业概念用标准术语，不用自造词、网络用语和比喻。

## 技术决策（已定）
- 模拟核心：Rust，无界面纯逻辑库，固定时间步长（1 拍 = 1 模拟分钟），每代理独立随机数子流。
- 行为树：自写运行时，JSON 定义热重载，结构共享、每代理权重表分离；内容全数据驱动（行为注册表 + 子树复用）。
- 存档：append-only 事件日志（JSONL，含周期性状态快照）+ bincode 快照。
- 调试方案：原生运行写日志 + 日志回放查看器（静态网页读 JSONL），不用 WASM。
- 表现：2.5D 街区视角，Godot 为首选客户端候选（M3 竖切验证 godot-rust）；时间速度档 暂停/×1/×2/×10/×100（正常 1 现实秒 = 1 模拟分钟）由表现层控制。
- 项目代码目录只放代码。

## 代码结构（M6 二阶段完成）
- `core/`（crate living-core）：`scenario.rs` 目标关卡（条件谓词/进度/结算/开局注入）、`diary.rs` 日记/年度总结/你的故事，其余同 M5：`rng.rs` 确定性随机数、`time.rs` 时间（小时窗）、`needs.rs` 需求、`config.rs` 数据配置与校验（多物种树、子树、职业 JobDef、区域含照明、时间窗事件、变体 VariantDef、地点菜单、M4/M5/M6 参数组）、`tree.rs` 树可用性评估（需求门 + 时间门 + 子树引用 + 职业门 + 伴侣门）、`memory.rs` 记忆表（桶索引泛化检索 + 缓存 + valence + 间接记忆（目睹/传闻）+ 人物字段与人物印象 + 暴露恢复 + 怀旧消退）、`space.rs` 空间哈希网格（含 remove）、`relation.rs` 关系表（含安全感来源名单 + 恋人 couple 表）、`area.rs` 地点状态（声誉 EMA + 整洁度）、`agent.rs` 代理（物种、意志力、诚实度、钱包、职业、位置、LOD 层级、目标、年龄与可塑性、新奇需求、社交目标与新鲜度、变体权重表、父母链接、离场拍、模仿来源）、`goal.rs` 限期目标（戒断：压制/忍住/失守/反弹；支持变体级目标）、`news.rs` 新闻生成、`world.rs` 调度与决策核心（事件半径/区域/时间窗、目睹与传闻传播（含人物扩散）、惊吓、亲密增长含远程社交与恋人加成、安全感、地点声誉反馈、LOD 分层 T0–T2 与按天批量结算、习惯自动化定价与权重上限、恐惧加价、暴露结算、密友查看、目标对抗、职业分配与概率转行（小偷黏性、在岗冷静期）、经济结算（价格/日薪/生活费/资源门槛/童年免单）、犯罪结算（转移/被抓/社区服务/声誉冲击）、巡逻与清洁效果、社交对象选择（亲密×新鲜×可及×人物印象，结仇绝交）与寻人移动、恋爱状态机（概率成对、日界分手与生育、新生儿继承父母权重）、新奇需求（相对权重比、恐惧优先）与行为模仿（只种没做过的事）、童年实时模仿（孩子跟父母做同样的事，优先级低于生理反射，门把照常把关）、人口退出 depart_daily（年迈离场/迁出：离场者退网格、腾岗位、断恋人，名册与记忆保留；人口上限只数在场者）、新闻摘要）、`log.rs` JSONL 日志（v8：Depart 事件、Decision.imitated）、`save.rs` bincode 快照存档（v8；Agent 字段禁用 skip_serializing_if——bincode 位置格式会错位）。
- `cli/`（crate living-cli）：`living-cli` 批量运行（`--humans/--cats` 混合物种，`--save/--load`，结尾小镇快报、玩家日记与年度总结、统计含离场）与 `--bench` 基准（含 LOD 分层对比）；`living-play` 终端交互调试器（M4：习惯面板、习惯报告、目标子菜单、密友查看、恐惧/自动化标记；M5 二阶段：日记菜单、恋人显示、口味级目标候选；M6：离场居民🧳标记）。
- `godot_adapter/`（crate godot_adapter）+ `godot/`：M3 表现竖切 + M7 一阶段 3D 小镇。GDExtension（godot-rust 0.5，API 4.7）：区域按声誉着色、MultiMesh 批量渲染代理、⏸/×1/×2/×10/×100 速度档、事件播报；M6：点选检查器（离场者显示"已离开小镇"）。`scenes/town3d.tscn` + `scripts/town3d.gd`：3D 同构小镇客户端（正交 31° 斜俯视锁旋转；`assets3d/*.glb` 按 Ground/Area_*/Wires 分文件，由 Blender 同构场景导出；卡通外观由 `shaders/toon_*.gdshader` 按材质名携带的参数重建——flat|#色 / fx|#墙|#窗|#门楣|楼高 / stripe|轴|周期|占空|#on|#off（立面默认纯色模式：窗户/店面图案关，`--windows` 开回；GLB 内烘焙贴图保留供外部查看器）；代理 MultiMesh3D 渲染（视觉平滑：核心位置跳变在表现层指数趋近），坐标换算 g2w：游戏(x,y)→Godot(x-28,h,18-y)；相机=正交注视点锚定，拖拽/WASD 平移、滚轮锚定缩放、Q/E 90° 步进旋转（太阳方向随视角转，四朝向不背光）；截图自检 `-- --screenshot-after=秒` 出 logs/godot_town3d.png）。构建：`scripts/build_godot.sh`；资产再生成：`blender -b -P ~/Documents/blender_scenes/build_town_sync.py && TOWN_TEX_STAGE=1 blender -b -P ~/Documents/blender_scenes/export_town_gltf.py && python3 ~/Documents/blender_scenes/gen_export_textures.py && blender -b -P ~/Documents/blender_scenes/export_town_gltf.py` 后 `godot --headless --import`（两段式导出：先收集贴图清单，系统 Python 生成烘焙贴图，再导出——GLB 自带窗户/店面/标线贴图与重写过的米制 UV，外部查看器开箱即彩色；`--no-toon` 运行参数=纯 GLB 外观视角）。
- `assets/`：`cats.json`（单房间猫群）、`street.json`（一条街：六区域、六种吃法、子树化人类树；M6 二阶段回填：写字间职员职业 + 上班挂职业门 + 老龄离场参数）、`town.json`（一座镇：56×36 十一区域（公园居中、四路环绕、店面/写字间/派出所/住宅围绕；三条新路 road_e/road_s/road_n 为纯加法区域、无行为引用）、9 职业（含小偷）、暗巷与派出所、吃饭变体与菜单、约会行为与奶茶摊、经济治安与恋爱/出生/流行参数；M6 调平：工资/物价/威慑重平衡；M6 二阶段：童年模仿 0.5、年迈离场 300 天/0.0025；区域数据与 Blender 同构场景生成脚本共用——`~/Documents/blender_scenes/build_town_sync.py` 读本文件出图）、`scenarios/`（关卡：公园复苏/好好过日子/人间烟火）。调参数值改这里，不改代码。内容新旧梯度：新机制接线最全的是 town.json，street.json 是小体量竖切（无恋爱生育/经济/犯罪），cats.json 是 M0 猫群。
- `viewer/`：日志回放查看器（需求/意志力/钱包曲线、决策因素分解、街道动态含目标/暴露/案发/职业/恋爱/新生儿/离场🧳、亲近关系、地点声誉曲线、LOD 层级、习惯自动化标记），静态网页零依赖。
- `scripts/`：`build_godot.sh` 开发构建；`package.sh` 发布打包（release → `dist/city_life_simulator-<日期>/`：Godot 工程 + assets + viewer + bin + 运行说明；CLI/Godot 双冒烟；`--archive` 出 tar.gz）。
- 进度：M0–M6 完成（M6 二阶段：童年实时模仿、人口退出、打包脚本、试玩验收指引；street.json 职业回填）。测试 113 个全过。M6 真人试玩验收待执行（《M6 试玩验收指引》：连续 3 位非开发者玩家通过判定即关闭）；后续候选：情绪气泡与夸张动作（美术）、挑战星级/关卡编辑器。
