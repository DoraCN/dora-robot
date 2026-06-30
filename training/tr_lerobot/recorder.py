"""DORA Python node: forward the teleop dataflow to lerobot (record a v3 dataset).

**Boundary (only-transmit-data):** this node *acquires* arrays from the DORA
dataflow and *hands them* to an injected :class:`EpisodeWriter` (production =
lerobot). **Persistence / encoding / v3 layout is lerobot's domain, not this
project** — see `docs/specs/001-so101-teleop-record/spec.md` §5/§6. It consumes
**plain Arrow** and never decodes ``tr-messages``/``Codec``.

Expected dataflow inputs (Arrow):
  - ``action``                   : Float32Array(dof)
  - ``observation_state``        : Float32Array(dof)
  - ``observation_images_<cam>`` : UInt8Array(H*W*3) flat HWC, metadata {width,height,encoding}
  - ``episode_end``              : episode boundary; metadata ``outcome`` ∈
                                   {success, fail, rerecord} (forwarded by the
                                   robot-side bridge; finalized in task F2)

Frame clock: one row per **primary-camera** frame (≈ dataset fps); with no camera
the **action** stream drives it. Latest ``action``/``observation_state`` are
paired with each frame.

Config via env (set in the dataflow node ``env``):
  LEROBOT_REPO_ID, LEROBOT_ROOT, LEROBOT_FPS, LEROBOT_TASK, LEROBOT_ROBOT_TYPE,
  TR_ACTION_DIM, TR_STATE_DIM, TR_CAMERAS="front:480x640,wrist:480x640",
  TR_CAM_BGR(=true), TR_STREAMING_ENCODING(=true), TR_ENCODER_THREADS,
  TR_ENCODER_QUEUE(=30), TR_BATCH_ENCODING(=1)
"""

from __future__ import annotations

import os

import numpy as np

from .schema import CameraSpec, DatasetSpec
from .writer import EpisodeWriter, LerobotEpisodeWriter


def _envbool(name: str, default: bool) -> bool:
    return os.environ.get(name, str(default)).strip().lower() in {"1", "true", "yes", "on"}


def _envint_opt(name: str) -> int | None:
    v = os.environ.get(name)
    return int(v) if v not in (None, "") else None


def _parse_cameras(spec: str) -> list[CameraSpec]:
    cams: list[CameraSpec] = []
    for item in filter(None, (s.strip() for s in spec.split(","))):
        name, _, dims = item.partition(":")
        h, _, w = dims.partition("x")
        cams.append(CameraSpec(name=name, height=int(h), width=int(w)))
    return cams


def _camera_for_input(input_id: str, cams: list[CameraSpec]) -> CameraSpec | None:
    for cam in cams:
        if input_id == f"observation_images_{cam.name}":
            return cam
    return None


def _to_numpy(value) -> np.ndarray:
    # DORA delivers pyarrow arrays; fall back gracefully for plain buffers.
    try:
        return value.to_numpy(zero_copy_only=False)
    except AttributeError:
        return np.asarray(value)


def _vec_f32(value) -> np.ndarray:
    """action / observation.state -> contiguous (dof,) float32."""
    return np.ascontiguousarray(_to_numpy(value), dtype=np.float32)


def _image_hwc_rgb(value, height: int, width: int, channels: int, bgr: bool) -> np.ndarray:
    """flat uint8 -> contiguous (H, W, C) uint8, RGB (lerobot expects RGB)."""
    img = _to_numpy(value).astype(np.uint8, copy=False).reshape(height, width, channels)
    if bgr:  # OpenCV capture is BGR
        img = img[:, :, ::-1]
    return np.ascontiguousarray(img)


def _event_metadata(event) -> dict:
    try:
        return event["metadata"] or {}
    except (KeyError, TypeError):
        return {}


def _episode_keep(event) -> bool:
    """Decode the episode outcome forwarded by the robot-side bridge (task F2).

    Convention (finalized in F2): ``metadata['outcome'] ∈ {success, fail,
    rerecord}``. Defaults to *success* (keep) when absent.
    """
    outcome = str(_event_metadata(event).get("outcome", "success")).strip().lower()
    return outcome not in {"fail", "failure", "rerecord", "discard"}


