# spec.md — 系统守护与服务化（feature `002`）

- Status: **Draft 6 / 待 Owner 审核**
- Owner: （待填）
- 规范: `.opencode/skills/spec-driven-development/SKILL.md`
- 信条: Spec 是唯一真实来源；人定义 WHAT，AI 实现 HOW。本文件**只写 WHAT/WHY**，HOW 留给 `plan.md`。

---

## 0. 决策层级自检（SDD §0）

- 运行期逻辑：daemon（读配置 → 扫描 USB → 状态机 → 驱臂 → 管 DORA dataflow 生命周期）+ DORA dataflow（录制时启动，桥接+录制）。全链路**确定性控制逻辑**，属「代码层」。
- 不需要 AI/Agent 介入运行期。
- 结论：SDD 在此作为**开发期规范**。

---

## 1. Problem Statement（WHY）

当前 `001` 已实现 SO-101 主从遥操作与录制能力，但存在三个工程化缺口：

1. **启动方式**：所有功能通过开发者终端命令启动（`cargo run -p tr-so101 --example ...`），非开发者无法使用。
2. **容错性**：程序崩溃、设备掉线后需手动重启，无自动恢复能力。
3. **架构不合规**：`001` 的录制走 stdout pipe（临时方案）；按照项目设计，从臂在录制时应运行 DORA dataflow（桥接 → 录制），后续接入 camera 节点（`004`）。此架构尚未工程化落地。

系统处在"原型可用"阶段，**缺乏工程化运行能力**。

---

## 2. User Stories（谁在什么场景下用）

- 作为**操作者**，我插上 SO-101 臂的 USB 线、打开电源后，系统**自动识别并准备好**，无需碰任何终端。
- 作为**操作者**，我不需要知道串口路径、命令行参数、编译命令——只需要插电就能用。
- 作为**操作者**，如果设备意外掉线（USB 松动、总线报错），系统会**自动重试恢复**并通知我状态（无需我手动重启程序）。
- 作为**操作者**，上力/卸力/开始录制/结束录制这些操作，通过**一个统一的指令接口**完成（`002` 期内仍可以是终端交互或 CLI；图形界面是 `003`）。
- 作为**运维者**，我能把 daemon 配置为**开机自启**（systemd/launchd），断电重启后无需人工介入。

---

## 3. Success Metrics（每条均可测试）

| # | 指标 | 判定（可测试） |
|---|---|---|
| M1 | 从臂启动即用 | 插 USB + 上电 → 运行 `follower-daemon`（不传 `--config`）→ **10 秒内** USB 发现完成、从臂进入 IDLE、status 频道有推送 |
| M2 | 自动发现 | 更换 USB 端口后重启 daemon，**不修改任何配置**，系统仍能正确识别并连上同一支臂（serial 匹配） |
| M3 | 掉线恢复 | 拔掉从臂 USB → status 推送 `offline`；10 秒内插回 → **30 秒内**自动恢复至 IDLE（无需手动重启 daemon） |
| M4 | 录制启动/停止 | 下发"开始录制"指令 → follower-daemon **拉起 DORA dataflow**，dataflow 内录制节点接收数据；下发"结束录制" → DORA dataflow **正常终止**，无残留进程 |
| M5 | daemon 故障隔离 | 以下任一异常发生：follower-daemon **不崩溃**，状态机进入 IDLE（扭矩 OFF），可恢复：(a) FeetechBus 读写报错；(b) zenoh 连接断开；(c) DORA dataflow 崩溃 |
| M6 | 安全默认 | daemon 启动后从臂扭矩为 **OFF**，需显式[上力]指令后才进入 READY |
| M7 | 多臂对隔离 | 同一台机器上跑两个 follower-daemon（arm_1 / arm_2），各自独立，互不干扰 |
| M8 | 配置热加载 | 修改配置文件 → 重启 daemon → 新配置生效，**无需重新编译** |
| M9 | 数据走 DORA | 录制期间，跟随动作和关节状态通过 DORA Arrow 在 dataflow 内传递给录制节点（符合 constitution C1/C3） |
| M10 | 主臂启动即用 | 运行 `leader-daemon`（不传 `--config`）→ **10 秒内** USB 发现完成、开始 pub control + command |
| M11 | daemon 持续存活 | follower-daemon 连续运行 ≥ 1 小时，期间经历 ≥ 3 次异常事件（总线报错/zenoh 断连/录制崩溃各一次）→ **进程不退出**，每次异常后状态机回到 IDLE 或 READY |

---

## 4. Acceptance Criteria（怎么验证）

