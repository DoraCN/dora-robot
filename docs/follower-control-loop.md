# Follower Control Loop — 从臂控制循环设计

> 解决 STS3215 伺服电机在遥操作中卡顿的方案：M1 已验证的消息驱动 + 25ms 限速 + 0.002 rad 去重 + 读写分离。

---

## 1. 问题背景

MVP 阶段发现在线遥操作时**从臂（follower）运动出现明显卡顿、抖动**，特别是低速微调时表现明显。该问题在 M1 阶段曾通过 25ms 速率限制 + 0.002 rad 去重解决，但在迁移到 daemon 架构时丢失。

## 2. 根因分析

### 2.1 主因：消息驱动的突发性写入

原始的 daemon 内循环是**消息驱动**的：

```
Leader 100Hz publish → zenoh → Follower recv(5ms timeout)
    → 立即 follower.command() → sync_write_goals → USB 写入
    → read_joints() → USB 读取
    → sleep(500µs) → 下一个消息
```

每个 zenoh 消息到达都触发一次 `sync_write_goals`，写入频率完全由消息到达频率决定。实测 follower 写入频率在 60-200Hz 间波动。

### 2.2 商业系统 vs 当前实现

| 维度 | 商业系统（UR / ROS2 Control） | 当前实现（修复前） |
|------|-------------------------------|--------------------|
| **触发方式** | 时间驱动：固定速率控制循环 | 消息驱动：来一条写一条 |
| **写入频率** | 精确可控（如 40Hz） | 跟随消息到达频率（60-200Hz 波动） |
| **运动平滑** | 速率限制 + 加速度限制 | 无限制 |
| **读写关系** | 独立调度，读写分离 | 串行读写，读延迟写 |
| **网络抖动** | Jitter Buffer + 固定时钟 | 无缓冲，直接写入 |

### 2.3 STS3215 伺服特性

STS3215 使用内部 PID 位置环。写入频率超过 ~40Hz（25ms）时：
- PID 控制器来不及收敛就被新的目标打断
- 伺服进入持续的**调节-中断-调节**振荡状态
- 宏观表现为关节抖动、嗡嗡声

M1 已通过 45Hz→25ms 限速验证了这一阈值。

### 2.4 次要因素

- **无去重**：位置变化 < 0.002 rad 的微小指令也写入 USB 总线，增加无效流量
- **每次写入都读关节**：`read_joints()` 耗时 3-8ms 的 USB 事务，与写入争抢总线
- **slew_clamp 无固定频率**：`slew_rad=0.05236 rad/tick` 在突发写入下实际物理速度不固定

## 3. 商业系统模式

成熟商业系统（UR、KUKA、ROS2 Control）的控制循环架构：

```
时间触发时钟 (25ms)
    │
    ├─ ① Jitter Buffer：排空消息通道，只保留最新一条
    ├─ ② 轨迹平滑：速度限制 → 加速度限制 → 低通滤波
    ├─ ③ 死区去重：微小变化跳过写入
    ├─ ④ sync_write_goals → 伺服（1kHz 内部 PID）
    ├─ ⑤ 读取关节状态（降低频率，独立调度）
    └─ ⑥ 等待至下一个时钟节拍
```

关键设计原则：
1. **时间驱动而非事件驱动**：控制周期固定，不受网络/消息到达影响
2. **读写分离**：写入不等待读取完成
3. **多级平滑**：速度限制 → 加速度限制 → 死区去重 → 低通滤波

## 4. 终极方案分析（已废弃 — 保留作为反面教材）

### 4.1 第一版尝试：固定速率控制循环

最初尝试了商业系统的**固定速率控制循环**模式：

```
25ms 时钟节拍
    ├─ 排空消息，只保留最新一条
    ├─ 写入 follower.command()（含 slew_clamp 3°/tick）
    ├─ 读取关节 @ 20Hz
    └─ sleep 至 25ms 边界
```

### 4.2 失败原因

该方案在测试中暴露了两个致命问题：

1. **丢弃中间位置**：25ms 内排空 2-3 条消息 → 只保留最新一条。Leader 100Hz 的轨迹在 follower 侧只剩 40Hz 的稀疏采样，从臂永远看不到完整的运动路径。
2. **slew_clamp 阻止追赶**：每 tick 最多移 3°，但 leader 在 25ms 内可能已移 6°。误差只增不减（3° → 6° → 9°...），从臂永远追不上主臂。

**教训**：遥操作场景要求「每条位置指令都必须被执行」，不能丢弃中间帧。固定速率采样适用于传感器融合（如 IMU 1000Hz → 控制 100Hz），但不适用于位置追踪控制。

## 5. 最终方案：M1 已验证的消息驱动 + 25ms 限速

### 5.1 架构图

```
  Leader zenoh pub ──→  msg1  ──→  Follower recv
  (100Hz, 10ms)           │          (5ms timeout)
                          │              │
                     msg2 │       ┌──────┘
                          │       │
                     msg3 ▼       ▼
                          ┌──────────────┐
                          │  Decode       │
                          │  + Dedup      │ ← 与上次写入 < 0.002 rad → continue
                          │  + 限速       │ ← 距上次写入 < 25ms → sleep 补足
                          │  + write_joints│ ← 绕过 slew_clamp，直接 sync_write_goals
                          └──────────────┘
                               │
                               ▼
                          STS3215 × 6
                         (1kHz 内部 PID)
```

