#!/usr/bin/env bash
# uninstall.sh — DoraRobot 卸载脚本（跨平台）
#
# 使用：
#   bash scripts/uninstall.sh          # 标准卸载（停止服务 + 删除 bin/config/logs）
#   bash scripts/uninstall.sh --all    # 完全卸载（含构建产物、venv、dora/lerobot）
#   bash scripts/uninstall.sh --nuke   # 核爆级（含 DORA CLI、恢复 linger）
#
# 用法执行过程中会列出将要删除的内容并确认。

set -eo pipefail
PROJECT="$(cd "$(dirname "$0")/.." && pwd)"
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
OS="$(uname -s)"

log()  { echo -e "${GREEN}  ❯${NC} $*"; }
warn() { echo -e "${YELLOW}[warn]${NC} $*"; }
info() { echo -e "${CYAN}        $*${NC}"; }

# ──────────────────────────────────────────────
# 确定卸载级别
# ──────────────────────────────────────────────
LEVEL="standard"
case "${1:-}" in
    --all)   LEVEL="full" ;;
    --nuke)  LEVEL="nuke" ;;
    --soft)  LEVEL="soft" ;;
    --help|-h)
        echo "DoraRobot 卸载脚本"
        echo ""
        echo "  bash scripts/uninstall.sh           标准卸载（停止服务 + bin/config/logs）"
        echo "  bash scripts/uninstall.sh --all     完全卸载（+ target/venv/dora/lerobot）"
        echo "  bash scripts/uninstall.sh --nuke    彻底清洁（+ DORA CLI + 取消 linger）"
        echo "  bash scripts/uninstall.sh --soft    仅停止 + 注销服务"
        exit 0
        ;;
esac

echo ""
echo "  ┌──────────────────────────────────────┐"
echo "  │  DoraRobot 卸载 ($LEVEL)              │"
echo "  └──────────────────────────────────────┘"

# ──────────────────────────────────────────────
# 第 1 层：停止 + 注销所有服务
# ──────────────────────────────────────────────
stop_and_unregister_services() {
    log "停止并注销服务..."
    echo ""

    case "$OS" in
        Linux)
            # 停止 user 服务
            for svc in dorarobot-follower dorarobot-leader; do
                if systemctl --user is-active --quiet "$svc" 2>/dev/null; then
                    log "停止 systemd 用户服务: $svc"
                    systemctl --user stop "$svc" 2>/dev/null || true
                fi
                if systemctl --user is-enabled --quiet "$svc" 2>/dev/null; then
                    log "取消 systemd 用户服务: $svc"
                    systemctl --user disable "$svc" 2>/dev/null || true
                fi
            done

            # 删除 service 文件
            for svc in dorarobot-follower dorarobot-leader; do
                local f="$HOME/.config/systemd/user/${svc}.service"
                if [ -f "$f" ]; then
                    log "删除: $f"
                    rm -f "$f"
                    info "已移除"
                fi
            done
            systemctl --user daemon-reload 2>/dev/null || true

            # 清理旧 root 级别服务
            for svc in dorarobot-follower dorarobot-leader; do
                local f="/etc/systemd/system/${svc}.service"
                if [ -f "$f" ]; then
                    warn "发现旧 root 级服务: $f"
                    read -rp "  删除? [Y/n]: " c
                    if [ "$c" != "n" ] && [ "$c" != "N" ]; then
                        sudo systemctl stop "$svc" 2>/dev/null || true
                        sudo systemctl disable "$svc" 2>/dev/null || true
                        sudo rm -f "$f"
                        sudo systemctl daemon-reload
                        info "已移除"
                    fi
                fi
            done

            # nuke: 取消 linger
            if [ "$LEVEL" = "nuke" ]; then
                if loginctl show-user "$USER" 2>/dev/null | grep -q 'Linger=yes'; then
                    warn "loginctl linger 已启用"
                    read -rp "  取消 linger? [Y/n]: " c
                    if [ "$c" != "n" ] && [ "$c" != "N" ]; then
                        sudo loginctl disable-linger "$USER"
                        log "linger 已取消"
                    fi
                fi
            fi
            ;;

        Darwin)
            # 停掉 user LaunchAgent
            for svc in com.dorarobot.follower com.dorarobot.leader; do
                local f="$HOME/Library/LaunchAgents/${svc}.plist"
                if [ -f "$f" ]; then
                    log "停止: $svc"
                    launchctl stop "$svc" 2>/dev/null || true
                    launchctl unload "$f" 2>/dev/null || true
                    rm -f "$f"
                    info "已移除"
                fi
            done

            # 清理旧 system 级 LaunchDaemon
            for svc in com.dorarobot.follower com.dorarobot.leader; do
                local f="/Library/LaunchDaemons/${svc}.plist"
                if [ -f "$f" ]; then
                    warn "发现旧系统级服务: $f"
                    read -rp "  删除? [Y/n]: " c
                    if [ "$c" != "n" ] && [ "$c" != "N" ]; then
                        sudo launchctl stop "$svc" 2>/dev/null || true
                        sudo launchctl unload "$f" 2>/dev/null || true
                        sudo rm -f "$f"
                        info "已移除"
                    fi
                fi
            done
            ;;

        MINGW*|MSYS*|CYGWIN*)
            # Windows (Git Bash)
            for tn in "DoraRobot Follower" "DoraRobot Leader"; do
                if powershell -Command "Get-ScheduledTask -TaskName '$tn' -ErrorAction SilentlyContinue" 2>/dev/null | grep -q "$tn"; then
                    log "删除计划任务: $tn"
                    powershell -Command "Stop-ScheduledTask -TaskName '$tn' -ErrorAction SilentlyContinue" 2>/dev/null || true
                    powershell -Command "Unregister-ScheduledTask -TaskName '$tn' -Confirm:\`\$false -ErrorAction SilentlyContinue" 2>/dev/null || true
                    info "已移除"
                fi
            done
            ;;
        *)
            warn "未知系统: $OS，跳过服务清理"
            ;;
    esac

    echo ""
}

