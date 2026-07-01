# tasks.md — feature `002` 任务清单（原子任务）

- **追溯**：spec `spec.md`(Draft 6) M1–M11 / AC1–AC11；plan `plan.md`(Draft 4) §10 实现顺序；遵守 `constitution.md`。
- Status：**Ready for Implement**
- 状态图例：☐ 待办 · ◐ 进行中 · ☑ 完成 · ✖ 取消
- **并发规则**：组间并发（不同 crate）；同文件串行（同组内改同一文件的任务排队）；失败隔离。

---

## A. 契约 (`crates/tr-messages`) ｜ 同文件串行

- **A1** 新增 `ControlCommand` 枚举 + 单测
  - 输入: plan §4.3 · 输出: `src/control.rs` enum + match 单测 · 完成: `cargo test -p tr-messages` 过、**std-only** · 追溯: plan §4.3 / 所有 M · 依赖: — · 状态: ☐

- **A2** 新增 `DaemonStatus` 结构 + serde 单测
  - 输入: plan §4.3 · 输出: `src/control.rs` struct + JSON roundtrip 单测 · 完成: serde 序列化/反序列化正确 · 追溯: plan §4.3 / M8 · 依赖: A1 · 状态: ☐

---

## B. USB 发现 (`crates/tr-so101`) ｜ 与 A 并发

- **B1** 新增 `resolver.rs`：`resolve_arm_port(device: &UsbDeviceConfig) → Result<String>`
  - 输入: plan §4.2, `docs/usb-resolver-integration.md` §4 · 输出: `src/resolver.rs` + 单测（direct port / empty / 解析 hex） · 完成: `cargo test -p tr-so101` 过 · 追溯: M2/AC2 · 依赖: —（需 workspace 加 `usb-resolver` 依赖） · 状态: ☐

- **B2** 新增 `usb_scan` 示例
  - 输入: plan §4.6 · 输出: `examples/usb_scan.rs` · 完成: `cargo run -p tr-so101 --example usb_scan` 打印 VID/PID/Serial · 追溯: M2 · 依赖: B1 · 状态: ☐

---

## C. `tr-daemon` 骨架 ｜ 依赖 A, B

- **C1** `config.rs`：DaemonConfig TOML 解析
  - 输入: plan §4.1, §2 配置格式 · 输出: `src/config.rs` + 解析单测 · 完成: 读取示例 `config/follower.toml` 成功 · 追溯: M8/AC8 · 依赖: — · 状态: ☐

- **C2** `state.rs`：Fsm 状态机 + TDD 单测
  - 输入: plan §5 状态转移表 · 输出: `src/state.rs` + 全部 6 条转移单测 · 完成: 每条转移输入/输出正确（含 DataflowAction） · 追溯: M6 · 依赖: A1 · 状态: ☐

- **C3** `dora.rs`：DoraFlow 生命周期 stub
  - 输入: plan §7 · 输出: `src/dora.rs` + launch/stop/alive 骨架 · 完成: 编译通过（stub 阶段 dump 命令行参数即可） · 追溯: M4 · 依赖: C1 · 状态: ☐

---

## D. follower-daemon ｜ 依赖 C

- **D1** USB 发现 + 硬件初始化 + 状态机接续
  - 输入: C1, C2, B1 · 输出: `src/bin/follower.rs` 启动到 Idle + pub 首次 status · 完成: `cargo run` → 控制台输出 `IDLE` · 追溯: M1/AC1, M6/AC6 · 依赖: C1, C2, B1 · 状态: ☐

- **D2** zenoh sub `control` → `So101Follower.command()` 驱臂回路
  - 输入: 已有 `ZenohTransport` + `So101Follower` · 输出: 遥操作驱臂功能 · 完成: leader 搬动主臂、从臂跟随 · 追溯: M1 · 依赖: D1 · 状态: ☐

- **D3** zenoh sub `command` → 状态机 → TorqueOn/TorqueOff 回路
  - 输出: `o`/`x` 启停扭矩 + 启停 DORA dataflow · 完成: [上力] → 扭矩 ON + DORA 启动；[卸力] → 扭矩 OFF + DORA 停止 · 追溯: M4, M6 · 依赖: D2 · 状态: ☐

