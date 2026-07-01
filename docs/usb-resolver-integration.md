# USB 设备路径解析 — 技术方案

> 用 `usb-resolver` 替代硬编码 `/dev/cu.usbmodem*`，启动时自动发现 SO-101 臂的串口路径。

---

## 1. 决策

| # | 决策 | 结论 |
|---|---|---|
| 1 | 配置格式 | 硬件参数收归 `[arm.<type>]` 段，由 `arm.type` 指定 |
| 2 | 生产要求 | 必须配 `serial`，精确匹配 |
| 3 | 启动流程 | `scan_now()` 一次 → 拿路径 → 打开 `FeetechBus` |
| 4 | 热插拔 | 后续实现（daemon 掉线重连阶段） |
| 5 | 工具 | 提供 `usb_scan` 诊断示例 |

---

## 2. 配置格式

硬件参数严格关联臂型号，通过 `arm.type` 指定，不同型号对应独立的配置段：

```toml
[arm]
id = "arm_1"                # 臂对实例标识（和硬件无关）
type = "so101"              # 硬件型号，决定读取哪个段

# SO-101 硬件定义 — 所有 SO-101 臂都一样（替换型号时整体替换此段）
[arm.so101]
baud = 1_000_000
ids = [1, 2, 3, 4, 5, 6]
vid = "0x0483"              # STMicroelectronics (STM32 CDC)
pid = "0x5740"              # STM32 Virtual COM Port
serial = "5A7A0555021"      # 生产环境必填，精确匹配
```

程序逻辑：读 `arm.type` → 找 `[arm.<type>]` 段 → 用该段参数初始化硬件 + 解析 USB 路径。

未来接入其他型号（如 UR5）时：

```toml
[arm]
id = "arm_3"
type = "ur5"

[arm.ur5]
host = "192.168.1.100"
port = 30002
```

每个硬件类型对应独立的配置段和 Rust 解析结构体，互不干扰。

---

## 3. 匹配策略

由 `usb-resolver::DeviceRule::matches()` 提供：

| 优先级 | 条件 | 方法 |
|---|---|---|
| 1 | VID+PID 匹配 + serial 相等 | `SerialExact` |
| 2 | VID+PID 匹配 + port_path 相等 | `PortPath` |
| 3 | VID+PID 匹配，规则无 serial 无 port_path | `VidPidOnly` |

生产环境配 serial → 走 SerialExact，确保不会误匹配其他同 VID/PID 的设备。

---

## 4. 解析器模块

放在 `crates/tr-so101/src/resolver.rs`，函数签名做成通用的：

```rust
pub fn resolve_arm_port(device: &UsbDeviceConfig) -> Result<String>
```

`UsbDeviceConfig` 为 `[arm.device]` 节的 Rust 表示。

---

## 5. usb_scan 工具

示例 `crates/tr-so101/examples/usb_scan.rs`：

```
$ cargo run -p tr-so101 --example usb_scan

─── 设备 1 ───
  VID         : 0x0483
  PID         : 0x5740
  Serial      : 5A7A0555021
  Device file : /dev/cu.usbmodem5A7A0555021

  ✅ 匹配 SO-101
  建议配置:
    [arm.device]
    vid = "0x0483"
    pid = "0x5740"
    serial = "5A7A0555021"
```

---

## 6. 跨平台

`usb_resolver::get_monitor()` 自动平台检测，无需 `#[cfg]` 分支。

| 平台 | 实现 | 路径来源 |
|---|---|---|
| macOS | IOKit (`MacMonitor`) | `system_path_alt` → `/dev/cu.*` |
| Linux | udev (`LinuxMonitor`) | `system_path_alt` → `/dev/ttyUSB*` / `/dev/ttyACM*` |

---

## 7. 依赖

```toml
# workspace
usb-resolver = "0.1.1"
```
