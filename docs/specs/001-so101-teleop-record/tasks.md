# tasks.md — feature `001` 任务清单（原子任务）

- **追溯**：spec `spec.md`(Rev 4) 的 **M1–M5, M8–M11 / AC1–AC5, AC8–AC11**（落盘 M6/M7/AC6/7/12 归 lerobot、非本项目）；plan `plan.md`(Rev 5) §10 里程碑；遵守 `constitution.md`。
- Status：**Ready for Implement（待开工）**。每个任务=一个**可独立验证**的交付物。
- 状态图例：☐ 待办 · ◐ 进行中 · ☑ 完成 · ✖ 取消。
- **并发规则**（SDD §6）：**组间并发**（不同 crate/目录，无冲突）；**同文件串行**（仅改同一文件的任务排队，同组不同文件可并行）；**失败隔离**（单任务失败不扩散到其它组）。

---

## A. 契约 & 编解码（`crates/tr-messages`, `crates/tr-codec`）｜同文件串行

- **A1** tr-messages 增 `EpisodeEvent`（`Start` / `End{outcome: Success|Fail|Rerecord}`）
  - 输入: M9, plan §4 · 输出: enum + 构造/match 单测 · 完成: `cargo test -p tr-messages` 过、**仍 std-only** · 追溯: M9/AC9, §10.1 · 依赖: — · 状态: ☐
- **A2** 新增 crate `tr-codec`：postcard 实现 `Codec`（command/feedback/EpisodeEvent）+ roundtrip 单测
  - 输入: plan §2/§3, 宪法 C8 · 输出: `crates/tr-codec` · 完成: roundtrip 单测过；**tr-messages 不引 serde** · 追溯: §10.1（编解码底座）· 依赖: A1 · 状态: ☐

## B. 传输（`crates/tr-transport`）｜同文件串行（与 A 并发）

- **B1** 增 `Channel::Episode`（Reliable）+ QoS 映射
  - 输入: plan §3 · 输出: Channel + QoS · 完成: `cargo test -p tr-transport` 过 · 追溯: M9, §10.2 · 依赖: — · 状态: ☐
- **B2** 新增 `LoopbackTransport`（进程内 `Transport`）+ 单测（收发/丢弃语义）
  - 输入: plan §3(P5) · 输出: impl Transport · 完成: 单测过 · 追溯: §10.2、§9 快门禁基础 · 依赖: — · 状态: ☐

## C. SO-101 臂（`crates/tr-so101`）｜同文件串行（与 A/B 并发）

- **C1** `So101Arm<B: MotorBus>`（read/write_joints, set_torque）+ config（ids 1..6/限位/偏置）+ MockBus 单测
  - 输入: plan §4/§5, so101 design §5, 宪法 C3/C4 · 输出: arm.rs/config.rs · 完成: `cargo test -p tr-so101 --features mock` 过 · 追溯: M1/M3, §10.3 · 依赖: — · 状态: ☐
- **C2** `So101Leader: TeleopDevice`（卸力, poll→JointTargets）+ 单测
  - 输出: leader.rs · 完成: poll 产出正确 JointTargets(MockBus) · 追溯: M1, §10.3 · 依赖: C1 · 状态: ☐
- **C3** `So101Follower: RobotDriver`（上电, 限位+单拍 slew≤3° + e_stop, read_state）+ 单测
  - 输出: follower.rs · 完成: 单测验证 限位/slew(M10)/e_stop · 追溯: M10/M5, AC10, §10.3 · 依赖: C1 · 状态: ☐
- **C4** async↔sync 桥（tokio 舵机任务 + 最新值 cell/通道）
  - 输出: arm.rs 内异步任务 · 完成: `poll`/`command` **非阻塞**（不等待总线 IO），单测验证调用返回 ≤ 1 ms · 追溯: M3, §10.3 · 依赖: C1 · 状态: ☐

## D. 会话 & 安全（`crates/tr-session` + follower 接线）｜依赖 C

- **D1** 防弹射上线对齐（follower 先驱到首个收到的主臂位姿）+ 单测
  - 输出: align 逻辑 · 完成: 首拍各关节步长 ≤2° · 追溯: M4/AC4, §10.4 · 依赖: C3 · 状态: ☐
