#!/usr/bin/env python3
"""Standalone pipe recorder — consumes frames + episode events from stdin.

Protocol (one line per item):
  D j1 j2 j3 j4 j5 j6     data frame (radians)
  @START                  begin a new episode
  @SUCCESS                end current episode → save
  @FAIL                   end current episode → discard
  @RERECORD               end current episode → discard, stay in recording
  @STOP                   quit: finalize + exit (also stops on EOF / SIGTERM)

Usage:
  cargo run ... --record | python -m tr_lerobot.pipe_recorder --repo-id my/teleop --task "grab cube"
"""

from __future__ import annotations

import argparse
import os
import signal
import sys

import numpy as np

from .schema import DatasetSpec
from .writer import LerobotEpisodeWriter


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-id", default=os.environ.get("TR_REPO_ID", "local/teleop"))
    parser.add_argument("--root", default=os.environ.get("TR_ROOT", "./datasets"))
    parser.add_argument("--fps", type=int, default=int(os.environ.get("TR_FPS", "30")))
    parser.add_argument("--task", default=os.environ.get("TR_TASK", "teleoperation"))
    parser.add_argument("--action-dim", type=int, default=6)
    parser.add_argument("--state-dim", type=int, default=6)
    args = parser.parse_args()

    spec = DatasetSpec(action_dim=args.action_dim, state_dim=args.state_dim, fps=args.fps)
    writer = LerobotEpisodeWriter(
        spec, repo_id=args.repo_id, root=args.root, use_videos=False,
    )

    state = "IDLE"  # IDLE | RECORDING
    frame_count = 0
    episode_count = 0

    def finish(msg: str) -> None:
        if episode_count > 0:
            writer.finalize()
        print(f"[recorder] {msg} (episodes={episode_count})", file=sys.stderr)
        sys.exit(0)

    # Graceful shutdown on SIGTERM / SIGINT (pipe closed)
    signal.signal(signal.SIGTERM, lambda *_: finish("SIGTERM"))
    signal.signal(signal.SIGINT, lambda *_: finish("SIGINT"))

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue

        if line.startswith("D "):
            parts = line.split()
            if len(parts) != 7:
                continue
            try:
                joints = np.array([float(x) for x in parts[1:7]], dtype=np.float32)
            except ValueError:
                continue
            if state == "RECORDING":
                frame = {
                    "action": joints,
                    "observation.state": joints,
                    "task": args.task,
                }
                writer.add_frame(frame)
                frame_count += 1
        elif line == "@START":
            if state == "RECORDING":
                writer.discard()  # discard incomplete previous
            state = "RECORDING"
            frame_count = 0
            print("[recorder] ▶ episode started", file=sys.stderr)
        elif line == "@SUCCESS":
            if state == "RECORDING" and frame_count > 0:
                writer.save_episode()
                episode_count += 1
                print(f"[recorder] ✅ saved ({frame_count} frames)", file=sys.stderr)
            state = "IDLE"
        elif line == "@FAIL":
            if state == "RECORDING" and frame_count > 0:
                writer.discard()
                print(f"[recorder] ❌ discarded ({frame_count} frames)", file=sys.stderr)
            state = "IDLE"
        elif line == "@RERECORD":
            if state == "RECORDING" and frame_count > 0:
                writer.discard()
                print(f"[recorder] 🔄 discarded, rerecord ({frame_count} frames)", file=sys.stderr)
            state = "RECORDING"
            frame_count = 0
        elif line == "@STOP":
            break

    finish("EOF")


if __name__ == "__main__":
    main()
