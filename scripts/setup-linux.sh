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

set -eo pipefail
PROJECT="$(cd "$(dirname "$0")/.." && pwd)"
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'

log()  { echo -e "${GREEN}[setup]${NC} $*"; }
warn() { echo -e "${YELLOW}[warn]${NC}  $*"; }
err()  { echo -e "${RED}[error]${NC} $*"; exit 1; }
info() { echo -e "${CYAN}        $*${NC}"; }

# 设置代理环境变量（sudo 默认清除，需保留到子进程）
PROXY_ENV_CMD=""
ALL_PROXY_VARS="https_proxy http_proxy HTTPS_PROXY HTTP_PROXY all_proxy ALL_PROXY no_proxy NO_PROXY"

# 先导出到当前 shell（避免 sudo 非法参数）
for var in $ALL_PROXY_VARS; do
    val="$(eval echo "\${${var}:-}" 2>/dev/null || true)"
    [ -n "$val" ] && export "${var}=${val}" || true
done

# 构建正确的 env 命令字符串
for var in $ALL_PROXY_VARS; do
    val="$(eval echo "\${${var}:-}" 2>/dev/null || true)"
    [ -n "$val" ] && PROXY_ENV_CMD="${PROXY_ENV_CMD}${PROXY_ENV_CMD:+ }$var=${val}"
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
    
    # 导出代理到当前 shell（sudo env 中使用全局 PROXY_ENV_CMD）
    for var in $ALL_PROXY_VARS; do
        val="$(eval echo "\${${var}:-}" 2>/dev/null || true)"
        [ -n "$val" ] && export "${var}=${val}" || true
    done

    # Rust
    if [ ! -x "$CARGO_BIN" ]; then
        warn "Rust 未安装，正在自动安装..."
        sudo -u "$REAL_USER" env $PROXY_ENV_CMD bash -c "
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        "
        log "Rust 已安装"
    fi

    # uv
    if [ ! -x "$UV_BIN" ]; then
        warn "uv 未安装，正在自动安装..."
        sudo -u "$REAL_USER" env $PROXY_ENV_CMD bash -c "
            curl -LsSf https://astral.sh/uv/install.sh | sh
        "
        log "uv 已安装"
    fi

    # uv venv --python 3.12 会自动下载所需 Python，无需系统预装

    # DORA 源码 — 编译 workspace 需要的依赖，任意模式下都必须存在
    if [ ! -d "$PROJECT/dora" ]; then
        warn "dora 源码不存在，正在自动克隆（workspace 依赖需要）..."
        sudo -u "$REAL_USER" env $PROXY_ENV_CMD \
            git clone https://github.com/dora-rs/dora.git "$PROJECT/dora" || err "克隆 dora 仓库失败"
    fi

    # lerobot 源码 — 从臂录制/训练需要
    if [ "$NEED_DORA" = true ] && [ ! -d "$PROJECT/lerobot" ]; then
        warn "lerobot 源码不存在，正在自动克隆..."
        sudo -u "$REAL_USER" env $PROXY_ENV_CMD \
            git clone https://github.com/huggingface/lerobot.git "$PROJECT/lerobot" || warn "克隆 lerobot 失败（可后续手动克隆）"
    fi

    # DORA CLI — 只有从臂需要编译安装（需要 PYO3_PYTHON 指向 Python ≥3.11）
    if [ "$NEED_DORA" = true ] && [ ! -x "$DORA_BIN" ]; then
        warn "dora CLI 未安装，从源码编译安装..."
        cd "$PROJECT"
        sudo -u "$REAL_USER" env PYO3_PYTHON="${PYO3_PYTHON:-}" \
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
# 0. 预创建 Python 3.12 venv（DORA 编译需要 pyo3 ≥3.11）
# ──────────────────────────────────────────────
ensure_python312() {
    local REAL_USER="${SUDO_USER:-$USER}"
    local REAL_HOME
    if [ "$REAL_USER" != "root" ] && [ -n "$SUDO_USER" ]; then
        REAL_HOME=$(eval echo "~$REAL_USER")
    else
        REAL_HOME="$HOME"
    fi

    local UV_BIN="$REAL_HOME/.local/bin/uv"
    local VENV="$PROJECT/training/.venv"

    if [ -d "$VENV" ]; then
        log "Python venv 已存在，跳过创建"
    else
        log "预创建 Python 3.12 虚拟环境（供 DORA 编译使用）..."
        sudo -u "$REAL_USER" env $PROXY_ENV_CMD \
            "$UV_BIN" venv "$VENV" --python 3.12 || err "创建 venv 失败"
        log "Python 3.12 venv 已创建 → $VENV"
    fi
    export PYO3_PYTHON="$VENV/bin/python"
}

# ──────────────────────────────────────────────
# 1b. Linux 系统依赖（编译 usb-resolver 等需要）
# ──────────────────────────────────────────────