- **D2** 看门狗安全态接线（>200ms→保持；2s→卸力；不重放过期；e_stop）+ 注入 gap 单测
  - 输出: session→follower 安全态 · 完成: AC5 逻辑可测 · 追溯: M5/AC5, §10.4 · 依赖: C3, tr-session · 状态: ☐

## E. DORA 接入（节点 Arrow I/O）｜根基（E0 spike 先行）

- **E0** `dora-node-api` spike（核对 Rust Arrow 收发 API；最小收发 PoC）
  - 输入: plan §11/Q5 · 输出: PoC + API 记录 · 完成: PoC 跑通收发 · 追溯: §10.0 · 依赖: — · 状态: ☐
- **E1** `tr-teleop-node` 接 DORA（tick→poll→codec→`send_output(command)`）
  - 完成: `dora start` 该节点(mock)可运行 · 追溯: §10.0/§10.7 · 依赖: E0, A2, C2 · 状态: ☐
- **E2** `tr-robot-node` 接 DORA（sub command→follower→写舵机；发 **Arrow `action`/`observation_state`**；`feedback` 随双边 M3 预留、本期不建）
  - 完成: 节点可运行、按拍发出 action/state Arrow · 追溯: §10.0/§6, M8/M11 · 依赖: E0, A2, C3, D1, D2 · 状态: ☐
- **E3** `tr-bridge` 接 DORA（dataflow⇄Transport；pub/sub **control/episode**；feedback 通道 M3 预留）
  - 完成: 节点可运行(loopback/stub transport) · 追溯: §10.0/§10.7 · 依赖: E0, A2, B1 · 状态: ☐

## F. operator-control + episode 通道｜依赖 A/B/E

- **F1** `operator-control` 节点（操作机；发 `start`/`end{outcome}` 到 episode；**事件源可注入供测试**）
  - 完成: 发出 EpisodeEvent；测试可注入 · 追溯: M9/AC9, §10.5/Q5 · 依赖: A1, E0 · 状态: ☐
- **F2** 机器人侧 bridge **解码 EpisodeEvent→DORA 消息转给录制器**（Q2，保录制器 codec-free）
  - 完成: `episode_end{outcome}` 抵达录制器 · 追溯: M9/AC9/Q2, §10.5 · 依赖: E3, A2 · 状态: ☐

## G. 录制器（`training/tr_lerobot`, Python）｜与 Rust 组并发

- **G1** recorder 抽出可注入接口 `EpisodeWriter`（add_frame/save_episode/discard/finalize）；生产实现包真 lerobot
  - 输入: plan §4/§6(Q1) · 输出: 接口 + Lerobot 实现 + 依赖注入 · 完成: `py_compile` 过；recorder 用注入 writer · 追溯: M9, §10.6 · 依赖: — · 状态: ☐
- **G2** 录制器 success/fail/discard 决策 + 30Hz 节拍取样 + 帧字段映射
  - 完成: 见 G3 · 追溯: M9, §10.6 · 依赖: G1 · 状态: ☐
- **G3** 录制器适配器单测（注入 **spy** `EpisodeWriter`；断言 success→save/fail→discard、30Hz 降采样、字段映射）**零 torch**
  - 输出: pytest · 完成: pytest 过、**不引 lerobot/torch** · 追溯: AC9 决策, §9 快门禁(b)/§10.8 · 依赖: G2 · 状态: ☐

## H. 节点选择 + dataflows｜依赖 C/E/B/F/G

- **H1** 节点按 env/feature 选 `So101Leader`/`So101Follower`/`ZenohTransport`
  - 完成: 配置切换可用 · 追溯: §10.7, 宪法 C5 · 依赖: C2,C3,E1,E2,E3 · 状态: ☐
- **H2** dataflows `so101_leader.yml`（teleop+operator-control+bridge）/ `so101_follower.yml`（bridge+robot+recorder）
  - 完成: `dora check` 通过 · 追溯: §10.7 · 依赖: H1, F1, F2, G1 · 状态: ☐

## I. 快门禁（CI fast gate）｜依赖 A/B/C/D/G

- **I1** Rust 进程内 loopback 集成测试：So101Leader(Mock)→`LoopbackTransport`→So101Follower(Mock)
  - 完成: `cargo test` 过，覆盖 **AC4/AC5/AC10** + 数据正确性 · 追溯: AC4/5/10, §9 快门禁(a)/§10.8 · 依赖: A2,B2,C2,C3,D1,D2 · 状态: ☐
