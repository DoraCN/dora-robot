# plan.md — feature `001` 技术方案（HOW）

- **追溯**：实现 `spec.md`(Rev 4) 的 **M1–M5, M8–M11 / AC1–AC5, AC8–AC11**（落盘 M6/M7/AC6/7/12 归 lerobot、非本项目）；遵守 `constitution.md`(C1–C9)。
- **复用既有设计文档**（不重复，只引用）：
  - 架构与 SO-101 主从：`docs/so101-teleoperation-design.md`
  - v3 数据格式（源码核实）：`docs/lerobot-dataset-v3-format.md`
  - 录制/编码性能：`docs/recording-video-encoding-performance.md`
- Status：**Draft Rev 5（落盘相关验收/任务移出：删 M6/M7/AC6/AC7/AC9-落盘/AC12，集成作业缩为控制侧；待人审核）** — SDD §2。

---

## 1. 架构总览

一个 SO-101 **类型**、两个独立**实例**（leader/follower，各自串口+标定）；zenoh 连接两侧**独立 DORA dataflow**；录制为**旁路** Python 节点。本期**无摄像头**（spec Non-Goal）。

**操作者在操作机**（搬动主臂 + 通过 `operator-control` 标记回合）；**录制在机器人机**（本地已有 action/state）。回合事件（start/end{outcome}）经**专用 episode 通道（Reliable）**跨链路送达机器人侧录制器。

```
操作机 (dataflow A)                              机器人机 (dataflow B)
┌───────────────────────────┐                   ┌─────────────────────────────────┐
│ teleop-node: So101Leader   │                   │ bridge: ZenohTransport           │
│   100Hz read → JointTargets│   ──control──►    │   sub control / episode          │
│ operator-control           │  (BestEffort)     │     │cmd          │episode       │
│   start/end{success|fail}  │   ──episode──►    │     ▼             ▼              │
│ bridge: ZenohTransport     │  (Reliable)       │ robot-node: So101Follower(上电)  │
│   pub control / episode    │                   │   限位+slew → sync_write_goals    │
│   sub feedback         ◄───┼── feedback ───────│   └Arrow(action/observation_state)│→ recorder
└───────────────────────────┘                   │                    episode_end ◄──┘  success:save
   leader SO-101 (卸力)                          └─────────────────────────────────┘  fail:discard
```

> 注：`feedback` 回传为 **M3 双边**预留；本期 M1 不接消费者（图示为目标拓扑）。

## 2. 技术选型（HOW；钉定见宪法 C8）

| 关注点 | 选型 | 说明 |
|---|---|---|
| 舵机总线 | **feetech-servo-sdk 0.3.0**（async/tokio, `MotorBus`） | 已核实：`FeetechBus::new`/`enable_torque`/`disable_torque`/`sync_read_positions`→`Vec<f32>`(rad)/`sync_write_goals`/`ControlOp::Position(rad)` |
| 臂间传输 | **zenoh 1.9.0**（实现 `tr_transport::Transport`） | key expr 映射 channel；确切 API 以 1.9 为准 |
| 线编解码 | **postcard**（serde），实现于**独立 crate `tr-codec`**（P1/K3） | `tr-messages` 仅留 `Codec` **trait**、保持 std-only |
| 数据落盘 | **lerobot v3 writer**（**lerobot 的功能，非本项目**） | 本项目只把数据**传给** lerobot（经 `EpisodeWriter`）；不改 lerobot；落盘/编码/v3 由 lerobot 负责 |
| 控制率 / 数据集率 | 控制 **100 Hz**；数据集 **fps=30** | 录制器以 30Hz 节拍从 100Hz 流降采样（P3） |

## 3. 模块划分（新增/改动）