install_system_deps() {
    log "检查系统依赖..."
    local missing=""

    # 检查并安装缺失的包
    for pkg in pkg-config libudev-dev; do
        if ! dpkg -s "$pkg" >/dev/null 2>&1; then
            missing="$missing $pkg"
        fi
    done

    if [ -n "$missing" ]; then
        warn "安装系统依赖: $missing"
        apt-get update -qq
        apt-get install -y -qq $missing || warn "安装 $missing 失败，编译可能报错"
        log "系统依赖安装完成"
    fi
}

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
# 3a. 交互式选择单个臂
# ──────────────────────────────────────────────

select_single_arm() {
    local label="$1"
    echo ""
    log "请选择 $label"

    while true; do
        read -rp "  序号 [1-${#DEVICES[@]}]: " idx
        if [[ "$idx" =~ ^[0-9]+$ ]] && [ "$idx" -ge 1 ] && [ "$idx" -le "${#DEVICES[@]}" ]; then
            break
        fi
        warn "无效序号，请重试"
    done

    IFS='|' read -r DEV VID PID SERIAL DESC <<< "${DEVICES[$((idx - 1))]}"
    VID="${VID#0x}"; PID="${PID#0x}"

    if [ "$NEED_LEADER" = true ]; then
        LEADER_DEV="$DEV"; LEADER_VID="$VID"; LEADER_PID="$PID"; LEADER_SERIAL="$SERIAL"
    fi
    if [ "$NEED_FOLLOWER" = true ]; then
        FOLLOWER_DEV="$DEV"; FOLLOWER_VID="$VID"; FOLLOWER_PID="$PID"; FOLLOWER_SERIAL="$SERIAL"
    fi

    echo ""
    log "配置确认："
    info "$label: $DEV  (VID=0x$VID PID=0x$PID Serial=$SERIAL)"
    read -rp "  确认? [Y/n]: " confirm
    if [ "$confirm" = "n" ] || [ "$confirm" = "N" ]; then
        select_single_arm "$label"
        return
    fi
}

# ──────────────────────────────────────────────
# 3b. 交互式选择主臂和从臂（同一台机器两个臂）
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

    if [ "$NEED_FOLLOWER" = true ]; then
        cat > "$PROJECT/config/follower.toml" << EOF
# config/follower.toml — 从臂
# 由 setup-linux.sh 自动生成 ($(date '+%Y-%m-%d %H:%M'))

[arm]
id = "$ARM_ID"
type = "so101"

[arm.so101]
baud = 1_000_000
ids = [1, 2, 3, 4, 5, 6]
vid = "0x${FOLLOWER_VID}"
pid = "0x${FOLLOWER_PID}"
serial = "${FOLLOWER_SERIAL}"
EOF
        info "  $PROJECT/config/follower.toml"
    fi

    if [ "$NEED_LEADER" = true ]; then
        cat > "$PROJECT/config/leader.toml" << EOF
# config/leader.toml — 主臂 + Web 控制台
# 由 setup-linux.sh 自动生成 ($(date '+%Y-%m-%d %H:%M'))

[arm]
id = "$ARM_ID"
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
        info "  $PROJECT/config/leader.toml"
    fi

    log "配置文件已生成"
}

# ──────────────────────────────────────────────
# 5a. Python 虚拟环境 + 依赖安装（仅从臂）
# ──────────────────────────────────────────────

install_venv() {
    log "安装 Python 虚拟环境..."

    local REAL_USER="${SUDO_USER:-$USER}"
    local REAL_HOME
    if [ "$REAL_USER" != "root" ] && [ -n "$SUDO_USER" ]; then
        REAL_HOME=$(eval echo "~$REAL_USER")
    else
        REAL_HOME="$HOME"
    fi
    local CARGO_BIN="$REAL_HOME/.cargo/bin/cargo"
    local UV_BIN="$REAL_HOME/.local/bin/uv"
    local VENV="$PROJECT/training/.venv"
    local VENV_PYTHON="$VENV/bin/python"

    if [ ! -d "$VENV" ]; then
        sudo -u "$REAL_USER" env $PROXY_ENV_CMD "$UV_BIN" venv "$VENV" --python 3.12 || err "创建 venv 失败"
    fi

    log "安装 Python 依赖 (numpy, opencv, pyarrow)..."
    sudo -u "$REAL_USER" env $PROXY_ENV_CMD "$UV_BIN" pip install --python "$VENV_PYTHON" \
        numpy opencv-python pyarrow pyyaml || err "pip install 失败"

    log "安装 lerobot (含 torch, ~2GB)..."
    sudo -u "$REAL_USER" env $PROXY_ENV_CMD "$UV_BIN" pip install --python "$VENV_PYTHON" \
        lerobot || err "lerobot 安装失败"

    log "构建 DORA Python 包..."
    if ! command -v maturin >/dev/null; then
        sudo -u "$REAL_USER" env $PROXY_ENV_CMD "$CARGO_BIN" install maturin || warn "maturin 编译失败"
    fi
    MATURIN="$REAL_HOME/.cargo/bin/maturin"

    [ -x "$MATURIN" ] && sudo -u "$REAL_USER" env PATH="$REAL_HOME/.cargo/bin:$PATH" $PROXY_ENV_CMD \
        PYO3_PYTHON="$VENV_PYTHON" "$MATURIN" build \
        -m "$PROJECT/dora/apis/python/node/Cargo.toml" --release || warn "DORA wheel 构建失败"

    local wheel=$(ls "$PROJECT/dora/target/wheels/dora_rs-"*.whl 2>/dev/null | head -1)
    if [ -n "$wheel" ]; then
        sudo -u "$REAL_USER" env $PROXY_ENV_CMD "$UV_BIN" pip install --python "$VENV_PYTHON" "$wheel" || warn "wheel 安装失败"
        log "DORA Python 包已安装"
    fi
    log "Python 环境安装完成"
}

