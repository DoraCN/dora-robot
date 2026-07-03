# setup-windows.ps1 — DoraRobot 一键部署脚本 (Windows)
#
# 使用（PowerShell 管理员）：
#   Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
#   .\scripts\setup-windows.ps1

param()

$ErrorActionPreference = "Stop"
$PROJECT = (Get-Item (Join-Path $PSScriptRoot "..")).FullName
$script:DEVICES = @()

function Write-Log   { Write-Host "[setup] $args" -ForegroundColor Green }
function Write-Warn  { Write-Host "[warn]  $args" -ForegroundColor Yellow }
function Write-Err   { Write-Host "[error] $args" -ForegroundColor Red; exit 1 }

# ──────────────────────────────────────────────
# 1. 前置检查
# ──────────────────────────────────────────────

function Check-Deps {
    Write-Log "检查前置依赖..."

    # Rust — 自动安装
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Warn "Rust 未安装，正在自动安装..."
        Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile "$env:TEMP\rustup-init.exe"
        & "$env:TEMP\rustup-init.exe" -y
        $env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path
        Write-Log "Rust 已安装"
    }

    # uv — 自动安装
    if (-not (Get-Command uv -ErrorAction SilentlyContinue)) {
        Write-Warn "uv 未安装，正在自动安装..."
        powershell -ExecutionPolicy ByPass -c "irm https://astral.sh/uv/install.ps1 | iex"
        $env:Path = "$env:USERPROFILE\.local\bin;" + $env:Path
        Write-Log "uv 已安装"
    }

    # uv venv --python 3.12 会自动下载所需 Python，无需系统预装

    # DORA CLI — 如果未安装，从本地源码编译安装（源码不存在则自动克隆）
    if ($script:NEED_DORA -and -not (Get-Command dora -ErrorAction SilentlyContinue)) {
        Write-Warn "dora CLI 未安装，从源码编译安装..."
        if (-not (Test-Path "$PROJECT\thirdparty\dora")) {
            Write-Warn "dora 子模块未初始化，正在拉取..."
            Set-Location $PROJECT
            git submodule update --init -- thirdparty/dora
            if ($LASTEXITCODE -ne 0) { Write-Err "拉取 dora 子模块失败" }
        }
        cargo build -p dora-cli --release --manifest-path "$PROJECT\thirdparty\dora\Cargo.toml"
        if ($LASTEXITCODE -ne 0) { Write-Err "dora 编译失败" }
        $binDir = "$env:LOCALAPPDATA\dora"
        New-Item -ItemType Directory -Force -Path $binDir | Out-Null
        Copy-Item "$PROJECT\thirdparty\dora\target\release\dora.exe" "$binDir\dora.exe"
        $env:Path = "$binDir;$env:Path"
        [Environment]::SetEnvironmentVariable("Path", "$binDir;" + [Environment]::GetEnvironmentVariable("Path", "User"), "User")
        Write-Log "dora CLI 已安装到 $binDir\"
    }
    Write-Log "依赖检查通过"
}

# ──────────────────────────────────────────────
# 2. 扫描 COM 端口
# ──────────────────────────────────────────────

function Scan-USBDevices {
    Write-Log "扫描 COM 端口..."

    Write-Host ""
    Write-Host "  序号  COM端口    描述"
    Write-Host "  ────  ─────────  ──────────────────────────────"

    $ports = Get-CimInstance Win32_PnPEntity | Where-Object {
        $_.Name -match '\(COM\d+\)'
    } | ForEach-Object {
        if ($_.Name -match '\((COM\d+)\)') {
            [PSCustomObject]@{
                Port = $matches[1]
                Name = ($_.Name -replace '\s*\(COM\d+\)', '').Trim()
                PNPDeviceID = $_.PNPDeviceID
            }
        }
    }

    $i = 0
    foreach ($port in $ports) {
        $i++
        $script:DEVICES += $port
        Write-Host ("  [{0}]   {1,-9}  {2}" -f $i, $port.Port, $port.Name)
    }

    if ($i -eq 0) {
        Write-Err "未发现 COM 端口。请连接机械臂并重试。"
    }

    Write-Host ""
    Write-Log "发现 $i 个串口设备"
}

# ──────────────────────────────────────────────
# 3. 交互式选择主臂/从臂
# ──────────────────────────────────────────────

