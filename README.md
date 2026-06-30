# dora-robot

A **three-tier decoupled teleoperation platform** for heterogeneous robots, built in
Rust on top of the [DORA](https://github.com/dora-rs/dora) dataflow framework.

```
   ┌─────────────────┐        ┌──────────────────────┐        ┌─────────────────┐
   │   TELEOP TIER    │       │   COMMUNICATION TIER   │       │   ROBOT TIER     │
   │  (operator side) │◄─────►│   (pluggable bridge)   │◄─────►│ (controlled side)│
   │                  │  unified message contract over │      │                  │
   │ keyboard / gamepad│  transport (USB/WiFi/ETH/4G/5G │      │ any robot driver │
   │ VR / isomorphic   │   /BLE/NearLink ...)           │      │ + IK/retargeting │
   │ arm (master)      │                                │      │ + force feedback │
   └─────────────────┘        └──────────────────────┘        └─────────────────┘
        DORA dataflow A             middleware                     DORA dataflow B
```

The three tiers are **independently replaceable**:

- **Teleop tier** — any input device emits a *canonical, device-agnostic command*.
- **Communication tier** — a pluggable transport carries the canonical contract;
  swap USB ↔ WiFi ↔ 4G/5G ↔ Bluetooth ↔ NearLink (星闪) without touching either end.
- **Robot tier** — any robot driver consumes the canonical command and performs
  retargeting / inverse kinematics locally; optionally streams force feedback back.

See **[docs/architecture.md](docs/architecture.md)** for the full technical design.

## Workspace layout

| Crate              | Tier      | Responsibility                                              |
|--------------------|-----------|-------------------------------------------------------------|
| `tr-messages`      | contract  | Canonical command/feedback schema, capability negotiation, `Codec` |
| `tr-transport`     | comm      | `Transport` trait + QoS + framing; TCP/UDP real, USB/BLE/NearLink/cellular stubs |
| `tr-session`       | comm      | Session lifecycle, capability handshake, heartbeat, reconnect |
| `tr-teleop`        | teleop    | `TeleopDevice` trait + device adapters (keyboard/gamepad/VR/isomorphic-arm) |
| `tr-robot`         | robot     | `RobotDriver` trait, `Kinematics` (FK/IK), robot model, drivers |
| `tr-teleop-node`   | teleop    | DORA node: device → canonical command → bridge              |
| `tr-robot-node`    | robot     | DORA node: bridge → retarget/IK → robot; feedback back      |
| `tr-bridge`        | comm      | DORA node: dataflow ⇄ transport middleware                  |
| `tr-policy`        | learning  | DORA node: replay a trained policy (no human in the loop)   |

Plus a Python subproject:

| Path               | Tier      | Responsibility                                              |
|--------------------|-----------|-------------------------------------------------------------|
| `training/`        | learning  | LeRobot **v3** recorder (DORA Python node), conformance validation, training |

## LeRobot v3 (record / train)

Recording targets **LeRobotDataset v3** (`docs/lerobot-dataset-v3.mdx`). The
recorder is a **DORA Python node** that drives lerobot's own v3 writer, so
conformance (chunked Parquet, episode offset tables, `finalize()` footers) is
guaranteed by construction. lerobot is **pinned** in `training/requirements.txt`,
not vendored as a git submodule. See **[training/README.md](training/README.md)**.

## Build

```sh
cargo build            # std-only skeleton, compiles offline
cargo test
```

> The skeleton uses **only the standard library** so it builds without network
> access. Each crate documents the production dependency to add
> (`serde`, `prost`, `nalgebra`, `dora-node-api`, `tokio`, `mcap`, ...).

## Run a teleop session (target topology)

```sh
# robot machine
dora up && dora start dataflows/robot_side.yml

# operator machine
dora up && dora start dataflows/teleop_side.yml

# record a LeRobotDataset v3 (robot machine; use instead of robot_side.yml)
dora start dataflows/record.yml

# autonomous replay of a trained policy
dora start dataflows/replay.yml
```
