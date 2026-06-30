# constitution.md — dora-robot 项目宪法

> 项目级**不可违背**的全局约束。所有 `spec.md` / `plan.md` 必须遵守；与之冲突的设计一律驳回。
> 由 SDD 规范（`.opencode/skills/spec-driven-development/SKILL.md` §5）要求设立。

---

## C1. 架构原则（三端解耦）

- **三端解耦**：摇操端(teleop) / 通讯层(comm) / 被控端(robot)，三者独立可替换。
- **唯一解耦缝** = 规范化消息契约（canonical contract，crate `tr-messages`）。换设备/换传输/换机器人，只改对应一端。
- 运行框架：**DORA 数据流**；两台机器各跑独立 DORA dataflow，由可插拔通讯中间件(bridge)连接。
- **实时控制路径** 与 **录制/学习路径** 必须解耦：录制是**旁路、只丢不堵**，绝不反压控制路径。

## C2. 目录结构约定

| 路径 | 用途 | 入库 |
|---|---|---|
| `crates/` | Rust workspace（实时 + 契约代码） | ✓ |
| `training/` | Python 子项目（lerobot 封装：录制/校验/训练），隔离 venv | ✓ |
| `docs/` | 设计文档；`docs/specs/<id>/` = SDD 规格(spec/plan/tasks) | ✓ |
| `dataflows/` | DORA 数据流 yaml | ✓ |
| `.leon/` | 第三方下载源码（lerobot 等）—**只读、禁改、禁入库** | ✗ |
| `target/` `datasets/` `checkpoints/` `__pycache__/` `.opencode/` | 产物/缓存/工具 | ✗ |

## C3. 模块边界（依赖方向单向，禁止成环）

```
tr-messages        ← 不依赖任何 tier（纯契约 + 标准库）
tr-transport       ← 不依赖 tr-messages（只搬字节 + QoS/分帧）
tr-session         → tr-messages
tr-teleop          → tr-messages
tr-robot           → tr-messages
tr-so101           → feetech-servo-sdk + tr-messages + tr-teleop + tr-robot
tr-transport-zenoh → zenoh + tr-transport
tr-codec           → tr-messages + serde/postcard（编解码**实现**；隔离重依赖，使 tr-messages 保持 std-only）
节点(bin)          → 各 lib，按配置选实现
录制器(Python)     → 只消费 DORA Arrow，**不依赖 tr-messages/Codec**，不感知 v3 内部结构
```

- 控制路径(Rust) 与 录制路径(Python) **仅通过 DORA Arrow** 交互。

## C4. 数据与互通约定

- 数据集格式与"好坏数据"处理**一律遵循 lerobot 约定**（v3；失败即丢弃），不自造。
- **禁止修改 lerobot 源码**（`.leon/lerobot-main` 只读），只通过其公开 API 使用。
- canonical 特征命名遵循 lerobot：`action` / `observation.state` / `observation.images.<cam>`。
- 单位 **SI（弧度）**；SO-101 关节序固定 **ID 1..6**（底座/肩/肘/腕旋/腕弯/夹爪）。
- 跨语言：同机数据流走 **DORA Arrow**；跨机走通讯层（codec 序列化 canonical）。

## C5. 命名规则

- Rust crate 前缀 `tr-`（teleop-robot）。
- 角色适配器：`<Hw>Leader`(impl `TeleopDevice`) / `<Hw>Follower`(impl `RobotDriver`)。
- DORA 输出 id：`action` / `observation_state` / `observation_images_<cam>` / `feedback` / `command`。
- 环境变量前缀：`TR_*`（本项目）、`LEROBOT_*`（数据集配置）。

## C6. 安全约束

- **实时控制环优先级最高**（调度优先级/绑核）；录制进程降级。
- 录制**只丢不堵**：录制慢/崩不得影响控制环。
- 从臂：写前必做**限位钳制 + 单拍 slew 钳制**；上线先做**防弹射对齐**。
- 急停**本地即时**生效、最高优先级、可靠通道；掉线进入**安全态**，**绝不重放过期指令**。
- **禁止提交**：密钥、构建产物、第三方源码、数据集/权重（见 `.gitignore`）；日志不得输出敏感信息。

## C7. 测试要求

- 每个交付物须**可独立验证**：核心逻辑单测；硬件相关用 **Mock（MockBus）** 做无硬件验证。
- 主从链路必须有 **MockBus 端到端 CI**，不依赖真机。
- CI 门禁：`cargo build --workspace`、`cargo test --workspace`、Python `py_compile`/lint。**落盘/数据集加载校验属 lerobot 域，非本项目门禁。**
- 真机相关指标在 **Validate** 阶段实测，不阻断 CI。

## C8. 依赖与可编译性

- 契约核心（`tr-messages`/`tr-transport`/`tr-session`/`tr-teleop`/`tr-robot`）保持 **std-only、离线可编**。
- **`Codec` 实现独立成 crate `tr-codec`**（引 serde/postcard）；`tr-messages` **仅含 `Codec` trait**，不得在其中引入 serde 等重依赖。
- 仅硬件/网络 crate（tokio / zenoh / feetech-servo-sdk）引重依赖，**按 feature 门控**。
- 版本钉定：`zenoh=1.9.0`、`feetech-servo-sdk=0.3.0`；lerobot 按 `training/requirements.txt` 钉定。

## C9. SDD 流程纪律

- 所有功能走 **Specify→Plan→Implement→Validate**；**变更先改 spec 再改代码**。
- `plan.md`/`tasks.md` 必须可**追溯到 spec 的 M/AC**；**Validate（自动化测试 + 人工 Review）不可省略**。
- **留痕**：spec 含修订记录、评审发现编号留存。