function Select-Arms {
    Write-Host ""
    Write-Log "请选择主臂（Leader）和从臂（Follower）"

    do {
        $idx = Read-Host "  主臂序号 [1-$($script:DEVICES.Count)]"
    } while (-not ($idx -match '^\d+$' -and [int]$idx -ge 1 -and [int]$idx -le $script:DEVICES.Count))
    $script:LEADER = $script:DEVICES[[int]$idx - 1]

    do {
        $idx = Read-Host "  从臂序号 [1-$($script:DEVICES.Count)]"
    } while (-not ($idx -match '^\d+$' -and [int]$idx -ge 1 -and [int]$idx -le $script:DEVICES.Count))
    $script:FOLLOWER = $script:DEVICES[[int]$idx - 1]

    Write-Host ""
    Write-Log "配置确认："
    Write-Host "        主臂: $($script:LEADER.Port) - $($script:LEADER.Name)"
    Write-Host "        从臂: $($script:FOLLOWER.Port) - $($script:FOLLOWER.Name)"

    $confirm = Read-Host "  确认? [Y/n]"
    if ($confirm -eq "n" -or $confirm -eq "N") {
        Select-Arms
    }
}

# ──────────────────────────────────────────────
# 4. 生成配置文件
# ──────────────────────────────────────────────

function Generate-Configs {
    Write-Log "生成配置文件..."
    New-Item -ItemType Directory -Force -Path "$PROJECT\config" | Out-Null

    $followerContent = @"
# config/follower.toml — 从臂
# 由 setup-windows.ps1 自动生成 ($(Get-Date -Format 'yyyy-MM-dd HH:mm'))

[arm]
id = "arm_1"
type = "so101"

[arm.so101]
baud = 1_000_000
ids = [1, 2, 3, 4, 5, 6]
vid = "0x1A86"
pid = "0x55D3"
serial = "$($script:FOLLOWER.Port)"
"@

    $leaderContent = @"
# config/leader.toml — 主臂 + Web 控制台
# 由 setup-windows.ps1 自动生成 ($(Get-Date -Format 'yyyy-MM-dd HH:mm'))

[arm]
id = "arm_1"
type = "so101"

[arm.so101]
baud = 1_000_000
ids = [1, 2, 3, 4, 5, 6]
vid = "0x1A86"
pid = "0x55D3"
serial = "$($script:LEADER.Port)"

[console]
bind = "0.0.0.0"
port = 8080
"@

    $followerContent | Out-File -FilePath "$PROJECT\config\follower.toml" -Encoding UTF8
    $leaderContent   | Out-File -FilePath "$PROJECT\config\leader.toml"   -Encoding UTF8
    Write-Log "配置文件已生成"
}

# ──────────────────────────────────────────────
# 5. 编译 + 部署到 bin/
# ──────────────────────────────────────────────

function Build-Project {
    Write-Log "编译项目（首次约 10-20 分钟）..."
    Push-Location $PROJECT
    if ($script:NEED_DORA) {
        cargo build --release
        if ($LASTEXITCODE -ne 0) { Write-Err "编译失败（从臂模式）" }
        cargo build -p tr-capture --release
        if ($LASTEXITCODE -ne 0) { Write-Err "tr-capture 编译失败" }
    } else {
        cargo build --workspace --exclude tr-capture --release
        if ($LASTEXITCODE -ne 0) { Write-Err "编译失败（主臂模式）" }
    }

    Write-Log "部署二进制到 bin\..."
    New-Item -ItemType Directory -Force -Path "$PROJECT\bin" | Out-Null
    Copy-Item "$PROJECT\target\release\follower.exe"   "$PROJECT\bin\follower.exe"
    Copy-Item "$PROJECT\target\release\leader.exe"     "$PROJECT\bin\leader.exe"
    if ($script:NEED_DORA) {
        Copy-Item "$PROJECT\target\release\tr-capture.exe" "$PROJECT\bin\tr-capture.exe"
    }
    Pop-Location
    Write-Log "编译完成 → $PROJECT\bin\"
}

# ──────────────────────────────────────────────
# 6. 注册任务计划程序服务
# ──────────────────────────────────────────────

