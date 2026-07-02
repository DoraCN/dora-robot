# SO-101 遥操作数据采集 — 新手操作指南

> 从零开始，一台全新的 macOS 机器，跑通主从臂遥操作 + DORA 数据流录制 + 摄像头采集的完整链路。

---

## 0. 环境安装

### 0.1 Xcode Command Line Tools

```bash
xcode-select --install
```

### 0.2 Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# 选默认选项，安装后重新打开终端
rustc --version   # 应 ≥ 1.88
```

### 0.3 uv（Python 包管理 + venv）

```bash
curl -LsSf https://astral.sh/uv/install.sh | sh
# 重新打开终端
uv --version
```

### 0.4 Git LFS

```bash
brew install git-lfs
git lfs install
```

### 0.5 DORA CLI

```bash
# 从 GitHub Releases 下载二进制
# 以 1.0.0-rc1 为例，根据架构选对应文件：
curl -L https://github.com/dora-rs/dora/releases/download/v1.0.0-rc1/dora-cli-aarch64-apple-darwin.tar.gz \
  -o /tmp/dora.tar.gz
tar -xzf /tmp/dora.tar.gz -C /tmp
cp /tmp/dora ~/.local/bin/dora
chmod +x ~/.local/bin/dora

# 确保 ~/.local/bin 在 PATH 中
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
dora --version   # dora-cli 1.0.0-rc1
```

### 0.6 maturin（构建 DORA Python 包）

```bash
# maturin 是 Rust 工具，用 cargo 安装
cargo install maturin
maturin --version
```

---

## 1. 克隆项目 + 放置 DORA 源码

```bash
git clone https://github.com/DoraCN/dora-robot.git
cd dora-robot

# DORA 1.0-rc1 源码（gitignored，需手动克隆）
git clone https://github.com/dora-rs/dora.git
cd dora && git checkout v1.0.0-rc1 && cd ..
```

---

## 2. 创建 Python 虚拟环境 + 安装依赖

```bash
# 创建 venv（gitignored）
uv venv training/.venv --python 3.12
source training/.venv/bin/activate

# 安装 Python 依赖
uv pip install numpy opencv-python pyarrow pyyaml
uv pip install lerobot          # 含 torch，约 2GB

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

输出示例（找带有 `/dev/cu.usbmodem` 的设备）：

```
Device 11:
  VID   : 0x1A86
  PID   : 0x55D3
  Serial: 5A7A055502
  Dev   : /dev/cu.usbmodem5A7A0555021
```

### 3.2 编辑配置文件

`config/follower.toml`（从臂侧）和 `config/leader.toml`（主臂侧）各填一份。**两个臂的串口号不同**：

```toml
# config/follower.toml
[arm]
id = "arm_1"
type = "so101"

[arm.so101]
baud = 1_000_000
ids = [1, 2, 3, 4, 5, 6]
vid = "0x1A86"
pid = "0x55D3"
serial = "5A7A055502"    # 从 arm 的 USB Serial
```

`config/leader.toml` 同样内容，`serial` 填主臂的值。

---

## 4. 编译项目

```bash
# 全项目编译（首次约 10-20 分钟）
cargo build --release
cargo build -p tr-capture --release

# 关键二进制：
#   target/release/tr-capture   — DORA capture 节点
#   target/debug/follower       — 从臂 daemon
#   target/debug/leader         — 主臂 daemon + Web 控制台
```

---

## 5. 启动系统

> 两个终端需同时运行。

### 5.1 从臂（终端 1）

```bash
cargo run -p tr-daemon --bin follower -- --port /dev/cu.usbmodem5A7A0555021
```

输出 `state=IDLE` → 就绪，等待主臂指令。

### 5.2 主臂 + Web 控制台（终端 2）

```bash
cargo run -p tr-daemon --bin leader -- --port /dev/cu.usbmodem5AB01836201
```

浏览器打开 `http://localhost:8080`。

---

## 6. 操作流程

### Web 控制台（推荐）