- **D4** zenoh pub `observation`（从臂实际关节位置周期性推送）
  - 输出: 1Hz pub `Vec<f32>` 到 `tr/<id>/observation` · 完成: `dora spy` 可观测到数据 · 追溯: M9 · 依赖: D2 · 状态: ☐

---

## E. leader-daemon ｜ 与 D 并发

- **E1** USB 发现 + 硬件初始化
  - 输入: C1, B1 · 输出: `src/bin/leader.rs` 启动 + So101Leader 就绪 · 完成: `cargo run` → 开始 pub control · 追溯: M10/AC10 · 依赖: C1, B1 · 状态: ☐

- **E2** zenoh pub `control` + `command` 回路
  - 输入: 已有 `ZenohTransport` + `So101Leader` · 输出: 读主臂 → postcard → zenoh pub · 完成: follower 收到 control 数据 · 追溯: M10 · 依赖: E1 · 状态: ☐

- **E3** 键盘交互（o/x/s/Enter/f/r/q）
  - 输入: plan §8 键位表 · 输出: 键盘输入 → `ControlCommand` → zenoh pub `command` · 完成: 按键 → follower 状态机响应 · 追溯: M4 · 依赖: E2 · 状态: ☐

---

## F. `tr-capture` DORA 节点 ｜ 依赖 A

- **F1** capture 节点实现：zenoh sub → DORA Arrow
  - 输入: plan §4.4 · 输出: `crates/tr-capture/src/main.rs` · 完成: `dora start dataflows/record.yml` → recorder 收到 Arrow action + observation_state · 追溯: M9/AC9 · 依赖: A1, A2 · 状态: ☐

---

## G. DORA dataflow + 录制 ｜ 依赖 D, F

- **G1** `dataflows/record.yml` 集成 + `dora check` 通过
  - 输入: plan §4.5 · 输出: 可运行 dataflow · 完成: `dora up && dora check dataflows/record.yml` 通过 · 追溯: M4 · 依赖: F1 · 状态: ☐

- **G2** TorqueOn → dataflow launch + TorqueOff → stop 集成测试
  - 输出: follower-daemon 控制 dataflow 生命周期 · 完成: follower `o` → dataflow running；`x` → dataflow stopped · 追溯: M4/AC4 · 依赖: D3, G1 · 状态: ☐

- **G3** StartRecord/EndRecord/ReRecord episode 边界集成
  - 输出: episode 边界传达到 recorder，录制 v3 可加载 · 完成: 1 个 episode 录制成功、数据可被 lerobot 加载 · 追溯: M4/AC4, M9/AC9 · 依赖: G2 · 状态: ☐

- **G4** 录制故障隔离测试（DORA crash → daemon 不崩）
  - 输出: kill DORA → daemon 存活 + 状态机回 Idle · 完成: AC5 通过 · 追溯: M5/AC5 · 依赖: G2 · 状态: ☐

---

## H. 异常处理 ｜ 依赖 D

- **H1** FeetechBus 报错 → 状态机回 Idle + 指数退避重试
  - 输入: plan §9 · 输出: 拔 USB → offline → 插回 → auto reconnect · 完成: AC3 通过 · 追溯: M3/AC3, M5/AC5 · 依赖: D2 · 状态: ☐

- **H2** zenoh 断连 → 状态机回 Idle + 指数退避重连
  - 输出: 断 zenoh → offline → 恢复 → Idle · 完成: 手动断连测试通过 · 追溯: M5/AC5, M11 · 依赖: D2 · 状态: ☐

- **H3** daemon 持续存活 ≥ 1h + 三次异常注入测试
  - 输出: 长期运行验证 · 完成: AC11 通过 · 追溯: M11/AC11 · 依赖: H1, H2, G4 · 状态: ☐

---

## I. 多臂 + 配置 ｜ 依赖 D

- **I1** 多臂对隔离（两个 follower-daemon 不同 `--config`）
  - 输出: arm_1 / arm_2 独立运行、互不干扰 · 完成: AC7 通过 · 追溯: M7/AC7 · 依赖: D2 · 状态: ☐

- **I2** 配置热加载验证（改 baud → 重启生效）
  - 输出: 修改 toml → 重启 → 新参数生效 · 完成: AC8 通过 · 追溯: M8/AC8 · 依赖: C1 · 状态: ☐
