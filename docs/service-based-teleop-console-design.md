# 操作台服务化 — 技术方案

> 从臂守护进程常驻、主臂启动 web 控制台（React）、浏览器一键操控。
> 配置文件在项目目录、Python 录制器走项目内 venv。

---

## 1. 从臂状态机（商业级前置条件）

```
系统启动 ──→ IDLE（扭矩 OFF，默认状态）
              │ [上力]
              ▼
           READY（扭矩 ON，可驱臂）
              │ [开始采集]
              ▼
           RECORDING（扭矩 ON，录制中）
              │ [成功保存]/[丢弃] → READY
              │ [重录] → RECORDING
              │ [停止采集] → READY
              │
           [卸力]（任意状态）→ IDLE
```

### 按钮可用性

| 按钮 | IDLE | READY | RECORDING |
|---|---|---|---|
| 上力 | ✅ | 灰显 | 灰显 |
| 卸力 | 灰显 | ✅ | ✅ |
| 开始采集 | 灰显 | ✅ | 灰显 |
| 成功保存 | 灰显 | 灰显 | ✅ |
| 丢弃 | 灰显 | 灰显 | ✅ |
| 重录 | 灰显 | 灰显 | ✅ |
| 停止采集 | 灰显 | 灰显 | ✅ |

---

## 2. 配置文件

统一放在项目目录 `config/` 下：

```
dora-robot/
  config/
    follower.toml     # 从臂侧
    leader.toml       # 主臂侧
```

```toml
# config/follower.toml — 从臂
[arm]
id = "arm_1"                # 臂对实例标识，必须和主臂一致
type = "so101"              # 硬件型号

[arm.so101]
baud = 1_000_000
ids = [1, 2, 3, 4, 5, 6]
vid = "0x0483"              # STMicroelectronics (STM32 CDC)
pid = "0x5740"              # STM32 Virtual COM Port
serial = "5A7A0555021"      # 生产环境必填，精确匹配

# config/leader.toml — 主臂
[arm]
id = "arm_1"                # 必须和从臂一致
type = "so101"

[arm.so101]
baud = 1_000_000
ids = [1, 2, 3, 4, 5, 6]
vid = "0x0483"
pid = "0x5740"
serial = "5AB01836201"

[console]
bind = "0.0.0.0:8080"       # web 控制台监听地址
```

`arm.id` 区分不同臂对（隔离 zenoh 通道）；`arm.type` 指定硬件型号，程序据此读取 `[arm.<type>]` 段来初始化硬件和解析 USB 路径（详见 `docs/usb-resolver-integration.md`）。

硬件参数（`baud`, `ids`, `vid`, `pid`, `serial`）只和臂型号相关，所有同型号臂配置相同。换型号时整体替换该段。

优先级：**CLI `--config` > 环境变量 `TR_ARM_ID` > `config/follower.toml`**。

---

## 3. Web 控制台

### 技术栈

| 层 | 选型 |
|---|---|
| 后端 | `axum`（Rust，和主臂 daemon 同进程） |
| 实时状态 | **SSE**（Server-Sent Events，1Hz 推送从臂状态） |
| 指令下发 | HTTP POST `/api/command` |
| 前端 | **React**（独立构建，产物作为静态文件内嵌到 Rust 二进制） |

### 构建流程

```
dora-robot/web/         ← React 项目（Vite）
  ├── src/App.tsx
  ├── package.json
  └── ...

npm run build  →  web/dist/  →  axum 内嵌为静态资源
```

### 页面布局

```
┌─────────────────────────────────────────────────────┐
│  SO-101 Teleop Console                    ● ONLINE  │
│  Arm: arm_1                                        │
├─────────────────────────────────────────────────────┤
│  [ ⚡ 上力 ]  [ ⏻ 卸力 ]                            │
│  ── 录制 ─────────────────────────────────────────  │
│  [ ▶ 开始采集 ]                                     │
│  [ ✅ 成功保存 ]  [ ❌ 丢弃 ]  [ 🔄 重录 ]          │
│  [ ⏹ 停止采集 ]                                     │
├─────────────────────────────────────────────────────┤
│  扭矩: ON  │  录制: ● REC (ep#2, 342f)  │  FPS: 43 │
│  主臂: OK  │  从臂: OK                   │  err: - │
└─────────────────────────────────────────────────────┘
```

### 后端 API

| 端点 | 方法 | 作用 |
|---|---|---|
| `/` | GET | React 页面 |
| `/api/status` | GET (SSE) | 实时推送从臂状态（JSON，1Hz） |
| `/api/command` | POST `{"cmd":"TorqueOn"}` | 下发控制指令 |

---

## 4. Python 录制器环境

**不做二进制打包。** lerobot 依赖 PyTorch（~2GB），跨平台打包不实际。

方案：**录制器跑在项目内 venv**，和当前 `training/.venv` 一致。

```
dora-robot/
  training/.venv/     ← uv 创建，含 lerobot + torch
```

从臂 daemon 拉起录制器时直接指定 venv 的 Python：

```rust
Command::new("training/.venv/bin/python")
    .arg("-m").arg("tr_lerobot.pipe_recorder")
    .arg("--task").arg(task)
    .stdin(pipe)      // stdin 接 daemon 管道，收帧数据
    .spawn()
```