- **I2** CI 快门禁配置：`cargo build/test --workspace` + `py_compile` + G3 spy 测（**不引 zenoh/torch**）
  - 完成: CI 绿、无重依赖 · 追溯: 宪法 C7（快门禁部分；与 plan §9 一致）· 依赖: I1, G3 · 状态: ☐
  - 注: 本项目**不做 lerobot 加载校验**（落盘归 lerobot）；宪法 C7 的"加载校验门禁"**已删除（对齐）**。

## J. zenoh + 集成作业（控制侧，on-demand）｜依赖 H/E/G

- **J1** 新增 crate `tr-transport-zenoh`（`ZenohTransport: Transport`）+ 两进程 loopback 连通测
  - 输入: plan §2/§11（zenoh 1.9 API 核对）· 完成: 两进程经 zenoh 收发框过 · 追溯: M2, §10.9 · 依赖: B1, E3 · 状态: ☐
- **J2a** 集成-录制无侵入（真 zenoh + 真 DORA 多进程 + 录制进程在跑，MockBus 臂）：录制开/关对照
  - 完成: 录制开启时 M3 降幅<5%、M2 增幅<10% · 追溯: AC8/M8, §9 集成作业/§10.9 · 依赖: J1, H2, G2 · 状态: ☐
- **J2b** 集成-录制故障隔离：杀录制进程
  - 完成: 杀录制进程后控制环 M3 不受影响、遥操作继续 · 追溯: AC11/M11, §10.9 · 依赖: J1, H2, G2 · 状态: ☐
  - 注: **不验数据集 / 不验落盘**（lerobot 域，非本项目）。

## K. 标定 + 真机（Validate）｜依赖全链路

- **K1** 标定/bring-up 指南（`scan`→`set_id`→`set_mid`→`to_zero`，使主从同零位）
  - 完成: 文档可操作 · 追溯: spec 前置条件, §10.10 · 依赖: C1 · 状态: ☐
- **K2a** 真机实测 **M1 跟随精度**
  - 完成: 臂关节 P95 ≤ 2°、夹爪 ≤ 5°（或微调留痕）· 追溯: M1/AC1, §10.10 · 依赖: 全链路 + K1 · 状态: ☐
- **K2b** 真机实测 **M2 时延**
  - 完成: `t1−t0` ≤ 50 ms（LAN）（或微调留痕）· 追溯: M2/AC2, §10.10 · 依赖: 全链路 + K1 · 状态: ☐
- **K2c** 真机实测 **M3 频率**
  - 完成: P5 ≥ 100 Hz、最小 ≥ 90 Hz（或微调留痕）· 追溯: M3/AC3, §10.10 · 依赖: 全链路 + K1 · 状态: ☐

---

## 并发执行计划（波次）

| 波 | 可并行任务 | 说明 |
|---|---|---|
| W0 | **A1 · B1 · B2 · C1 · G1 · E0** | 不同 crate/目录，组间并发起步 |
| W1 | A2 · C2 · C3 · C4 · G2 · E1/E2/E3 · D1 · D2 | 各组同文件串行推进 |
| W2 | G3 · F1 · F2 · H1 · I1 | 接缝与快门禁 |
| W3 | H2 · I2 · J1 | 数据流 + CI + zenoh |
| W4 | J2a · J2b | 集成作业（控制侧）|
| W5 | K1 · K2a · K2b · K2c | 真机 Validate |

## 覆盖核对（AC → 任务）

AC1→K2a · AC2→K2b · AC3→K2c · AC4→D1/I1 · AC5→D2/I1 · AC8→J2a · AC9→G3(spy)+F1/F2 · AC10→C3/I1 · AC11→J2b —— **本项目 AC（AC1–5, 8–11）全覆盖**；AC6/AC7/AC12（落盘）已移出、归 lerobot。

## 前置/暂缓（不阻断开工）
- **K2（C7 加载校验门禁）：已删除**（本轮对齐——落盘归 lerobot、非本项目门禁）。
- ~~spec M6/M7 定位~~：**已在 spec Rev 4 解决**（M6/M7 及 AC6/AC7/AC12 移出，落盘归 lerobot）。
