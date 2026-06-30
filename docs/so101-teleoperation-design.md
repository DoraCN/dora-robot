# SO-101 Homogeneous Teleoperation — Design (M1)

> Status: **design for review** (no code yet).
> M1 goal: drive a **follower SO-101** with a **leader SO-101** — **two separate
> physical arms**, both built on Feetech servos — over the **network via zenoh
> 1.9.0**, by adding **one arm *type* `SO101`** instantiated independently on each
> side.

---

## 0. Correction vs. the previous draft

The two ends do **not** share one arm. There is **one arm *type* (`SO101`) and two
independent *instances***: a leader arm (operator machine, its own serial bus +
servos) and a follower arm (robot machine, its own serial bus + servos). They are
related **only through the canonical `JointTargets` contract over zenoh** — there
is no shared driver instance, no shared bus, no shared calibration object.

This matches the SDK's own example, which creates **two separate `FeetechBus`
objects** (`leader`, `follower`) on two different ports.

---

## 1. Scope

- Hardware: **two** SO-101 arms (same *model*, two physical units). One **leader**
  (human backdrives it, torque off), one **follower** (torque on, mimics leader).
- Servo layout (confirmed from the SDK examples): **6 Feetech servos, IDs 1–6**:

  | ID | Joint            |
  |----|------------------|
  | 1  | base (底座)       |
  | 2  | shoulder (肩部)   |
  | 3  | elbow (肘部)      |
  | 4  | wrist roll (腕旋) |
  | 5  | wrist flex (腕弯) |
  | 6  | gripper (夹爪)    |

- Control mode: **`JointTargets`** — isomorphic arms ⇒ the leader's 6 joint angles
  are written verbatim as the follower's goals. **No IK / no retargeting in M1.**
- Inter-machine transport: **zenoh 1.9.0**.
- Local servo bus: **`feetech-servo-sdk` 0.3.0** (serial @ 1 Mbps).

Two distinct "communications" — keep them separate:

| Layer            | Medium                    | Library              |
|------------------|---------------------------|----------------------|
| servo bus (local)| serial / USB-TTL @ 1 Mbps | `feetech-servo-sdk`  |
| arm ↔ arm (net)  | LAN / WiFi / 4G/5G        | `zenoh 1.9.0`        |

---

## 2. Analysis of the SDK example `examples/teleop_simple_safe.rs`

This example is the reference master-slave (主从) logic. It runs **both arms in one
process on one machine**; our job is to split that loop across the network.

### 2.1 Setup

```rust
use feetech_servo_sdk::{ControlOp, FeetechBus, MotorBus};

let mut leader   = FeetechBus::new(&args.leader_port,   1_000_000)?; // e.g. /dev/ttyUSB0
let mut follower = FeetechBus::new(&args.follower_port, 1_000_000)?; // e.g. /dev/ttyUSB1
let ids = [1,2,3,4,5,6];                                             // 6 servos

leader.disable_torque(&ids).await?;   // leader: torque OFF  → hand-backdrivable
follower.enable_torque(&ids).await?;  // follower: torque ON → actively mimics
```

→ Two independent bus instances; the **role asymmetry is exactly: torque off (read
side) vs. torque on (write side)**.

### 2.2 Anti-jerk alignment (before the loop)

```rust
let start = leader.sync_read_positions(&ids).await?;                 // Vec<f32>, radians
let cmds  = ids.iter().zip(&start).map(|(&id,&p)| (id, ControlOp::Position(p)));
follower.sync_write_goals(&cmds.collect()).await?;                  // move follower to leader
```

→ The follower is first driven to the leader's current pose so it doesn't snap
("防止瞬间弹射") when the loop starts. **We must keep this.**

### 2.3 The teleop loop (100 Hz)

```rust
let mut interval = tokio::time::interval(Duration::from_millis(10)); // 100 Hz
loop {
  tokio::select! {
    _ = interval.tick() => {
      let positions = leader.sync_read_positions(&ids).await?;        // read leader (rad)
      let commands  = ids.iter().zip(&positions)
                         .map(|(&id,&p)| (id, ControlOp::Position(p)));
      follower.sync_write_goals(&commands.collect()).await?;         // write follower
    }
    res = &mut ctrl_c => break,                                       // Ctrl+C → park
  }
}
```

→ The whole control law is: **every 10 ms, copy leader joint radians → follower
goal radians.** No calibration math, no IK. It assumes both arms share the same
mechanical zero (achieved physically, see §6). Transient read errors are tolerated
(`warn!` + `continue`); write errors `error!` + continue.

### 2.4 Safe shutdown

On Ctrl+C it `move_smoothly(follower, ids, SAFE_PARK_POSE, park_duration)` — a
software **linear interpolation at 50 Hz** from the current pose to a safe park
pose, then `follower.disable_torque(&ids)`. Note `SAFE_PARK_POSE` is in **degrees**
(`[0, -105, 90, 74, 0, 0]`) and converted with `.to_radians()` before sending.

