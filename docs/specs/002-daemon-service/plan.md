# plan.md — feature `002` 技术方案（HOW）

- **追溯**：实现 `spec.md`(Draft 6) 的 M1–M11 / AC1–AC11；遵守 `constitution.md`(C1–C9)。
- **复用既有文档**：
  - 服务化设计：`docs/service-based-teleop-console-design.md`
  - USB 发现：`docs/usb-resolver-integration.md`
- Status：**Draft 4 / 待 Owner 审核**

---

## 1. 架构总览

```
操作机 (机器 A)                              机器人机 (机器 B)
┌────────────────────────────┐               ┌──────────────────────────────────┐
│ leader-daemon              │               │ follower-daemon                  │
│   USB 发现 → So101Arm.init │  ═══ zenoh ═══ │   USB 发现 → So101Arm.init      │
│   poll() → JointTargets    │── control ──→  │   sub control → 驱臂            │
│   (键盘交互) → ControlCmd  │── command ──→  │   sub command → 状态机           │
│   sub status ◄─────────────│── status ────  │← pub status (1Hz JSON)          │
│                             │               │                                  │
│   (主臂 SO-101, 卸力)      │               │ ┌── DORA dataflow (上力时) ────┐ │
└────────────────────────────┘               │ │ capture  →  recorder          │ │
                                             │ │ sub zenoh → Arrow            │ │
                                             │ │ action + observation.state    │ │
                                             │ │        → lerobot v3           │ │
                                             │ └──────────────────────────────┘ │
                                             │   (从臂 SO-101, 驱臂)            │
                                             └──────────────────────────────────┘
```

**关键关系**：

- daemon 和 DORA dataflow **是同级组件**，不嵌套。各自独立从 zenoh 接收数据，互不干扰。
- **关节数据直接走 zenoh**，不经 daemon 中转。
- DORA dataflow 随 **TorqueOn 启动、TorqueOff 停止**——覆盖整个活跃期（READY + RECORDING）。
- dataflow 内只含 `capture → recorder`，无 robot-node（robot-node 属 `004` 策略推理）。

---

## 2. 技术选型

| 关注点 | 选型 | 说明 |
|---|---|---|
| 运行时 | **独立二进制** + tokio | 不依赖 DORA 做控制面 |
| 配置 | **toml** crate | 读 `config/follower.toml` / `leader.toml` |
| USB 发现 | **usb-resolver 0.1.1** | `scan_now()` 冷扫描；热插拔最后实现 |
| 跨机通信 | **zenoh 1.9.0**（复用 `ZenohTransport` MPSC） | 已验证 |
| 线编解码 | **postcard**（复用 `tr-codec`） | JointTargets / ControlCommand / Vec<f32>（observation） |
| 状态推送 | **JSON**（`serde_json`） | `tr/<id>/status`，1Hz，人可读 |
| DORA | 系统预装，`dataflows/record.yml`，daemon 用 CLI 管启停 | — |

---

## 3. zenoh 通道

| key | 方向 | 编码 | 用途 |
|---|---|---|---|
| `tr/<id>/control` | leader → {follower-daemon, DORA capture} | postcard `JointTargets` | 遥操作关节指令、录制 action |
| `tr/<id>/command` | leader → {follower-daemon, DORA capture} | postcard `ControlCommand` | 状态机指令 + episode 边界转发 |
| `tr/<id>/observation` | follower-daemon → DORA capture | postcard `Vec<f32>` | 从臂实际关节位置（DOF 动态），录制 observation.state |
| `tr/<id>/status` | follower-daemon → leader-daemon | JSON `DaemonStatus` | 1Hz 守护进程状态推送 |
| `tr/<id>/episode` | leader → follower | postcard `EpisodeEvent` | M1 遗留，002 保留不维护 |

---

## 4. 模块划分

### 4.1 新增 `crates/tr-daemon/`

```
crates/tr-daemon/
├── Cargo.toml
├── src/
│   ├── lib.rs            # pub mod state, config, dora
│   ├── state.rs          # ArmState + Fsm
│   ├── config.rs         # DaemonConfig (TOML)
│   └── dora.rs           # DoraFlow: launch/stop dataflow
└── src/bin/
    ├── follower.rs
    └── leader.rs
```

依赖：`tr-so101`, `tr-messages = { features = ["serde"] }`, `tr-codec`, `tr-transport-zenoh`, `tokio`, `toml`, `serde_json`, `usb-resolver`

### 4.2 新增 `crates/tr-so101/src/resolver.rs`

`resolve_arm_port(config) → Result<String>`（详见 `docs/usb-resolver-integration.md`）。

