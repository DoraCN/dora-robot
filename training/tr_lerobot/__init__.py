"""dora-robot <-> LeRobot v3 bridge.

This isolated Python subproject is the *downstream* of the (Rust/DORA) realtime
teleoperation core. It never runs on the control loop:

- ``recorder``  : a DORA Python node that consumes the dataflow (action /
  observation.state / camera images) and writes a **LeRobotDataset v3** using
  lerobot's own writer — so v3 conformance (chunked parquet, episode offsets,
  ``finalize()`` footers) is guaranteed by construction.
- ``validate``  : loads the produced dataset back through ``LeRobotDataset`` as
  an executable conformance gate.
- ``train``     : thin wrapper around lerobot's training entrypoint.

lerobot is pinned here (see ``requirements.txt``); it is NOT vendored into the
Rust workspace as a git submodule.
"""

__all__ = ["schema", "recorder", "validate", "train"]
