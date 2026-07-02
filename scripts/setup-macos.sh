#!/usr/bin/env bash
# setup-macos.sh — DoraRobot 一键部署脚本 (macOS)
#
# 使用：
#   chmod +x scripts/setup-macos.sh
#   sudo ./scripts/setup-macos.sh

set -eo pipefail
PROJECT="$(cd "$(dirname "$0")/.." && pwd)"
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'

log()  { echo -e "${GREEN}[setup]${NC} $*"; }
warn() { echo -e "${YELLOW}[warn]${NC}  $*"; }
err()  { echo -e "${RED}[error]${NC} $*"; exit 1; }
info() { echo -e "${CYAN}        $*${NC}"; }

# 代理环境变量（sudo 默认清除，需显式传递到子进程）
PROXY_ENV=""
for v in https_proxy http_proxy HTTPS_PROXY HTTP_PROXY all_proxy ALL_PROXY; do
    eval "val=\${$v:-}"
    [ -n "$val" ] && PROXY_ENV="$PROXY_ENV $v=$val"
done

# 同时保留到当前 shell
for var in http_proxy https_proxy HTTP_PROXY HTTPS_PROXY all_proxy ALL_PROXY no_proxy NO_PROXY; do
    val="$(eval echo "\${${var}:-}" 2>/dev/null || true)"
    [ -n "$val" ] && export "${var}=${val}" || true
done

# ──────────────────────────────────────────────
# 1. 前置检查
# ──────────────────────────────────────────────
check_deps() {
    log "检查前置依赖..."

    # 如果通过 sudo 运行，以实际用户身份安装 Rust/uv/dora
    local REAL_USER="${SUDO_USER:-$USER}"
    local REAL_HOME
    if [ "$REAL_USER" != "root" ] && [ -n "$SUDO_USER" ]; then
        REAL_HOME=$(eval echo "~$REAL_USER")
    else
        REAL_HOME="$HOME"
    fi

    local CARGO_BIN="$REAL_HOME/.cargo/bin/cargo"
    local UV_BIN="$REAL_HOME/.local/bin/uv"
    local DORA_BIN="$REAL_HOME/.local/bin/dora"

    # Rust
    if [ ! -x "$CARGO_BIN" ]; then
        warn "Rust 未安装，正在自动安装..."
        sudo -u "$REAL_USER" env $PROXY_ENV bash -c "
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        "
        log "Rust 已安装"
    fi

    # uv
    if [ ! -x "$UV_BIN" ]; then
        warn "uv 未安装，正在自动安装..."
        sudo -u "$REAL_USER" env $PROXY_ENV bash -c "
            curl -LsSf https://astral.sh/uv/install.sh | sh
        "
        log "uv 已安装"
    fi

    # uv venv --python 3.12 会自动下载所需 Python，无需系统预装

    # DORA CLI — 如果未安装，从本地源码编译安装（源码不存在则自动克隆）
    if [ ! -x "$DORA_BIN" ]; then
        warn "dora CLI 未安装，从源码编译安装..."
        if [ ! -d "$PROJECT/dora" ]; then
            warn "dora 源码不存在，正在自动克隆..."
            sudo -u "$REAL_USER" env $PROXY_ENV \
                git clone https://github.com/dora-rs/dora.git "$PROJECT/dora" || err "克隆 dora 仓库失败"
        fi
        cd "$PROJECT/dora"
        git fetch --tags 2>/dev/null || true
        if ! git checkout v1.0.0-rc.1 2>/dev/null; then
            warn "tag v1.0.0-rc.1 不存在，使用默认分支..."
        fi
        cd "$PROJECT"
        sudo -u "$REAL_USER" env $PROXY_ENV \
            "$CARGO_BIN" build -p dora-cli --release --manifest-path "$PROJECT/dora/Cargo.toml" || err "dora 编译失败"
        mkdir -p "$REAL_HOME/.local/bin"
        cp "$PROJECT/dora/target/release/dora" "$REAL_HOME/.local/bin/dora"
        chown "$REAL_USER" "$REAL_HOME/.local/bin/dora" 2>/dev/null || true
        export PATH="$REAL_HOME/.local/bin:$PATH"
        log "dora CLI 已安装到 $REAL_HOME/.local/bin/dora"
    fi
    log "依赖检查通过"
}

