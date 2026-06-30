"""Conformance gate: load a produced dataset back through ``LeRobotDataset``.

If lerobot can open it and the expected keys are present, the Rust/DORA side
really did emit a valid LeRobotDataset v3. Run this in CI after recording.

    python -m tr_lerobot.validate --repo-id local/teleop --root ./datasets
"""

from __future__ import annotations

import argparse
import sys


def validate(repo_id: str, root: str) -> int:
    from lerobot.datasets import LeRobotDataset  # noqa: PLC0415

    dataset = LeRobotDataset(repo_id, root=root)
    n = len(dataset)
    if n == 0:
        print("FAIL: dataset is empty")
        return 1

    sample = dataset[0]
    required = {"action", "observation.state"}
    missing = required - set(sample.keys())
    if missing:
        print(f"FAIL: sample missing keys: {sorted(missing)}")
        return 1

    print(f"OK: LeRobotDataset v3 loaded — {n} frames, sample keys: {sorted(sample.keys())}")
    return 0


def main() -> None:
    parser = argparse.ArgumentParser(description="Validate a LeRobotDataset v3.")
    parser.add_argument("--repo-id", default="local/teleop")
    parser.add_argument("--root", default="./datasets")
    args = parser.parse_args()
    sys.exit(validate(args.repo_id, args.root))


if __name__ == "__main__":
    main()