- **新增 `crates/tr-so101`**：`arm.rs`(`So101Arm<B: MotorBus>`) / `leader.rs` / `follower.rs` / `config.rs`（端口/ids/限位/标定偏置）。feature `mock`→`MockBus`。
- **新增 `crates/tr-codec`（P1）**：postcard 实现 `tr_messages::Codec`（command/feedback/**episode 事件**编解码）。`tr-messages` 不动、仅留 trait。
- **新增 `crates/tr-transport-zenoh`**：`ZenohTransport: tr_transport::Transport`。
- **`tr-transport` 增 `LoopbackTransport`（进程内/同机，P5）**：供**快门禁端到端测试**用，不引 zenoh。
- **改 `tr-transport` 通道**：新增 `Channel::Episode`（Reliable），承载回合事件。
- **接入 `dora-node-api`（P4，根基性前置）**：把 `tr-teleop-node`/`tr-robot-node`/`tr-bridge` 由桩接到 DORA 的 **Arrow I/O**；`tr-robot-node` 输出 **Arrow `action`/`observation_state`**（`feedback` 回传随**双边 M3** 预留，本期 M1 不建）。
- **节点选择**：teleop-node 选 `So101Leader`、robot-node 选 `So101Follower`、bridge 选 `ZenohTransport`（env/feature，宪法 C5）。
- **新增 `operator-control` 节点（P2）**：跑在**操作机**；发 `start` / `end{outcome ∈ success|fail|rerecord}` 到 `Channel::Episode`。
- **新增 dataflows**：`so101_leader.yml`（操作机：teleop-node + operator-control + bridge）/ `so101_follower.yml`（机器人机：bridge + robot-node + recorder）。
- **改 `training/tr_lerobot/recorder.py`**：`episode_end{success}`→保存；`{fail|rerecord}`→丢弃；停止→收尾；`task` 非空注入；**无相机时按 30Hz 节拍取样**（P3）。
- **录制器把 lerobot writer 抽成可注入接口 `EpisodeWriter`**（`add_frame/save_episode/discard/finalize`，Q1）：生产实现包真 lerobot（`save_episode`/`clear_episode_buffer`/`finalize`）；测试注入 **spy**。把"我们的交接逻辑"与"lerobot 落盘"在代码层解耦，使交接逻辑**无 torch 可测**。

依赖方向严格遵守宪法 **C3**。

## 4. 关键接口（详签名见 `so101-teleoperation-design.md` §5/§9）

- `So101Arm<B>`: `read_joints()->[f32;6]` / `write_joints(&[f32;6])` / `set_torque(bool)`（弧度，关节序 1..6）。
- `So101Leader: TeleopDevice`（卸力，poll→`JointTargets`）；`So101Follower: RobotDriver`（上电，command→写舵机）。
- `Transport`（`send`/`recv`/`link_state`）：实现 `ZenohTransport`、`LoopbackTransport`。
- `tr-codec`：`Codec` 的 postcard 实现（command / feedback / **EpisodeEvent**）。
- **`EpisodeEvent`**（新增于 `tr-messages`）：`Start` / `End { outcome: Success|Fail|Rerecord }`。
- **`EpisodeWriter`**（录制器内的数据交接接口，Q1）：`add_frame` / `save_episode` / `discard` / `finalize`；生产实现包真 lerobot，测试注入 spy。
- 录制器 DORA 输入：`action` f32[6] / `observation_state` f32[6] / `episode_end{outcome}`（来自 episode 通道）/ `task`（配置）。

## 5. 控制律与时序（满足 M1–M5, M10）

- **leader**：`set_torque(false)`；100Hz：`sync_read_positions`→`JointTargets`(f32 rad)→发 control。
- **follower**：`set_torque(true)`；收 `JointTargets`→**限位钳制 + 单拍 slew ≤3°/拍(M10)**→`sync_write_goals(Position)`。
- **防弹射(M4)**：上线先把从臂驱到**首个收到的主臂位姿**，再开环路。
- **看门狗(M5)**（tr-session）：control 流间隔 > 200ms→安全态（保持位姿）；`T_drop=2s` 未恢复→卸力；**不重放过期指令**。
- **急停(M5)**：本地即时 `set_torque(false)`/保持，可靠通道、最高优先级。
- **async↔sync 桥**：`So101Arm` 跑 tokio 舵机任务 + 最新值 cell/通道，节点 `poll/command` 不阻塞总线。

## 6. 录制数据转发路径（本项目**只传数据** → lerobot 落盘；满足 M8/M9/M11；遵守 C1/C6 旁路只丢不堵）

- `robot-node` 除 `feedback`（canonical, 走 zenoh）外，**另发 Arrow** `action`/`observation_state`（与 canonical 分开，宪法 C3）。
- 录制器（**独立 Python 进程**）订阅 `action`/`observation_state`；**以 30Hz 节拍取样**最新值成帧（无相机，故用固定 fps 时钟，P3）；经 `EpisodeWriter.add_frame({...,"task":...})` 交给 lerobot。
- 回合事件由 operator-control 经 **episode 通道**送达：机器人侧 **bridge 解码 `EpisodeEvent`(tr-codec) 后作为 DORA 消息转给录制器**（保录制器 codec-free，Q2）；录制器据此 `end{success}`→`EpisodeWriter.save_episode`；`{fail|rerecord}`→`EpisodeWriter.discard`；停止→`EpisodeWriter.finalize`。
- **责任边界（本项目只传数据）**：本项目只负责「**获取数据 + 把数据/回合决策传给 `EpisodeWriter`（lerobot 的入口）**」。**落盘/编码/v3 写入是 lerobot 的功能，不属本项目**——不实现、不负责、不测、**不做加载校验**。
- **故障隔离(M11)**：录制独立进程、DORA 边 best-effort，崩溃/变慢不影响控制环。

## 7. 指标 → 设计手段 / 验证 映射（可追溯）

| M | 设计手段 | 验证 |
|---|---|---|
| M1 跟随精度 | 直接关节拷贝 + 标定共零（无 IK） | AC1（真机） |
| M2 时延 | 100Hz + zenoh LAN + 轻量 postcard | AC2（真机, RTT/2） |
| M3 频率 | tokio 舵机任务 100Hz, sync 批量读写 | AC3（真机日志） |
| M4 防弹射 | 上线对齐 | AC4（快 CI 可测） |
| M5 安全态 | 看门狗 + 急停 + 保持/卸力 | AC5（快 CI 可测） |
| M8 录制无侵入 | 录制独立进程/旁路 | AC8（集成作业） |
| M9 数据交付（传给 lerobot） | 按约定 features/30Hz 交付 + success:save/fail:discard **调用**（`EpisodeWriter`） | AC9（快门禁 spy）；落盘归 lerobot、非本项目 |
| M10 单拍上限 | follower slew 钳制 | AC10（快 CI 可测） |
| M11 故障隔离 | 录制独立进程 + best-effort 边 | AC11（集成作业：杀录制进程） |

## 8. 标定 / bring-up（spec 前置条件）

会话前用 SDK examples：`scan`→`set_id`(ID 1..6)→`set_mid`(置零)→`to_zero`(回零)，使主从同关节语义一致。

## 9. 测试策略（→ AC；按**责任边界**分层，Q1）

> 边界：本项目**只传数据**——测「**获取 + 下发 + 把数据交给 lerobot 的决策**」；**落盘是 lerobot 的功能、不属本项目、不测、不做加载校验**（快门禁 mock 其 writer）。见 §6 责任边界。

- **单测**：`tr-codec` roundtrip；`So101Arm`(MockBus) 读写/torque；follower 限位+slew+e_stop；session 看门狗（已有）。
- **快门禁（真不引 torch/zenoh）**
  - (a) **Rust 进程内 loopback**：So101Leader(Mock)→`LoopbackTransport`→So101Follower(Mock)，验证 获取+控制+`action`/`observation_state` 发射 + episode 事件处理 ⇒ **AC4 / AC5 / AC10** + 数据正确性。
  - (b) **录制器适配器单测**：注入 **spy `EpisodeWriter`**，断言 success→`save_episode`、fail/rerecord→`discard`、30Hz 降采样、帧字段映射 ⇒ **AC9（数据交付 + 调用决策）**，零 torch。
- **集成作业（on-demand/nightly，真 zenoh + 真 DORA 多进程 + 录制进程在跑）**：仅测**控制侧**——录制开/关对照（**AC8**）；杀录制进程控制环不受影响（**AC11**）。**不验数据集、不验落盘**（那是 lerobot 域）。
- **真机（Validate）**：M1 / M2 / M3 实测，按 spec 阈值；可微调。

> 注：M2 仅在真机/真网测；Mock/Loopback 不评时延。

## 10. 关键实现步骤（里程碑，细节归 `tasks.md`）

0. **接入 `dora-node-api`**：节点桩→Arrow I/O（根基前置，P4）。
1. `EpisodeEvent` 类型 + `tr-codec`(postcard) + roundtrip 单测（P1）。
2. `tr-transport`：`Channel::Episode` + `LoopbackTransport`（P5）。
3. `tr-so101`（`So101Arm`<MockBus> + Leader/Follower）+ async↔sync 桥 + 单测。
4. session 防弹射对齐 + 看门狗安全态接线。
5. `operator-control`（操作机）+ episode 通道贯通（P2）。
6. 录制器 success/fail/discard + **30Hz 取样**（P3）。
7. 节点接线（so101/zenoh，env/feature）+ dataflows（leader/follower）。
8. **快门禁**（真不引 torch/zenoh）：(a) Rust loopback 控制端到端（AC4/AC5/AC10）；(b) 录制器适配器 spy 单测（AC9 决策 + 30Hz + 字段映射）。
9. `tr-transport-zenoh` + **集成作业**（真 zenoh + 真 DORA 多进程 + 录制进程在跑）：**AC8/AC11（控制侧）**；不验落盘。
10. 真机标定 + M1–M3 实测（Validate）。

## 11. 风险 / 待确认

- **zenoh 1.9.0 确切 API**（`open`/`declare_publisher`/`declare_subscriber`/`put`/`recv`/`Sample`）— 实现前核对。
- **feetech 0.3.0**：`disable_torque`/`sync_read_positions` 已核实；本期不需要 load/current（无双边）。
- **dora-node-api** 的 Rust Arrow 收发 API 细节（步骤 0 核对）。
- **async↔sync 桥**实现（cell/通道 vs async trait）。
- episode 通道的可靠送达与录制器侧的回合对齐（start 与首帧、end 与末帧的时序）。
- （遗留 Minor）P6：robot-node 每拍多发 2 路 Arrow 的开销须纳入 100Hz 预算，M3 测时验证。
