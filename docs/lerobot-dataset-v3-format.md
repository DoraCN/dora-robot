# LeRobotDataset **v3.0** — Actual On-Disk Format (source-verified)

> Reverse-engineered from the real source in `lerobot/src/lerobot/datasets/`.
> Every claim below cites `file:line` so it can be re-verified. This supersedes the
> prose in `docs/lerobot-dataset-v3.mdx` where they differ (see §9).

Version tag: **`CODEBASE_VERSION = "v3.0"`** — `dataset_metadata.py:57`.

---

## 1. Core idea

v3 **decouples storage from the API**: many episodes are concatenated into a few
large files; per-episode views are reconstructed from **metadata offsets**, not
from filenames (`lerobot_dataset.py:91-148`). Three pillars: **tabular** (Parquet),
**visual** (MP4, or PNG for `image` features), **metadata** (JSON + Parquet).

---

## 2. Directory layout (`utils.py:78-101`, `lerobot_dataset.py:100-148`)

```
<root>/
├── data/
│   └── chunk-{NNN}/file-{NNN}.parquet          # tabular; MANY episodes per file
├── videos/
│   └── {video_key}/chunk-{NNN}/file-{NNN}.mp4  # one tree per camera; many episodes per file
├── images/                                      # ONLY for dtype="image" (not "video")
│   └── {image_key}/episode-{NNNNNN}/frame-{NNNNNN}.png
└── meta/
    ├── info.json
    ├── stats.json
    ├── tasks.parquet                            # ← v3 uses PARQUET (not jsonl)
    └── episodes/
        └── chunk-{NNN}/file-{NNN}.parquet       # ← chunked PARQUET (not jsonl)
```

Path templates & limits (constants, `utils.py`):

| Constant | Value |
|---|---|
| `CHUNK_FILE_PATTERN` | `chunk-{chunk_index:03d}/file-{file_index:03d}` |
| `DEFAULT_DATA_PATH` | `data/chunk-{NNN}/file-{NNN}.parquet` |
| `DEFAULT_VIDEO_PATH` | `videos/{video_key}/chunk-{NNN}/file-{NNN}.mp4` |
| `DEFAULT_EPISODES_PATH` | `meta/episodes/chunk-{NNN}/file-{NNN}.parquet` |
| `DEFAULT_TASKS_PATH` | `meta/tasks.parquet` |
| `INFO_PATH` / `STATS_PATH` | `meta/info.json` / `meta/stats.json` |
| `IMAGE_FILE_PATTERN` / depth | `frame-{frame_index:06d}.png` / `.tiff` |
| `DEFAULT_CHUNK_SIZE` | `1000` (max files per chunk dir) |
| `DEFAULT_DATA_FILE_SIZE_IN_MB` | `100` (roll to new file past this) |
| `DEFAULT_VIDEO_FILE_SIZE_IN_MB` | `200` |

`LEGACY_*` (`utils.py:99-101`) are the **v2.1** `meta/*.jsonl` paths — not v3.

---

## 3. `meta/info.json` (`utils.py:104-168`, `feature_utils.py:87-122`)

Serialized from the `DatasetInfo` dataclass. Fields:

```jsonc
{
  "codebase_version": "v3.0",
  "fps": 30,
  "features": { /* see §4 */ },
  "total_episodes": 0,
  "total_frames": 0,
  "total_tasks": 0,
  "chunks_size": 1000,
  "data_files_size_in_mb": 100,
  "video_files_size_in_mb": 200,
  "data_path":  "data/chunk-{chunk_index:03d}/file-{file_index:03d}.parquet",
  "video_path": "videos/{video_key}/chunk-{chunk_index:03d}/file-{file_index:03d}.mp4", // null if no videos
  "robot_type": "so101_follower",
  "splits": { "train": "0:N" },        // updated each save_episode (dataset_metadata.py:594)
  "tools": null                         // optional; dropped when unset
}
```

- `total_*`, `splits` are **counters updated as episodes are saved**
  (`dataset_metadata.py:591-596`), not known up front.
- Feature `shape` is stored as a JSON **list** (tuple in memory, `utils.py:139-168`).

