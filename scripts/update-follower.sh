#!/usr/bin/env bash
# ──────────────────────────────────────────────
# 从臂守护进程更新脚本（git pull → build → deploy → restart）
# ──────────────────────────────────────────────
set -eo pipefail

PROJECT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT"

log() { echo "  ❯ $*"; }

# 1. 拉取最新代码
log "git pull..."
git pull

# 2. 编译（仅 tr-daemon，约 30s-2min）
log "cargo build --release -p tr-daemon..."
cargo build --release -p tr-daemon

# 3. 先停止服务（避免 Text file busy），再部署到 bin/
case "$(uname -s)" in
    Linux)
        systemctl --user stop dorarobot-follower 2>/dev/null || true
        killall -9 follower 2>/dev/null || true
        ;;
    Darwin)
        # 优先停 user LaunchAgent，再试旧系统级 LaunchDaemon
        launchctl unload "$HOME/Library/LaunchAgents/com.dorarobot.follower.plist" 2>/dev/null || true
        launchctl unload /Library/LaunchDaemons/com.dorarobot.follower.plist 2>/dev/null || true
        killall -9 follower 2>/dev/null || true
        ;;
esac

mkdir -p "$PROJECT/bin"
cp "$PROJECT/target/release/follower" "$PROJECT/bin/follower"

# 4. 重启服务
case "$(uname -s)" in
    Linux)
        log "重启 systemd 用户服务..."
        systemctl --user daemon-reload
        systemctl --user restart dorarobot-follower
        systemctl --user status dorarobot-follower --no-pager -l
        ;;
    Darwin)
        log "重启 launchd 服务..."
        if [ -f "$HOME/Library/LaunchAgents/com.dorarobot.follower.plist" ]; then
            launchctl unload "$HOME/Library/LaunchAgents/com.dorarobot.follower.plist" 2>/dev/null || true
            launchctl load "$HOME/Library/LaunchAgents/com.dorarobot.follower.plist"
        elif [ -f /Library/LaunchDaemons/com.dorarobot.follower.plist ]; then
            sudo launchctl unload /Library/LaunchDaemons/com.dorarobot.follower.plist 2>/dev/null || true
            sudo launchctl load /Library/LaunchDaemons/com.dorarobot.follower.plist
        fi
        launchctl list com.dorarobot.follower
        ;;
    *)
        echo "未知系统: $(uname -s)"
        echo "二进制已更新到 bin/follower，请手动重启服务"
        ;;
esac

log "✅ 更新完成"
