# Recording / Video-Encoding Performance Design

> Source: `lerobot/docs/source/streaming_video_encoding.mdx` +
> `LeRobotDataset.create()` params (`datasets/lerobot_dataset.py:657-678`).
> Goal: record LeRobot **v3** episodes during SO-101 teleop **without ever
> disturbing the realtime Rust control loop**.

---

## 1. What lerobot's streaming encoder does (summary)

Instead of *capture → write PNG → (episode end) read PNGs → encode MP4 → delete*,
streaming mode does *capture → queue → encode to MP4 live* (mdx §1). Effect:
`save_episode()` becomes near-instant; no blocking gap between episodes.

Knobs (mdx §2; mirror of `create()` params):

| Param | Default | Meaning |
|---|---|---|
| `streaming_encoding` | CLI `True` / **`create()` default `False`** ⚠️ | live encode during capture |
| `vcodec` (`rgb_encoder.vcodec`) | `libsvtav1` | codec; `"auto"` = probe best HW encoder |
| `encoder_threads` | `None` (codec decides) | threads **per camera** encoder |
| `encoder_queue_maxsize` | `30` (~1 s @30 fps) | bounded frame queue **per camera** (RAM) |
| `batch_encoding_size` | `1` | episodes accumulated before batch encode |

⚠️ **`create()` defaults `streaming_encoding=False`** while the `lerobot-record`
CLI defaults it `True`. Our recorder calls `create()` directly, so we must set it
explicitly.

Backpressure (mdx §3): when an encoder can't keep up, its queue fills (RAM), then
**new frames are dropped, not blocked** — the capture loop is never stalled; a
warning + per-episode dropped count is logged.

Codec/CPU trade (mdx §4): `libsvtav1` = smallest files / best training quality but
highest CPU; `h264` = medium CPU, 30–50% larger; **HW encoders = very low CPU**,
larger files. HW: `h264_videotoolbox`/`hevc_videotoolbox` (macOS),
`h264_nvenc`/`hevc_nvenc` (NVIDIA), `h264_vaapi` (Intel/AMD Linux), `h264_qsv`
(Intel QSV), `auto` (probe → fallback `libsvtav1`).

---

## 2. Why our architecture changes the picture (the key insight)

In lerobot's monolithic `lerobot-record`, **capture loop + robot control + encoder
share one process** — that's why a starved CPU causes *"choppy robot movement"*
(mdx §3). **Our system is different and better:**

- The realtime loop (leader→zenoh→follower→servos) is **Rust, in its own DORA
  node/process**.
- Camera capture and the **lerobot encoder live in a separate Python recorder
  process**.

So encoding **cannot directly block** the Rust servo loop — they are separate OS
processes. The remaining risk is only **CPU contention / scheduling jitter** on a
*shared machine*. That risk is controllable (priority, affinity, offload), which is
the heart of this design.

---

## 3. Performance design

### 3.1 Isolate the realtime loop (most important)

The Rust servo loop is light on CPU but **latency-sensitive** (100 Hz). Protect its
*timing*, not just its CPU share:

- Give the Rust follower/teleop node **elevated/RT scheduling priority**; run the
  Python recorder at **lower priority** (`nice`).
- **Pin** the servo loop to a dedicated core; keep encoder threads off that core
  (CPU affinity / cgroups / `taskset`).
- This guarantees the 100 Hz control loop keeps its deadline even if the encoder
  saturates the other cores.

### 3.2 Never let recording backpressure the control path

The recording branch must be **drop, not block**:

- The `camera → recorder` and `robot → recorder` DORA edges are **best-effort**
  (newest-wins / bounded); a slow recorder must never stall the Rust nodes.
- lerobot's streaming encoder already drops on a full queue (mdx §3) — good; we
  keep that as the second line of defense.

### 3.3 Record at dataset fps, not control fps

Control loop = 100 Hz, dataset = camera fps (e.g. **30 Hz**). The recorder
**downsamples** (one `add_frame` per `1/fps`). This cuts encoder load **~3×** vs.
recording at 100 Hz. Record video at camera rate; the low-dim action/state are
sampled at the same dataset fps.

### 3.4 Size by pixel throughput

```
throughput_px_per_s = Σ_cameras ( W × H × 3 × fps )
```
Calibrated against the mdx §3 table (matches exactly):

