#!/usr/bin/env bash
# setup-linux.sh — DoraRobot 一键部署脚本 (Linux)
#
# 流程：
#   1. 检查前置依赖
#   2. 扫描 USB 串口设备
#   3. 交互式选择主臂/从臂
#   4. 自动生成配置文件
#   5. 编译项目
#   6. 注册 systemd 服务
#   7. 启动服务
#
# 使用：
#   chmod +x scripts/setup-linux.sh
#   sudo ./scripts/setup-linux.sh

set -euo pipefail
PROJECT="$(cd "$(dirname "$0")/.." && pwd)"
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'

log()  { echo -e "${GREEN}[setup]${NC} $*"; }
warn() { echo -e "${YELLOW}[warn]${NC}  $*"; }
err()  { echo -e "${RED}[error]${NC} $*"; exit 1; }
info() { echo -e "${CYAN}        $*${NC}"; }

# sudo 默认清除代理环境变量，手动保留
for var in http_proxy https_proxy HTTP_PROXY HTTPS_PROXY all_proxy ALL_PROXY no_proxy NO_PROXY; do
    val=$(eval echo "\${$var}" 2>/dev/null || true)
    [ -n "$val" ] && export "$var"="$val"
done

# ──────────────────────────────────────────────
# 1. 前置检查
# ──────────────────────────────────────────────
check_deps() {
    log "检查前置依赖..."

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
        sudo -u "$REAL_USER" bash -c "
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        "
        log "Rust 已安装"
    fi

    # uv
    if [ ! -x "$UV_BIN" ]; then
        warn "uv 未安装，正在自动安装..."
        sudo -u "$REAL_USER" bash -c "
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
            git clone https://github.com/dora-rs/dora.git "$PROJECT/dora" || err "克隆 dora 仓库失败"
        fi
        cd "$PROJECT/dora"
        if ! git describe --tags --exact-match 2>/dev/null | grep -q "v1.0.0-rc1"; then
            warn "切换到 v1.0.0-rc1..."
            git fetch --tags && git checkout v1.0.0-rc1 || warn "git checkout 失败，继续编译（可能版本不匹配）"
        fi
        cd "$PROJECT"
        # 以实际用户身份编译（避免 sudo 下 cargo 权限问题）
        sudo -u "$REAL_USER" cargo build -p dora-cli --release --manifest-path "$PROJECT/dora/Cargo.toml" || err "dora 编译失败"
        mkdir -p "$REAL_HOME/.local/bin"
        cp "$PROJECT/dora/target/release/dora" "$REAL_HOME/.local/bin/dora"
        chown "$REAL_USER" "$REAL_HOME/.local/bin/dora" 2>/dev/null || true
        export PATH="$REAL_HOME/.local/bin:$PATH"
        log "dora CLI 已安装到 $REAL_HOME/.local/bin/dora"
    fi
    log "依赖检查通过"
}

# ──────────────────────────────────────────────
# 2. 扫描 USB 串口设备
# ──────────────────────────────────────────────

scan_usb_devices() {
    log "扫描 USB 串口设备..."

    echo ""
    echo "  序号  串口路径               VID:PID      Serial           描述"
    echo "  ────  ──────────────────────  ───────────  ────────────────  ──────"

    local i=0
    DEVICES=()

    # 扫描 /dev/ttyUSB* 和 /dev/ttyACM*
    for dev in /dev/ttyUSB* /dev/ttyACM*; do
        [ -e "$dev" ] || continue
        i=$((i + 1))

        # 获取 udev 信息
        local syspath=$(udevadm info -q path -n "$dev" 2>/dev/null || echo "")
        local vid=""; local pid=""; local serial=""; local desc=""

        # 沿 syspath 向上找 USB 设备信息
        if [ -n "$syspath" ]; then
            while [ "$syspath" != "/" ] && [ -z "$vid" ]; do
                vid=$(cat "/sys$syspath/idVendor" 2>/dev/null || echo "")
                pid=$(cat "/sys$syspath/idProduct" 2>/dev/null || echo "")
                serial=$(cat "/sys$syspath/serial" 2>/dev/null || echo "")
                desc=$(cat "/sys$syspath/product" 2>/dev/null || echo "")
                syspath=$(dirname "$syspath")
            done
        fi

        vid="${vid:-????}"
        pid="${pid:-????}"
        serial="${serial:-(无)}"
        desc="${desc:-未知设备}"

        DEVICES+=("$dev|$vid|$pid|$serial|$desc")
        printf "  %-4s  %-24s  0x%-8s  %-16s  %s\n" \
            "[$i]" "$dev" "${vid}:${pid}" "$serial" "$desc"
    done

    if [ "$i" -eq 0 ]; then
            err "未发现 USB 串口设备。请连接机械臂并重试。"
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

    # 去掉 0x 前缀
    LEADER_VID="${LEADER_VID#0x}"
    LEADER_PID="${LEADER_PID#0x}"
    FOLLOWER_VID="${FOLLOWER_VID#0x}"
    FOLLOWER_PID="${FOLLOWER_PID#0x}"

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
# 由 setup-linux.sh 自动生成 ($(date '+%Y-%m-%d %H:%M'))

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
# 由 setup-linux.sh 自动生成 ($(date '+%Y-%m-%d %H:%M'))

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

    log "配置文件已生成:"
    info "  $PROJECT/config/follower.toml"
    info "  $PROJECT/config/leader.toml"
}