function Register-Services {
    Write-Log "注册任务计划程序服务..."

    New-Item -ItemType Directory -Force -Path "$PROJECT\logs" | Out-Null
    New-Item -ItemType Directory -Force -Path "$PROJECT\scripts" | Out-Null

    $venvBin = "$PROJECT\training\.venv\Scripts"

    # 从臂启动脚本
    @"
@echo off
cd /d $PROJECT
set PATH=$venvBin;%PATH%
set VIRTUAL_ENV=$PROJECT\training\.venv
bin\follower.exe --config config\follower.toml >> logs\follower.log 2>&1
"@ | Out-File -FilePath "$PROJECT\scripts\start-follower.bat" -Encoding ASCII

    # 主臂启动脚本
    @"
@echo off
cd /d $PROJECT
set PATH=$venvBin;%PATH%
set VIRTUAL_ENV=$PROJECT\training\.venv
bin\leader.exe --config config\leader.toml >> logs\leader.log 2>&1
"@ | Out-File -FilePath "$PROJECT\scripts\start-leader.bat" -Encoding ASCII

    $action = New-ScheduledTaskAction -Execute "$PROJECT\scripts\start-follower.bat"
    $trigger = New-ScheduledTaskTrigger -AtLogon
    $settings = New-ScheduledTaskSettingsSet -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1)
    Register-ScheduledTask -TaskName "DoraRobot Follower" -Action $action -Trigger $trigger -Settings $settings -Force | Out-Null

    $action = New-ScheduledTaskAction -Execute "$PROJECT\scripts\start-leader.bat"
    Register-ScheduledTask -TaskName "DoraRobot Leader" -Action $action -Trigger $trigger -Settings $settings -Force | Out-Null

    Write-Log "任务计划程序已注册并设为开机自启"
}

# ──────────────────────────────────────────────
# 7. 启动服务
# ──────────────────────────────────────────────

function Start-Services {
    Write-Log "启动服务..."
    Start-ScheduledTask -TaskName "DoraRobot Follower"
    Start-ScheduledTask -TaskName "DoraRobot Leader"
    Start-Sleep -Seconds 3

    Write-Host ""
    $fState = (Get-ScheduledTask -TaskName "DoraRobot Follower").State
    $lState = (Get-ScheduledTask -TaskName "DoraRobot Leader").State

    if ($fState -eq "Running") { Write-Log "从臂服务: 运行中" }
    else { Write-Warn "从臂服务: $fState → 查看 logs\follower.log" }

    if ($lState -eq "Running") { Write-Log "主臂服务: 运行中" }
    else { Write-Warn "主臂服务: $lState → 查看 logs\leader.log" }
}

# ──────────────────────────────────────────────
# 入口
# ──────────────────────────────────────────────

function Main {
    if (([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        Write-Warn "检测到管理员身份运行，建议以普通用户身份运行"
    }

    Write-Host ""
    Write-Host "  ╔════════════════════════════════════════════╗"
    Write-Host "  ║   DoraRobot 一键部署脚本 (Windows)        ║"
    Write-Host "  ╚════════════════════════════════════════════╝"
    Write-Host ""

    # ── 第一步：全部交互式问题 ──
    $role = Read-Host "  部署角色 [1]主臂 [2]从臂 [3]全部 (1/2/3)"
    if ($role -notin @("1","2","3")) { Write-Err "无效选择" }
    $script:NEED_DORA = ($role -in @("2","3"))
    $script:NEED_FOLLOWER = ($role -in @("2","3"))
    $script:NEED_LEADER = ($role -in @("1","3"))

    Scan-USBDevices
    Select-Arms
    Generate-Configs

    Write-Host ""
    Write-Host "  ╔════════════════════════════════════════════╗"
    Write-Host "  ║   配置已确认，即将开始全自动安装          ║"
    Write-Host "  ║   预计耗时 10-30 分钟（取决于网络）       ║"
    Write-Host "  ╚════════════════════════════════════════════╝"
    Write-Host ""
    $start = Read-Host "  开始自动化安装? [Y/n]"
    if ($start -eq "n" -or $start -eq "N") { Write-Log "已取消"; return }

    # ── 第二步：全自动安装 ──
    Check-Deps
    Build-Project
    Register-Services
    Start-Services

    Write-Host ""
    Write-Host "  ╔════════════════════════════════════════════╗"
    Write-Host "  ║   部署完成！                              ║"
    Write-Host "  ╠════════════════════════════════════════════╣"
    Write-Host "  ║  Web 控制台: http://localhost:8080         ║"
    Write-Host "  ║  查看日志:   type logs\follower.log        ║"
    Write-Host "  ╚════════════════════════════════════════════╝"
}

Main
