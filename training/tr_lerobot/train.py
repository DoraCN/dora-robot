"""Thin wrapper around lerobot's training entrypoint.

Keeps training in this isolated Python subproject; the Rust/DORA core only
*produces* the dataset. Adjust the policy type and the exact lerobot training
CLI/args to match the pinned lerobot version.

    python -m tr_lerobot.train --repo-id local/teleop --root ./datasets --policy act
"""

from __future__ import annotations

import argparse
import subprocess
import sys


def main() -> None:
    parser = argparse.ArgumentParser(description="Train a policy on a recorded dataset.")
    parser.add_argument("--repo-id", default="local/teleop")
    parser.add_argument("--root", default="./datasets")
    parser.add_argument("--policy", default="act", help="act | diffusion | tdmpc | ...")
    parser.add_argument("--output-dir", default="./checkpoints")
    args, extra = parser.parse_known_args()

    # NOTE: confirm the exact training entrypoint/flags for the pinned lerobot.
    cmd = [
        sys.executable,
        "-m",
        "lerobot.scripts.train",
        f"--dataset.repo_id={args.repo_id}",
        f"--dataset.root={args.root}",
        f"--policy.type={args.policy}",
        f"--output_dir={args.output_dir}",
        *extra,
    ]
    print("running:", " ".join(cmd))
    raise SystemExit(subprocess.call(cmd))


if __name__ == "__main__":
    main()
