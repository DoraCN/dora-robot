"""DORA camera node — captures frames from a USB camera and outputs Arrow arrays.

Usage in dataflow YAML:
  nodes:
    - id: camera_front
      path: python
      args: -m tr_lerobot.camera_node
      outputs:
        - image
      env:
        TR_CAMERA_ID: "0x2122000046d082d"   # macOS Unique ID
        TR_CAMERA_WIDTH: "640"
        TR_CAMERA_HEIGHT: "480"
        TR_CAMERA_FPS: "30"
"""

from __future__ import annotations

import os
import time

import cv2
import numpy as np


def _env(name: str, default: str) -> str:
    return os.environ.get(name, default).strip()


def main() -> None:
    from dora import Node

    camera_id = _env("TR_CAMERA_ID", "0")
    width = int(_env("TR_CAMERA_WIDTH", "640"))
    height = int(_env("TR_CAMERA_HEIGHT", "480"))
    target_fps = float(_env("TR_CAMERA_FPS", "30"))

    if camera_id.isdigit():
        cap = cv2.VideoCapture(int(camera_id))
    else:
        cap = cv2.VideoCapture(camera_id)

    cap.set(cv2.CAP_PROP_FRAME_WIDTH, width)
    cap.set(cv2.CAP_PROP_FRAME_HEIGHT, height)
    cap.set(cv2.CAP_PROP_FPS, target_fps)

    if not cap.isOpened():
        raise RuntimeError(f"cannot open camera: {camera_id}")

    actual_w = int(cap.get(cv2.CAP_PROP_FRAME_WIDTH))
    actual_h = int(cap.get(cv2.CAP_PROP_FRAME_HEIGHT))
    print(f"[camera] {camera_id} {actual_w}x{actual_h} @ {target_fps}fps")

    node = Node()
    interval = 1.0 / target_fps
    last = time.monotonic()

    for event in node:
        if event["type"] == "STOP":
            break

        now = time.monotonic()
        if now - last < interval:
            continue
        last = now

        ret, frame_bgr = cap.read()
        if not ret:
            continue

        frame_rgb = cv2.cvtColor(frame_bgr, cv2.COLOR_BGR2RGB)

        # flat HWC uint8
        data = frame_rgb.flatten().astype(np.uint8)

        node.send_output(
            "image",
            data,
            {
                "width": str(actual_w),
                "height": str(actual_h),
                "encoding": "rgb8",
                "channels": "3",
            },
        )

    cap.release()


if __name__ == "__main__":
    main()
