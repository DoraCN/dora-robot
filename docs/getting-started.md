# SO-101 遥操作数据采集 — 新手操作指南

> 从零开始，跑通主从臂遥操作 + DORA 数据流录制 + 摄像头采集的完整链路。

---

## 0. 前置条件

| 依赖 | 版本 | 检查命令 |
|---|---|---|
| Rust | ≥1.88 | `rustc --version` |
| DORA CLI | 1.0.0-rc1 | `which dora && dora --version` |
| Python | 3.12+ | `python3 --version` |
| uv | 最新 | `which uv` |
| maturin | ≥1.7 | `maturin --version`（仅构建 DORA Python 包时需要） |
| Git LFS | 最新 | `git lfs version`（仅克隆 dora 源码时需要） |

硬件：
- 两台 SO-101 臂（主臂/从臂），通过 USB 连接
- 2 个 Logitech C920 摄像头（可选）

---

## 1. 克隆项目 + 放置 DORA 源码

```bash
git clone https://github.com/DoraCN/dora-robot.git
cd dora-robot

# DORA 1.0-rc1 源码（gitignored，需手动放置）
git clone https://github.com/dora-rs/dora.git
cd dora && git checkout v1.0.0-rc1 && cd ..
```

---

## 2. 创建 Python 虚拟环境

```bash
# 创建 venv（gitignored）
uv venv training/.venv --python 3.12
source training/.venv/bin/activate

# 安装 Python 依赖
uv pip install numpy opencv-python pyarrow pyyaml
uv pip install lerobot  # 含 torch，约 2GB

# 安装 DORA Python 包（从本地源码构建）
cd dora
maturin build -m apis/python/node/Cargo.toml --release
uv pip install --python ../training/.venv/bin/python \
  target/wheels/dora_rs-1.0.0rc1-*.whl
cd ..
```

---

## 3. 配置机械臂

### 3.1 发现设备

```bash
# 扫描 USB 设备，获取 VID/PID/Serial
cargo run -p tr-so101 --example usb_scan
```

输出示例：

```
Device 11:
  VID   : 0x1A86
  PID   : 0x55D3
  Serial: 5A7A055502
  Dev   : /dev/cu.usbmodem5A7A0555021
```

### 3.2 编辑配置文件

`config/follower.toml`（从臂侧）和 `config/leader.toml`（主臂侧）各填一份。**两个串口号不一样**，用 `usb_scan` 输出中 `Dev` 字段的值：

```toml
# config/follower.toml
[arm]
id = "arm_1"
type = "so101"

[arm.so101]
baud = 1_000_000
ids = [1, 2, 3, 4, 5, 6]
vid = "0x1A86"           # 从 usb_scan 获取
pid = "0x55D3"           # 从 usb_scan 获取
serial = "5A7A055502"    # 从 usb_scan 获取
```

---

## 4. 编译项目

```bash
# 全项目编译
cargo build --release

# 关键二进制：
#   target/release/tr-capture   — DORA capture 节点
#   target/debug/follower       — 从臂 daemon
#   target/debug/leader         — 主臂 daemon + Web 控制台
```

---

## 5. 启动系统

### 5.1 启动从臂（终端 1）

```bash
cargo run -p tr-daemon --bin follower -- --port /dev/cu.usbmodem5A7A0555021
```

输出 `state=IDLE` → 就绪。

### 5.2 启动主臂 + Web 控制台（终端 2）

```bash
cargo run -p tr-daemon --bin leader -- --port /dev/cu.usbmodem5AB01836201
```

浏览器打开 `http://localhost:8080`。

---

## 6. 操作流程

### Web 控制台（推荐）

| 步骤 | 按钮 | 说明 |
|---|---|---|
| 1 | ⚡ 使能 | 舵机上电，DORA 数据流启动，摄像头开始采集 |
| 2 | 搬动主臂 | 从臂实时跟随 |
| 3 | ▶ 开始采集 | 开始录制 episode |
| 4 | 搬动主臂 | 执行操作任务 |
| 5 | ✅ 成功保存 | 保存当前 episode |
| 6 | ▶ 开始采集 | 开始下一个 episode（可重复 3-5） |
| 7 | 🔄 重录 | 丢弃当前 episode，立即重来 |
| 8 | ⏹ 停止采集 | 停止录制，回到就绪 |
| 9 | ⏻ 失能 | 舵机断电，DORA 数据流关闭 |

> 防误操作：未使能时其他按钮灰显；未开始采集时保存/重录/停止灰显。

### 键盘（备选）

```
o → 使能    x → 失能
s → 开始采集  f → 成功保存
r → 重录      q → 停止
```

---

## 7. 验证数据

```bash
# 查找最新 session 目录
ls datasets/2026-07-02/

# 用 lerobot 加载验证
source training/.venv/bin/activate
python -c "
from lerobot.datasets import LeRobotDataset
ds = LeRobotDataset('local/teleop', root='datasets/2026-07-02/14-30-00')
print(f'Episodes: {len(ds.meta.episodes)}, Frames: {ds.num_frames}')
"
```

---

## 8. 目录结构

```
datasets/                    ← gitignored
  2026-07-02/
    14-30-00/                ← 每次使能生成独立目录
      data/chunk-000/        ← 关节数据 (parquet)
      meta/info.json         ← 元信息
      videos/                ← 摄像头视频 (mp4)
```

---

## 9. 故障排查

| 症状 | 原因 | 解决 |
|---|---|---|
| `no matching USB device` | VID/PID/Serial 配置错误 | 重跑 `usb_scan`，更新 config |
| `FileExistsError: datasets` | 旧数据残留 | `rm -rf datasets/` |
| DORA `version mismatch` | dora 源码版本 vs CLI 版本不一致 | 确保 `dora` 目录是 `v1.0.0-rc1` 分支 |
| `ModuleNotFoundError: dora` | Python 节点未装 dora 包 | 重做步骤 2 的 `maturin build + pip install` |
| Web 页面不更新 | 从臂未发送 status | 确认从臂 daemon 正常运行 |
| 摄像头不工作 | 索引不稳定 | 交换 `TR_CAMERA_ID` 的 0/1 |

---

## 10. 依赖清单总览

```
gitignored (需手动准备):
  dora/              ← DORA 1.0-rc1 源码
  training/.venv/    ← Python venv (uv 创建)
  datasets/          ← 录制数据（运行中生成）

git tracked (开箱即用):
  config/            ← 配置文件模板
  crates/            ← Rust 源码
  dataflows/         ← DORA 数据流 YAML
  training/tr_lerobot/ ← Python 录制器源码
```