### 2.5 Confirmed API surface (now grounded, not guessed)

| Call                                              | Meaning                          |
|---------------------------------------------------|----------------------------------|
| `FeetechBus::new(port, 1_000_000) -> Result`      | open serial bus (sync ctor)      |
| `enable_torque(&[u8]).await`                       | torque on (follower)             |
| `disable_torque(&[u8]).await`                      | torque off (leader, backdrive)   |
| `sync_read_positions(&[u8]).await -> Vec<f32>`     | batch read, **radians**          |
| `sync_write_goals(&[(u8, ControlOp)]).await`       | batch write goals                |
| `ControlOp::Position(f32 /*rad*/)`                 | position goal                    |
| `ControlOp::RawEffort(raw)`                        | raw effort (M3 bilateral)        |

---

## 3. Mapping the example onto our distributed architecture

The example's single in-process loop becomes a **split** loop with the canonical
contract + zenoh inserted between the *read* (leader) and the *write* (follower):

```
   teleop_simple_safe.rs  (one machine)            our M1  (two machines)
   ─────────────────────────────────────           ─────────────────────────────────
   leader.sync_read_positions()      ───►   So101Leader.poll()  (operator machine)
                                            → JointTargets{positions}  (radians)
            │                                        │  zenoh tr/<s>/control
            ▼                                        ▼
   follower.sync_write_goals()        ◄───   So101Follower.command()  (robot machine)
                                            → sync_write_goals(Position)
```

So: the **read half** lives in `So101Leader` (teleop tier), the **write half** in
`So101Follower` (robot tier), and the verbatim copy that was a local function call
is now a `JointTargets` message traveling over zenoh. The anti-jerk align (§2.2),
error tolerance (§2.3) and safe parking (§2.4) carry over.

---

## 4. The core design: one `SO101` type, two independent instances

Add **one crate `crates/tr-so101`** containing one hardware driver and two thin
role adapters. Each side constructs its **own** instance against its **own** bus.

```
                 crates/tr-so101  (one arm TYPE)
   ┌──────────────────────────────────────────────────────┐
   │  So101Arm<B: MotorBus>   ← driver code (one copy)     │
   │   • owns ONE FeetechBus (this machine's arm)          │
   │   • read_joints()/write_joints()/set_torque()         │
   │   • generic over MotorBus → real bus OR MockBus       │
   └───────────────┬──────────────────────┬───────────────┘
        instance A (operator)     instance B (robot)
                   │                      │
        ┌──────────▼─────────┐  ┌─────────▼────────────────┐
        │ So101Leader        │  │ So101Follower            │
        │  impl TeleopDevice │  │  impl RobotDriver        │
        │  torque OFF, read  │  │  torque ON, write        │
        └────────────────────┘  └──────────────────────────┘
```

The two instances are **fully independent hardware-wise** (own port, own servos,
own zeroing). Their only link is the canonical `JointTargets` stream over zenoh.
**Adding SO-101 = adding this one crate** (+ selecting it in the nodes); the
contract / transport / session crates are untouched.

---

## 5. New crate `crates/tr-so101`

```
crates/tr-so101/
  Cargo.toml      # feetech-servo-sdk = "0.3", tokio, tr-messages, tr-teleop,
                  # tr-robot ; feature "mock" → MockBus (HIL-free tests)
  src/
    arm.rs        # So101Arm<B: MotorBus> : open bus, set_torque,
                  #   read_joints()->[f32;6] (sync_read_positions),
                  #   write_joints(&[f32;6]) (sync_write_goals + ControlOp::Position)
    leader.rs     # So101Leader  : tr_teleop::TeleopDevice
    follower.rs   # So101Follower: tr_robot::RobotDriver
    config.rs     # ports, ids [1..6], rate, limits, optional zero offsets
```

- `So101Leader::poll()` → `read_joints()` → `CommandBody::Joint(JointTargets)`
  (mode `JointTargets`, dof 6); torque kept **off**.
- `So101Follower::command(cmd)` → clamp + slew-limit → `write_joints()`; torque
  **on**. `read_state()` → `JointState`. `e_stop()` → `set_torque(false)`.
- `So101Arm` is **generic over `MotorBus`**, so `MockBus` gives a full software
  test of leader→follower without any hardware.

> The adapters mirror the example's calls exactly: leader = `disable_torque` +
> `sync_read_positions`; follower = `enable_torque` + `sync_write_goals(Position)`.

---

## 6. Calibration & bring-up — use the SDK's own examples

The teleop example does **no software calibration**: it relies on both arms being
**physically zeroed the same**. The SDK ships the tools for that, so we do **not**
invent a calibration scheme:

