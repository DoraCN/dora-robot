# SO-101 遥操作数据采集 — 新手操作指南

> 从零开始，一台全新的机器，跑通主从臂遥操作 + DORA 数据流录制 + 摄像头采集的完整链路。

---

## 0. 环境安装

### 0.1 基础工具

| 工具 | macOS | Linux (Debian/Ubuntu) | Windows |
|---|---|---|---|
| 系统工具 | `xcode-select --install` | `sudo apt install build-essential curl pkg-config` | 安装 [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022)，勾选"使用 C++ 的桌面开发" |

### 0.2 Rust

三平台相同：

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# 选默认选项 (1)，安装后重新打开终端
rustc --version   # 应 ≥ 1.88
```

### 0.3 uv（Python 包管理 + venv）

| 平台 | 命令 |
|---|---|
| macOS / Linux | `curl -LsSf https://astral.sh/uv/install.sh \| sh` |
| Windows | `powershell -ExecutionPolicy ByPass -c "irm https://astral.sh/uv/install.ps1 \| iex"` |

```bash
# 重新打开终端
uv --version
```

### 0.4 DORA CLI

从 GitHub Releases 下载二进制：

| 操作系统 | 下载文件 |
|---|---|
| macOS (Apple Silicon) | `dora-cli-aarch64-apple-darwin.tar.gz` |
| macOS (Intel) | `dora-cli-x86_64-apple-darwin.tar.gz` |
| Linux (x86_64) | `dora-cli-x86_64-unknown-linux-gnu.tar.gz` |
| Linux (ARM) | `dora-cli-aarch64-unknown-linux-gnu.tar.gz` |
| Windows | `dora-cli-x86_64-pc-windows-msvc.zip` |

```bash
# macOS / Linux
curl -L "https://github.com/dora-rs/dora/releases/download/v1.0.0-rc.1/<文件名>" -o /tmp/dora.tar.gz
tar -xzf /tmp/dora.tar.gz -C /tmp
mkdir -p ~/.local/bin
cp /tmp/dora ~/.local/bin/dora
chmod +x ~/.local/bin/dora
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
```

```powershell
# Windows (PowerShell 管理员)
$url = "https://github.com/dora-rs/dora/releases/download/v1.0.0-rc.1/dora-cli-x86_64-pc-windows-msvc.zip"
Invoke-WebRequest $url -OutFile $env:TEMP\dora.zip
Expand-Archive $env:TEMP\dora.zip -DestinationPath $env:LOCALAPPDATA\dora
[Environment]::SetEnvironmentVariable("Path", "$env:LOCALAPPDATA\dora;" + [Environment]::GetEnvironmentVariable("Path", "User"), "User")
```

验证：

```bash
dora --version   # dora-cli 1.0.0-rc1
```

### 0.5 maturin（构建 DORA Python 包）

```bash
cargo install maturin
maturin --version
```

### 0.6 Linux 额外依赖

```bash
# USB 设备监控 (usb-resolver crate)
sudo apt install libudev-dev
```

### 0.8 macOS 额外依赖

无需额外操作。

---

## 1. 克隆项目 + 放置 DORA 源码

```bash
git clone --recursive https://github.com/DoraCN/dora-robot.git
cd dora-robot

# 如果已克隆但没有初始化子模块，执行：
git submodule update --init --recursive
```

---

## 2. 创建 Python 虚拟环境 + 安装依赖

```bash
# 创建 venv（gitignored）
uv venv training/.venv --python 3.12

# 安装 Python 依赖（macOS / Linux）
source training/.venv/bin/activate
# Windows:
#   training\.venv\Scripts\activate

uv pip install numpy opencv-python pyarrow pyyaml
uv pip install lerobot          # 含 torch，约 2GB

# 安装 DORA Python 包（从本地源码构建）
cd dora
maturin build -m apis/python/node/Cargo.toml --release
# macOS / Linux:
uv pip install --python ../training/.venv/bin/python target/wheels/dora_rs-1.0.0rc1-*.whl
# Windows:
#   uv pip install --python ..\training\.venv\Scripts\python.exe target\wheels\dora_rs-1.0.0rc1-*.whl
cd ..
```