### 4.3 改 `crates/tr-messages/`

新增 `src/control.rs`：

```rust
pub enum ControlCommand {
    TorqueOn,                        // 上力 → 启动 DORA dataflow + 使能扭矩
    TorqueOff,                       // 卸力 → 停止 DORA dataflow + 失能扭矩
    StartRecord { task: String },    // 开始录制 episode
    EndRecord { outcome: EpisodeOutcome }, // 结束当前 episode
    ReRecord,                        // 丢弃当前 episode，立即开始新的
    Stop,                            // 停止录制，回到 READY
}

pub struct DaemonStatus {
    pub state: String,              // IDLE | READY | RECORDING | OFFLINE
    pub torque_on: bool,
    pub recording: bool,
    pub episode: Option<u32>,
    pub frame_count: u64,
    pub fps: f32,
    pub error: Option<String>,
}
```

`EpisodeOutcome` 已有（`tr_messages::episode`）。

> `tr-messages` 的 serde derive 在 `serde` feature gate 后。daemon 和 capture crate 需要 `tr-messages = { features = ["serde"] }`。

### 4.4 新增 DORA capture 节点（`crates/tr-capture/`）

```
crates/tr-capture/
├── Cargo.toml
└── src/main.rs
```

zenoh → DORA Arrow 桥接节点：

```
输入 (zenoh): tr/<id>/control     → decode JointTargets → Arrow Float32Array action
              tr/<id>/observation → decode Vec<f32>     → Arrow Float32Array observation_state
              tr/<id>/command     → decode ControlCommand → episode 边界转发给 recorder
           
输出 (DORA): action (Arrow), observation_state (Arrow), episode_cmd (Arrow 或 meta)
```

capture 同时 sub `tr/<id>/command` 通道，用于接收 episode 边界指令（StartRecord/EndRecord/ReRecord）并转发给 recorder。

依赖：`tr-messages = { features = ["serde"] }`, `tr-codec`, `tr-transport-zenoh`, `dora-node-api`, `arrow`, `tokio`

### 4.5 改为 dataflows

`dataflows/record.yml`：

```yaml
nodes:
  - id: capture
    path: target/release/tr-capture
    outputs:
      - action
      - observation_state
      - episode_cmd          # StartRecord/EndRecord/ReRecord → recorder
    env:
      TR_ARM_ID: "${TR_ARM_ID}"
      TR_ZENOH_MODE: "peer"

  - id: recorder
    path: python
    args: -m tr_lerobot.recorder
    inputs:
      action: capture/action
      observation_state: capture/observation_state
      episode_cmd: capture/episode_cmd
    env:
      PYTHONPATH: "training"
      LEROBOT_REPO_ID: "local/teleop"
      LEROBOT_ROOT: "./datasets"
      LEROBOT_FPS: "30"
      LEROBOT_TASK: "${TR_TASK}"
```

### 4.6 新增 `crates/tr-so101/examples/usb_scan.rs`

诊断工具：扫描所有 USB 设备，打印 VID/PID/Serial 和推荐配置。

---

## 5. 状态机（`state.rs`）

```
                启动 ──→ IDLE (扭矩 OFF, 无 dataflow)
                           │ TorqueOn → 启动 DORA dataflow → 使能扭矩
                           ▼
                        READY (扭矩 ON, dataflow 运行中)
                           │ StartRecord
                           ▼
                     RECORDING (扭矩 ON, dataflow 运行中)
                           │ EndRecord / Stop → READY
                           │ ReRecord → 重置 episode → 保持 RECORDING
                          
                    任意状态 TorqueOff → 停止 dataflow → 失能扭矩 → IDLE
```

```rust
pub enum ArmState { Idle, Ready, Recording }

pub struct Fsm { state: ArmState }

impl Fsm {
    pub fn apply(&mut self, cmd: &ControlCommand) -> Result<(ArmState, Option<DataflowAction>)>;
    pub fn current(&self) -> ArmState;
}

pub enum DataflowAction { Launch, Stop, None }
```

| 指令 | 状态变更 | Dataflow 动作 |
|---|---|---|
| TorqueOn | Idle → Ready | Launch |
| TorqueOff | 任意 → Idle | Stop |
| StartRecord | Ready → Recording | None（已在运行） |
| EndRecord | Recording → Ready | None |
| ReRecord | Recording → Recording | None（recorder 内部 reset） |
| Stop | Recording → Ready | None |

---

## 6. 关键流程

### 6.1 follower-daemon 启动

