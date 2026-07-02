//! Web console — axum HTTP server with SSE status + command API.
//!
//! Serves a self-contained HTML page (no build step) that connects to the
//! SSE endpoint for real-time status and POSTs commands.

use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Json, Router,
};
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

/// Shared state between HTTP handlers and the daemon main loop.
pub struct WebState {
    /// Latest follower status (JSON string) pushed via broadcast.
    pub status_tx: broadcast::Sender<String>,
    /// Channel to send commands TO the daemon main loop.
    pub cmd_tx: tokio::sync::mpsc::UnboundedSender<String>,
}

/// Command request body.
#[derive(serde::Deserialize)]
struct CommandReq {
    cmd: String,
}

/// Build the axum router.
pub fn router(state: Arc<WebState>) -> Router {
    Router::new()
        .route("/", get(index_html))
        .route("/api/status", get(sse_status))
        .route("/api/command", post(handle_command))
        .with_state(state)
}

/// Serve the self-contained HTML page.
async fn index_html() -> axum::response::Html<&'static str> {
    axum::response::Html(HTML_PAGE)
}

/// SSE endpoint — streams the latest follower status (1 Hz).
async fn sse_status(
    State(state): State<Arc<WebState>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.status_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|msg| {
        match msg {
            Ok(json) => Some(Ok(Event::default().data(json))),
            Err(_) => None,
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// POST /api/command — sends a control command to the daemon loop.
async fn handle_command(
    State(state): State<Arc<WebState>>,
    Json(req): Json<CommandReq>,
) -> &'static str {
    let _ = state.cmd_tx.send(req.cmd);
    "ok"
}

/// Self-contained HTML + JS page — no build step needed.
const HTML_PAGE: &str = r#"<!DOCTYPE html>
<html lang="zh">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>SO-101 Teleop Console</title>
<style>
*{box-sizing:border-box;margin:0;padding:0}
body{font-family:system-ui;background:#0f172a;color:#e2e8f0;display:flex;justify-content:center;align-items:center;min-height:100vh}
.c{background:#1e293b;border-radius:12px;padding:24px;width:420px;box-shadow:0 4px 24px rgba(0,0,0,.4)}
h1{font-size:20px;margin-bottom:4px}
.st{font-size:13px;color:#94a3b8;margin-bottom:16px}
.status-bar{display:flex;gap:12px;margin-bottom:20px;flex-wrap:wrap}
.status-item{background:#334155;border-radius:8px;padding:8px 14px;font-size:13px}
.status-item .l{color:#94a3b8}
.status-item .v{font-weight:600}
.btn-row{display:flex;gap:8px;margin-bottom:8px;flex-wrap:wrap}
.btn{border:none;border-radius:8px;padding:10px 16px;font-size:14px;font-weight:600;cursor:pointer;transition:.15s;white-space:nowrap}
.btn:active{transform:scale(.97)}
.btn-on{background:#22c55e;color:#fff}
.btn-on:hover{background:#16a34a}
.btn-off{background:#ef4444;color:#fff}
.btn-off:hover{background:#dc2626}
.btn-rec{background:#3b82f6;color:#fff}
.btn-rec:hover{background:#2563eb}
.btn-save{background:#22c55e;color:#fff}
.btn-save:hover{background:#16a34a}
.btn-discard{background:#f59e0b;color:#fff}
.btn-discard:hover{background:#d97706}
.btn-rerecord{background:#8b5cf6;color:#fff}
.btn-rerecord:hover{background:#7c3aed}
.btn-stop{background:#6b7280;color:#fff}
.btn-stop:hover{background:#4b5563}
.btn:disabled{opacity:.35;cursor:not-allowed;transform:none}
.err{color:#f87171;font-size:12px;margin-top:12px}
.online{display:inline-block;width:8px;height:8px;border-radius:50%;background:#22c55e;margin-right:6px}
.offline{background:#ef4444}
</style>
</head>
<body>
<div class="c">
  <h1>SO-101 Teleop Console</h1>
  <div class="st"><span id="led" class="online"></span> <span id="arm_id">arm_1</span></div>

  <div class="status-bar">
    <div class="status-item"><span class="l">State</span><br><span class="v" id="state">--</span></div>
    <div class="status-item"><span class="l">Torque</span><br><span class="v" id="torque">--</span></div>
    <div class="status-item"><span class="l">Episode</span><br><span class="v" id="ep">--</span></div>
    <div class="status-item"><span class="l">Frames</span><br><span class="v" id="frames">0</span></div>
    <div class="status-item"><span class="l">FPS</span><br><span class="v" id="fps">0</span></div>
  </div>

  <div class="btn-row">
    <button class="btn btn-on" id="btn-torque-on" onclick="cmd('TorqueOn')">⚡ 上力</button>
    <button class="btn btn-off" id="btn-torque-off" onclick="cmd('TorqueOff')" disabled>⏻ 卸力</button>
  </div>

  <div class="btn-row" style="border-top:1px solid #334155;padding-top:8px;margin-top:4px">
    <button class="btn btn-rec" id="btn-start" onclick="cmd('StartRecord')" disabled>▶ 开始采集</button>
  </div>
  <div class="btn-row">
    <button class="btn btn-save" id="btn-success" onclick="cmd('EndRecord')" disabled>✅ 成功保存</button>
    <button class="btn btn-discard" id="btn-fail" onclick="cmd('ReRecord')" disabled>❌ 丢弃</button>
    <button class="btn btn-rerecord" id="btn-rerecord" onclick="cmd('ReRecord')" disabled>🔄 重录</button>
  </div>
  <div class="btn-row">
    <button class="btn btn-stop" id="btn-stop" onclick="cmd('Stop')" disabled>⏹ 停止采集</button>
  </div>

  <div class="err" id="err"></div>
</div>

<script>
let currentState = 'IDLE';
const evt = new EventSource('/api/status');

evt.onmessage = function(e) {
  try {
    const s = JSON.parse(e.data);
    document.getElementById('state').textContent = s.state;
    document.getElementById('torque').textContent = s.torque_on ? 'ON' : 'OFF';
    document.getElementById('ep').textContent = s.recording ? '● REC' : '--';
    document.getElementById('frames').textContent = s.frame_count || 0;
    document.getElementById('fps').textContent = (s.fps || 0).toFixed(0);
    if (s.error) document.getElementById('err').textContent = s.error;
    else document.getElementById('err').textContent = '';

    const led = document.getElementById('led');
    if (s.state === 'OFFLINE') { led.className = 'offline'; }
    else { led.className = 'online'; }

    currentState = s.state;
    updateButtons(s.state, s.torque_on, s.recording);
  } catch(_) {}
};

function updateButtons(state, torque_on, recording) {
  document.getElementById('btn-torque-on').disabled  = (state !== 'IDLE');
  document.getElementById('btn-torque-off').disabled = (state === 'IDLE');
  document.getElementById('btn-start').disabled      = !(state === 'READY');
  document.getElementById('btn-success').disabled    = !(state === 'RECORDING');
  document.getElementById('btn-fail').disabled       = !(state === 'RECORDING');
  document.getElementById('btn-rerecord').disabled   = !(state === 'RECORDING');
  document.getElementById('btn-stop').disabled       = !(state === 'RECORDING');
}

async function cmd(c) {
  await fetch('/api/command', {
    method:'POST',
    headers:{'Content-Type':'application/json'},
    body: JSON.stringify({cmd:c})
  });
}
</script>
</body>
</html>
"#;
