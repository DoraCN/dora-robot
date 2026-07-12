# Leader Daemon CPU 100% & Dual Process — Root Cause Analysis

> 全志 ARM 开发板上 leader 守护进程 CPU 持续 100%，htop 显示两个同名进程。
> 本文档只做分析，不包含代码修改。

---

## 1. 双重进程根因

### 1.1 症状

```
PID 3843: CPU 99.9%  /bin/leader --config .../config/leader.toml
PID 3850: CPU 99.9%  /bin/leader --config .../config/leader.toml
```

两个 PID 相差很小（3843→3850），内存几乎一致，说明**不是 crash-restart 循环**，而是两个实例**同时运行**。

### 1.2 可能原因

| 原因 | 可能性 | 说明 |
|------|--------|------|
| **旧服务未停** | ★★★★★ | 卸载/重装时 `systemctl --user stop` 失败但不报错（`2>/dev/null \|\| true`），旧进程继续跑，新进程并行启动 |
| **手动运行** | ★★★★☆ | 用户手动 `./bin/leader` 同时 systemd 也在跑 |
| **Type=simple** | ★★★☆☆ | `Type=simple` 表示 systemd 在 execve 后立即认为服务已启动。如果启动脚本中有多次 start 调用，不会报"already running" |
| **fork 行为** | ★★★☆☆ | tokio `new_multi_thread` 创建 worker 线程，但 htop 默认显示进程而非线程。需按 `H` 确认是否真的是独立进程 |

### 1.3 成熟系统如何防止双开

| 方案 | 机制 | 复杂度 |
|------|------|--------|
| **PID file** | 进程启动时写 `$PROJECT/run/leader.pid`，systemd 通过 `PIDFile=` 追踪，重复启动自动拒绝 | ⭐ 低 |
| **systemd `Type=forking`** | 进程 fork 后父进程退出，systemd 等待子进程就绪。子进程未退出则不会启动第二个 | ⭐ 低（改一行 unit） |
| **sd_notify** | 进程启动后调用 `sd_notify(0, "READY=1")`，systemd 等待通知。`Type=notify` + `NotifyAccess=all` | ⭐⭐ 中 |
| **Unix socket lock** | bind 一个抽象 socket 做互斥锁，第二个进程 bind 失败自动退出 | ⭐⭐ 中 |
| **flock** | 进程启动时 `flock(lockfile)`，第二个进程获取锁失败自动退出 | ⭐ 低 |

### 1.4 最低成本方案（推荐）

**方案 A：PID file**（最简单，1 行改 unit + 代码不用动）

```ini
# ~/.config/systemd/user/dorarobot-leader.service
[Service]
Type=simple
PIDFile=%h/Dev/dora-robot/run/leader.pid   # ← 加这一行
ExecStart=.../leader --config ...
```

systemd 会在启动前检查 PIDFile 是否已有活跃进程，有则拒绝启动。

**方案 B：flock 自检**（代码级，更可靠）

```rust
// leader.rs main() 最开头
use std::fs::File;
let _lock = File::create("/tmp/dorarobot-leader.lock")
    .and_then(|f| f.try_lock_exclusive().map(|_| f))
    .expect("another leader instance is already running");
```

---

## 2. CPU 100% 根因

### 2.1 已排除的因素

| 因素 | 原始状态 | 当前状态 | 效果 |
|------|---------|---------|------|
| Zenoh 多 session | 3 session × 4 worker 线程 | 1 session × 1 worker 线程 | 内存 2.3GB → 1.2GB |
| 串口 runtime 缺 enable_io() | 无 I/O driver | 已补齐 | 理论应收效，实际未变 |
| Leader poll 频率 | ~50Hz (10ms sleep) | ~25Hz (25ms sleep) | 应降低 50% CPU，实际未变 |

**结论：CPU 热点不在应用层逻辑，而在底层 SDK/驱动。**

### 2.2 调用链分析

```
main() 主线程 ──leader.poll()──→ So101Leader::poll()
                                    │
                                    └── self.rt.block_on(async {
                                            self.arm.read_joints().await
                                                │
                                                └── bus.sync_read_positions(ids).await
                                                      │
                                                      └── FeetechController
                                                          [feetech-servo-sdk v0.3]
```

**每一帧都会走这条调用链。25Hz × ~5ms/次 = 125ms 忙等/秒。**

### 2.3 怀疑对象：feetech-servo-sdk 的串口实现

`FeetechController<tokio_serial::SerialStream>` 内部 `sync_read_positions()`:

