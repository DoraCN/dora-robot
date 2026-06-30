# dora-robot — Technical Architecture

> Three-tier decoupled teleoperation platform for heterogeneous robots,
> built in Rust on the DORA dataflow framework.

---

## 1. Goals & non-goals

**Goals**

1. **Three independently replaceable tiers** — *teleop device*, *communication
   medium*, and *robot* can each be swapped without modifying the other two.
2. Support heterogeneous **teleop devices**: keyboard, gamepad/remote, VR
   headset+controllers, isomorphic master arm (leader), ...
3. Support heterogeneous **robots**: arms, dual-arm, mobile bases, humanoids,
   simulators, with arbitrary DoF and kinematics.
4. Support heterogeneous **transports**: USB, WiFi, wired Ethernet, 4G/5G,
   Bluetooth, NearLink (星闪), ... selectable at deploy time.
5. Support **both unilateral and bilateral (force-feedback) teleoperation**,
   configured per device/robot, at control rates from tens of Hz up to ~1 kHz.
6. **Record teleoperation episodes** in LeRobot dataset format, then **replay a
   trained policy** on the robot tier (imitation learning loop).

**Non-goals (for this stage)**

- Implementing every concrete device/robot/transport driver (skeleton ships
  traits + stubs + 1–2 real reference impls such as TCP/UDP).
- Policy *training* (offloaded to LeRobot/PyTorch); we cover *capture* + *replay*.

---

## 2. Design principles & prior art

The design borrows proven patterns from mainstream robotics & systems work:

| Concern                         | Pattern / prior art we follow                                            |
|---------------------------------|--------------------------------------------------------------------------|
| Decoupled nodes + dataflow      | **DORA** (dora-rs), ROS 2 node graph                                     |
| QoS-tagged pub/sub over any wire | **DDS / ROS 2 QoS profiles**, **zenoh** (reliability/durability/deadline) |
| Device-agnostic command space   | ROS `geometry_msgs` (Pose/Twist/Wrench), MoveIt servo, `ros2_control`    |
| Retargeting at the edge         | **GELLO**, **ALOHA/ACT**, **Open-TeleVision** (VR→robot retargeting)     |
| Bilateral haptics               | classic 4-channel teleoperation (position-force / force-force)           |
| Capability negotiation handshake | WebRTC SDP offer/answer, gRPC reflection                                 |
| Dataset capture for IL          | **HuggingFace LeRobot** dataset v2, MCAP/Foxglove for logging            |
| Schema evolution on the wire    | Protobuf/FlatBuffers/Cap'n Proto field-tagging + version field           |

The **canonical command contract** is the single most important decoupling
boundary — it is what lets the three tiers evolve independently.

---

## 3. System topology (confirmed decisions)

- **Each side runs its own DORA dataflow.** The operator machine runs dataflow A;
  the robot machine runs dataflow B. They are **not** one distributed DORA graph.
- **Communication is an independent, pluggable middleware** (the *bridge*) sitting
  between the two dataflows. It speaks the canonical contract over a selectable
  `Transport`. This is what makes the wire medium (USB/WiFi/4G/BLE/NearLink)
  swappable without touching DORA or the device/robot code.

```
 OPERATOR MACHINE                         ROBOT MACHINE
 ┌───────────────────────────┐           ┌───────────────────────────┐
 │ DORA dataflow A           │           │ DORA dataflow B            │
 │                           │           │                            │
 │  [tr-teleop-node]         │           │   [tr-bridge]              │
 │   device→canonical cmd    │           │    transport→canonical     │
 │        │                  │           │        │                   │
 │        ▼                  │  TRANSPORT │        ▼                   │
 │  [tr-bridge] ───canonical─┼──(USB/WiFi─┼──►[tr-robot-node]          │
 │    canonical→transport    │  /4G/BLE/  │    retarget+IK→driver      │
 │        ▲                  │  NearLink) │        │  ▲                │
 │        │ feedback         │◄───────────┼─feedback│  │ force fb      │
 │  [tr-teleop-node]◄────────┘           │   [robot driver]           │
 │   (haptics to master)     │           │                            │
 └───────────────────────────┘           └───────────────────────────┘
```