**核心原则**：每条消息都处理（dedup 通过即写入），不丢弃任何位置。25ms 限速只是延时写入，而非跳过写入。

### 5.2 核心机制

```rust
const MIN_WRITE_DT: Duration = Duration::from_millis(25);
const DEDUP_THRESH: f32 = 0.002;

let mut first_write = true;
let mut last_write = Instant::now();
let mut last_written = [0.0_f32; 6];

'inner: loop {
    // ① FSM 命令（非阻塞排空）
    loop { match t_cmd.recv(Duration::ZERO) { ... } }

    // ② 一条 joint 命令（阻塞 5ms 等待）
    match t_ctrl.recv(Duration::from_millis(5)) {
        Ok(Some(inbound)) => {
            // 去重
            if !first_write {
                let max_d = compute_max_delta(positions, last_written);
                if max_d < DEDUP_THRESH { continue; }
            }
            // 限速（不足 25ms 则 sleep 补足）
            if !first_write {
                let elapsed = last_write.elapsed();
                if elapsed < MIN_WRITE_DT {
                    std::thread::sleep(MIN_WRITE_DT - elapsed);
                }
            }
            // 写入（绕过 slew_clamp → arm.write_joints）
            let ok = rt_arm.block_on(async {
                follower.arm_mut().write_joints(&target).await.is_ok()
            });
        }
    }

    // ③ 读取关节 @ ~12Hz（每 ~3 个 loop）
    read_counter += 1;
    if read_counter >= 3 { read_joints(); }

    // DORA 存活检查、状态发布 @ 1Hz
    std::thread::sleep(Duration::from_micros(500));
}
```

### 5.3 关键参数

| 参数 | 值 | 说明 |
|------|-----|------|
| `MIN_WRITE_DT` | 25ms | 写入间隔下限，对应最大 40Hz 写入频率 |
| `DEDUP_THRESH` | 0.002 rad | 关节位置变化死区，减少无效写入 |
| 读取频率 | ~12Hz | 每 3 个 loop 读一次，写入后执行 |
| 写入方式 | `arm.write_joints()` | 绕过 `So101Follower::command()` 的 slew_clamp |

### 5.4 为什么 M1 方案能工作

| 特性 | M1 方案 | 固定速率方案（失败） |
|------|---------|---------------------|
| 消息处理 | 每条消息逐一处理 | 排空只保留最新 |
| 写入触发 | 消息到达 → dedup → 限速 → 写入 | 固定节拍采样 |
| 限速效果 | 延时写入，但**不跳过**任何消息 | 丢弃 1-2 条中间消息 |
| 位置追踪 | 精确复现所有中间位置 | 丢失轨迹，误差累积 |
| 追赶能力 | 每次写入移多少由**消息决定** | 被 slew_clamp 限制 |

M1 的 25ms 限速本质上是一个**消息节流阀**：当消息来得太快（< 25ms）时，follower 等待到 25ms 再写入。但**每一条消息最终都被写入**，没有位置信息丢失。follower 完整复现 leader 的运动轨迹，只是以 40Hz 而非 100Hz 的频率步进。

## 6. 代码结构

```
crates/tr-daemon/src/bin/follower.rs
├── main()
│   ├── 初始化：USB 发现、zenoh 连接、运行时
│   ├── 外层重试循环（USB/zenoh 断连恢复）
│   └── 'inner: loop                    ← 控制循环
│       ├── ① FSM 命令排空（第 91-116 行）
│       ├── ② Joint 命令 recv + 去重 + 限速 + 写入（第 118-176 行）
│       ├── ③ 读取关节 @ ~12Hz（第 178-196 行）
│       ├── DORA 存活检查
│       ├── 状态发布 @ 1Hz
│       └── sleep(500µs)
├── connect_arm() — USB/zenoh 连接建立
├── handle_dataflow_action() — DORA 生命周期管理
└── handle_recovery() — 错误恢复
```

## 7. 调优指南

### 7.1 如果仍然卡顿

| 检查项 | 方法 | 目标 |
|--------|------|------|
| USB 写入频率 | 打印 frames 的 1s 增量 | 最大 ~40Hz（25ms 限速） |
| 去重是否生效 | 静止时 frames 应不再增长 | 静止时几乎不写入 |
| Leader 发布频率 | 检查 leader.poll() 耗时 | ≤100Hz |
| 总线错误率 | 检查 stderr 输出 | 不应有 bus write error |
| 写入了哪些位置 | 日志对比 leader→follower 位置 | 不应有跳变或累积误差 |

### 7.2 参数调整

```rust
MIN_WRITE_DT = 25ms      // 加大 → 更低频但更平滑（如 33ms = 30Hz）
DEDUP_THRESH = 0.002     // 加大 → 更少写入但精度下降
first_write = true        // 重置后下一条消息无条件写入
```

## 8. 参考资料

- [M1 zenoh_follower 实现原理](../crates/tr-so101/examples/zenoh_follower.rs)
- [Follower daemon 当前实现](../crates/tr-daemon/src/bin/follower.rs)
- [So101Follower command + slew_clamp 实现](../crates/tr-so101/src/follower.rs)
- [商业系统控制循环设计](https://control.ros.org/master/doc/ros2_control/controller_types/doc/userdoc.html)