| Task                          | SDK example      |
|-------------------------------|------------------|
| discover servos on the bus    | `scan.rs`        |
| assign IDs 1–6                | `set_id.rs`      |
| set servo mid/zero position   | `set_mid.rs`     |
| move arm to zero pose         | `to_zero.rs`     |
| read load/current/temp (M3)   | `monitor.rs`, `read_info.rs` |
| raw effort / current control  | `raw_effort.rs`  |

**M1 plan:** zero each arm with `set_mid` so leader and follower share a joint
frame, then do the verbatim copy (as the example does). *Optionally* `tr-so101`
may add a per-joint software offset/sign in `config.rs` for arms that can't be
perfectly co-zeroed — but it is not required to reproduce the example.

---

## 7. Transport: zenoh 1.9.0

Add zenoh as a **new backend of the existing `tr_transport::Transport` trait**, in
a separate crate so `tr-transport` stays std-only.

```
crates/tr-transport-zenoh/   # zenoh = "1.9.0", tr-transport
  src/lib.rs                 # ZenohTransport : tr_transport::Transport
```

- Key expressions per `Channel`: `tr/<session>/control`, `tr/<session>/feedback`,
  `tr/<session>/handshake`.
- `send` → `publisher.put(bytes)`; `recv` → subscriber receive → `Sample` payload.
- QoS map (`tr_transport::Qos` → zenoh 1.9.0): `Reliable` → reliability Reliable +
  `CongestionControl::Block`; high-rate control → `CongestionControl::Drop`;
  `priority` → zenoh `Priority`.
- Discovery: zenoh scouting (peer mode on LAN; router for WAN/4G-5G) removes the
  hardcoded IP that raw TCP/UDP needs.

> Pin `zenoh = "1.9.0"`; confirm the exact 1.9 API (`zenoh::open`,
> `declare_publisher`/`declare_subscriber`, `put`, `recv_async`, `Sample::payload`).

---

## 8. End-to-end dataflow (M1)

```
OPERATOR MACHINE (DORA dataflow A)            ROBOT MACHINE (DORA dataflow B)
┌──────────────────────────────┐             ┌──────────────────────────────┐
│ tr-teleop-node               │             │ tr-bridge (ZenohTransport)    │
│   So101Leader (torque OFF)   │   zenoh     │   sub tr/<s>/control          │
│   poll @ 100 Hz              │  1.9.0      │        ▼                      │
│   → JointTargets ─ tr-bridge ┼────────────►│ tr-robot-node                 │
│       (pub control)          │  control    │   So101Follower (torque ON)   │
│        ▲ feedback            │◄────────────┼── clamp+slew → sync_write_goals│
└──────────────────────────────┘             └──────────────────────────────┘
   leader SO-101 (own bus)                       follower SO-101 (own bus)
```

Startup: handshake (both advertise `JointTargets`, dof 6) → negotiate
`JointTargets @ 100 Hz` → **anti-jerk align** (follower driven to first received
leader pose before going live) → stream.

---

## 9. Recording data contract — Rust → DORA → LeRobot v3 (M2)

The recording path (M2) reuses the Python recorder in `training/tr_lerobot/`, which
drives **lerobot's own v3 writer**. The Rust nodes therefore emit **only flat, typed
Arrow primitives** into the local DORA dataflow; all v3 structure (chunked parquet,
episode offset tables, `finalize()` footers, video sharding, stats) is produced by
lerobot inside the recorder. The recorder consumes **plain Arrow** and never decodes
`tr-messages`/`Codec`.

> v3 format facts: `docs/lerobot-dataset-v3-format.md`.
> Encoding performance: `docs/recording-video-encoding-performance.md`.

### 9.1 Rust DORA outputs (recording path)

| DORA output | Arrow type | Metadata | Emitted by | → recorder → lerobot key | lerobot feature |
|---|---|---|---|---|---|
| `action` | `Float32Array(6)` | `stamp_nanos` | `tr-robot-node` — the received `JointTargets` (= leader joints over zenoh) | `action` | `float32, (6,)` |
| `observation_state` | `Float32Array(6)` | `stamp_nanos` | `tr-robot-node` — measured joints (`sync_read_positions`) | `observation.state` | `float32, (6,)` |
| `observation_images_<cam>` | `UInt8Array(H*W*3)` flat HWC | `width,height,encoding,stamp_nanos` | camera node | `observation.images.<cam>` | `video, (H,W,3) uint8` |

- 6-vector order = servo IDs 1..6 (base, shoulder, elbow, wrist_roll, wrist_flex,
  gripper), **radians**; `action` and `observation.state` share order + units.
- **dtype**: the canonical `JointTargets.positions` is `f64`; emit `Float32Array`
  (cast once) to match lerobot's `float32` and avoid a re-cast in Python.

### 9.2 Rust must NOT emit (lerobot auto-generates them)

