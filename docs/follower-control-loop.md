# Follower Control Loop — 从臂控制循环设计

> 解决 STS3215 伺服电机在遥操作中卡顿的终极方案：固定速率控制循环 + 去重 + 读写分离。

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

## 4. 终极解决方案

### 4.1 架构图

```
                 ┌─────────────────────────────────┐
 Leader ─zenoh─→ │       Fixed-Rate Control Loop    │
                 │           40Hz (25ms)            │
                 │                                 │
                 │  ① Drain FSM commands (ZERO)    │
                 │  ② Drain joint cmds, keep latest │
                 │  ③ Dedup (< 0.002 rad → skip)   │
                 │  ④ follower.command() → USB     │
                 │  ⑤ read_joints @ 20Hz (隔次)    │
                 │  ⑥ sleep 精确到 25ms 边界        │
                 └─────────────────────────────────┘
                              │
                              ▼
                         STS3215 × 6
                        (1kHz 内部 PID)
```

### 4.2 核心机制

```rust
'inner: loop {
    let tick_start = Instant::now();

    // ① FSM 命令（非阻塞排空）
    loop { match t_cmd.recv(Duration::ZERO) { ... } }

    // ② Joint 命令（非阻塞排空，只保留最新）
    loop { match t_ctrl.recv(Duration::ZERO) { ... } }

    // ③ 写入：40Hz + 去重
    if torque_on && latest_cmd.exists() {
        let max_delta = compute_max_delta(latest, last_written);
        if max_delta >= 0.002 {
            follower.command(&latest_cmd)?;  // 内含 slew_clamp
            update_last_written();
        }
    }

    // ④ 读取：20Hz（隔次），写入之后
    if tick_count % 2 == 0 {
        read_joints() → publish observation
    }

    // ⑤ 精确维持 25ms 周期
    let elapsed = tick_start.elapsed();
    if elapsed < CTRL_DT {
        std::thread::sleep(CTRL_DT - elapsed);
    }
    tick_count += 1;
}
```

### 4.3 关键参数

| 参数 | 值 | 说明 |
|------|-----|------|
| `CTRL_DT` | 25ms | 控制循环周期，对应 40Hz 固定速率 |
| `DEDUP_THRESH` | 0.002 rad | 关节位置变化死区，减少无效写入 |
| `slew_rad` | 0.05236 rad (3°) | 每 tick 最大关节位移，约 120°/s @ 40Hz |
| 读取频率 | 20Hz | 每隔一次循环读关节，写入后执行 |

### 4.4 多级平滑链

```
Leader raw (100Hz)
    │
    ▼ ① Jitter Buffer（丢弃中间帧，只保留最新）
Latest command
    │
    ▼ ② Dead-zone Dedup（< 0.002 rad → skip）
    │
    ▼ ③ Slew Clamp（max 0.05236 rad/tick = 120°/s）
    │
    ▼ ④ 固定 25ms 时钟节拍
    │
    ▼ 写入 STS3215
```

每一级都是可叠加的、确定性的，形成完整的运动学约束链。

## 5. 与 M1 方案对比

| 维度 | M1（已验证） | 当前方案 |
|------|------------|---------|
| 速率限制 | 25ms window | 固定 25ms 时钟节拍 |
| 去重 | 0.002 rad | 0.002 rad ✓ |
| 写入时机 | 消息触发 + 限速窗口 | 固定节拍采样最新指令 |
| 读取关节 | 控制循环中不读 | 20Hz，写入后，隔次读 |
| 网络抖动 | 无缓冲 | Jitter Buffer（drain → latest） |
| 运动平滑 | 无 | slew_clamp（防甩臂突跳） |
| 确定性 | 受消息到达间隔影响 | 完全确定性的 40Hz |

## 6. 代码结构

```
crates/tr-daemon/src/bin/follower.rs
├── main()
│   ├── 初始化：USB 发现、zenoh 连接、运行时
│   ├── 外层重试循环（USB/zenoh 断连恢复）
│   └── 'inner: loop                    ← 控制循环
│       ├── ① FSM 命令排空（第 92-115 行）
│       ├── ② Joint 命令排空（第 118-132 行）
│       ├── ③ Dedup + 写入（第 134-169 行）
│       ├── ④ 读取关节 @ 20Hz（第 171-187 行）
│       ├── DORA 存活检查
│       ├── 状态发布 @ 1Hz
│       └── ⑤ 精确 25ms 同步（第 222-227 行）
├── connect_arm() — USB/zenoh 连接建立
├── handle_dataflow_action() — DORA 生命周期管理
└── handle_recovery() — 错误恢复
```

## 7. 调优指南

### 7.1 如果仍然卡顿

| 检查项 | 方法 | 目标 |
|--------|------|------|
| USB 写入频率 | 在 follower.command() 前后打印时间戳 | 应稳定在 ≈25ms |
| 去重是否生效 | 打印跳过次数 vs 写入次数 | 静止时应几乎全部跳过 |
| Leader 发布频率 | 检查 leader.poll() 耗时 | ≤100Hz |
| 总线错误率 | 检查 stderr 输出 | 不应有 bus write error |

### 7.2 参数调整

```rust
CTRL_DT = 25ms      // 降低 → 更平滑但延迟增加（如 30ms = 33Hz）
DEDUP_THRESH = 0.002 // 提高 → 更少写入但精度下降
slew_rad = 0.05236   // 降低 → 更柔和但追尾更明显
```

## 8. 参考资料

- [M1 zenoh_follower 实现原理](../crates/tr-so101/examples/zenoh_follower.rs)
- [Follower daemon 当前实现](../crates/tr-daemon/src/bin/follower.rs)
- [So101Follower command + slew_clamp 实现](../crates/tr-so101/src/follower.rs)
- [商业系统控制循环设计](https://control.ros.org/master/doc/ros2_control/controller_types/doc/userdoc.html)