The bridge is **duplex**: commands flow operator→robot, feedback (joint/EE state,
wrench for haptics, health) flows robot→operator.

---

## 4. The canonical message contract (`tr-messages`)

Device- and robot-agnostic. Wire-serialized via a swappable `Codec`.

### 4.1 Header (every message)

```
MessageHeader {
  protocol_version: u16,     // wire compatibility gate
  session_id: u64,           // negotiated at handshake
  source_id: String,         // logical sender (e.g. "vr_left", "arm0")
  seq: u64,                  // monotonic per source; gap = loss
  stamp_nanos: u64,          // sender clock (see §9 time sync)
  control_mode: ControlMode, // active mode for this stream
}
```

### 4.2 Commands (operator → robot)

`ControlMode` selects the *semantic space* the command is expressed in:

- `CartesianPose` — end-effector `Pose` (position + unit quaternion) in a named
  `frame`. **Default for VR / heterogeneous master-slave.**
- `JointTargets` — per-joint position (+ optional velocity/effort). Used by
  isomorphic master arms with matched DoF.
- `Twist` — linear+angular velocity (mobile bases, velocity servoing).
- `Gripper` — normalized open/close (+ optional force target).
- `Composite` — multiple of the above keyed by end-effector name (dual-arm,
  humanoid: `left_arm`, `right_arm`, `base`, `head` ...).
- `Custom { type_id, payload }` — escape hatch for device-specific data.

> **Retargeting / IK live at the edges, never in the contract.** A VR pose enters
> as `CartesianPose`; the **robot tier** solves IK to its own joints. An
> isomorphic arm may emit `JointTargets` only if the robot advertises a matching
> joint layout, otherwise the teleop adapter converts to `CartesianPose` first.

### 4.3 Feedback (robot → operator)

- `JointState` — positions/velocities/efforts.
- `EndEffectorState` — `Pose` (+ optional `Wrench`).
- `ForceFeedback` — `Wrench` (force+torque) destined for the haptic master.
  Only present when both ends advertise `force_feedback = true`.
- `Status` / `Health` — mode, e-stop, latency estimate, errors.
- (High-bandwidth camera/point-cloud streams travel on a **separate** transport
  channel/QoS — see §6.3 — not interleaved with the control stream.)

### 4.4 Capability negotiation (handshake)

Before streaming, both ends exchange `Capabilities`:

```
Capabilities {
  dof: u32,
  supported_modes: Vec<ControlMode>,
  force_feedback: bool,
  max_rate_hz: u32,
  frames: Vec<String>,          // available reference frames
  end_effectors: Vec<String>,   // named EEs for Composite
  gripper: Option<GripperSpec>,
}
```

The session picks the **intersection**: highest mutually-supported mode, min of
the two `max_rate_hz`, and enables haptics only if both sides support it. This is
the WebRTC-style offer/answer that makes *any device* talk to *any robot*.

### 4.5 Codec

`Codec` is a trait (`encode`/`decode`). The skeleton ships a `PlaceholderCodec`;
production should use **Protobuf/`prost`** or **FlatBuffers** (cross-language with
the Python LeRobot side, field-tagged for schema evolution) or **`postcard`**
(compact, Rust↔Rust). The `protocol_version` header field gates incompatibilities.

---

## 5. Teleop tier (`tr-teleop`)

```
trait TeleopDevice {
    fn capabilities(&self) -> Capabilities;
    fn poll(&mut self) -> Option<TeleopCommand>;   // device-native → canonical
    fn apply_feedback(&mut self, fb: &RobotFeedback); // e.g. drive haptics
}
```

- Each device adapter is responsible for mapping its native input into a
  **canonical command** in a mode the robot advertised.
- VR / mismatched master arms perform **device-side retargeting** to Cartesian.
- Bilateral devices implement `apply_feedback` to render `ForceFeedback`.
- Reference adapters (behind cargo features): `keyboard`, `gamepad`, `vr`,
  `isomorphic_arm`.

---

