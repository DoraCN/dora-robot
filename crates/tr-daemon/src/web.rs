//! Web console — axum HTTP server with SSE status + command API.
//!
//! Serves a self-contained HTML page (no build step) that connects to the
//! SSE endpoint for real-time status and POSTs commands.

use axum::{
    Json, Router,
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
};
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

/// Shared state between HTTP handlers and the daemon main loop.
pub struct WebState {
    /// Latest follower status (JSON string) pushed via broadcast.
    pub status_tx: broadcast::Sender<String>,
    /// Channel to send commands TO the daemon main loop.
    pub cmd_tx: tokio::sync::mpsc::UnboundedSender<String>,
    /// Arm display info from config.
    pub arm_info: String,
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

/// Serve the self-contained HTML page (arm info injected from config).
async fn index_html(State(state): State<Arc<WebState>>) -> axum::response::Html<String> {
    let html = HTML_PAGE.replace("ARM_INFO_PLACEHOLDER", &state.arm_info);
    axum::response::Html(html)
}

/// SSE endpoint — streams the latest follower status (1 Hz).
async fn sse_status(
    State(state): State<Arc<WebState>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.status_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|msg| match msg {
        Ok(json) => Some(Ok(Event::default().data(json))),
        Err(_) => None,
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
<title>DOROBOT Console</title>
<style>
:root{
  --bg:#0f172a;--card:#1e293b;--text:#e2e8f0;--sub:#94a3b8;
  --stat:#334155;--border:#334155
}
body.light{
  --bg:#f1f5f9;--card:#fff;--text:#1e293b;--sub:#64748b;
  --stat:#e2e8f0;--border:#cbd5e1
}
*{box-sizing:border-box;margin:0;padding:0}
body{
  font-family:system-ui;background:var(--bg);color:var(--text);
  display:flex;justify-content:center;align-items:center;min-height:100vh;
  transition:background .3s,color .3s
}
.c{
  background:var(--card);border-radius:16px;padding:28px 24px;
  width:500px;max-width:96vw;box-shadow:0 4px 32px rgba(0,0,0,.3);
  transition:background .3s,transform .3s;position:relative
}
.c.r90{transform:rotate(90deg);transform-origin:center center}
.c.r180{transform:rotate(180deg)}
.c.r270{transform:rotate(270deg)}
.toolbar{
  display:flex;gap:6px;position:absolute;top:12px;right:16px;z-index:10
}
.tbtn{
  border:none;background:var(--stat);color:var(--text);
  border-radius:8px;width:36px;height:36px;font-size:18px;
  cursor:pointer;display:flex;align-items:center;justify-content:center;
  line-height:1;transition:.15s
}
.tbtn:hover{filter:brightness(1.2)}
.tbtn:active{transform:scale(.93)}
h1{font-size:20px;margin-bottom:2px;padding-right:110px}
.st{font-size:13px;color:var(--sub);margin-bottom:18px}
.status-bar{display:flex;gap:10px;margin-bottom:24px;flex-wrap:wrap}
.status-item{
  background:var(--stat);border-radius:10px;padding:10px 16px;
  font-size:14px;flex:1;min-width:70px;text-align:center
}
.status-item .l{color:var(--sub);font-size:12px}
.status-item .v{font-weight:700;font-size:18px;margin-top:2px;display:block}
.btn-row{display:flex;gap:10px;margin-bottom:10px;flex-wrap:wrap}
.btn{
  border:none;border-radius:12px;padding:16px 20px;font-size:16px;
  font-weight:700;cursor:pointer;transition:.15s;white-space:nowrap;
  flex:1;min-width:100px;min-height:56px;letter-spacing:.3px
}
.btn:active{transform:scale(.96)}
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
.btn:disabled{opacity:.3;cursor:not-allowed;transform:none;filter:none}
.err{color:#f87171;font-size:13px;margin-top:14px;min-height:18px}
.online{display:inline-block;width:9px;height:9px;border-radius:50%;background:#22c55e;margin-right:6px}
.offline{background:#ef4444}
.fs .c{border-radius:0;max-width:100vw;width:100vw;min-height:100vh}
</style>
</head>
<body>
<div class="c" id="main">
  <div class="toolbar">
    <button class="tbtn" id="btn-rotate" onclick="toggleRotate()" title="旋转">↻</button>
    <button class="tbtn" id="btn-theme" onclick="toggleTheme()" title="主题">☀</button>
    <button class="tbtn" id="btn-fs" onclick="toggleFs()" title="全屏">⛶</button>
  </div>

  <h1>DOROBOT 遥操作控制台</h1>
  <div class="st"><span id="led" class="online"></span> <span id="arm_id">ARM_INFO_PLACEHOLDER</span></div>

  <div class="status-bar">
    <div class="status-item"><span class="l">状态</span><span class="v" id="state">--</span></div>
    <div class="status-item"><span class="l">扭矩</span><span class="v" id="torque">--</span></div>
    <div class="status-item"><span class="l">录制</span><span class="v" id="rec">--</span></div>
    <div class="status-item"><span class="l">回合</span><span class="v" id="ep">--</span></div>
    <div class="status-item"><span class="l">帧数</span><span class="v" id="frames">0</span></div>
  </div>

  <div class="btn-row">
    <button class="btn btn-on" id="btn-torque-on" onclick="cmd('TorqueOn')">⚡ 使能</button>
    <button class="btn btn-off" id="btn-torque-off" onclick="cmd('TorqueOff')" disabled>⏻ 失能</button>
  </div>

  <div class="btn-row" style="border-top:1px solid var(--border);padding-top:12px;margin-top:6px">
    <button class="btn btn-rec" id="btn-start" onclick="cmd('StartRecord')" disabled>▶ 开始采集</button>
  </div>
  <div class="btn-row">
    <button class="btn btn-save" id="btn-success" onclick="cmd('EndRecord')" disabled>✅ 保存</button>
    <button class="btn btn-discard" id="btn-fail" onclick="cmd('ReRecord')" disabled>❌ 丢弃</button>
    <button class="btn btn-rerecord" id="btn-rerecord" onclick="cmd('ReRecord')" disabled>🔄 重录</button>
  </div>
  <div class="btn-row">
    <button class="btn btn-stop" id="btn-stop" onclick="cmd('Stop')" disabled>⏹ 停止</button>
  </div>

  <div class="err" id="err"></div>
</div>

<script>
let rotateIdx=0;
const ROTATES=['','r90','r180','r270'];
const c=document.getElementById('main');

function toggleRotate(){
  rotateIdx=(rotateIdx+1)%4;
  ROTATES.forEach(r=>c.classList.remove(r));
  const r=ROTATES[rotateIdx];
  if(r)c.classList.add(r);
  document.getElementById('btn-rotate').textContent=[ '↻','↺','↻','↺'][rotateIdx];
}

function toggleTheme(){
  const isLight=document.body.classList.toggle('light');
  document.getElementById('btn-theme').textContent=isLight?'☾':'☀';
  localStorage.theme=isLight?'light':'dark';
}

function toggleFs(){
  if(document.fullscreenElement){
    document.exitFullscreen();
    document.body.classList.remove('fs');
  }else{
    document.documentElement.requestFullscreen();
    document.body.classList.add('fs');
  }
}

(function(){
  if(localStorage.theme==='light'){document.body.classList.add('light');document.getElementById('btn-theme').textContent='☾';}
  document.addEventListener('fullscreenchange',()=>{
    if(!document.fullscreenElement)document.body.classList.remove('fs');
  });
})();

let currentState='IDLE';
const evt=new EventSource('/api/status');
evt.onmessage=function(e){
  try{
    const s=JSON.parse(e.data);
    document.getElementById('state').textContent=({IDLE:'待机',READY:'就绪',RECORDING:'采集中',OFFLINE:'离线'})[s.state]||s.state;
    document.getElementById('torque').textContent=s.torque_on?'已使能':'已失能';
    document.getElementById('rec').textContent=s.recording?'● 采集中':'--';
    document.getElementById('ep').textContent=s.episode||'--';
    document.getElementById('frames').textContent=s.frame_count||0;
    document.getElementById('err').textContent=s.error||'';
    document.getElementById('led').className=s.state==='OFFLINE'?'offline':'online';
    currentState=s.state;
    updateButtons(s.state);
  }catch(_){}
};

function updateButtons(state){
  document.getElementById('btn-torque-on').disabled=state!=='IDLE';
  document.getElementById('btn-torque-off').disabled=state==='IDLE';
  document.getElementById('btn-start').disabled=state!=='READY';
  document.getElementById('btn-success').disabled=state!=='RECORDING';
  document.getElementById('btn-fail').disabled=state!=='RECORDING';
  document.getElementById('btn-rerecord').disabled=state!=='RECORDING';
  document.getElementById('btn-stop').disabled=state!=='RECORDING';
}

async function cmd(c){
  await fetch('/api/command',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({cmd:c})});
}
</script>
</body>
</html>
"#;
