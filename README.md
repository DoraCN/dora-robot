# DoraRobot

**DoraRobot** is a cross-platform teleoperation data collection platform built on Rust and [DORA](https://dora-rs.ai). It enables real-time leader-follower teleoperation, multimodal recording (proprioception + multi-camera video), and produces [LeRobot](https://github.com/huggingface/lerobot) v3-compatible datasets for imitation learning.

---

## Features

- **Real-time teleoperation** — Leader arm drives follower arm via zenoh pub/sub over LAN (<50ms latency)
- **Multimodal recording** — Joint states (action/observation) + multi-camera video at 30 FPS
- **LeRobot v3 output** — Datasets directly loadable by LeRobot for training
- **Web console** — Browser-based control panel with real-time status (SSE), anti-misoperation button states
- **System services** — Follower and leader daemons run as OS services (launchd / systemd / Task Scheduler), auto-start on boot, auto-restart on crash
- **Multi-arm ready** — Arm type is a config parameter; currently supports SO-101 (Feetech STS3215); UR5 and other robot drivers planned
- **Cross-platform** — macOS, Linux, Windows

---

## Architecture

```
Leader Machine                          Follower Machine
┌──────────────────────────┐            ┌────────────────────────────────────────┐
│ leader-daemon            │            │ follower-daemon (OS service)           │
│  ├─ USB arm driver       │  ═ zenoh ═ │  ├─ USB arm driver                     │
│  ├─ Web console (:8080)  │            │  ├─ State machine (Idle→Ready→Rec)     │
│  └─ Control + Command pub│            │  └─ DORA dataflow (on torque enable)   │
└──────────────────────────┘            │     ├─ capture (zenoh→Arrow bridge)    │
                                        │     ├─ camera_front / camera_wrist     │
                                        │     └─ recorder (→ LeRobot v3 dataset) │
                                        └────────────────────────────────────────┘
```

**Communication**: zenoh peer mode (auto-discovery over LAN).  
**Recording**: DORA dataflow on follower machine — capture node bridges zenoh ↔ Arrow, camera nodes acquire on tick, recorder writes LeRobot v3.  
**Control path and recording path are decoupled** — recording failure never impacts teleoperation.

---

## Quick Start

See [docs/getting-started.md](docs/getting-started.md) for the complete from-scratch guide covering all three operating systems.

### Prerequisites

- Rust ≥1.88, Python 3.12+, uv, [DORA CLI](https://github.com/dora-rs/dora) 1.0.0-rc1
- Two robotic arms (currently SO-101) connected via USB
- Two cameras (optional, Logitech C920 recommended)

### One-Command Setup

```bash
# Linux
./scripts/setup-linux.sh         # 以普通用户运行，需要提权时会提示输入密码

# macOS
./scripts/setup-macos.sh         # 以普通用户运行

# Windows
.\scripts\setup-windows.ps1      # 以普通用户运行 PowerShell
```

The setup script will:
1. Scan USB devices
2. Let you select leader and follower arms
3. Generate configuration files
4. Build the project
5. Register OS services (auto-start on boot)
6. Start the daemons

### Manual Setup

```bash
# Build
cargo build --release
cargo build -p tr-capture --release
mkdir -p bin
cp target/release/follower target/release/leader target/release/tr-capture bin/

# Configure (edit VID/PID/Serial from `cargo run -p tr-so101 --example usb_scan`)
#   config/follower.toml
#   config/leader.toml

# Run follower daemon
./bin/follower --config config/follower.toml

# Run leader daemon + web console
./bin/leader --config config/leader.toml
# → Open http://localhost:8080
```

---

## Operation

### Web Console

| State | Available Actions |
|---|---|
| **待机** (Idle) | ⚡ 使能 |
| **就绪** (Ready) | ⏻ 失能, ▶ 开始采集 |
| **采集中** (Recording) | ⏻ 失能, ✅ 成功保存, 🔄 重录, ⏹ 停止采集 |

> Buttons are automatically enabled/disabled based on FSM state — no misoperation possible.

### Keyboard (alternative)

```
o → 使能    x → 失能
s → 开始采集  f → 成功保存
r → 重录      q → 停止
```

### Session Flow

```
⚡ 使能 → 搬动主臂 → ▶ 开始采集 → 执行任务 → ✅ 成功保存
                                              → 🔄 重录（不满意）
                                              → 开始下一个 episode ...
         → ⏻ 失能（结束 session）
```

Each torque-on session creates a timestamped dataset directory:
```
datasets/2026-07-02/14-30-00/
  data/     — joint action & observation (parquet)
  meta/     — dataset metadata (info.json)
  videos/   — camera recordings (mp4)
```

---

## Project Structure

```
dora-robot/
├── bin/                     ← Compiled binaries (gitignored)
├── config/                  ← Arm configuration (follower.toml, leader.toml)
├── crates/
│   ├── tr-messages/         ← Canonical message contract (std-only)
│   ├── tr-codec/            ← postcard codec implementation
│   ├── tr-transport/        ← Transport trait (QoS, framing)
│   ├── tr-transport-zenoh/  ← Zenoh transport implementation
│   ├── tr-so101/            ← SO-101 hardware driver + resolver + examples
│   ├── tr-daemon/           ← Daemon library (state machine, DORA lifecycle, web)
│   └── tr-capture/          ← DORA capture node (zenoh → Arrow bridge)
├── dataflows/               ← DORA dataflow YAML definitions
├── training/                ← Python: recorder, camera node, LeRobot writer
├── scripts/                 ← Auto-setup scripts (Linux/macOS/Windows)
├── docs/                    ← Design documents and specs
│   ├── getting-started.md
│   ├── service-setup.md
│   └── specs/               ← SDD specifications
└── constitution.md          ← Project-wide constraints
```

---

## Adding a New Robot Type

1. Implement `TeleopDevice` and `RobotDriver` traits for the new arm
2. Create a new crate under `crates/tr-<name>/`
3. Add a config section `[arm.<name>]` with arm-specific parameters
4. Set `type = "<name>"` in `config/follower.toml`

See `crates/tr-so101/` and `config/follower.toml` as reference.

---

## Documentation

| Document | Description |
|---|---|
| [Getting Started](docs/getting-started.md) | Full setup guide (all OS) |
| [Service Setup](docs/service-setup.md) | Register as OS service (auto-start) |
| [Architecture](docs/architecture.md) | Three-tier decoupled architecture |
| [USB Resolver](docs/usb-resolver-integration.md) | Persistent USB device identification |
| [Service Design](docs/service-based-teleop-console-design.md) | Daemon service + web console design |
| [Camera Integration](docs/camera-integration-design.md) | Camera capture pipeline |
| [constitution.md](constitution.md) | Project-wide design constraints |

---

## License

Apache 2.0
