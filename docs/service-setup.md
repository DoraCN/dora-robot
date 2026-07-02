# 注册为系统服务 — 开机自启操作手册

> 将从臂 daemon（follower）和主臂 daemon（leader）注册为操作系统服务，实现开机自启、崩溃自动重启。

---

## 1. 构建 Release 二进制

```bash
cargo build --release
cargo build -p tr-capture --release
```

二进制位置：

```
target/release/follower
target/release/leader
target/release/tr-capture
```

> 下面所有服务配置中的 `$PROJECT` 替换为项目实际路径，例如 `/home/echo/dora-robot`。

---

## 2. macOS — launchd

### 2.1 从臂服务

创建 `/Library/LaunchDaemons/com.dorarobot.follower.plist`：

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.dorarobot.follower</string>

    <key>ProgramArguments</key>
    <array>
        <string>$PROJECT/target/release/follower</string>
        <string>--config</string>
        <string>$PROJECT/config/follower.toml</string>
    </array>

    <key>WorkingDirectory</key>
    <string>$PROJECT</string>

    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>$PROJECT/logs/follower.log</string>
    <key>StandardErrorPath</key>
    <string>$PROJECT/logs/follower.log</string>

    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>$PROJECT/training/.venv/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
        <key>VIRTUAL_ENV</key>
        <string>$PROJECT/training/.venv</string>
    </dict>
</dict>
</plist>
```

### 2.2 主臂 + Web 控制台服务

创建 `/Library/LaunchDaemons/com.dorarobot.leader.plist`：

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.dorarobot.leader</string>

    <key>ProgramArguments</key>
    <array>
        <string>$PROJECT/target/release/leader</string>
        <string>--config</string>
        <string>$PROJECT/config/leader.toml</string>
    </array>

    <key>WorkingDirectory</key>
    <string>$PROJECT</string>

    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>$PROJECT/logs/leader.log</string>
    <key>StandardErrorPath</key>
    <string>$PROJECT/logs/leader.log</string>

    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>$PROJECT/training/.venv/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
        <key>VIRTUAL_ENV</key>
        <string>$PROJECT/training/.venv</string>
    </dict>
</dict>
</plist>
```

### 2.3 加载服务

```bash
# 创建日志目录
mkdir -p $PROJECT/logs

# 加载
sudo cp com.dorarobot.follower.plist /Library/LaunchDaemons/
sudo cp com.dorarobot.leader.plist /Library/LaunchDaemons/
sudo launchctl load /Library/LaunchDaemons/com.dorarobot.follower.plist
sudo launchctl load /Library/LaunchDaemons/com.dorarobot.leader.plist

# 立即启动
sudo launchctl start com.dorarobot.follower
sudo launchctl start com.dorarobot.leader

# 查看状态
sudo launchctl list | grep dorarobot

# 查看日志
tail -f $PROJECT/logs/follower.log
tail -f $PROJECT/logs/leader.log
```

### 2.4 管理命令

```bash
# 停止
sudo launchctl stop com.dorarobot.follower
sudo launchctl stop com.dorarobot.leader

# 卸载
sudo launchctl unload /Library/LaunchDaemons/com.dorarobot.follower.plist
sudo launchctl unload /Library/LaunchDaemons/com.dorarobot.leader.plist
```

---

## 3. Linux — systemd

### 3.1 从臂服务

创建 `/etc/systemd/system/dorarobot-follower.service`：

```ini
[Unit]
Description=DoraRobot Follower Daemon
After=network.target

[Service]
Type=simple
WorkingDirectory=$PROJECT
ExecStart=$PROJECT/target/release/follower --config $PROJECT/config/follower.toml
Restart=always
RestartSec=5
Environment="PATH=$PROJECT/training/.venv/bin:/usr/bin:/bin"
Environment="VIRTUAL_ENV=$PROJECT/training/.venv"
StandardOutput=append:$PROJECT/logs/follower.log
StandardError=append:$PROJECT/logs/follower.log

[Install]
WantedBy=multi-user.target
```

### 3.2 主臂 + Web 控制台服务

创建 `/etc/systemd/system/dorarobot-leader.service`：

```ini
[Unit]
Description=DoraRobot Leader Daemon + Web Console
After=network.target

[Service]
Type=simple
WorkingDirectory=$PROJECT
ExecStart=$PROJECT/target/release/leader --config $PROJECT/config/leader.toml
Restart=always
RestartSec=5
Environment="PATH=$PROJECT/training/.venv/bin:/usr/bin:/bin"
Environment="VIRTUAL_ENV=$PROJECT/training/.venv"
StandardOutput=append:$PROJECT/logs/leader.log
StandardError=append:$PROJECT/logs/leader.log

[Install]
WantedBy=multi-user.target
```