# ──────────────────────────────────────────────
# 第 2 层：删除项目运行时文件
# ──────────────────────────────────────────────
clean_runtime_files() {
    if [ "$LEVEL" = "soft" ]; then
        log "软卸载：保留 bin/config/logs"
        return
    fi

    log "清理运行时文件..."
    echo ""

    local dirs=()
    [ -d "$PROJECT/bin" ]    && dirs+=("$PROJECT/bin    ($(du -sh "$PROJECT/bin" 2>/dev/null | cut -f1))")
    [ -d "$PROJECT/config" ] && dirs+=("$PROJECT/config")
    [ -d "$PROJECT/logs" ]   && dirs+=("$PROJECT/logs   ($(du -sh "$PROJECT/logs" 2>/dev/null | cut -f1))")

    local win_dirs=()
    if [ -d "$PROJECT/scripts" ]; then
        for bat in "$PROJECT/scripts/start-follower.bat" "$PROJECT/scripts/start-leader.bat"; do
            [ -f "$bat" ] && win_dirs+=("$bat")
        done
    fi

    if [ ${#dirs[@]} -eq 0 ] && [ ${#win_dirs[@]} -eq 0 ]; then
        log "没有运行时文件需要清理"
        return
    fi

    echo "  将删除:"
    for d in "${dirs[@]}"; do info "  $d"; done
    for d in "${win_dirs[@]}"; do info "  $d"; done
    echo ""

    for d in "${dirs[@]}"; do
        d="${d%% *}"  # remove size info
        rm -rf "$d"
    done
    for d in "${win_dirs[@]}"; do
        rm -f "$d"
    done
    log "运行时文件已清理"
    echo ""
}

# ──────────────────────────────────────────────
# 第 3 层：删除构建产物（可选，非常占空间）
# ──────────────────────────────────────────────
clean_build_artifacts() {
    if [ "$LEVEL" != "full" ] && [ "$LEVEL" != "nuke" ]; then
        log "跳过构建产物（$LEVEL 模式。使用 --all 或 --nuke 可删除）"
        return
    fi

    echo ""
    log "清理构建产物..."
    echo ""

    local dirs=()
    [ -d "$PROJECT/target" ]          && dirs+=("$PROJECT/target          ($(du -sh "$PROJECT/target" 2>/dev/null | cut -f1))")
    [ -d "$PROJECT/training/.venv" ]  && dirs+=("$PROJECT/training/.venv ($(du -sh "$PROJECT/training/.venv" 2>/dev/null | cut -f1))")
    [ -d "$PROJECT/dora" ]            && dirs+=("$PROJECT/dora            ($(du -sh "$PROJECT/dora" 2>/dev/null | cut -f1))")
    [ -d "$PROJECT/lerobot" ]         && dirs+=("$PROJECT/lerobot         ($(du -sh "$PROJECT/lerobot" 2>/dev/null | cut -f1))")

    if [ ${#dirs[@]} -eq 0 ]; then
        log "没有构建产物需要清理"
        return
    fi

    echo "  将删除:"
    for d in "${dirs[@]}"; do info "  $d"; done
    echo ""
    read -rp "  确认删除这些目录? [Y/n]: " c
    if [ "$c" = "n" ] || [ "$c" = "N" ]; then
        log "跳过构建产物清理"
        return
    fi

    for d in "${dirs[@]}"; do
        d="${d%% *}"
        log "删除: $d"
        rm -rf "$d"
    done
    log "构建产物已清理"
    echo ""

    # 统计释放的空间
    log "释放的磁盘空间：$RELEASED"
}

# ──────────────────────────────────────────────
# 第 4 层：清理工具链（nuke 级别）
# ──────────────────────────────────────────────
clean_tools() {
    if [ "$LEVEL" != "nuke" ]; then
        log "跳过工具链清理（使用 --nuke 可删除 DORA CLI）"
        return
    fi

    echo ""
    log "清理项目工具链..."
    echo ""

    # DORA CLI — 仅本项目使用
    local dora_cli="$HOME/.local/bin/dora"
    if [ -f "$dora_cli" ]; then
        read -rp "  删除 DORA CLI ($dora_cli)? [Y/n]: " c
        if [ "$c" != "n" ] && [ "$c" != "N" ]; then
            rm -f "$dora_cli"
            log "DORA CLI 已删除"
        fi
    fi

    # maturin — 其他项目可能也会用
    local maturin="$HOME/.cargo/bin/maturin"
    if [ -f "$maturin" ]; then
        warn "发现 maturin ($maturin)"
        warn "  maturin 是通用工具，其他 Python+Rust 项目可能依赖它"
        read -rp "  仍然删除? [y/N]: " c
        if [ "$c" = "y" ] || [ "$c" = "Y" ]; then
            rm -f "$maturin"
            log "maturin 已删除"
        fi
    fi
}

# ──────────────────────────────────────────────
# 第 5 层：数据集（默认不删，除非 --nuke 确认）
# ──────────────────────────────────────────────
clean_datasets() {
    if [ "$LEVEL" != "nuke" ]; then
        return
    fi
    if [ ! -d "$PROJECT/datasets" ]; then
        return
    fi

    echo ""
    warn "发现 datasets 目录: $PROJECT/datasets ($(du -sh "$PROJECT/datasets" 2>/dev/null | cut -f1))"
    warn "  这是录制数据，删除后无法恢复"
    read -rp "  删除? [y/N]: " c
    if [ "$c" = "y" ] || [ "$c" = "Y" ]; then
        rm -rf "$PROJECT/datasets"
        log "datasets 已删除"
    fi
}

# ──────────────────────────────────────────────
# 入口
# ──────────────────────────────────────────────
main() {
    echo ""
    echo "  卸载级别: $LEVEL"
    echo "  项目目录: $PROJECT"
    echo ""
    echo "  将执行:"
    case "$LEVEL" in
        soft)
            info "  ✓ 停止 + 注销服务" ;;
        standard)
            info "  ✓ 停止 + 注销服务"
            info "  ✓ 删除 bin/ config/ logs/" ;;
        full)
            info "  ✓ 停止 + 注销服务"
            info "  ✓ 删除 bin/ config/ logs/"
            info "  ✓ 删除 target/ .venv/ dora/ lerobot/" ;;
        nuke)
            info "  ✓ 停止 + 注销服务"
            info "  ✓ 删除 bin/ config/ logs/"
            info "  ✓ 删除 target/ .venv/ dora/ lerobot/"
            info "  ✓ 删除 DORA CLI / 取消 linger"
            info "  ✓ 询问是否删除 datasets" ;;
    esac
    echo ""
    echo "  ⚠  Rust / uv / 系统包 不会被删除"
    echo ""
    read -rp "  开始卸载? [Y/n]: " do_start
    if [ "$do_start" = "n" ] || [ "$do_start" = "N" ]; then
        log "已取消"
        exit 0
    fi

    echo ""
    stop_and_unregister_services
    clean_runtime_files
    clean_build_artifacts
    clean_datasets
    clean_tools

    echo ""
    echo "  ┌──────────────────────────────────────┐"
    echo "  │  ✅ 卸载完成                         │"
    echo "  └──────────────────────────────────────┘"
    echo ""
}

main