---

## 4. The `features` dict (`feature_utils.py:43-84`)

Per feature: `{ "dtype": ..., "shape": [...], "names": [...] }`. `dtype` decides
storage:

| `dtype` | Stored as | HF type |
|---|---|---|
| `"video"` | **MP4** under `videos/` (NOT in parquet) | skipped in parquet (`:63-64`) |
| `"image"` | **PNG** under `images/` (path / embedded) | `datasets.Image()` |
| numpy dtype (`"float32"`, `"int64"`, …), `shape==(1,)` | parquet scalar | `datasets.Value` |
| numpy dtype, 1-D shape | parquet fixed-length list | `datasets.Sequence(length,…)` |
| numpy dtype, 2–5-D shape | parquet tensor | `datasets.Array2D…Array5D` |

Example (SO-101, joints in radians + one camera):

```jsonc
"action":            { "dtype": "float32", "shape": [6], "names": ["j1","j2","j3","j4","j5","gripper"] },
"observation.state": { "dtype": "float32", "shape": [6], "names": ["j1","j2","j3","j4","j5","gripper"] },
"observation.images.front": { "dtype": "video", "shape": [480,640,3], "names": ["height","width","channels"],
                              "info": { /* codec/fps/... filled in after 1st episode (dataset_metadata.py:601-645) */ } }
```

---

## 5. `data/…parquet` — one row per **frame** (`dataset_writer.py:184-279`)

You pass user features + `"task"` to `add_frame`; lerobot **auto-adds the rest**:

| Column | Source |
|---|---|
| your numeric/string features (e.g. `action`, `observation.state`) | `add_frame` (`:240`) |
| `timestamp` | auto = `frame_index / fps` (`:207-209`) |
| `frame_index` | auto, per-episode `0..L-1` (`:206-208`) |
| `episode_index` | filled in `save_episode` (`:261`) |
| `index` | global frame index `np.arange(total, total+L)` (`:260`) |
| `task_index` | mapped from the `task` string (`:267`) |

**Video features are NOT pixel-stored in parquet** — frames go to the MP4; the
parquet `timestamp` is what the reader uses to decode the matching video frame
(`dataset_writer.py:227-238`, `lerobot_dataset.py:98`). `image` features store a
PNG path / embedded image.

`add_frame` rules (`dataset_writer.py:184-200`, docstring): the frame dict **must
include `"task"`**, **must NOT include `"timestamp"`/`"frame_index"`** (auto), and
torch tensors are auto-converted to numpy. `validate_frame` enforces the schema.

---

## 6. `meta/episodes/…parquet` — one row per **episode** (`dataset_metadata.py:559-588`, `io_utils.py:212-218`)

```
episode_index            int
tasks                    list[str]
length                   int (frames)
dataset_from_index       global frame start  ─┐ data-shard reference
dataset_to_index         global frame end     │
data/chunk_index         int                  │
data/file_index          int                 ─┘
from_timestamp           float  ─┐ per-video offsets into the shared MP4
to_timestamp             float  ─┘  (+ videos/{key}/chunk_index, file_index)
stats/<feature>/<mean|std|min|max|count>   flattened per-episode stats (:587)
```

(`load_episodes` drops the `stats/*` columns for a light index — `io_utils.py:217`.)
This offset table is exactly **how a logical episode is reconstructed from the
shared parquet/MP4 files**.

---

## 7. `meta/tasks.parquet` & `meta/stats.json`

- **tasks** (`io_utils.py:178-187`): pandas DataFrame indexed by the **task string**
  (`index.name = "task"`), mapping task → `task_index`. New tasks are registered on
  `save_episode` (`dataset_metadata.py:264, save_episode_tasks:459`).
- **stats** (`io_utils.py:137-145`, `dataset_metadata.py:598-599`): global
  per-feature normalization stats `{mean, std, min, max, count}` (quantiles
  `[0.01,0.10,0.50,0.90,0.99]` available — `compute_stats.py:27`), aggregated
  incrementally as episodes are saved.

---

## 8. Write lifecycle / API (`lerobot_dataset.py:398-467, 657-678`)