# ──────────────────────────────────────────────
# 5. 编译项目
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
    sudo -u "$REAL_USER" env $PROXY_ENV_CMD "$CARGO" build --release || err "编译失败"
    if [ "$NEED_DORA" = true ]; then
        sudo -u "$REAL_USER" env $PROXY_ENV_CMD "$CARGO" build -p tr-capture --release || err "tr-capture 编译失败"
    fi

    log "部署二进制到 bin/..."
    mkdir -p "$PROJECT/bin"

    # 停止运行中的服务，避免 cp 被占用（Text file busy）
    if [ "$NEED_FOLLOWER" = true ]; then
        systemctl stop dorarobot-follower 2>/dev/null || true
    fi
    if [ "$NEED_LEADER" = true ]; then
        systemctl stop dorarobot-leader 2>/dev/null || true
    fi

    if [ "$NEED_FOLLOWER" = true ]; then
        cp "$PROJECT/target/release/follower" "$PROJECT/bin/follower"
    fi
    if [ "$NEED_LEADER" = true ]; then
        cp "$PROJECT/target/release/leader" "$PROJECT/bin/leader"
    fi
    if [ "$NEED_DORA" = true ]; then
        cp "$PROJECT/target/release/tr-capture" "$PROJECT/bin/tr-capture"
    fi

    # 重启服务
    if [ "$NEED_FOLLOWER" = true ]; then
        systemctl start dorarobot-follower 2>/dev/null || true
    fi
    if [ "$NEED_LEADER" = true ]; then
        systemctl start dorarobot-leader 2>/dev/null || true
    fi
    log "编译完成 → $PROJECT/bin/"
}

# ──────────────────────────────────────────────
# 6. 注册 systemd 服务
# ──────────────────────────────────────────────

register_services() {
    log "注册 systemd 服务..."

    local REAL_USER="${SUDO_USER:-$USER}"
    local REAL_HOME
    if [ "$REAL_USER" != "root" ] && [ -n "$SUDO_USER" ]; then
        REAL_HOME=$(eval echo "~$REAL_USER")
    else
        REAL_HOME="$HOME"
    fi

    mkdir -p "$PROJECT/logs"
    local venv="$PROJECT/training/.venv"

    if [ "$NEED_FOLLOWER" = true ]; then
        cat > /etc/systemd/system/dorarobot-follower.service << EOF
[Unit]
Description=DoraRobot Follower Daemon
After=network.target

[Service]
Type=simple
User=$REAL_USER
SupplementaryGroups=video dialout
WorkingDirectory=$PROJECT
ExecStart=$PROJECT/bin/follower --config $PROJECT/config/follower.toml
Restart=always
RestartSec=5
Environment="PATH=${venv}/bin:${REAL_HOME}/.local/bin:/usr/bin:/bin"
Environment="VIRTUAL_ENV=${venv}"
StandardOutput=append:$PROJECT/logs/follower.log
StandardError=append:$PROJECT/logs/follower.log

[Install]
WantedBy=multi-user.target
EOF
        systemctl enable dorarobot-follower
    fi

    if [ "$NEED_LEADER" = true ]; then
        cat > /etc/systemd/system/dorarobot-leader.service << EOF
[Unit]
Description=DoraRobot Leader Daemon + Web Console
After=network.target

[Service]
Type=simple
User=$REAL_USER
SupplementaryGroups=video dialout
WorkingDirectory=$PROJECT
ExecStart=$PROJECT/bin/leader --config $PROJECT/config/leader.toml
Restart=always
RestartSec=5
Environment="PATH=${venv}/bin:${REAL_HOME}/.local/bin:/usr/bin:/bin"
Environment="VIRTUAL_ENV=${venv}"
StandardOutput=append:$PROJECT/logs/leader.log
StandardError=append:$PROJECT/logs/leader.log

[Install]
WantedBy=multi-user.target
EOF
        systemctl enable dorarobot-leader
    fi

    systemctl daemon-reload
    log "systemd 服务已注册并设为开机自启"
}