| Setup | px/s | Load |
|---|---|---|
| 2× 640×480 @30 | 55 M | low |
| 2× 1280×720 @30 | 166 M | moderate |
| 2× 1920×1080 @30 | 373 M | high (needs strong CPU **or** HW/offload) |

Targets (mdx §6): high-end 12+ cores ≈ 250–500 M comfortable; mid 8+ cores / Apple
Silicon ≈ 80–300 M; constrained 4-core / Pi 5 → keep low or offload.

### 3.5 RAM budget

```
encoder_RAM ≈ encoder_queue_maxsize × (W×H×3) × n_cameras
```
e.g. 30 × (1280×720×3) × 2 ≈ **166 MB**. Lower `encoder_queue_maxsize` on
RAM-constrained robots.

### 3.6 Codec / HW selection per deployment

| Where recording runs | Recommended `vcodec` | `streaming_encoding` |
|---|---|---|
| Dev box / offload PC, many cores | `libsvtav1` (best for training) | `true` |
| On-robot, NVIDIA Jetson | `h264_nvenc` / `hevc_nvenc` | `true` |
| On-robot, Apple Silicon | `h264_videotoolbox` | `true` |
| On-robot, Intel iGPU (Linux) | `h264_qsv` / `h264_vaapi` | `true` |
| On-robot, weak CPU, no HW | `h264` + `batch_encoding_size>1`, or **`streaming_encoding=false`** (PNG-then-encode between episodes) | `false` |
| Unknown | `auto` (probe → fallback `libsvtav1`) | `true` |

---

## 4. Deployment topologies (CPU isolation by placement)

```
A) On-robot recording (light setups: ≤2 cams 640×480 + HW encoder)
   robot machine: [Rust servo loop ⟂ cores] + [cameras] + [Python recorder+encoder]
   → isolate via priority/affinity (§3.1)

B) Offloaded recording (heavy: multi-cam / 1080p / libsvtav1)   ← recommended for high throughput
   robot machine : Rust servo loop + cameras
        │ DORA over zenoh (distributed dataflow)
        ▼
   recording PC : Python recorder + encoder (all the CPU here)
   → control loop machine never sees encoding load at all
```

Topology B is the cleanest performance answer: it physically removes encoder load
from the robot. Topology A is fine for light setups with a HW encoder.

---

## 5. Config knobs to expose in our recorder

Extend `training/tr_lerobot/recorder.py` config (env) and pass through to
`LeRobotDataset.create(...)`:

| Env | → `create()` arg | Default (proposed) |
|---|---|---|
| `LEROBOT_FPS` | `fps` | `30` |
| `TR_STREAMING_ENCODING` | `streaming_encoding` | `true` |
| `TR_VCODEC` | `rgb_encoder.vcodec` | `auto` |
| `TR_ENCODER_THREADS` | `encoder_threads` | platform-tuned (2 mid / 5 high) |
| `TR_ENCODER_QUEUE` | `encoder_queue_maxsize` | `30` |
| `TR_BATCH_ENCODING` | `batch_encoding_size` | `1` |
| `TR_CAMERAS` | feature shapes | e.g. `front:480x640` |

Recorder must also set `streaming_encoding=True` explicitly (it is **not** the
`create()` default).

---

## 6. Health checks (mdx §5, §7)

- Watch for `"Encoder queue full … dropped N frame(s)"` warnings.
- After recording verify: **rows == fps × episode_duration**, and video duration ≈
  CLI episode duration.
- ~2% missing frames = transient startup spikes (tolerable); **~5%+ = overloaded** →
  apply §3 (lower `encoder_threads`, switch to HW codec, offload, reduce throughput).

---

## 7. Honest notes / open decisions

1. **Extra hop**: frames travel camera-node → (DORA shared-mem Arrow, zero-copy) →
   recorder process → numpy reshape/BGR→RGB/contiguous → lerobot queue. The encoder
   still runs in the recorder process, so that process is the heavy one — size/place
   it per §3–4.
2. **`streaming_encoding` default mismatch** (`create()` False vs CLI True) — we set
   it explicitly.
3. **Must validate on target hardware** (mdx §7): the robot's actual CPU/HW encoder
   decides A vs B. Numbers above are conservative estimates.

**For you to decide:** (a) recording **on-robot (A)** or **offloaded to a recording
PC (B)**? (b) target **#cameras / resolution / fps**? (c) robot compute (Jetson /
Apple / Intel / other) → fixes the default `vcodec`. These pin the defaults in §5.