---

## 3. 配置机械臂

### 3.1 发现设备

```bash
cargo run -p tr-so101 --example usb_scan
```

**macOS**：找 `/dev/cu.usbmodemXXXX` 的设备。
**Linux**：找 `/dev/ttyUSB0` 或 `/dev/ttyACM0` 的设备。
**Windows**：找 `COM3` 等串口设备。

输出示例：

```
Device:  VID 0x1A86  PID 0x55D3  Serial: 5A7A055502  Dev: /dev/cu.usbmodem5A7A0555021
```

### 3.2 编辑配置文件

`config/follower.toml`（从臂侧）和 `config/leader.toml`（主臂侧）各填一份。**两个臂的串口号/Serial 不同**：

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
serial = "5A7A055502"    # 从臂 USB Serial
```

---

## 4. 编译项目

```bash
cargo build --release
cargo build -p tr-capture --release

# 部署到 bin/
mkdir -p bin
cp target/release/follower   bin/follower
cp target/release/leader     bin/leader
cp target/release/tr-capture bin/tr-capture
```

首次编译约 10-20 分钟。

---

## 5. 启动系统

### 5.1 从臂（终端 1）

```bash
# macOS / Linux
cargo run -p tr-daemon --bin follower -- --port /dev/cu.usbmodem5A7A0555021

# Windows
# cargo run -p tr-daemon --bin follower -- --port COM3
```

输出 `state=IDLE` → 就绪。

### 5.2 主臂 + Web 控制台（终端 2）

```bash
# macOS / Linux
cargo run -p tr-daemon --bin leader -- --port /dev/cu.usbmodem5AB01836201

# Windows
# cargo run -p tr-daemon --bin leader -- --port COM4
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

### 键盘（备选）

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
print(f'Episodes: {len(ds.meta.episodes)}, Frames: {ds.num_frames}')
"
```

---

## 8. 目录结构（运行时生成，gitignored）

```
datasets/
  2026-07-02/
    14-30-00/                   ← 每次使能独立目录
      data/chunk-000/           ← 关节数据 (parquet)
      meta/info.json            ← 数据集元信息
      videos/                   ← 摄像头视频 (mp4)
```

---

## 9. 故障排查

| 症状 | 解决 |
|---|---|
| `command not found: cargo` | 执行 §0.2 |
| `command not found: uv` | 执行 §0.3 |
| `command not found: dora` | 执行 §0.4 |
| `command not found: maturin` | 执行 §0.5 |
| `dora-node-api v1.0.0-rc.1` 编译失败 | dora 源码未克隆 → §1 |
| `no matching USB device` | 重跑 `usb_scan`，更新 config |
| `FileExistsError: datasets` | `rm -rf datasets/` |
| DORA `version mismatch` | `cd dora && git checkout v1.0.0-rc.1` |
| `ModuleNotFoundError: dora` | 重做 §2 maturin 部分 |
| `ModuleNotFoundError: cv2` / `numpy` | `uv pip install opencv-python numpy` |
| Web 页面不更新 | 先启动终端 1 再启动终端 2 |
| 摄像头索引交换 | 交换 `record.yml` 中 `TR_CAMERA_ID` 的 0/1 |
| Linux 编译 `usb-resolver` 失败 | `sudo apt install libudev-dev` (§0.6) |

---

## 10. gitignored 清单

依赖通过 git submodule 管理（`thirdparty/dora`, `thirdparty/lerobot`），详见 `.gitmodules`。

| 路径 | 说明 | 获取方式 |
|---|---|---|
| `thirdparty/` | 第三方源码 (git submodule) | `git submodule update --init --recursive` |
| `training/.venv/` | Python venv | `uv venv` (§2) |
| `datasets/` | 录制数据 | 运行时自动生成 |
| `target/` | Rust 编译产物 | `cargo build` |
| `logs/` | 日志 | 运行时生成 |
