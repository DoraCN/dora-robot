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
        ;;
    Darwin)
        launchctl unload /Library/LaunchDaemons/com.dorarobot.follower.plist 2>/dev/null || true
        ;;
esac

mkdir -p "$PROJECT/bin"
if cp "$PROJECT/target/release/follower" "$PROJECT/bin/follower" 2>/dev/null; then
    :  # 复制成功
else
    log "普通用户权限不足，尝试 sudo cp..."
    sudo cp "$PROJECT/target/release/follower" "$PROJECT/bin/follower"
    sudo chown "$(whoami)" "$PROJECT/bin/follower"
fi

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
        launchctl unload /Library/LaunchDaemons/com.dorarobot.follower.plist 2>/dev/null || true
        launchctl load /Library/LaunchDaemons/com.dorarobot.follower.plist
        launchctl list com.dorarobot.follower
        ;;
    *)
        echo "未知系统: $(uname -s)"
        echo "二进制已更新到 bin/follower，请手动重启服务"
        ;;
esac

log "✅ 更新完成"
