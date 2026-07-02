# DoraRobot

**DoraRobot** 是一个基于 Rust 和 [DORA](https://dora-rs.ai) 的跨平台遥操作数据采集平台。支持主从臂实时遥操作、多模态数据录制（本体关节 + 多摄像头视频），产出 [LeRobot](https://github.com/huggingface/lerobot) v3 兼容的数据集，用于模仿学习训练。

---

## 特性

- **实时遥操作** — 主臂驱动从臂，通过 zenoh pub/sub 在局域网内低延迟传输（<50ms）
- **多模态录制** — 关节数据（action/observation.state）+ 多摄像头视频，30 FPS
- **LeRobot v3 输出** — 数据集可直接被 LeRobot 加载用于训练
- **Web 控制台** — 浏览器操控面板，实时状态显示（SSE），按钮按状态机自动启用/禁用，防误操作
- **系统服务化** — 主从臂 daemon 注册为操作系统服务（launchd / systemd / 任务计划程序），开机自启，崩溃自动重启
- **多机械臂支持** — 机械臂型号通过配置参数切换；当前支持 SO-101（Feetech STS3215），UR5 等驱动规划中
- **跨平台** — macOS、Linux、Windows

---

## 系统架构

```
主臂机器                               从臂机器
┌──────────────────────────┐            ┌──────────────────────────────────────┐
│ leader-daemon            │            │ follower-daemon（系统服务，开机自启）    │
│  ├─ USB 臂驱动            │  ═ zenoh ═ │  ├─ USB 臂驱动                        │
│  ├─ Web 控制台 (:8080)    │            │  ├─ 状态机（待机→就绪→采集中）           │
│  └─ 控制指令 + 关节数据     │            │  └─ DORA 数据流（使能时启动）           │
└──────────────────────────┘            │     ├─ capture（zenoh→Arrow 桥接）     │
                                        │     ├─ camera_front / camera_wrist   │
                                        │     └─ recorder（→ LeRobot v3 数据集） │
                                        └──────────────────────────────────────┘
```

**通讯层**：zenoh peer 模式（局域网自动发现）。  
**录制层**：从臂机器上运行 DORA 数据流——capture 节点桥接 zenoh ↔ Arrow，camera 节点定时采帧，recorder 节点写入 LeRobot v3。  
**控制路径与录制路径解耦**——录制失败绝不影响遥操作。

---

## 快速开始

完整的从零安装指南见 [docs/getting-started.md](docs/getting-started.md)，覆盖三种操作系统。

### 前置要求

- Rust ≥1.88、Python 3.12+、uv、[DORA CLI](https://github.com/dora-rs/dora) 1.0.0-rc1
- 两支机械臂（当前支持 SO-101），通过 USB 连接
- 两个摄像头（可选，推荐 Logitech C920）

### 一键部署

```bash
# Linux
sudo ./scripts/setup-linux.sh

# macOS
sudo ./scripts/setup-macos.sh

# Windows（PowerShell 管理员）
.\scripts\setup-windows.ps1
```

脚本自动完成：
1. 扫描 USB 设备
2. 交互式选择主臂和从臂
3. 生成配置文件
4. 编译项目
5. 注册系统服务（开机自启）
6. 启动 daemon

### 手动部署

```bash
# 编译
cargo build --release
cargo build -p tr-capture --release
mkdir -p bin
cp target/release/follower target/release/leader target/release/tr-capture bin/

# 配置（运行 usb_scan 获取 VID/PID/Serial）
#   cargo run -p tr-so101 --example usb_scan
#   编辑 config/follower.toml 和 config/leader.toml

# 启动从臂 daemon
./bin/follower --config config/follower.toml

# 启动主臂 daemon + Web 控制台
./bin/leader --config config/leader.toml
# → 浏览器打开 http://localhost:8080
```

---

## 操作方式

### Web 控制台

| 状态 | 可用按钮 |
|---|---|
| **待机** | ⚡ 使能 |
| **就绪** | ⏻ 失能, ▶ 开始采集 |
| **采集中** | ⏻ 失能, ✅ 成功保存, 🔄 重录, ⏹ 停止采集 |

> 按钮根据状态机自动启用/禁用，杜绝误操作。

### 键盘（备选）

```
o → 使能    x → 失能
s → 开始采集  f → 成功保存
r → 重录      q → 停止
```

### 采集流程

```
⚡ 使能 → 搬动主臂 → ▶ 开始采集 → 执行任务 → ✅ 成功保存
                                              → 🔄 重录（不满意，丢弃当前）
                                              → 开始下一个 episode ...
         → ⏻ 失能（结束本次 session）
```

每次使能生成一个带时间戳的独立数据集目录：

```
datasets/2026-07-02/14-30-00/
  data/     — 关节 action 与 observation.state（parquet）
  meta/     — 数据集元信息（info.json）
  videos/   — 摄像头录制视频（mp4）
```

---

## 项目结构

```
dora-robot/
├── bin/                     ← 编译产物（gitignored）
├── config/                  ← 机械臂配置（follower.toml, leader.toml）
├── crates/
│   ├── tr-messages/         ← 规范化消息契约（std-only）
│   ├── tr-codec/            ← postcard 编解码实现
│   ├── tr-transport/        ← Transport trait（QoS、分帧）
│   ├── tr-transport-zenoh/  ← Zenoh transport 实现
│   ├── tr-so101/            ← SO-101 硬件驱动 + USB 解析 + 示例
│   ├── tr-daemon/           ← daemon 库（状态机、DORA 生命周期、Web）
│   └── tr-capture/          ← DORA capture 节点（zenoh → Arrow 桥接）
├── dataflows/               ← DORA 数据流 YAML 定义
├── training/                ← Python：录制器、摄像头节点、LeRobot writer
├── scripts/                 ← 自动化部署脚本（Linux/macOS/Windows）
├── docs/                    ← 设计文档与规格
│   ├── getting-started.md
│   ├── service-setup.md
│   └── specs/               ← SDD 规格文档
└── constitution.md          ← 项目全局约束
```

---

## 扩展新机械臂

1. 为新机械臂实现 `TeleopDevice` 和 `RobotDriver` trait
2. 创建新 crate：`crates/tr-<name>/`
3. 在配置文件中添加 `[arm.<name>]` 节，定义该型号的特定参数
4. 在 `config/follower.toml` 中设置 `type = "<name>"`

参考实现：`crates/tr-so101/` 和 `config/follower.toml`。

---

## 文档索引

| 文档 | 说明 |
|---|---|
| [快速开始](docs/getting-started.md) | 全平台从零安装指南 |
| [服务部署](docs/service-setup.md) | 注册为操作系统服务（开机自启） |
| [系统架构](docs/architecture.md) | 三端解耦架构设计 |
| [USB 解析器](docs/usb-resolver-integration.md) | USB 设备持久化识别方案 |
| [服务化设计](docs/service-based-teleop-console-design.md) | daemon 服务化 + Web 控制台设计 |
| [摄像头集成](docs/camera-integration-design.md) | 摄像头采集管线设计 |
| [constitution.md](constitution.md) | 项目全局设计约束 |

---

## 许可证

Apache 2.0