class RecorderConfig:
    def __init__(self) -> None:
        self.repo_id = os.environ.get("LEROBOT_REPO_ID", "local/teleop")
        self.root = os.environ.get("LEROBOT_ROOT", "./datasets")
        self.fps = int(os.environ.get("LEROBOT_FPS", "30"))
        self.task = os.environ.get("LEROBOT_TASK", "teleoperation")
        self.robot_type = os.environ.get("LEROBOT_ROBOT_TYPE", "generic")
        self.action_dim = int(os.environ.get("TR_ACTION_DIM", "6"))
        self.state_dim = int(os.environ.get("TR_STATE_DIM", "6"))
        self.cameras = _parse_cameras(os.environ.get("TR_CAMERAS", ""))
        self.bgr = _envbool("TR_CAM_BGR", True)
        # Encoding knobs (see docs/recording-video-encoding-performance.md).
        self.streaming_encoding = _envbool("TR_STREAMING_ENCODING", True)
        self.encoder_threads = _envint_opt("TR_ENCODER_THREADS")
        self.encoder_queue = int(os.environ.get("TR_ENCODER_QUEUE", "30"))
        self.batch_encoding = int(os.environ.get("TR_BATCH_ENCODING", "1"))

    @property
    def primary_camera(self) -> str | None:
        return self.cameras[0].name if self.cameras else None

    def spec(self) -> DatasetSpec:
        return DatasetSpec(
            action_dim=self.action_dim,
            state_dim=self.state_dim,
            fps=self.fps,
            cameras=self.cameras,
        )


class Recorder:
    """Frame assembly + episode decisions. lerobot-free; testable with a spy writer."""

    def __init__(self, cfg: RecorderConfig, writer: EpisodeWriter) -> None:
        self.cfg = cfg
        self.writer = writer
        self.spec = cfg.spec()
        self._image_keys = self.spec.image_keys()
        self._latest: dict[str, np.ndarray] = {}
        self._frames_in_episode = 0

    def update(self, key: str, array: np.ndarray) -> None:
        self._latest[key] = array

    def _have_full_frame(self) -> bool:
        if "action" not in self._latest or "observation.state" not in self._latest:
            return False
        return all(k in self._latest for k in self._image_keys)

    def record_frame(self) -> None:
        """Compose one dataset row from the latest values and hand it to the writer."""
        if not self._have_full_frame():
            return  # still warming up (no state / not all cameras seen yet)
        frame: dict = {
            "action": self._latest["action"],
            "observation.state": self._latest["observation.state"],
            # v3.0: `task` is a KEY of the frame dict (dataset_writer pops it).
            "task": self.cfg.task,
        }
        for key in self._image_keys:
            frame[key] = self._latest[key]  # (H, W, C) uint8 RGB
        self.writer.add_frame(frame)
        self._frames_in_episode += 1

    def end_episode(self, keep: bool = True) -> None:
        """End the current episode: keep (success) → save; otherwise → discard."""
        if self._frames_in_episode == 0:
            return
        if keep:
            self.writer.save_episode()
        else:
            self.writer.discard()
        self._frames_in_episode = 0

    def finalize(self) -> None:
        # An episode still open at shutdown was never marked success → discard.
        if self._frames_in_episode > 0:
            self.writer.discard()
            self._frames_in_episode = 0
        self.writer.finalize()


def main() -> None:
    from dora import Node  # noqa: PLC0415

    cfg = RecorderConfig()
    writer = LerobotEpisodeWriter(
        cfg.spec(),
        cfg.repo_id,
        cfg.root,
        cfg.robot_type,
        streaming_encoding=cfg.streaming_encoding,
        encoder_threads=cfg.encoder_threads,
        encoder_queue=cfg.encoder_queue,
        batch_encoding=cfg.batch_encoding,
    )
    rec = Recorder(cfg, writer)
    primary = cfg.primary_camera
    node = Node()

    for event in node:
        if event["type"] != "INPUT":
            if event["type"] == "STOP":
                break
            continue

        input_id = event["id"]
        if input_id == "episode_end":
            rec.end_episode(keep=_episode_keep(event))
            continue

        if input_id == "action":
            rec.update("action", _vec_f32(event["value"]))
            if primary is None:  # state-only dataset: action is the clock
                rec.record_frame()
        elif input_id == "observation_state":
            rec.update("observation.state", _vec_f32(event["value"]))
        else:
            cam = _camera_for_input(input_id, cfg.cameras)
            if cam is not None:
                md = _event_metadata(event)
                h = int(md.get("height", cam.height))
                w = int(md.get("width", cam.width))
                bgr = str(md.get("encoding", "bgr8" if cfg.bgr else "rgb8")).lower().startswith("bgr")
                rec.update(
                    f"observation.images.{cam.name}",
                    _image_hwc_rgb(event["value"], h, w, cam.channels, bgr),
                )
                if cam.name == primary:  # primary camera drives the dataset fps clock
                    rec.record_frame()

    rec.finalize()


if __name__ == "__main__":
    main()