# ──────────────────────────────────────────────
# 2. 扫描 USB 串口设备 (macOS — /dev/cu.usbmodem*)
# ──────────────────────────────────────────────

scan_usb_devices() {
    log "扫描 USB 串口设备..."

    echo ""
    echo "  序号  串口路径                     VID:PID      Serial           描述"
    echo "  ────  ────────────────────────────  ───────────  ────────────────  ──────"

    local i=0
    DEVICES=()

    for dev in /dev/cu.usbmodem*; do
        [ -e "$dev" ] || continue
        i=$((i + 1))

        local vid=""; local pid=""; local serial=""; local desc=""

        # 用 ioreg 查找该设备对应的 USB 信息
        while IFS='|' read -r v p s d; do
            vid="$v"; pid="$p"; serial="$s"; desc="$d"
            break
        done < <(ioreg -p IOUSB -l -w0 2>/dev/null | \
            awk -v dev="$(basename "$dev")" '
            /IOCalloutDevice.*'"$(basename "$dev")"'/ { found=1 }
            found && /"idVendor"/  { gsub(/[^0-9]/,"",$NF); vid=$NF }
            found && /"idProduct"/ { gsub(/[^0-9]/,"",$NF); pid=$NF }
            found && /"USB Serial Number"/ { gsub(/[^=]*= /,""); gsub(/"/,""); serial=$0 }
            found && /"USB Product Name"/  { gsub(/[^=]*= /,""); gsub(/"/,""); desc=$0 }
            found && /^[}]/ { if(vid && pid) printf "%04x|%04x|%s|%s\n",vid,pid,serial,desc; found=0; vid=""; pid=""; serial=""; desc="" }
        ' 2>/dev/null)

        # 回退：用 system_profiler 获取
        if [ -z "$vid" ]; then
            local info
            info=$(system_profiler SPUSBDataType 2>/dev/null | \
                awk -v dev="$(basename "$dev")" '
                /Product ID:/  { pid=$3 }
                /Vendor ID:/   { vid=$3 }
                /Serial Number:/ && /[0-9a-fA-F]/ { gsub(/.*: /,""); serial=$0 }
                /Product:/     { gsub(/.*: /,""); desc=$0; if(vid && pid) printf "%s|%s|%s|%s\n",vid,pid,serial,desc; vid=""; pid=""; serial=""; desc="" }
            ')
            if [ -n "$info" ]; then
                IFS='|' read -r vid pid serial desc <<< "$info"
            fi
        fi

        # 从设备文件名提取 serial（macOS 设备名格式: cu.usbmodem<SERIAL>）
        if [ "$serial" = "(无)" ] || [ -z "$serial" ]; then
            serial=$(basename "$dev" | sed 's/cu\.usbmodem//')
        fi

        vid="${vid:-????}"
        pid="${pid:-????}"
        serial="${serial:-(无)}"
        desc="${desc:-未知设备}"

        DEVICES+=("$dev|$vid|$pid|$serial|$desc")
        printf "  %-4s  %-30s  0x%-8s  %-16s  %s\n" \
            "[$i]" "$dev" "${vid}:${pid}" "$serial" "$desc"
    done

    if [ "$i" -eq 0 ]; then
            err "未发现 USB 串口设备 (/dev/cu.usbmodem*)。请连接机械臂并重试。"
    fi

    echo ""
    log "发现 $i 个 USB 串口设备"
}

# ──────────────────────────────────────────────
# 3. 交互式选择主臂/从臂
# ──────────────────────────────────────────────

select_arms() {
    echo ""
    log "请选择主臂（Leader）和从臂（Follower）"

    while true; do
        read -rp "  主臂序号 [1-${#DEVICES[@]}]: " leader_idx
        if [[ "$leader_idx" =~ ^[0-9]+$ ]] && [ "$leader_idx" -ge 1 ] && [ "$leader_idx" -le "${#DEVICES[@]}" ]; then
            break
        fi
        warn "无效序号，请重试"
    done

    while true; do
        read -rp "  从臂序号 [1-${#DEVICES[@]}]: " follower_idx
        if [[ "$follower_idx" =~ ^[0-9]+$ ]] && [ "$follower_idx" -ge 1 ] && [ "$follower_idx" -le "${#DEVICES[@]}" ]; then
            break
        fi
        warn "无效序号，请重试"
    done

    IFS='|' read -r LEADER_DEV LEADER_VID LEADER_PID LEADER_SERIAL LEADER_DESC <<< "${DEVICES[$((leader_idx - 1))]}"
    IFS='|' read -r FOLLOWER_DEV FOLLOWER_VID FOLLOWER_PID FOLLOWER_SERIAL FOLLOWER_DESC <<< "${DEVICES[$((follower_idx - 1))]}"

    LEADER_VID="${LEADER_VID#0x}"; LEADER_PID="${LEADER_PID#0x}"
    FOLLOWER_VID="${FOLLOWER_VID#0x}"; FOLLOWER_PID="${FOLLOWER_PID#0x}"

    echo ""
    log "配置确认："
    info "主臂: $LEADER_DEV  (VID=0x$LEADER_VID PID=0x$LEADER_PID Serial=$LEADER_SERIAL)"
    info "从臂: $FOLLOWER_DEV  (VID=0x$FOLLOWER_VID PID=0x$FOLLOWER_PID Serial=$FOLLOWER_SERIAL)"

    read -rp "  确认? [Y/n]: " confirm
    if [ "$confirm" = "n" ] || [ "$confirm" = "N" ]; then
        select_arms
        return
    fi
}

# ──────────────────────────────────────────────
# 4. 生成配置文件
# ──────────────────────────────────────────────

generate_configs() {
    log "生成配置文件..."
    mkdir -p "$PROJECT/config"

    cat > "$PROJECT/config/follower.toml" << EOF
# config/follower.toml — 从臂
# 由 setup-macos.sh 自动生成 ($(date '+%Y-%m-%d %H:%M'))

[arm]
id = "arm_1"
type = "so101"

[arm.so101]
baud = 1_000_000
ids = [1, 2, 3, 4, 5, 6]
vid = "0x${FOLLOWER_VID}"
pid = "0x${FOLLOWER_PID}"
serial = "${FOLLOWER_SERIAL}"
EOF

    cat > "$PROJECT/config/leader.toml" << EOF
# config/leader.toml — 主臂 + Web 控制台
# 由 setup-macos.sh 自动生成 ($(date '+%Y-%m-%d %H:%M'))

[arm]
id = "arm_1"
type = "so101"

[arm.so101]
baud = 1_000_000
ids = [1, 2, 3, 4, 5, 6]
vid = "0x${LEADER_VID}"
pid = "0x${LEADER_PID}"
serial = "${LEADER_SERIAL}"

[console]
bind = "0.0.0.0"
port = 8080
EOF

    log "配置文件已生成"
}

# ──────────────────────────────────────────────
# 5. 编译 + 部署到 bin/
# ──────────────────────────────────────────────

build_project() {
    log "编译项目（首次约 10-20 分钟）..."

    local REAL_USER="${SUDO_USER:-$USER}"
    local REAL_HOME
    if [ "$REAL_USER" != "root" ] && [ -n "$SUDO_USER" ]; then
        REAL_HOME=$(eval echo "~$REAL_USER")
    else
        REAL_HOME="$HOME"
    fi
    local CARGO="$REAL_HOME/.cargo/bin/cargo"

    cd "$PROJECT"
    sudo -u "$REAL_USER" env $PROXY_ENV "$CARGO" build --release || err "编译失败"
    sudo -u "$REAL_USER" env $PROXY_ENV "$CARGO" build -p tr-capture --release || err "tr-capture 编译失败"

    log "部署二进制到 bin/..."
    mkdir -p "$PROJECT/bin"
    cp "$PROJECT/target/release/follower"   "$PROJECT/bin/follower"
    cp "$PROJECT/target/release/leader"     "$PROJECT/bin/leader"
    cp "$PROJECT/target/release/tr-capture" "$PROJECT/bin/tr-capture"
    log "编译完成 → $PROJECT/bin/"
}

# ──────────────────────────────────────────────
# 6. 注册 launchd 服务
# ──────────────────────────────────────────────

register_services() {
    log "注册 launchd 服务..."

    mkdir -p "$PROJECT/logs"
    local venv="$PROJECT/training/.venv"

    cat > /Library/LaunchDaemons/com.dorarobot.follower.plist << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.dorarobot.follower</string>
    <key>ProgramArguments</key>
    <array>
        <string>$PROJECT/bin/follower</string>
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
        <string>${venv}/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
        <key>VIRTUAL_ENV</key>
        <string>${venv}</string>
    </dict>
</dict>
</plist>
EOF

    cat > /Library/LaunchDaemons/com.dorarobot.leader.plist << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.dorarobot.leader</string>
    <key>ProgramArguments</key>
    <array>
        <string>$PROJECT/bin/leader</string>
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
        <string>${venv}/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
        <key>VIRTUAL_ENV</key>
        <string>${venv}</string>
    </dict>
</dict>
</plist>
EOF

    launchctl load /Library/LaunchDaemons/com.dorarobot.follower.plist
    launchctl load /Library/LaunchDaemons/com.dorarobot.leader.plist
    log "launchd 服务已注册并设为开机自启"
}

# ──────────────────────────────────────────────
# 7. 启动服务
# ──────────────────────────────────────────────

start_services() {
    log "启动服务..."
    launchctl start com.dorarobot.follower
    launchctl start com.dorarobot.leader
    sleep 3

    echo ""
    if launchctl list | grep -q com.dorarobot.follower; then
        log "从臂服务: ${GREEN}运行中${NC}"
    else
        warn "从臂服务: 启动失败 → tail $PROJECT/logs/follower.log"
    fi

    if launchctl list | grep -q com.dorarobot.leader; then
        log "主臂服务: ${GREEN}运行中${NC}"
    else
        warn "主臂服务: 启动失败 → tail $PROJECT/logs/leader.log"
    fi
}

# ──────────────────────────────────────────────
# 入口
# ──────────────────────────────────────────────

main() {
    if [ "$(id -u)" -ne 0 ]; then
        err "请用 sudo 运行此脚本"
    fi

    echo ""
    echo "  ╔══════════════════════════════════════════╗"
    echo "  ║   DoraRobot 一键部署脚本 (macOS)           ║"
    echo "  ╚══════════════════════════════════════════╝"
    echo ""

    check_deps
    scan_usb_devices
    select_arms
    generate_configs

    echo ""
    read -rp "  现在编译项目? [Y/n]: " do_build
    if [ "$do_build" != "n" ] && [ "$do_build" != "N" ]; then
        build_project
    fi

    register_services
    start_services

    echo ""
    echo "  ╔══════════════════════════════════════════╗"
    echo "  ║   部署完成！                               ║"
    echo "  ╠══════════════════════════════════════════╣"
    echo "  ║  Web 控制台: http://localhost:8080        ║"
    echo "  ║  查看日志:   tail -f logs/follower.log    ║"
    echo "  ║  停止服务:   sudo launchctl stop ...      ║"
    echo "  ╚══════════════════════════════════════════╝"
}

main