### 3.3 加载服务

```bash
# 创建日志目录
mkdir -p $PROJECT/logs

# 加载
sudo systemctl daemon-reload
sudo systemctl enable dorarobot-follower
sudo systemctl enable dorarobot-leader

# 启动
sudo systemctl start dorarobot-follower
sudo systemctl start dorarobot-leader

# 查看状态
sudo systemctl status dorarobot-follower
sudo systemctl status dorarobot-leader

# 查看日志
sudo journalctl -u dorarobot-follower -f
sudo journalctl -u dorarobot-leader -f
```

### 3.4 管理命令

```bash
sudo systemctl stop dorarobot-follower
sudo systemctl stop dorarobot-leader
sudo systemctl restart dorarobot-follower
sudo systemctl disable dorarobot-follower   # 取消开机自启
```

---

## 4. Windows — 任务计划程序

Windows 下推荐用**任务计划程序 (Task Scheduler)** 而非 Windows Service，更简单可靠。

### 4.1 创建启动脚本

**从臂** — 创建 `$PROJECT\scripts\start-follower.bat`：

```bat
@echo off
cd /d $PROJECT
set PATH=$PROJECT\training\.venv\Scripts;%PATH%
set VIRTUAL_ENV=$PROJECT\training\.venv
target\release\follower.exe --config config\follower.toml >> logs\follower.log 2>&1
```

**主臂 + Web** — 创建 `$PROJECT\scripts\start-leader.bat`：

```bat
@echo off
cd /d $PROJECT
set PATH=$PROJECT\training\.venv\Scripts;%PATH%
set VIRTUAL_ENV=$PROJECT\training\.venv
target\release\leader.exe --config config\leader.toml >> logs\leader.log 2>&1
```

### 4.2 注册任务（PowerShell 管理员）

```powershell
# 创建日志目录
New-Item -ItemType Directory -Force -Path "$PROJECT\logs"

$action = New-ScheduledTaskAction -Execute "$PROJECT\scripts\start-follower.bat"
$trigger = New-ScheduledTaskTrigger -AtStartup
$settings = New-ScheduledTaskSettingsSet -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1)
Register-ScheduledTask -TaskName "DoraRobot Follower" -Action $action -Trigger $trigger -Settings $settings -RunLevel Highest

$action = New-ScheduledTaskAction -Execute "$PROJECT\scripts\start-leader.bat"
Register-ScheduledTask -TaskName "DoraRobot Leader" -Action $action -Trigger $trigger -Settings $settings -RunLevel Highest
```

### 4.3 管理命令

```powershell
# 查看状态
Get-ScheduledTask -TaskName "DoraRobot*" | Select TaskName, State

# 手动启动
Start-ScheduledTask -TaskName "DoraRobot Follower"

# 停止
Stop-ScheduledTask -TaskName "DoraRobot Follower"

# 删除
Unregister-ScheduledTask -TaskName "DoraRobot Follower" -Confirm:$false
```

---

## 5. 验证服务

```bash
# 重启机器
sudo reboot

# 重启后检查
# macOS:
sudo launchctl list | grep dorarobot
tail -f $PROJECT/logs/follower.log

# Linux:
sudo systemctl status dorarobot-follower

# Windows:
Get-ScheduledTask -TaskName "DoraRobot*" | Select TaskName, State
```

正常输出：follower 日志显示 `state=IDLE`，leader 日志显示 `web console → http://localhost:8080`。

---

## 6. 配置自定义串口

如果 USB 自动发现不工作，可以在启动参数中指定串口：

```bash
# 在服务配置中修改 ExecStart / ProgramArguments，追加 --port：
follower --config config/follower.toml --port /dev/cu.usbmodemXXXX
leader --config config/leader.toml --port /dev/cu.usbmodemYYYY
```

Windows 下串口格式为 `COM3` 等。

---

## 7. 崩溃重启策略

| 系统 | 配置项 | 行为 |
|---|---|---|
| macOS | `KeepAlive=true` | 进程退出后自动重启 |
| Linux | `Restart=always` + `RestartSec=5` | 5 秒后重启 |
| Windows | `RestartCount=999` + `RestartInterval=1min` | 最多重启 999 次，间隔 1 分钟 |
