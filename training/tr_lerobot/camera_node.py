"""DORA camera node — captures frames from a USB camera and outputs Arrow arrays.

This node has no DORA inputs — it captures frames on its own clock and
pushes them as outputs. The DORA event loop is used only to detect STOP.

Usage in dataflow YAML:
  nodes:
    - id: camera_front
      path: ../training/tr_lerobot/camera_node.py
      outputs:
        - image
      env:
        TR_CAMERA_ID: "0"
        TR_CAMERA_WIDTH: "640"
        TR_CAMERA_HEIGHT: "480"
        TR_CAMERA_FPS: "30"
"""

from __future__ import annotations

import os
import time
from threading import Lock

import cv2
import numpy as np


def _env(name: str, default: str) -> str:
    return os.environ.get(name, default).strip()


class FrameGrabber:
    """Background camera reader — continuously captures the latest frame."""

    def __init__(self, camera_id: str, width: int, height: int):
        if camera_id.isdigit():
            self._cap = cv2.VideoCapture(int(camera_id))
        else:
            self._cap = cv2.VideoCapture(camera_id)
        self._cap.set(cv2.CAP_PROP_FRAME_WIDTH, width)
        self._cap.set(cv2.CAP_PROP_FRAME_HEIGHT, height)
        if not self._cap.isOpened():
            raise RuntimeError(f"cannot open camera: {camera_id}")
        self._actual_w = int(self._cap.get(cv2.CAP_PROP_FRAME_WIDTH))
        self._actual_h = int(self._cap.get(cv2.CAP_PROP_FRAME_HEIGHT))
        self._lock = Lock()
        self._latest: np.ndarray | None = None
        self._running = True

    @property
    def width(self) -> int:
        return self._actual_w

    @property
    def height(self) -> int:
        return self._actual_h

    def start(self):
        """Spawn a background thread that continuously grabs frames."""
        import threading
        t = threading.Thread(target=self._run, daemon=True)
        t.start()
        return t

    def _run(self):
        import time
        while self._running:
            ret, frame = self._cap.read()
            if ret:
                rgb = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
                with self._lock:
                    self._latest = rgb
            else:
                time.sleep(0.001)

    def latest_rgb(self) -> np.ndarray | None:
        with self._lock:
            return self._latest.copy() if self._latest is not None else None

    def stop(self):
        self._running = False
        self._cap.release()


def main() -> None:
    from dora import Node

    camera_id = _env("TR_CAMERA_ID", "0")
    width = int(_env("TR_CAMERA_WIDTH", "640"))
    height = int(_env("TR_CAMERA_HEIGHT", "480"))
    target_fps = float(_env("TR_CAMERA_FPS", "30"))

    grabber = FrameGrabber(camera_id, width, height)
    grabber.start()
    print(f"[camera] {camera_id} {grabber.width}x{grabber.height} @ {target_fps}fps")

    node = Node()
    interval = 1.0 / target_fps
    last_send = time.monotonic()

    # Process DORA events (only STOP matters) and send frames on our clock.
    for event in node:
        if event["type"] == "STOP":
            break

        now = time.monotonic()
        if now - last_send < interval:
            continue
        last_send = now

        frame = grabber.latest_rgb()
        if frame is None:
            continue

        data = frame.flatten().astype(np.uint8)
        node.send_output(
            "image",
            data,
            {
                "width": str(grabber.width),
                "height": str(grabber.height),
                "encoding": "rgb8",
                "channels": "3",
            },
        )

    grabber.stop()


if __name__ == "__main__":
    main()