| 步骤 | 按钮 | 系统行为 |
|---|---|---|
| — | 页面加载 | SSE 连接从臂，实时显示状态 |
| 1 | ⚡ 使能 | 从臂上电 + DORA 数据流启动 + 摄像头开始采集 |
| 2 | 搬动主臂 | 从臂实时跟随 |
| 3 | ▶ 开始采集 | 开始录制 episode |
| 4 | 搬动主臂 | 执行操作任务 |
| 5 | ✅ 成功保存 | 保存当前 episode 到 `datasets/` |
| 6 | ▶ 开始采集 | 开始下一个 episode（可在同一使能期内重复 3-5） |
| — | 🔄 重录 | 丢弃当前 episode，立即开始新的 |
| 7 | ⏹ 停止采集 | 停止录制 |
| 8 | ⏻ 失能 | 从臂断电，DORA 数据流关闭 |

> **防误操作**：未使能时录制按钮全部灰显；未开始采集时保存/重录不可用。

### 键盘（备选，终端 2 中操作）

```
o → 使能     x → 失能
s → 开始采集  f → 成功保存
r → 重录      q → 停止
```

---

## 7. 验证录制数据

```bash
source training/.venv/bin/activate
python -c "
from lerobot.datasets import LeRobotDataset
ds = LeRobotDataset('local/teleop', root='datasets/<日期>/<时间>')
for ep in ds.meta.episodes:
    print(f'Episode {ep[\"episode_index\"]}: {ep[\"length\"]} frames')
print(f'Total: {ds.num_frames} frames, {len(ds.meta.episodes)} episodes')
"
```

---

## 8. 目录结构（运行时生成，gitignored）

```
datasets/                       ← 项目根目录
  2026-07-02/
    14-30-00/                   ← 每次使能独立目录
      data/chunk-000/           ← 关节数据 (parquet)
      meta/info.json            ← 数据集元信息
      videos/                   ← 摄像头视频 (mp4)
        observation.images.front/
        observation.images.wrist/
    15-45-30/                   ← 第二次使能
```

---

## 9. 故障排查

| 症状 | 原因 | 解决 |
|---|---|---|
| `command not found: cargo` | Rust 未安装 | 执行步骤 0.2 |
| `command not found: uv` | uv 未安装 | 执行步骤 0.3 |
| `command not found: dora` | DORA CLI 未安装 | 执行步骤 0.5 |
| `maturin: command not found` | maturin 未安装 | 执行步骤 0.6，或 `cargo install maturin` |
| `dora-node-api v1.0.0-rc1` 编译失败 | dora 源码未克隆 | 执行步骤 1 |
| `no matching USB device` | VID/PID/Serial 配置错误 | 重跑 `cargo run -p tr-so101 --example usb_scan` |
| `FileExistsError: datasets` | 旧数据残留 | `rm -rf datasets/` |
| DORA `version mismatch` | 源码版本与 CLI 不一致 | `cd dora && git checkout v1.0.0-rc1` |
| `ModuleNotFoundError: No module named 'dora'` | Python 未装 dora 包 | 重做步骤 2 的 maturin 部分 |
| `ModuleNotFoundError: No module named 'cv2'` | opencv 未装 | `uv pip install opencv-python` |
| Web 页面不更新状态 | 从臂未启动 | 先启动终端 1，再启动终端 2 |
| 摄像头索引交换 | USB 枚举顺序不稳定 | 交换 `record.yml` 中 `TR_CAMERA_ID` 的 0/1 |

---

## 10. gitignored 清单（需手动准备或运行时生成）

| 路径 | 说明 | 如何获取 |
|---|---|---|
| `dora/` | DORA 1.0-rc1 源码 | `git clone https://github.com/dora-rs/dora.git` |
| `training/.venv/` | Python 虚拟环境 | `uv venv` 创建 |
| `datasets/` | 录制数据 | 运行时自动生成 |
| `target/` | Rust 编译产物 | `cargo build` 自动生成 |
| `.leon/` | lerobot 参考源码 | 不需要（只读参考） |
| `logs/` | 运行日志 | 运行时生成 |