# ──────────────────────────────────────────────
# 7. 启动服务
# ──────────────────────────────────────────────

start_services() {
    log "启动服务..."

    if [ "$NEED_FOLLOWER" = true ]; then
        systemctl start dorarobot-follower
    fi
    if [ "$NEED_LEADER" = true ]; then
        systemctl start dorarobot-leader
    fi

    sleep 3
    echo ""

    if [ "$NEED_FOLLOWER" = true ]; then
        if systemctl is-active --quiet dorarobot-follower; then
            log "从臂服务: ${GREEN}运行中${NC}"
        else
            warn "从臂服务: 启动失败，查看日志: journalctl -u dorarobot-follower -n 20"
        fi
    fi

    if [ "$NEED_LEADER" = true ]; then
        if systemctl is-active --quiet dorarobot-leader; then
            log "主臂服务: ${GREEN}运行中${NC}"
        else
            warn "主臂服务: 启动失败，查看日志: journalctl -u dorarobot-leader -n 20"
        fi
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
    echo "  ║   DoraRobot 一键部署脚本 (Linux)         ║"
    echo "  ╚══════════════════════════════════════════╝"
    echo ""

    # ── 第一步：全部交互式问题（人机交互一次性完成）──
    echo "  请选择部署角色："
    echo "    [1] 主臂 (Leader)       — 主臂驱动 + Web 控制台"
    echo "    [2] 从臂 (Follower)     — 从臂驱动 + DORA 录制"
    echo "    [3] 全部 (Both)         — 主臂 + 从臂（同一台机器）"
    echo ""
    while true; do
        read -rp "  选择 [1-3]: " ROLE
        case "$ROLE" in
            1) NEED_DORA=false; NEED_LEADER=true;  NEED_FOLLOWER=false; break ;;
            2) NEED_DORA=true;  NEED_LEADER=false; NEED_FOLLOWER=true;  break ;;
            3) NEED_DORA=true;  NEED_LEADER=true;  NEED_FOLLOWER=true;  break ;;
            *) warn "无效选择，请输入 1/2/3" ;;
        esac
    done

    scan_usb_devices

    if [ "$NEED_LEADER" = true ] && [ "$NEED_FOLLOWER" = true ]; then
        select_arms
    elif [ "$NEED_LEADER" = true ]; then
        select_single_arm "主臂 (Leader)"
    else
        select_single_arm "从臂 (Follower)"
    fi

    # 实例序号（zenoh 通道隔离不同臂对）
    echo ""
    read -rp "  实例序号 (默认: 1): " ARM_NUM
    ARM_NUM="${ARM_NUM:-1}"
    ARM_ID="arm_${ARM_NUM}"
    log "实例序号: $ARM_ID"

    echo ""
    echo "  ╔══════════════════════════════════════════╗"
    echo "  ║   配置已确认，即将开始全自动安装        ║"
    echo "  ║   预计耗时 10-30 分钟（取决于网络）     ║"
    echo "  ╚══════════════════════════════════════════╝"
    echo ""
    read -rp "  开始自动化安装? [Y/n]: " do_start
    if [ "$do_start" = "n" ] || [ "$do_start" = "N" ]; then
        log "已取消"
        exit 0
    fi

    # ── 第二步：全自动安装（无需人工干预）──────
    if [ "$NEED_DORA" = true ]; then
        ensure_python312
    fi
    check_deps
    install_system_deps
    generate_configs
    if [ "$NEED_DORA" = true ]; then
        install_venv
    fi
    build_project
    register_services
    start_services

    echo ""
    echo "  ╔══════════════════════════════════════════╗"
    echo "  ║   部署完成！                            ║"
    echo "  ╠══════════════════════════════════════════╣"
    if [ "$NEED_LEADER" = true ]; then
    echo "  ║  Web 控制台: http://<本机IP>:8080        ║"
    fi
    if [ "$NEED_FOLLOWER" = true ]; then
    echo "  ║  查看从臂日志: sudo journalctl -u dorarobot-follower -f   ║"
    echo "  ║  停止从臂服务: sudo systemctl stop dorarobot-follower     ║"
    fi
    if [ "$NEED_LEADER" = true ]; then
    echo "  ║  查看主臂日志: sudo journalctl -u dorarobot-leader -f    ║"
    echo "  ║  停止主臂服务: sudo systemctl stop dorarobot-leader      ║"
    fi
    echo "  ╚══════════════════════════════════════════╝"
}

main