# ──────────────────────────────────────────────
# 5. 编译项目
# ──────────────────────────────────────────────

build_project() {
    log "编译项目（首次约 10-20 分钟）..."

    cd "$PROJECT"
    cargo build --release || err "编译失败"
    cargo build -p tr-capture --release || err "tr-capture 编译失败"

    # 部署到 bin/
    log "部署二进制到 bin/..."
    mkdir -p "$PROJECT/bin"
    cp "$PROJECT/target/release/follower" "$PROJECT/bin/follower"
    cp "$PROJECT/target/release/leader"   "$PROJECT/bin/leader"
    cp "$PROJECT/target/release/tr-capture" "$PROJECT/bin/tr-capture"
    log "编译完成 → $PROJECT/bin/"
}

# ──────────────────────────────────────────────
# 6. 注册 systemd 服务
# ──────────────────────────────────────────────

register_services() {
    log "注册 systemd 服务..."

    mkdir -p "$PROJECT/logs"

    local venv="$PROJECT/training/.venv"

    # 从臂服务
    cat > /etc/systemd/system/dorarobot-follower.service << EOF
[Unit]
Description=DoraRobot Follower Daemon
After=network.target

[Service]
Type=simple
WorkingDirectory=$PROJECT
ExecStart=$PROJECT/bin/follower --config $PROJECT/config/follower.toml
Restart=always
RestartSec=5
Environment="PATH=${venv}/bin:/usr/bin:/bin"
Environment="VIRTUAL_ENV=${venv}"
StandardOutput=append:$PROJECT/logs/follower.log
StandardError=append:$PROJECT/logs/follower.log

[Install]
WantedBy=multi-user.target
EOF

    # 主臂 + Web 控制台服务
    cat > /etc/systemd/system/dorarobot-leader.service << EOF
[Unit]
Description=DoraRobot Leader Daemon + Web Console
After=network.target

[Service]
Type=simple
WorkingDirectory=$PROJECT
ExecStart=$PROJECT/bin/leader --config $PROJECT/config/leader.toml
Restart=always
RestartSec=5
Environment="PATH=${venv}/bin:/usr/bin:/bin"
Environment="VIRTUAL_ENV=${venv}"
StandardOutput=append:$PROJECT/logs/leader.log
StandardError=append:$PROJECT/logs/leader.log

[Install]
WantedBy=multi-user.target
EOF

    systemctl daemon-reload
    systemctl enable dorarobot-follower
    systemctl enable dorarobot-leader
    log "systemd 服务已注册并设为开机自启"
}

# ──────────────────────────────────────────────
# 7. 启动服务
# ──────────────────────────────────────────────

start_services() {
    log "启动服务..."

    systemctl start dorarobot-follower
    systemctl start dorarobot-leader

    sleep 3

    echo ""
    if systemctl is-active --quiet dorarobot-follower; then
        log "从臂服务: ${GREEN}运行中${NC}"
    else
        warn "从臂服务: 启动失败，查看日志: journalctl -u dorarobot-follower -n 20"
    fi

    if systemctl is-active --quiet dorarobot-leader; then
        log "主臂服务: ${GREEN}运行中${NC}"
    else
        warn "主臂服务: 启动失败，查看日志: journalctl -u dorarobot-leader -n 20"
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
    echo "  ║   DoraRobot 一键部署脚本 (Linux)        ║"
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
    echo "  ║   部署完成！                            ║"
    echo "  ╠══════════════════════════════════════════╣"
    echo "  ║  Web 控制台: http://<本机IP>:8080        ║"
    echo "  ║  查看日志:   journalctl -u dorarobot-* -f║"
    echo "  ║  停止服务:   systemctl stop dorarobot-*   ║"
    echo "  ╚══════════════════════════════════════════╝"
}

main