1. 读 `config/follower.toml`
2. `resolver::resolve_arm_port()` → 串口路径
3. `FeetechBus::new()` → `So101Arm::new()` → `So101Follower::new()`（扭矩 OFF）
4. zenoh sub `tr/<id>/control` + `tr/<id>/command`
5. zenoh pub `tr/<id>/observation` + `tr/<id>/status`
6. 进入 Idle，pub 首次 status
7. **DORA dataflow 未启动**（Idle 状态）

### 6.2 上力（TorqueOn）

1. leader pub `ControlCommand::TorqueOn`
2. follower 收到 → 状态机 `Idle → Ready`
3. 使能从臂扭矩（`So101Follower` 开始接收并执行 control 指令）
4. 调用 `DoraFlow::launch()` 启动 `dataflows/record.yml`
5. capture 节点 sub `tr/<id>/control` + `tr/<id>/observation` + `tr/<id>/command` → Arrow → recorder
6. recorder 等待 episode 边界指令

### 6.3 录制开始（StartRecord）

1. leader pub `ControlCommand::StartRecord { task }`
2. follower → 状态机 `Ready → Recording`
3. follower 将 StartRecord **透传给 DORA dataflow**（通过 zenoh command → capture sub → recorder）
4. recorder 开始写入 lerobot v3 episode

### 6.4 重录（ReRecord）

1. leader pub `ControlCommand::ReRecord`
2. follower 透传给 capture → recorder
3. recorder 丢弃当前 episode buffer，立即开始新 episode
4. **dataflow 不重启**，状态机保持 RECORDING

### 6.5 录制结束（EndRecord / Stop）

1. leader pub `ControlCommand::EndRecord { outcome }` 或 `Stop`
2. follower → 状态机 `Recording → Ready`
3. follower 透传指令给 recorder（保存 / 丢弃 episode）
4. **dataflow 继续运行**（扭矩仍 ON，主臂仍可控制从臂）

### 6.6 卸力（TorqueOff）

1. leader pub `ControlCommand::TorqueOff`
2. follower → 状态机 `任意 → Idle`
3. 失能从臂扭矩
4. 调用 `DoraFlow::stop()` 关闭 dataflow
5. 清理残留进程

---

## 7. DORA dataflow 生命周期（`dora.rs`）

**启停时机**：`TorqueOn` → launch；`TorqueOff` → stop。dataflow 覆盖 READY + RECORDING 整个活跃期。

```rust
pub struct DoraFlow;

impl DoraFlow {
    pub fn launch(config: &DaemonConfig) -> Result<Self>;
    pub fn stop(self) -> Result<()>;
    pub fn alive(&self) -> bool;
}
```

DORA 运行时由系统预装（`dora` CLI 在 PATH），daemon 通过 `dora up/dora start/dora destroy` 管理。

---

## 8. leader-daemon 交互

002 期内沿用键盘交互（与 M1 相同 + 新增上力/卸力）：

```
o       → TorqueOn  (上力)
x       → TorqueOff (卸力)
s/Enter → StartRecord
f       → EndRecord(Success)
r       → ReRecord
q       → Stop
```

键盘事件 → `ControlCommand` → postcard → zenoh pub `tr/<id>/command`。

---

## 9. 异常处理（follower-daemon 不可崩）

| 异常 | 检测 | 处理 |
|---|---|---|
| FeetechBus 报错 | `read_joints` / `write_joints` Err | 状态机 → Idle，指数退避重试 |
| zenoh 断连 | `recv` 超时/Err | 状态机 → Idle，指数退避重连 |
| DORA dataflow 崩溃 | 子进程退出 | 状态机 → Idle（扭矩 OFF），pub status |

**所有异常路径 daemon 自身不退出。**

---

## 10. 实现顺序

| 阶段 | 内容 | 追溯 spec |
|---|---|---|
| W1 | `tr-messages` 新增 `ControlCommand` / `DaemonStatus` | — |
| W2 | `tr-so101/src/resolver.rs` + `usb_scan` 示例 | M2 |
| W3 | `tr-daemon` 骨架：config.rs + state.rs + FSM TDD | M6 |
| W4 | follower-daemon：USB 发现 → zenoh sub → 驱臂 → 上力时启动 DORA dataflow | M1, M4, M7 |
| W5 | leader-daemon：USB 发现 → zenoh pub → 键盘交互 | M10 |
| W6 | `tr-capture` DORA 节点实现（zenoh sub control/observation/command → Arrow） | — |
| W7 | `dataflows/record.yml` 集成 + episode 边界（StartRecord/EndRecord/ReRecord） | M4, M9 |
| W8 | follower-daemon 异常处理：总线/zenoh/DORA | M3, M5, M11 |
| W9 | 多臂对隔离 + 配置热加载 | M7, M8 |
