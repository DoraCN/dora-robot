#!/usr/bin/env bash
# setup-linux.sh — DoraRobot 一键部署脚本 (Linux)
#
# 以普通用户身份运行。需要提权时自动使用 sudo（仅 apt/loginctl）。
#
# 使用：
#   bash scripts/setup-linux.sh

set -eo pipefail
PROJECT="$(cd "$(dirname "$0")/.." && pwd)"
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'

log()  { echo -e "${GREEN}[setup]${NC} $*"; }
warn() { echo -e "${YELLOW}[warn]${NC}  $*"; }
err()  { echo -e "${RED}[error]${NC} $*"; exit 1; }
info() { echo -e "${CYAN}        $*${NC}"; }

# ──────────────────────────────────────────────
# 1. 系统依赖（需要 sudo 提权）
# ──────────────────────────────────────────────
install_system_deps() {
    log "检查系统依赖..."
    local missing=""
    for pkg in pkg-config libudev-dev; do
        if ! dpkg -s "$pkg" >/dev/null 2>&1; then
            missing="$missing $pkg"
        fi
    done
    if [ -n "$missing" ]; then
        warn "安装系统依赖: $missing"
        sudo apt-get update -qq
        sudo apt-get install -y -qq $missing || warn "安装 $missing 失败，编译可能报错"
        log "系统依赖安装完成"
    fi
}

# ── Leader-only: 创建 dora 骨架（仅 Cargo.toml），满足 workspace 解析 ──
create_dora_stubs() {
    log "创建 dora Cargo.toml 骨架（主臂模式无需完整源码）..."
    local D="$PROJECT/thirdparty/dora"
    # (dir, package_name, optional_feature)
    local PAIRS=(
        "apis/rust/node:dora-node-api:"
        "libraries/core:dora-core:zenoh"
        "libraries/message:dora-message:"
        "libraries/arrow-convert:dora-arrow-convert:"
        "libraries/shared-memory-server:shared-memory-server:"
        "libraries/extensions/telemetry/tracing:dora-tracing:"
        "libraries/extensions/telemetry/metrics:dora-metrics:"
    )
    for entry in "${PAIRS[@]}"; do
        local dir="${entry%%:*}"
        local rest="${entry#*:}"
        local name="${rest%%:*}"
        local feat="${rest##*:}"
        [ "$feat" = "$name" ] && feat=""

        local p="$D/$dir"
        mkdir -p "$p/src"
        echo -n "" > "$p/src/lib.rs"

        if [ -n "$feat" ]; then
            cat > "$p/Cargo.toml" << CARGO_EOF
[package]
name = "$name"
version = "1.0.0-rc1"
edition = "2021"

[features]
$feat = []

[lib]
path = "src/lib.rs"
CARGO_EOF
        else
            cat > "$p/Cargo.toml" << CARGO_EOF
[package]
name = "$name"
version = "1.0.0-rc1"
edition = "2021"

[lib]
path = "src/lib.rs"
CARGO_EOF
        fi
    done
    log "骨架已创建（~KB 级，编译主臂无需拉取 dora 源码）"
}

# ──────────────────────────────────────────────
# 2. 前置依赖（Rust / uv / dora 源码）
# ──────────────────────────────────────────────
check_deps() {
    log "检查前置依赖..."

    local CARGO_BIN="$HOME/.cargo/bin/cargo"
    local UV_BIN="$HOME/.local/bin/uv"
    local DORA_BIN="$HOME/.local/bin/dora"

    if [ ! -x "$CARGO_BIN" ]; then
        warn "Rust 未安装，正在自动安装..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        log "Rust 已安装"
    fi

    if [ ! -x "$UV_BIN" ]; then
        warn "uv 未安装，正在自动安装..."
        curl -LsSf https://astral.sh/uv/install.sh | sh
        log "uv 已安装"
    fi

    # submodule 管理：从臂拉取全部，主臂仅创建 dora 骨架
    if [ "$NEED_DORA" = true ]; then
        # 从臂模式 — 初始化所有 thirdparty submodule
        for sub in thirdparty/dora thirdparty/lerobot; do
            if [ ! -d "$PROJECT/$sub/.git" ]; then
                warn "$sub 子模块未初始化，正在拉取..."
                rm -rf "$PROJECT/$sub"
                cd "$PROJECT" && git submodule update --init -- "$sub" || err "拉取 $sub 子模块失败"
                cd - > /dev/null
            fi
        done
    else
        # 主臂模式 — dora 骨架即可
        if [ ! -f "$PROJECT/thirdparty/dora/Cargo.toml" ] && [ ! -d "$PROJECT/thirdparty/dora/.git" ]; then
            create_dora_stubs
        fi
    fi

    if [ "$NEED_DORA" = true ] && [ ! -x "$DORA_BIN" ]; then
        warn "dora CLI 未安装，从源码编译..."
        cd "$PROJECT"
        "$CARGO_BIN" build -p dora-cli --release \
            --manifest-path "$PROJECT/thirdparty/dora/Cargo.toml" || err "dora 编译失败"
        mkdir -p "$HOME/.local/bin"
        cp "$PROJECT/thirdparty/dora/target/release/dora" "$HOME/.local/bin/dora"
        export PATH="$HOME/.local/bin:$PATH"
        log "dora CLI 已安装到 $HOME/.local/bin/dora"
    fi
    log "依赖检查通过"
}