```python
ds = LeRobotDataset.create(
    repo_id, fps, features, root=..., robot_type=..., use_videos=True,
    # ... batch_encoding_size, streaming_encoding, *_files_size_in_mb, metadata_buffer_size=10 ...
)
for ep in episodes:
    for step in ep:
        ds.add_frame({ "action": a, "observation.state": s,
                       "observation.images.front": img_hwc_uint8, "task": "..." })
    ds.save_episode()      # stack buffer → stats → write data parquet shard +
                           # encode video shards → append episode row → update info/stats/tasks
ds.finalize()             # MANDATORY
```

- v3 writes **incrementally with buffered metadata**: episode-metadata rows are
  buffered (`metadata_buffer_size`, flush `dataset_metadata.py:556-557`) and data is
  appended into the current shard until it exceeds `data_files_size_in_mb`, then
  rolls to a new `chunk/file`.
- **`finalize()` is mandatory** (`lerobot_dataset.py:454-467`): it flushes buffered
  metadata and **writes parquet footers** — *without it the parquet files are
  invalid and the dataset won't load*. Idempotent; `__del__` is a safety net.

---

## 9. Corrections vs. `docs/lerobot-dataset-v3.mdx`

| Topic | `.mdx` said | **Actual v3.0 source** |
|---|---|---|
| tasks file | `meta/tasks.jsonl` | **`meta/tasks.parquet`** (`utils.py:92`); `.jsonl` is LEGACY v2.1 |
| episodes file | "Parquet" (ok) | `meta/episodes/chunk-NNN/file-NNN.parquet` (chunked) |
| version string | "v3.0" (ok) | `CODEBASE_VERSION = "v3.0"` |

The `.mdx` is directionally right (file-based aggregation, offset metadata,
`finalize()`), but for exact filenames/columns trust this doc / the source.

---

## 10. Implications for our recorder (`training/tr_lerobot/`)

Because we drive **lerobot's own writer** (Option B), we get *all* of the above —
chunking, episode offset tables, video sharding, `tasks.parquet`, `stats.json`,
parquet footers — **for free**. Our responsibilities reduce to:

1. **`features` dict** must declare exactly what we feed: `action`/`observation.state`
   as `{"dtype":"float32","shape":[6],"names":[...]}`; cameras as
   `{"dtype":"video","shape":[H,W,3],"names":["height","width","channels"]}`.
   (matches `schema.py`).
2. **`add_frame` payload** per frame: `{"action": np.float32[6], "observation.state":
   np.float32[6], "observation.images.front": np.uint8[H,W,3] (HWC, RGB), "task": str}`
   — **include `task`**, **omit `timestamp`/`frame_index`** (lerobot computes them).
3. Call **`save_episode()`** at each episode boundary and **`finalize()`** on stop.
4. `fps` in `create` defines the dataset rate ⇒ the recorder samples the 100 Hz
   control stream down to e.g. 30 Hz (one `add_frame` per dataset tick).

So the Rust→Python data-format work from the previous discussion stands: emit
flat Arrow (`Float32Array` joints, `UInt8Array` images) from the Rust nodes; the
Python recorder converts to the numpy shapes in (2) and calls lerobot. Nothing in
the Rust core needs to know the v3 on-disk layout.

---

## 11. Key source files (for re-verification)

| File | What it defines |
|---|---|
| `datasets/utils.py:78-218` | layout constants + `DatasetInfo` (info.json schema) |
| `datasets/feature_utils.py:43-122` | feature dtype→storage mapping, `create_empty_dataset_info` |
| `datasets/lerobot_dataset.py:90-148, 398-467, 657-678` | layout docstring, `add_frame/save_episode/finalize/create` |
| `datasets/dataset_writer.py:155-324` | episode buffer, per-frame columns, video/image handling |
| `datasets/dataset_metadata.py:57, 559-599` | `CODEBASE_VERSION`, episodes-row schema, info/stats/tasks updates |
| `datasets/io_utils.py:120-218` | `write_info/stats/tasks/episodes` (exact files) |
| `datasets/compute_stats.py:27` | quantiles in stats |