```rust
// 推测 SDK 可能的实现模式：
async fn sync_read_positions(&self, ids: &[u8]) -> Result<Vec<f32>> {
    self.write_command(ids)?;           // 发送同步读指令
    
    // ↓ 这里是 CPU 热点的关键
    let deadline = Instant::now() + timeout;
    loop {
        if self.has_full_response() {
            return Ok(parse_response());
        }
        if Instant::now() > deadline {
            return Err(timeout);
        }
        // ← 如果这里没有 await/yield，就是 100% CPU 忙等
        tokio::task::yield_now().await;  // ← 如果有这行，CPU 会正常
    }
}
```

**验证方法**：在 ARM 板上运行：

```bash
# 查看 leader 进程的线程
ps -T -p $(pgrep leader) | wc -l

# strace 采样（看是否有大量重复的 syscall）
timeout 5 strace -p $(pgrep leader | head -1) -c
```

如果 `strace` 显示大量 `read()` returning `-EAGAIN` (arm: `-EWOULDBLOCK`)，就确认了 SDK 在忙等。

### 2.4 ARM 特有因素

| 因素 | x86 | ARM (全志) |
|------|-----|-----------|
| 串口驱动 | 16550A 兼容，DMA 中断 | 可能仅 polling 模式 |
| epoll on tty | 原生支持 | 部分内核版本有限制 |
| tokio I/O driver | epoll 多路复用 | 同上 |
| CPU 频率 | 2-4 GHz | 1-1.5 GHz，单核忙等极易 100% |
| 内核抢占 | 标准 | 嵌入式内核可能关闭 |

**ARM 上即使 SDK 正确使用了 `await`，tokio 的 epoll 在串口上也可能退化为 polling。**

### 2.5 为什么 M1 示例在 x86 上正常

M1 `zenoh_follower` 和 `leader_teleop_zenoh` 在**开发机 (x86 Mac/Linux)** 上运行，串口 I/O 行为与 ARM 完全不同：
- x86 的 tokio epoll 能正确等待串口数据
- ARM 上同样的代码可能退化为忙等

### 2.6 验证：strace 对比

```bash
# ARM 开发板
strace -e trace=read,write,poll,select,epoll_wait -p <leader_pid> -c -f

# x86 开发机（对比）
strace -e trace=read,write,poll,select,epoll_wait -p <leader_pid> -c -f
```

正常情况应看到 syscall 次数与 poll 频率成比例（~25 次/秒），而不是持续高频。

---

## 3. 解决方案矩阵

### 应对 CPU 100%

| 方案 | 改动量 | 效果 | 风险 |
|------|--------|------|------|
| **A. 同步化串口 I/O** | 中 | ★★★★★ | 串口改为 `std::io::Read` + `read_exact()` 阻塞读，去掉 async 层。在独立 `std::thread` 中做，用 mpsc 传结果 |
| **B. 降低 ARM 轮询到 10Hz** | 低 | ★★★☆☆ | 从 25Hz 降到 10Hz（100ms sleep），CPU 预期降 60%。代价：操作延迟增加到 100ms |
| **C. 替换 SDK 串口层** | 高 | ★★★★★ | 不用 `tokio_serial`，直接用 `serialport` 同步 crate，避免 tokio I/O driver 兼容问题 |
| **D. 内核参数调优** | 中 | ★★★☆☆ | 启用 ARM 内核的 `PREEMPT`、调高 `HZ`、确认串口驱动支持中断模式 |

### 应对双进程

| 方案 | 改动量 |
|------|--------|
| **PID file** | unit 文件加 1 行 `PIDFile=` |
| **flock 自检** | 代码加 5 行 |
| **Type=forking** | unit 改 `Type=forking` + 代码加 fork |

---

## 4. 推荐实施顺序

```
Step 1 — 诊断（必做，不要跳过）
  ├── ps -T -p $(pgrep leader) | wc -l    ← 确认是进程还是线程
  ├── strace -p <pid> -c -f 5秒采样      ← 找到 CPU syscall 热点
  └── cat /proc/<pid>/status | grep ^Threads

Step 2 — 止血（最低成本先上线）
  ├── flock 自检防双开（5 行代码）
  └── ARM 上 poll 降至 10Hz

Step 3 — 治本（根据 strace 结果选）
  ├── 若 strace 确认 SDK 忙等 → 方案 A: 串口同步化
  └── 若 strace 显示 epoll 异常  → 方案 D: 内核调优 + 方案 C
```

---

## 5. 参考

- systemd `Type=simple` vs `Type=forking`: https://www.freedesktop.org/software/systemd/man/systemd.service.html
- tokio I/O driver 内部机制: https://tokio.rs/tokio/topics/bridging
- ARM Linux 串口驱动: `drivers/tty/serial/8250/8250_port.c` (内核源码)