# ──────────────────────────────────────────────
# 3. Python 3.12 venv（DORA 编译需要 pyo3 ≥3.11）
# ──────────────────────────────────────────────
ensure_python312() {
    local UV_BIN="$HOME/.local/bin/uv"
    local VENV="$PROJECT/training/.venv"
    if [ -d "$VENV" ]; then
        log "Python venv 已存在，跳过创建"
    else
        log "预创建 Python 3.12 虚拟环境（供 DORA 编译使用）..."
        "$UV_BIN" venv "$VENV" --python 3.12 || err "创建 venv 失败"
        log "Python 3.12 venv 已创建 → $VENV"
    fi
    export PYO3_PYTHON="$VENV/bin/python"
}

# ──────────────────────────────────────────────
# 4. 扫描 USB 设备
# ──────────────────────────────────────────────
scan_usb_devices() {
    log "扫描 USB 串口设备..."
    echo ""
    echo "  序号  串口路径               VID:PID      Serial           描述"
    echo "  ────  ──────────────────────  ───────────  ────────────────  ──────"
    local i=0
    DEVICES=()
    for dev in /dev/ttyUSB* /dev/ttyACM*; do
        [ -e "$dev" ] || continue
        i=$((i + 1))
        local syspath=$(udevadm info -q path -n "$dev" 2>/dev/null || echo "")
        local vid=""; local pid=""; local serial=""; local desc=""
        if [ -n "$syspath" ]; then
            while [ "$syspath" != "/" ] && [ -z "$vid" ]; do
                vid=$(cat "/sys$syspath/idVendor" 2>/dev/null || echo "")
                pid=$(cat "/sys$syspath/idProduct" 2>/dev/null || echo "")
                serial=$(cat "/sys$syspath/serial" 2>/dev/null || echo "")
                desc=$(cat "/sys$syspath/product" 2>/dev/null || echo "")
                syspath=$(dirname "$syspath")
            done
        fi
        vid="${vid:-????}"; pid="${pid:-????}"
        serial="${serial:-(无)}"; desc="${desc:-未知设备}"
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

select_single_arm() {
    local label="$1"
    echo ""; log "请选择 $label"
    while true; do
        read -rp "  序号 [1-${#DEVICES[@]}]: " idx
        if [[ "$idx" =~ ^[0-9]+$ ]] && [ "$idx" -ge 1 ] && [ "$idx" -le "${#DEVICES[@]}" ]; then break; fi
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
    echo ""; log "配置确认："
    info "$label: $DEV  (VID=0x$VID PID=0x$PID Serial=$SERIAL)"
    read -rp "  确认? [Y/n]: " confirm
    if [ "$confirm" = "n" ] || [ "$confirm" = "N" ]; then
        select_single_arm "$label"; return
    fi
}

select_arms() {
    echo ""; log "请选择主臂和从臂"
    while true; do
        read -rp "  主臂序号 [1-${#DEVICES[@]}]: " leader_idx
        if [[ "$leader_idx" =~ ^[0-9]+$ ]] && [ "$leader_idx" -ge 1 ] && [ "$leader_idx" -le "${#DEVICES[@]}" ]; then break; fi
        warn "无效序号，请重试"
    done
    while true; do
        read -rp "  从臂序号 [1-${#DEVICES[@]}]: " follower_idx
        if [[ "$follower_idx" =~ ^[0-9]+$ ]] && [ "$follower_idx" -ge 1 ] && [ "$follower_idx" -le "${#DEVICES[@]}" ]; then break; fi
        warn "无效序号，请重试"
    done
    IFS='|' read -r LEADER_DEV LEADER_VID LEADER_PID LEADER_SERIAL LEADER_DESC <<< "${DEVICES[$((leader_idx - 1))]}"
    IFS='|' read -r FOLLOWER_DEV FOLLOWER_VID FOLLOWER_PID FOLLOWER_SERIAL FOLLOWER_DESC <<< "${DEVICES[$((follower_idx - 1))]}"
    LEADER_VID="${LEADER_VID#0x}"; LEADER_PID="${LEADER_PID#0x}"
    FOLLOWER_VID="${FOLLOWER_VID#0x}"; FOLLOWER_PID="${FOLLOWER_PID#0x}"
    echo ""; log "配置确认："
    info "主臂: $LEADER_DEV  (VID=0x$LEADER_VID PID=0x$LEADER_PID Serial=$LEADER_SERIAL)"
    info "从臂: $FOLLOWER_DEV  (VID=0x$FOLLOWER_VID PID=0x$FOLLOWER_PID Serial=$FOLLOWER_SERIAL)"
    read -rp "  确认? [Y/n]: " confirm
    if [ "$confirm" = "n" ] || [ "$confirm" = "N" ]; then select_arms; return; fi
}

# ──────────────────────────────────────────────
# 5. 生成配置文件
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
# 6. Python 虚拟环境（仅从臂）
# ──────────────────────────────────────────────
install_venv() {
    log "安装 Python 虚拟环境..."
    local CARGO_BIN="$HOME/.cargo/bin/cargo"
    local UV_BIN="$HOME/.local/bin/uv"
    local VENV="$PROJECT/training/.venv"
    local VENV_PYTHON="$VENV/bin/python"

    if [ ! -d "$VENV" ]; then
        "$UV_BIN" venv "$VENV" --python 3.12 || err "创建 venv 失败"
    fi

    log "安装 Python 依赖 (numpy, opencv, pyarrow)..."
    "$UV_BIN" pip install --python "$VENV_PYTHON" \
        numpy opencv-python pyarrow pyyaml || err "pip install 失败"

    log "安装 lerobot (含 torch, ~2GB)..."
    "$UV_BIN" pip install --python "$VENV_PYTHON" \
        lerobot || err "lerobot 安装失败"

    log "构建 DORA Python 包..."
    if ! command -v maturin >/dev/null; then
        "$CARGO_BIN" install maturin || warn "maturin 安装失败"
    fi
    local MATURIN="$HOME/.cargo/bin/maturin"
    if [ -x "$MATURIN" ]; then
        PYO3_PYTHON="$VENV_PYTHON" "$MATURIN" build \
            -m "$PROJECT/thirdparty/dora/apis/python/node/Cargo.toml" --release || warn "DORA wheel 构建失败"
        local wheel=$(ls "$PROJECT/thirdparty/dora/target/wheels/dora_rs-"*.whl 2>/dev/null | head -1)
        if [ -n "$wheel" ]; then
            "$UV_BIN" pip install --python "$VENV_PYTHON" "$wheel" || warn "wheel 安装失败"
            log "DORA Python 包已安装"
        fi
    fi
    log "Python 环境安装完成"
}

# ──────────────────────────────────────────────
# 7. 编译项目
# ──────────────────────────────────────────────
build_project() {
    log "编译项目（首次约 10-20 分钟）..."
    local CARGO="$HOME/.cargo/bin/cargo"
    cd "$PROJECT"

    # 先停服务，避免 cp 报 Text file busy
    systemctl --user stop dorarobot-follower 2>/dev/null || true
    systemctl --user stop dorarobot-leader 2>/dev/null || true

    if [ "$NEED_DORA" = true ]; then
        "$CARGO" build --release || err "编译失败（从臂模式）"
        "$CARGO" build -p tr-capture --release || err "tr-capture 编译失败"
    else
        "$CARGO" build -p tr-daemon --release || err "编译失败（主臂模式）"
    fi

    log "部署二进制到 bin/..."
    mkdir -p "$PROJECT/bin"
    if [ "$NEED_FOLLOWER" = true ]; then
        cp "$PROJECT/target/release/follower" "$PROJECT/bin/follower"
    fi
    if [ "$NEED_LEADER" = true ]; then
        cp "$PROJECT/target/release/leader" "$PROJECT/bin/leader"
    fi
    if [ "$NEED_DORA" = true ]; then
        cp "$PROJECT/target/release/tr-capture" "$PROJECT/bin/tr-capture"
    fi
    log "编译完成 → $PROJECT/bin/"
}

# ──────────────────────────────────────────────
# 8. 注册 systemd user 服务
# ──────────────────────────────────────────────
register_services() {
    log "注册 systemd user 服务..."

    # 允许用户服务在未登录时运行（需要 sudo）
    sudo loginctl enable-linger "$USER" || warn "enable-linger 失败，开机自动启动可能不可用"

    local UNIT_DIR="$HOME/.config/systemd/user"
    mkdir -p "$UNIT_DIR"

    # 清理旧的 root 级别服务
    for svc in dorarobot-follower dorarobot-leader; do
        if [ -f "/etc/systemd/system/${svc}.service" ]; then
            sudo systemctl stop "$svc" 2>/dev/null || true
            sudo systemctl disable "$svc" 2>/dev/null || true
            sudo rm -f "/etc/systemd/system/${svc}.service"
        fi
    done
    sudo systemctl daemon-reload 2>/dev/null || true

    mkdir -p "$PROJECT/logs"
    local venv="$PROJECT/training/.venv"

    if [ "$NEED_FOLLOWER" = true ]; then
        cat > "$UNIT_DIR/dorarobot-follower.service" << EOF
[Unit]
Description=DoraRobot Follower Daemon
After=default.target

[Service]
Type=simple
WorkingDirectory=$PROJECT
ExecStart=$PROJECT/bin/follower --config $PROJECT/config/follower.toml
Restart=always
RestartSec=5
Environment="PATH=${venv}/bin:${HOME}/.local/bin:/usr/bin:/bin"
Environment="VIRTUAL_ENV=${venv}"

[Install]
WantedBy=default.target
EOF
    fi

    if [ "$NEED_LEADER" = true ]; then
        cat > "$UNIT_DIR/dorarobot-leader.service" << EOF
[Unit]
Description=DoraRobot Leader Daemon + Web Console
After=default.target

[Service]
Type=simple
WorkingDirectory=$PROJECT
ExecStart=$PROJECT/bin/leader --config $PROJECT/config/leader.toml
Restart=always
RestartSec=5
Environment="PATH=${venv}/bin:${HOME}/.local/bin:/usr/bin:/bin"
Environment="VIRTUAL_ENV=${venv}"

[Install]
WantedBy=default.target
EOF
    fi

    systemctl --user daemon-reload
    if [ "$NEED_FOLLOWER" = true ]; then
        systemctl --user enable dorarobot-follower
    fi
    if [ "$NEED_LEADER" = true ]; then
        systemctl --user enable dorarobot-leader
    fi
    log "systemd user 服务已注册并设为开机自启"
}

# ──────────────────────────────────────────────
# 9. 启动服务
# ──────────────────────────────────────────────
start_services() {
    log "启动服务..."

    if [ "$NEED_FOLLOWER" = true ]; then
        systemctl --user start dorarobot-follower
    fi
    if [ "$NEED_LEADER" = true ]; then
        systemctl --user start dorarobot-leader
    fi
    sleep 3
    echo ""

    if [ "$NEED_FOLLOWER" = true ]; then
        if systemctl --user is-active --quiet dorarobot-follower; then
            log "从臂服务: ${GREEN}运行中${NC}"
        else
            warn "从臂服务: 启动失败 → journalctl --user -u dorarobot-follower -n 20"
        fi
    fi
    if [ "$NEED_LEADER" = true ]; then
        if systemctl --user is-active --quiet dorarobot-leader; then
            log "主臂服务: ${GREEN}运行中${NC}"
        else
            warn "主臂服务: 启动失败 → journalctl --user -u dorarobot-leader -n 20"
        fi
    fi
}

# ──────────────────────────────────────────────
# 入口
# ──────────────────────────────────────────────
main() {
    if [ "$(id -u)" -eq 0 ]; then
        err "请以普通用户身份运行此脚本（不要用 sudo）"
    fi

    echo ""
    echo "  ╔══════════════════════════════════════════╗"
    echo "  ║   DoraRobot 一键部署脚本 (Linux)         ║"
    echo "  ╚══════════════════════════════════════════╝"
    echo ""

    # ── 第一步：交互式问答 ──
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

    # ── 第二步：全自动安装 ──
    install_system_deps                    # ① 第一步：提权装系统依赖
    if [ "$NEED_DORA" = true ]; then
        ensure_python312
    fi
    check_deps
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
    echo "  ║  从臂日志: journalctl --user -u dorarobot-follower -f ║"
    echo "  ║  停止从臂: systemctl --user stop dorarobot-follower   ║"
    fi
    if [ "$NEED_LEADER" = true ]; then
    echo "  ║  主臂日志: journalctl --user -u dorarobot-leader -f  ║"
    echo "  ║  停止主臂: systemctl --user stop dorarobot-leader    ║"
    fi
    echo "  ╚══════════════════════════════════════════╝"
}

main
