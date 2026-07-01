# dora-robot — SO-101 主从遥操作 + LeRobot 采集

基于 Rust (DORA + feetech-servo-sdk) 与 zenoh 的**实时同构机械臂遥操作与数据集录制**平台。

---

## 快速开始

### 环境

- 两台机器各接一只 SO-101 机械臂（Feetech 舵机，USB）
- 同一局域网（zenoh peer 模式自动发现）
- Rust 1.95+、Python 3.12+

### 构建

```sh
cargo build --workspace
cargo test --workspace          # 所有测试（含 tr-so101 MockBus 16 测）
```

---

## 实时遥操作（主臂 → zenoh → 从臂）

两台机器，一个终端各跑一边：

**机器 A — 主臂（Leader）**
```sh
cargo run -p tr-so101 --example leader_teleop_zenoh -- /dev/cu.usbmodem5AB01836201
```

**机器 B — 从臂（Follower）**
```sh
cargo run -p tr-so101 --example zenoh_follower -- /dev/cu.usbmodem5A7A0555021
```

搬动主臂 → 从臂实时跟随。

> **多对隔离**：不同臂对用不同 key 区分——`--key tr/arm_1` 和 `--key tr/arm_2` 互不干扰。

---

## 录制（遥操 + 键盘控制 → LeRobot v3）

### 一次配置

```sh
cd training
uv python install 3.12
uv venv --python 3.12
source .venv/bin/activate
uv sync                           # 从本地 .leon/lerobot-main 安装 lerobot
```

### 录制会话

**机器 B — 从臂 + 录制（先启动）**
```sh
PYTHONPATH=training \
  cargo run -p tr-so101 --example zenoh_follower -- /dev/cu.usbmodem5A7A0555021 --record \
  | training/.venv/bin/python -m tr_lerobot.pipe_recorder --task "grab cube"
```

**机器 A — 主臂（后启动，在**此终端**用键盘控制回合）**
```sh
cargo run -p tr-so101 --example leader_teleop_zenoh -- /dev/cu.usbmodem5AB01836201
```

### 键盘控制（主臂终端）

| 键 | 功能 | 说明 |
|---|---|---|
| `s` | **Start** | 开始一个新回合（进入 RECORDING 状态） |
| `Enter` | **Success** | 结束当前回合并**保存**（回到 IDLE） |
| `f` | **Fail** | 结束当前回合并**丢弃**（回到 IDLE） |
| `r` | **Rerecord** | 丢弃当前回合，**立即开始重录**（留在 RECORDING） |
| `q` | **Quit** | 停止主臂 → 从臂卸力退出 → 录制 `finalize()` |

**流程示例**：
```
s → 搬臂做任务 → Enter（保存回合1）
s → 搬臂做任务 → f（放弃回合2）
s → 搬臂做任务 → r（不满意，重录→继续搬臂→ Enter（保存回合3）
q（结束）
```

### 验证录制结果

```sh
cd training && source .venv/bin/activate
python -c "
from lerobot.datasets import LeRobotDataset
ds = LeRobotDataset('local/teleop', root='../datasets')
s = ds[0]
print('episodes:', ds.meta.info['total_episodes'])
print('frames:  ', len(ds))
print('keys:    ', list(s.keys()))
"
# > episodes: 3
# > frames:   748
# > keys:     ['action', 'observation.state', ...]
```

---

## 对比测试：TCP 直连

和 zenoh 同样的操作方式，但走原生 TCP（零额外依赖，用于对比网络延迟）：

**从臂（先启动）**
```sh
cargo run -p tr-so101 --example tcp_follower -- /dev/cu.usbmodem5A7A0555021 0.0.0.0:9000
```

**主臂（后启动，`192.168.x.x` 换成从臂 IP）**
```sh
cargo run -p tr-so101 --example leader_teleop_tcp -- /dev/cu.usbmodem5AB01836201 192.168.x.x:9000
```

---

## 诊断工具

### 纯读主臂关节（不连网）
```sh
cargo run -p tr-so101 --example leader_diag -- /dev/cu.usbmodem5AB01836201
```

### 主臂录制到 CSV
```sh
cargo run -p tr-so101 --example leader_teleop_debug -- /dev/cu.usbmodem5AB01836201 --output logs/my.csv
```

### CSV 回放到从臂（离线测试）
```sh
cargo run -p tr-so101 --example follower_replay -- /dev/cu.usbmodem5A7A0555021 logs/my.csv
```

---

## 项目结构

| 路径 | 用途 |
|---|---|
| `crates/tr-messages` | 规范化消息契约（`TeleopCommand`/`RobotFeedback`/`EpisodeEvent`）+ `Codec` trait |
| `crates/tr-codec` | postcard 编解码实现（command/feedback/episode） |
| `crates/tr-transport` | `Transport` trait + QoS/分帧 + TCP/UDP/Loopback |
| `crates/tr-transport-zenoh` | zenoh 1.9 `Transport` 实现（pub/sub） |
| `crates/tr-session` | 会话生命周期 / 能力协商 / 看门狗 |
| `crates/tr-teleop` | `TeleopDevice` trait |
| `crates/tr-robot` | `RobotDriver` trait + `SimRobot` |
| `crates/tr-so101` | SO-101 硬件抽象（`So101Arm`/`So101Leader`/`So101Follower`）+ 16 个 MockBus 单测 |
| `tr-so101/examples/` | 所有可执行工具（遥操 / 录制 / 回放 / 诊断） |
| `training/` | Python 子项目：录制器 / 校验 / 训练 |

## 文档

| 文档 | 内容 |
|---|---|
| `docs/architecture.md` | 三端解耦总架构 |
| `docs/so101-teleoperation-design.md` | SO-101 主从遥操技术方案 |
| `docs/lerobot-dataset-v3-format.md` | LeRobot v3 真实格式（源码核实） |
| `docs/recording-video-encoding-performance.md` | 视频编码性能设计 |
| `docs/dora-node-api-spike.md` | dora-node-api 0.5.0 API 记录 |
| `docs/specs/001-so101-teleop-record/` | SDD 规格（spec / plan / tasks） |
| `constitution.md` | 项目宪法（全局约束） |