> 部署时 venv 由运维预建一次（`cd training && uv sync`），之后 daemon 直接用。

---

## 5. Arm ID 管理

zenoh 通道隔离不同臂对：`tr/<id>/control`、`tr/<id>/command`、`tr/<id>/status`。

**ID 需提前设定**——主从两端 ID 必须一致。来源：配置文件 (§2) > 环境变量 > 默认 `arm_1`。

示例：部署两对臂

```
# 臂对1：主臂机器 config/leader.toml       arm.id = "arm_1"
#        从臂机器 config/follower.toml     arm.id = "arm_1"

# 臂对2：主臂机器 config/leader.toml       arm.id = "arm_2"
#        从臂机器 config/follower.toml     arm.id = "arm_2"
```

---

## 6. 系统架构

```
┌── 机器 A（主臂 + Web 控制台）───────────────┐
│  leader-daemon + axum (同进程)               │
│   ├─ 接主臂 FeetechBus, 读关节 → pub control│
│   ├─ sub 从臂状态 → SSE 推给浏览器           │
│   ├─ POST /api/command → pub command         │
│   └─ :8080 提供 React 页面 + SSE + API       │
└──────────────────────────────────────────────┘
                      ││
          ════════ zenoh ════════
                      ││
┌── 机器 B（从臂守护进程）────────────────────┐
│  follower-daemon（开机自启）                  │
│   ├─ 接从臂 FeetechBus                       │
│   ├─ sub control (关节数据) → 驱臂            │
│   ├─ sub command → 状态机处理                 │
│   ├─ pub status (1Hz, JSON)                  │
│   └─ RecordStart → spawn 录制器子进程         │
└──────────────────────────────────────────────┘
```

### zenoh 通道

| key | 方向 | 内容 |
|---|---|---|
| `tr/<id>/control` | 主→从 | postcard `JointTargets` |
| `tr/<id>/command` | 主→从 | `ControlCommand`（上力/卸力/录制/回合） |
| `tr/<id>/status` | 从→主 | JSON（扭矩/录制动/fps/错误） |
| `tr/<id>/episode` | 主→从 | `EpisodeEvent`（已有，保留） |

---

## 7. 从臂掉线自动重连

### 7.1 Bus 断开（读写超时/权限丢失）

从臂 daemon 检测到 FeetechBus 断开：

1. 状态机 → IDLE，通知操作台 "从臂离线"（pub status）
2. 指数退避重试连接 FeetechBus（1s → 2s → 4s → 最长 30s）
3. 重连成功 → IDLE，通知操作台 "从臂在线"
4. 重连后状态机回到 IDLE（扭矩 OFF），需操作员重新 [上力]

### 7.2 USB 热插拔（最后实现）

通过 `usb_resolver::DeviceMonitor::start()` 监听设备插拔事件：

1. `DeviceEvent::Attached` → 解析新路径 → 自动打开 FeetechBus → 恢复 IDLE
2. `DeviceEvent::Detached` → 同 7.1 断线流程

> 优先级低，先做启动时冷扫描（`scan_now()`）。

---

## 8. 文件和改动

| 新增 | 说明 |
|---|---|---|
| `crates/tr-daemon/` | 守护进程库（状态机、command/status 处理） |
| `crates/tr-daemon/src/bin/follower.rs` | 从臂 daemon 入口 |
| `crates/tr-daemon/src/bin/leader.rs` | 主臂 daemon + axum web 入口 |
| `crates/tr-so101/src/resolver.rs` | USB 设备路径自动发现 |
| `crates/tr-so101/examples/usb_scan.rs` | USB 设备扫描诊断工具 |
| `web/` | React 前端（Vite 构建，输出到 `web/dist/`） |
| `config/follower.toml` `config/leader.toml` | 配置文件（含 `[arm.so101]` USB 发现规则） |
| `tr-messages` | 新增 `ControlCommand` 枚举、`Status` 结构 |
| `docs/usb-resolver-integration.md` | USB 解析器技术方案 |

| 保留不变 | 说明 |
|---|---|
| `pipe_recorder.py` | 作为子进程被 daemon 拉起，不改 |
| `training/.venv` | venv 路径，daemon 写死引用 |
| `tr-transport-zenoh` | zenoh 通道不变 |
| `tr-so101` | SO-101 硬件抽象不变 |

---

## 9. 已拍板

| # | 决策 | 结论 |
|---|---|---|
| 1 | Web 框架 | `axum`（Rust 异步，和 daemon 同进程） |
| 2 | 前端 | React → 独立构建，产物内嵌 |
| 3 | 配置位置 | 项目目录 `config/` |
| 4 | Python 环境 | 项目内 venv（`training/.venv`），不二进制打包 |
| 5 | 掉线恢复 | 自动重连 + 通知操作台 |
| 6 | React 目录 | `web/` 子目录 |
| 7 | 部署方式 | 从臂机器预装 Rust + Python venv **且** CI 出二进制 + 一键部署脚本 |
| 8 | USB 发现 | 用 `usb-resolver` 自动发现，硬件参数收归 `[arm.so101]`，不硬编码路径 |
| 9 | USB 热插拔 | 最后实现，先做启动时冷扫描 |