`timestamp`, `frame_index`, `episode_index`, `index`, `task_index` — added inside
`add_frame`/`save_episode` (`dataset_writer.py:206-267`). Sending them conflicts.

### 9.3 Not from the servo loop (control-plane inputs)

- `task` (string): episode-level constant injected by the recorder
  (`LEROBOT_TASK` or a control input) — **not** per-frame from Rust.
- `episode_end` (control signal): triggers `save_episode()`; session stop triggers
  `finalize()`.

### 9.4 Rate & alignment

- Rust emits at natural rates: servo loop ~100 Hz (`action`/`observation_state`),
  cameras ~30 fps. **The recorder samples at the dataset `fps`, keyed to camera
  frames**, pairing the latest `action`/`observation_state` ⇒ one dataset row per
  camera frame ⇒ `rows == fps × duration`, one video frame per row. **Rust does not
  know `fps`.**
- `stamp_nanos` (DORA metadata) is used **only** by the recorder for multi-stream
  alignment + downsampling. It is **not** lerobot's `timestamp` (that is the
  synthetic `frame_index / fps`).

### 9.5 Two distinct follower outputs (do not conflate)

`tr-robot-node` emits two different things:

- **control path → `feedback`**: full `RobotFeedback` (f64 canonical) serialized via
  `Codec`, returned over zenoh (operator / future haptics).
- **recording path → `action` + `observation_state`**: plain `Float32Array` into the
  local dataflow for the recorder.

### 9.6 schema ↔ Rust output ↔ lerobot frame

```
features(create): action: f32[6] | observation.state: f32[6] | observation.images.front: video (H,W,3)
Rust emits:       Float32Array(6)  Float32Array(6)             UInt8Array(H*W*3)+meta
recorder frame:   {"action": np.f32[6], "observation.state": np.f32[6],
                   "observation.images.front": np.uint8[H,W,3], "task": "..."}  → add_frame
```

---

## 10. Safety (adopted from the example + our session layer)

- **Anti-jerk align** (§2.2): follower moves to the first leader pose before the
  live loop.
- **Safe parking**: on stop/e-stop, `move_smoothly` the follower to a park pose,
  then `disable_torque` (reuse the example's interpolation).
- **Error tolerance**: transient `sync_read_positions`/`sync_write_goals` errors →
  log + skip the cycle (don't crash), as the example does.
- **Watchdog**: reuse `tr-session` deadline — control-stream gap ⇒ follower holds,
  then parks/torque-off after a longer timeout; never replays stale targets.
- **E-stop**: reliable, highest-priority channel; honored locally on the follower
  (`set_torque(false)`) even if the link drops.
- **Limits**: clamp + slew-rate limit on the follower before every write.

---

## 11. Async ↔ sync bridge

`feetech-servo-sdk` is async (Tokio); `TeleopDevice::poll` / `RobotDriver::command`
are sync. `So101Arm` runs a **tokio servo task** (the §2.3 loop, one side only) and
exposes the latest joint vector / accepts goals via a lock-free cell + channel, so
the sync DORA node never blocks on the serial bus. (Alternative: async tier traits
— deferred.)

---

## 12. Rates / performance

- Servo loop: **100 Hz** (matches the example's 10 ms tick); 1 Mbps bus + 6 servos
  sync read+write fits comfortably.
- Wire: `JointTargets` = 6×f32 (~tens of bytes) per tick over zenoh LAN — trivial.
- Control rate = `min(leader, follower)` from negotiation (100 Hz in M1).

---

## 13. Phasing

- **M1** (this doc): unilateral verbatim joint teleop, zenoh 1.9.0, `MockBus`
  first then real hardware.
- **M2**: follower state streamed back + monitoring; record to LeRobot v3
  (reuse `training/`).
- **M3**: **bilateral** — follower load/current (`monitor.rs`/`read_info.rs`) →
  leader torque via `ControlOp::RawEffort` (`raw_effort.rs`); passivity vs. RTT.
- **M4**: heterogeneous follower → `CartesianPose` + IK (`Kinematics` exists);
  multi-arm via `Composite`.

---

## 14. Open questions (most SDK items now resolved)

1. **Co-zeroing**: rely purely on `set_mid` physical zeroing (like the example), or
   also add optional per-joint software offsets in `tr-so101` for mismatched arms?
2. **M3 readback**: confirm the exact load/current read method (browse
   `monitor.rs` / `read_info.rs`) — needed only for bilateral.
3. **zenoh 1.9.0**: peer vs. router topology for your network; confirm exact API.
4. **Async strategy**: cell/channel bridge (proposed) vs. async tier traits.
5. **Codec**: M1 needs a real wire codec for `JointTargets` (the contract `Codec`
   is still a placeholder) — propose `postcard` for this Rust↔Rust path.
