"""LeRobotDataset v3 feature schema for dora-robot teleop datasets.

Maps the canonical contract onto the v3 feature naming convention documented in
``docs/lerobot-dataset-v3.mdx``:

- ``action``                     : commanded canonical action, flattened to a
                                   fixed-length vector (joint targets, or
                                   Cartesian pose+gripper).
- ``observation.state``          : robot proprioceptive state (joint positions).
- ``observation.images.<cam>``   : RGB frames (H, W, C) uint8, stored as MP4.

NOTE: the exact ``features`` dict accepted by ``LeRobotDataset.create`` must be
confirmed against the pinned lerobot version.
"""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class CameraSpec:
    name: str
    height: int
    width: int
    channels: int = 3


@dataclass
class DatasetSpec:
    action_dim: int
    state_dim: int
    fps: int = 30
    cameras: list[CameraSpec] = field(default_factory=list)
    action_names: list[str] | None = None
    state_names: list[str] | None = None

    def features(self) -> dict:
        feats: dict = {
            "action": {
                "dtype": "float32",
                "shape": (self.action_dim,),
                "names": self.action_names or [f"a{i}" for i in range(self.action_dim)],
            },
            "observation.state": {
                "dtype": "float32",
                "shape": (self.state_dim,),
                "names": self.state_names or [f"s{i}" for i in range(self.state_dim)],
            },
        }
        for cam in self.cameras:
            feats[f"observation.images.{cam.name}"] = {
                "dtype": "video",
                "shape": (cam.height, cam.width, cam.channels),
                "names": ["height", "width", "channels"],
            }
        return feats

    def image_keys(self) -> list[str]:
        return [f"observation.images.{c.name}" for c in self.cameras]