## 6. Communication tier (`tr-transport`, `tr-session`, `tr-bridge`)

### 6.1 Transport abstraction

```
trait Transport {
    fn send(&mut self, channel: Channel, frame: &[u8]) -> Result<()>;
    fn recv(&mut self, timeout: Duration) -> Result<Option<(Channel, Vec<u8>)>>;
    fn link_state(&self) -> LinkState;     // Up/Degraded/Down + RTT estimate
}
```

Implementations (selected by config, not by code on either tier):

| Transport     | Status in skeleton | Production backing                        |
|---------------|--------------------|-------------------------------------------|
| TCP           | **real (std::net)** | reliable control + handshake              |
| UDP           | **real (std::net)** | low-latency control, optional FEC         |
| USB / serial  | stub               | `serialport`/`nusb` (CDC-ACM)             |
| Bluetooth     | stub               | `btleplug` (BLE)                          |
| 4G/5G         | (use TCP/UDP/QUIC) | IP transport; QUIC (`quinn`) recommended  |
| WiFi/Ethernet | (use TCP/UDP/QUIC) | same IP stack                             |
| NearLink 星闪  | stub               | vendor SDK behind the `Transport` trait   |

### 6.2 QoS (DDS/ROS 2 inspired)

Each logical stream carries a `QoS`:

```
QoS { reliability: Reliable|BestEffort, history: KeepLast(n)|KeepAll,
      deadline_hz: Option<u32>, priority: u8 }
```

- Control commands at high rate → `BestEffort, KeepLast(1)` (newest wins; a
  dropped 1 kHz sample is better than a stale queued one).
- Handshake / mode changes / e-stop → `Reliable`.
- Feedback for haptics → `BestEffort, KeepLast(1)`.

### 6.3 Channels

Logical channels are multiplexed and may map to different transports:
`Control`, `Feedback`, `Handshake`, `Telemetry`, `Media` (cameras on their own
high-bandwidth link/QoS). This keeps a 30 MB/s camera feed from starving the
1 kHz control loop.

### 6.4 Session (`tr-session`)

State machine: `Discovering → Handshaking → Active → Degraded → Closed`.
Handles: capability negotiation, heartbeat/keepalive, RTT measurement,
**reconnect with safe-state fallback** (on link loss the robot tier latches to a
safe hold/zero-velocity — never replays stale commands), sequence-gap detection.

---

## 7. Robot tier (`tr-robot`, `tr-robot-node`)

```
trait RobotDriver {
    fn capabilities(&self) -> Capabilities;
    fn command(&mut self, cmd: &CanonicalCommand) -> Result<()>;
    fn read_state(&mut self) -> Result<RobotFeedback>;
    fn e_stop(&mut self) -> Result<()>;
}

trait Kinematics {           // retargeting / IK / FK live HERE
    fn ik(&self, ee: &str, target: &Pose, seed: &[f64]) -> Result<Vec<f64>>;
    fn fk(&self, joints: &[f64]) -> Result<Pose>;
}
```

- The robot node receives a canonical command, runs **IK/retargeting locally**
  using its own `RobotModel` (URDF-derived), enforces joint/velocity/workspace
  limits, then drives the hardware.
- Reads state, builds `RobotFeedback`, and (if bilateral) computes the `Wrench`
  to send back as `ForceFeedback`.
- Reference drivers (features): `sim` (kinematic sim), `generic` (template).
- Production IK: `nalgebra` + a solver (KDL-style / `k` crate / TRAC-IK port).

---

## 8. Bilateral force feedback & control loop

- Mode is chosen at handshake (`force_feedback` on both ends).
- **Unilateral**: command down, telemetry/video up; latency-tolerant (tens–
  hundreds of Hz).
- **Bilateral**: robot streams `Wrench` back; teleop device renders it. Requires
  a low-latency duplex transport (USB/NearLink/wired LAN/QUIC) and
  `BestEffort KeepLast(1)` on both directions. The control loop runs in the
  *node*, not the bridge, so the wire medium can change without touching loop
  timing; the session exposes measured RTT so the loop can apply
  passivity/damping when latency rises (wave-variable / time-domain passivity is
  the production hardening path).

