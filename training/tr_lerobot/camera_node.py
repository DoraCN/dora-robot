"""DORA camera node — driven by dora/timer/millis/33 tick input.

Captures a frame on each tick event and sends it as an Arrow array.

Usage in dataflow YAML:
  nodes:
    - id: camera_front
      path: ../training/tr_lerobot/camera_node.py
      inputs:
        tick: dora/timer/millis/33
      outputs:
        - image
      env:
        TR_CAMERA_ID: "0"
"""

from __future__ import annotations

import os

import cv2
import numpy as np


def _env(name: str, default: str) -> str:
    return os.environ.get(name, default).strip()


def main() -> None:
    from dora import Node

    camera_id = _env("TR_CAMERA_ID", "0")
    width = int(_env("TR_CAMERA_WIDTH", "640"))
    height = int(_env("TR_CAMERA_HEIGHT", "480"))

    if camera_id.isdigit():
        cap = cv2.VideoCapture(int(camera_id))
    else:
        cap = cv2.VideoCapture(camera_id)
    cap.set(cv2.CAP_PROP_FRAME_WIDTH, width)
    cap.set(cv2.CAP_PROP_FRAME_HEIGHT, height)
    if not cap.isOpened():
        raise RuntimeError(f"cannot open camera: {camera_id}")

    actual_w = int(cap.get(cv2.CAP_PROP_FRAME_WIDTH))
    actual_h = int(cap.get(cv2.CAP_PROP_FRAME_HEIGHT))
    print(f"[camera] {camera_id} {actual_w}x{actual_h}")

    node = Node()

    for event in node:
        if event["type"] == "STOP":
            break
        if event["type"] != "INPUT":
            continue

        ret, frame_bgr = cap.read()
        if not ret:
            continue

        frame_rgb = cv2.cvtColor(frame_bgr, cv2.COLOR_BGR2RGB)
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