- **AC1 ↔ M1**：启动 follower-daemon → 10 秒内 status 显示 "IDLE"。
- **AC2 ↔ M2**：(a) 根据 serial 匹配到正确设备；(b) 换 USB 端口后不改配置仍能匹配。
- **AC3 ↔ M3**：拔从臂 USB → status 推送 `offline`；插回 → 30 秒内 status 回到 `IDLE`。
- **AC4 ↔ M4**：(a) 发送录制指令 → DORA dataflow 启动，spy 确认录制节点接收数据；(b) 发送结束指令 → dataflow 终止、无残留进程。
- **AC5 ↔ M5**：依次触发三类异常：(a) 拔从臂 USB（总线报错）→ daemon 不崩、状态机回 IDLE；(b) 关闭 zenoh router（断连）→ daemon 不崩、状态机回 IDLE；(c) 录制中 `kill -9` DORA dataflow → daemon 不崩、状态机回 READY。
- **AC6 ↔ M6**：启动后确认各舵机扭矩为 OFF。
- **AC7 ↔ M7**：同一机器上启动两个 daemon 实例（不同 `--config`）→ 各自独立运行，zenoh 通道不串扰。
- **AC8 ↔ M8**：改 `config/follower.toml` 中的 baud 值 → 重启 daemon → 以新 baud 连接生效。
- **AC9 ↔ M9**：录制期间 spy DORA 输出，验证 action 和 observation.state 通过 Arrow 到达录制节点。
- **AC10 ↔ M10**：启动 leader-daemon → 10 秒内开始 pub control/command 到 zenoh。
- **AC11 ↔ M11**：follower-daemon 持续运行 ≥ 1 小时，依次注入总线报错→zenoh 断连→录制崩溃各一次，每次恢复后进程存活且状态机停在 IDLE 或 READY。

---

## 5. Non-Goals（本期明确**不做**，防止自由发挥）

- **Graphical UI / Web 控制台**（属 `003`）。
- **摄像头录制**（属 `004`）。
- **USB 热插拔自动恢复**（`002` 只做启动时冷扫描；热插拔监控留到最后实现）。
- 远程配置推送、在线升级、健康监控大盘。
- 配置文件 GUI 编辑器。
- 从臂的 AI 策略推理（只做遥操作 + 录制）。
- 多台从臂机器协同（daemon 只管本机的臂）。
- `001` 的控制面指标（M4 防弹射/M5 安全态/M10 slew 等）不重复验收——`002` 工程化外壳沿用 `001` 已验证的控制逻辑。

---

## 6. Constraints & 前置条件（外部约束，**非**实现方案）

- 硬件：SO-101 臂（6 DoF，Feetech STS3215），通过 STM32 CDC ACM 虚拟串口连接。
- 主臂和从臂分处**两台独立机器**，通过**局域网（LAN）**通信。
- 配置格式：`[arm]` + `[arm.so101]`（已定稿，见 `docs/usb-resolver-integration.md`）。
- 配置文件路径：项目目录 `config/`；daemon 默认读对应文件，可通过 CLI `--config` 覆盖。
- arm.id 区分不同臂对，zenoh 通道隔离。
- **架构分工**：
  - **主臂机器**：`leader-daemon` 读取主臂关节 → zenoh pub（control + command）；不运行 DORA。
  - **从臂机器**：`follower-daemon` 是宿主进程（USB 发现、状态机、zenoh sub 驱臂）。**录制时** daemon 拉起 DORA dataflow（bridge 收 zenoh → recorder 写 v3）。DORA dataflow **仅在录制期间运行**，录制结束后关闭。
- **follower-daemon 不可崩**：它是主从控制的唯一通道，必须在任意异常（总线报错、zenoh 断连、DORA dataflow 崩溃）下**自身不退出**。异常发生后状态机进入 IDLE（扭矩 OFF），待恢复。
- **录制器运行环境**：项目内 venv（`training/.venv`），含 lerobot + torch。**从臂机器需安装 DORA 运行时**。
- 须符合项目 `constitution.md` 的模块边界约束。
- `tr-teleop`/`tr-robot` trait 契约不变；`tr-messages` 协议不变（追加 `ControlCommand`/`Status` 不影响已有消息）。

---

## 7. 可观测性需求（本期最小化）

- 至少能看到：当前状态（IDLE/READY/RECORDING/OFFLINE）、扭矩 ON/OFF、是否在录制、DORA dataflow 运行状态、最后错误信息。
- 不做 Prometheus/Grafana 大盘。

---

## 8. 粒度检验（SDD §4 黄金法则）

- 本 spec 的指标与验收均以**可观测行为**表述（启动时间、掉线恢复时间、dataflow 进程状态、扭矩状态）。
- 具体技术（zenoh / DORA API / tokio / feetech-servo-sdk / 状态机实现细节）**不出现在本文**，归 `plan.md`。
- 唯一出现的外部技术名是 **DORA**：因录制 dataflow 是 constitution C1 要求的基础架构，属 Constraint。**具体启动方式、节点编排方式不出现在本文**，归 `plan.md`。
- 检验：换传输层（不用 zenoh）、换舵机 SDK、换操作系统 → 本 spec **指标仍然成立** → 纯 WHAT。

---

## 9. 评审与修订记录（留痕）

- **Draft 6**：审核修复——M5 扩展为三类异常（总线/zenoh/DORA）；新增 M11（daemon 持续存活）；§6 新增显式约束"follower-daemon 不可崩"；AC5/AC11 对应更新。
- **Draft 5**：审核修复——M1 10s、M10 10s；DORA dataflow 按需启动；M4/M5 措辞修正；Non-Goals 声明不重复验收 001 控制面。
- **Draft 4**：重写 §6 架构分工——follower-daemon 为宿主进程（不直接驱臂），DORA dataflow 为数据面（bridge→robot→recorder）。
- **Draft 3**：M1 30s→15s、M3 60s→30s，M7 保留。
- **Draft 2**：新增 M9（录制走 DORA），确定方案 C。
- **Draft 1**：初稿。