---

## 9. Time sync & timestamps

Every message is stamped with the sender clock (`stamp_nanos`). For datasets and
latency math, both machines should run PTP/NTP (or a session-level clock-offset
estimate derived from handshake RTT). The recorder stores both sender and
receiver stamps.

---

## 10. LeRobot integration — target **LeRobotDataset v3** (`training/`, `tr-policy`)

Spec: `docs/lerobot-dataset-v3.mdx`. v3 stores **many episodes per Parquet/MP4
file**, resolves episode boundaries through `meta/episodes/*` offset tables
(chunked Parquet), and requires a final **`finalize()`** to write parquet
footers. Layout: `meta/info.json` (schema + path templates), `meta/stats.json`,
`meta/tasks.jsonl`, `meta/episodes/` (Parquet), `data/` (Parquet shards),
`videos/` (per-camera MP4 shards). Feature naming: `action`,
`observation.state`, `observation.images.<cam>`, `timestamp`.

**Record — Option B (chosen): a DORA *Python* node drives lerobot's own writer.**
Re-implementing v3's buffered-parquet / offset-table / `finalize()` mechanics in
Rust would be fragile and chase a moving format, so the recorder
(`training/tr_lerobot/recorder.py`) calls lerobot's
`LeRobotDataset.create / add_frame / save_episode / finalize` directly —
**v3 conformance by construction**. Recording runs on the **robot side**, which
already has the executed `action`, local `observation.state`, and local cameras
(no need to stream video back to the operator).

- *Cross-language seam*: inside one machine's DORA dataflow, nodes exchange
  **Apache Arrow** natively — the Python recorder consumes already-decoded arrays.
  The custom `tr-transport`/`Codec` is used **only on the inter-machine bridge
  hop**, not here.
- *Pinning, not vendoring*: lerobot is pinned in `training/requirements.txt`
  (`lerobot >= 0.4.0`, or the v3 pre-release commit). It is **not** a git
  submodule in the Rust workspace. Reasons: the realtime core is Rust (cargo
  won't build a Python package), the format is a data contract, and lerobot drags
  a heavy torch stack.
- *Conformance gate*: `training/tr_lerobot/validate.py` loads the produced
  dataset back through `LeRobotDataset` (CI gate — if lerobot can open it, it is
  valid v3).

**Train** (Python, in-repo, isolated venv): `training/tr_lerobot/train.py` wraps
lerobot's training entrypoint over the recorded dataset.

**Replay**: `tr-policy` (Rust DORA node) emits canonical commands into the robot
node with the teleop tier unplugged — proving the canonical contract is the real
seam. A torch policy is consumed either by exporting it (ONNX → `ort`/`tch` in
`tr-policy`) or via a Python eval node — fork left open.

```
dataflows/record.yml :  bridge ─► robot ─┬─► action ───────────┐
                                         └─► observation_state ─┤
                         camera_front ─► image ─────────────────┴─► recorder.py ─► LeRobotDataset v3
dataflows/replay.yml :  policy-node ─► robot-node                          (no human)
```

---

## 11. Extension guide

- **Add a teleop device** → implement `TeleopDevice` in `tr-teleop`, emit a
  canonical command. No transport/robot change.
- **Add a transport** → implement `Transport` in `tr-transport`, register it.
  No teleop/robot change.
- **Add a robot** → implement `RobotDriver` (+ `Kinematics`/URDF) in `tr-robot`.
  No teleop/transport change.

---

## 12. Security

- Per-session authentication + key exchange during handshake; encrypt the
  transport (TLS/DTLS/QUIC, or link-layer for USB/NearLink).
- E-stop is a `Reliable`, highest-priority channel and is honored locally on the
  robot even if the link drops (deadline watchdog → safe state).

---

## 13. Open items to confirm later

- Concrete first-target robot + master device (drives which drivers we build first).
- Wire codec choice (prost vs flatbuffers vs postcard) once cross-language needs
  with the Python LeRobot side are finalized.
- Camera/observation transport (separate QUIC stream vs WebRTC) for §6.3.
