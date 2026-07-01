# 从臂摄像头接入 — 技术方案分析

> 现状：从臂侧接有两个摄像头。目标：摄像机数据随本体关节数据一起录进 LeRobot v3 数据集。

---

## 1. 数据流全景对比

### 当前（无摄像头）
```
leader --> zenoh --> zenoh_follower --stdout--> pipe_recorder --> lerobot v3
                         │              D j1..j6 + @START/@SUCCESS ...
                         └── sync_write_goals --> 从臂舵机
```
管道里每帧一条 `D ...` 协议行。录制器收到后组帧 `{action, observation.state}`。

### 目标（加摄像头）
```
leader --> zenoh --> zenoh_follower --stdout--> pipe_recorder --> lerobot v3
                         │              D j1..j6 + @START... + ??? 相机数据 ???
                         └── sync_write_goals --> 从臂舵机

camera_1 --??--> ???
camera_2 --??--> ???
```

核心问题：**相机数据走哪条路到录制器。**

---

## 2. 三方案对比

### 方案 A：相机走同一条 stdout 管道

相机帧和关节数据混在同一个 stdout 里，增强现有协议。

```
camera_1 ──┐
camera_2 ──┤── zenoh_follower ──stdout──→ pipe_recorder
            │    D j1..j6  (关节行)
            │    V front <len> <bytes>  (相机行)
            └── 同步时序由 follower 统一
```

- ✅ 最简单，不引入新进程/新管道。
- ✅ 时序天然对齐（相机帧和关节帧都在 follower 的循环里采集，时间戳一致）。
- ❌ stdout 管道混入大块图像数据（640×480×3 ≈ 900KB/帧×30fps×2 cameras ≈ 54MB/s），管道拥塞风险。
- ❌ 相机采集在 follower 的异步循环里，图像抓取（`VideoCapture::read()`）有延迟，可能拖慢舵机控制频率。

### 方案 B：相机独立进程 + 独立管道

相机由独立程序采集，通过各自管道发给录制器。录制器开三个 stdin（或一个多路复用）。

```
camera_1 ──pipe──┐
camera_2 ──pipe──┼── pipe_recorder (多路复用或文件描述符)
follower   ──pipe──┘
```

- ✅ 相机不拖慢控制环。
- ✅ 各管道独立，带宽隔离。
- ❌ 多管道复用在 Python 里复杂（需要 `select`/`epoll` 或 `asyncio`）。
- ❌ 三方数据对齐困难——相机帧、关节帧各有独立时间戳，需在录制器侧做时间对齐/降采样。

### 方案 C（推荐）：相机独立进程写临时图像，follower 只传引用

相机进程采集帧 → 存为临时文件（ring buffer of N frames），follower 在 stdout 里引用文件路径，录制器读文件后调 lerobot。

```
camera_1 ──→ /tmp/cam1/frame_000123.jpg
camera_2 ──→ /tmp/cam2/frame_000123.jpg

follower:  D j1..j6
           I front /tmp/cam1/frame_000123.jpg
           I wrist /tmp/cam2/frame_000123.jpg
           同步时序由 follower 的 stdout 行序保证（同一拍引用同一帧号）
```

- ✅ 管道只传几十字节的路径，不拥塞。
- ✅ 录制器可直接读到像素数据，跳过视频编码开销（lerobot 的 `add_frame` 接受 numpy 数组，后续 `save_episode` 时统一编码为 MP4）。
- ✅ 时序天然对齐。
- ✅ 相机独立进程，不影响控制环频率。
- ⚠️ 需要清理临时文件。

---

## 3. 推荐方案细化（方案 C）

### 架构

```
┌───────────────── follower 机器 ─────────────────┐
│                                                  │
│  camera_front ──→ /tmp/tr_cam/front/latest.jpg   │
│  camera_wrist ──→ /tmp/tr_cam/wrist/latest.jpg   │
│                                                  │
│  zenoh_follower (Rust):                           │
│    收帧 → 写舵机                                  │
│    每拍输出 stdout:                               │
│      D j1..j6                                     │
│      I front /tmp/tr_cam/front/latest.jpg         │
│      I wrist /tmp/tr_cam/wrist/latest.jpg         │
│                                                  │
│  pipe_recorder (Python):                          │
│    读 stdin → D行 → action/state                 │
│             → I行 → 读jpg → numpy → add_frame     │
│             → lerobot v3                          │
└──────────────────────────────────────────────────┘
```

### 相机进程（极简 Rust 或 Python）

```
loop:
  frame = camera.read()          # BGR HWC uint8
  cv2.imwrite("/tmp/tr_cam/front/latest.jpg", frame)
  sleep(1/30)                     # 30 fps
```

两个独立进程，一相机一个。每次覆盖写同一个文件（latest.jpg），节省磁盘且无清理负担。

### 协议新增

| 行格式 | 含义 |
|---|---|
| `D j1 j2 j3 j4 j5 j6` | 数据帧（已有） |
| `I <cam_name> <path>` | 图像帧引用，如 `I front /tmp/tr_cam/front/latest.jpg` |

录制器收到 `I` 行后：`img = cv2.imread(path); img_rgb = cv2.cvtColor(img, cv2.COLOR_BGR2RGB); frame[f"observation.images.{name}"] = img_rgb`。

### 数据集 features

```python
features = {
    "action":              {"dtype": "float32", "shape": (6,)},
    "observation.state":   {"dtype": "float32", "shape": (6,)},
    "observation.images.front": {"dtype": "video", "shape": (H, W, 3)},
    "observation.images.wrist": {"dtype": "video", "shape": (H, W, 3)},
}
use_videos = True   # lerobot 内部编码为 MP4
```

### fps 与对齐

- 关节帧 ~45Hz。相机帧 30Hz。
- 录制器**以相机帧为节拍**：收到 `I` 行时才取最新关节数据组一帧 → `add_frame`。`D` 行只更新最新关节值，不触发 `add_frame`。
- 最终数据集 fps = 30（相机帧率），行数 = 相机帧数。

### 性能

| 指标 | 估算 |
|---|---|
| 相机 JPEG 写入 | ~5ms/帧（640×480，OpenCV `imwrite`） |
| stdout 管道吞吐 | 关节 ~200 B/s + 路径引用 ~200 B/s = 可忽略 |
| 录制器读 JPEG | ~2ms/帧 |
| 磁盘 I/O | ~(50KB/帧 × 2相机 × 30fps) ≈ 3MB/s，可忽略 |

---

## 4. 与现有代码的改动面

| 改动 | 文件 |
|---|---|
| 协议新增 `I` 行 | `zenoh_follower.rs`（添加 stdout 输出）+ `pipe_recorder.py`（解析 `I` 行） |
| 相机进程 | 新建 `examples/camera_capture.py` 或 Rust 版（选 OpenCV / v4l2） |
| features 含图像 | `pipe_recorder.py`（`create` 时传相机特征） |
| 相机配置 | 环境变量 `TR_CAMERAS="front:640x480,wrist:640x480"` |
| 录制器 `add_frame` 触发逻辑 | 从"每 `D` 行触发"改为"每 `I` 行触发" |

---

## 5. 开放决策

1. **相机进程语言**：Python（OpenCV，最简）还是 Rust（`opencv`/`rscam` crate，零额外依赖）？
2. **编码方式**：临时 JPEG（方案 C）还是直接传 numpy 数组（需 IPC/Pipe）？
3. **数据集 fps**：30Hz（推荐）还是按控制频率？
4. **相机分辨率**：640×480？（影响数据集大小和编码性能）
