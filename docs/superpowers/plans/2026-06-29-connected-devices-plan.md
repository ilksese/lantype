# Connected Devices Display with Blocklist — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show connected devices on the desktop receiver, support disconnect and block per device.

**Architecture:** Extend the existing vanilla HTML/JS frontend with a device list section below the QR code. Backend adds a `Hello` protocol message, a `ClientRegistry` tracking active connections, and blocklist persistence in `config.json`. Events flow from WsServer → Tauri `emit()` → frontend `listen()` for real-time updates.

**Tech Stack:** Rust (Tauri v2, tokio, tungstenite), vanilla HTML/JS/CSS

## Global Constraints

- No tests exist, no test dependencies, no CI — manual verification only
- Single crate `lantype` — all modules can freely import each other
- Window is 320×460, non-resizable — device list uses container scroll when overflowing
- enigo 0.2 uses `Settings`, must import `Keyboard` trait for `.text()`
- Cargo.toml already has `uuid = { version = "1", features = ["v4"] }`
- `tokio::sync::Mutex` is the project convention for shared state
- Use `info!` / `error!` for logging
- Encode all JSON manually with `serde_json::json!()` or struct serialization

---

### Task 1: Protocol changes — `Hello` message + `client_id`

**Files:**
- Modify: `src/core/protocol.rs:11-29`

**Interfaces:**
- Consumes: nothing
- Produces: `ClientMessage::Hello { device_name: String }`, `ServerMessage::Connected { device: String, client_id: String }`

- [ ] **Step 1: Add `Hello` variant to `ClientMessage`**

In `protocol.rs`, add a new variant inside `#[serde(tag = "type")]`:

```rust
#[serde(rename = "hello")]
Hello { device_name: String },
```

- [ ] **Step 2: Add `client_id` to `ServerMessage::Connected`**

Change existing variant from:

```rust
Connected { device: String },
```

to:

```rust
Connected { device: String, client_id: String },
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: warning about unused import or dead code (protocol is used by ws.rs which imports `ServerMessage` with destructuring — the destructure will now need updating)

---

### Task 2: Backend — `ClientRegistry`, WsServer changes, blocklist in Config, Tauri commands

**Files:**
- Create: nothing (all changes in existing files)
- Modify: `src/core/mod.rs`, `src/core/ws.rs`, `src/core/config.rs`, `src/lib.rs`, `capabilities/default.json`

**Interfaces:**
- Consumes: `ClientMessage::Hello`, `ServerMessage::Connected` from Task 1
- Produces: Tauri commands `get_connected_devices`, `disconnect_device`, `block_device`; Tauri event `"clients-changed"`; config field `blocklist`

- [ ] **Step 1: Add `BlockEntry` to `src/core/config.rs`**

Add before `Config`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockEntry {
    pub ip: String,
    pub device_name: String,
}
```

Add to `Config`:

```rust
#[serde(default)]
pub blocklist: Vec<BlockEntry>,
```

Add a `save()` method to `Config`:

```rust
impl Config {
    /// Save current config to the global config file path.
    /// Merges with existing file content (preserving other keys) using the same
    /// shallow merge strategy as load().  On failure the error is logged and
    /// the in-memory config is unaffected.
    pub fn save(&self) -> Result<(), String> {
        let global_path = global_config_path().ok_or("No HOME directory")?;

        let mut existing: Value = if global_path.exists() {
            std::fs::read_to_string(&global_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_else(|| serde_json::json!({}))
        } else {
            serde_json::json!({})
        };

        // Serialize self, override matching keys in existing
        let self_val = serde_json::to_value(self).map_err(|e| e.to_string())?;
        merge(&mut existing, self_val);

        // Ensure parent directory exists
        if let Some(parent) = global_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let content = serde_json::to_string_pretty(&existing).map_err(|e| e.to_string())?;
        std::fs::write(&global_path, &content).map_err(|e| e.to_string())?;
        Ok(())
    }
}
```

Make `merge` function `pub(crate)` (change its visibility) so `save()` can call it.

- [ ] **Step 2: Add `ClientInfo` and `ClientRegistry` to `src/core/ws.rs`**

At the top of the file, add imports:

```rust
use std::collections::HashMap;
use std::sync::RwLock;
use tokio::sync::watch;
use uuid::Uuid;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use crate::core::config::BlockEntry;
```

Define types (before or after `WsServer`):

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ClientInfo {
    pub id: String,
    pub device_name: String,
    pub ip: String,
}

pub struct ClientRegistry {
    pub clients: RwLock<Vec<ClientInfo>>,
    pub shutdown_txs: RwLock<HashMap<String, watch::Sender<bool>>>,
}

impl ClientRegistry {
    pub fn new() -> Self {
        Self {
            clients: RwLock::new(Vec::new()),
            shutdown_txs: RwLock::new(HashMap::new()),
        }
    }
}
```

- [ ] **Step 3: Update `WsServer::start` signature and body**

Add new fields to `WsServer`:

```rust
pub struct WsServer {
    port: u16,
    keyboard: Arc<KeyboardInjector>,
    device_name: String,
    listener: Option<Arc<TcpListener>>,
    client_registry: Arc<ClientRegistry>,
    blocklist: Arc<RwLock<Vec<BlockEntry>>>,
}
```

Update constructor:

```rust
pub fn new(device_name: String) -> Self {
    Self {
        port: 0,
        keyboard: Arc::new(KeyboardInjector::new()),
        device_name,
        listener: None,
        client_registry: Arc::new(ClientRegistry::new()),
        blocklist: Arc::new(RwLock::new(Vec::new())),
    }
}
```

Add accessors:

```rust
pub fn client_registry(&self) -> Arc<ClientRegistry> {
    self.client_registry.clone()
}

pub fn set_blocklist(&mut self, blocklist: Vec<BlockEntry>) {
    *self.blocklist.write().unwrap() = blocklist;
}
```

Update `start()` parameter to accept `AppHandle`:

```rust
pub async fn start(&mut self, port_override: Option<u16>, app_handle: AppHandle) -> Result<(), String> {
```

Inside `start()`, where `handle_client` is spawned, change to:

```rust
let client_registry = self.client_registry.clone();
let blocklist = self.blocklist.clone();
tokio::spawn(handle_client(
    stream, addr,
    keyboard, device_name,
    client_registry, blocklist,
    app_handle.clone(),
));
```

- [ ] **Step 4: Rewrite `handle_client` function**

Replace the entire function:

```rust
async fn handle_client(
    stream: TcpStream,
    addr: SocketAddr,
    keyboard: Arc<KeyboardInjector>,
    device_name: String,
    client_registry: Arc<ClientRegistry>,
    blocklist: Arc<RwLock<Vec<BlockEntry>>>,
    app_handle: AppHandle,
) {
    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            error!("WS handshake error from {addr}: {e}");
            return;
        }
    };

    let (mut write, mut read) = ws_stream.split();

    // Generate client id and send Connected message
    let client_id = Uuid::new_v4().to_string();
    let connected_msg = protocol::serialize_server_message(&ServerMessage::Connected {
        device: device_name.clone(),
        client_id: client_id.clone(),
    });
    if let Err(e) = write.send(Message::Text(connected_msg.into())).await {
        error!("Send error to {addr}: {e}");
        return;
    }

    // Wait for Hello with timeout
    let hello_timeout = tokio::time::Duration::from_secs(5);
    let hello_result = tokio::time::timeout(hello_timeout, async {
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    let parsed = protocol::parse_client_message(&text).ok();
                    if let Some(ClientMessage::Hello { device_name }) = parsed {
                        return Ok(device_name);
                    }
                    // Ping is fine, keep waiting
                    if let Some(ClientMessage::Ping) = parsed {
                        let pong = protocol::serialize_server_message(&ServerMessage::Pong);
                        let _ = write.send(Message::Text(pong.into())).await;
                        continue;
                    }
                    // Unexpected message before Hello
                    return Err("expected hello first".to_string());
                }
                Ok(Message::Ping(_)) => {
                    let _ = write.send(Message::Pong(vec![])).await;
                }
                Ok(Message::Close(_)) | Err(_) => {
                    return Err("connection closed before hello".to_string());
                }
                _ => {}
            }
        }
        Err("stream ended before hello".to_string())
    }).await;

    let sender_name = match hello_result {
        Ok(Ok(name)) => name,
        Ok(Err(e)) => {
            info!("Client {addr} rejected during hello: {e}");
            return;
        }
        Err(_) => {
            info!("Client {addr} timed out waiting for hello");
            let _ = write.send(Message::Close(Some(
                tungstenite::protocol::CloseFrame {
                    code: tungstenite::protocol::frame::coding::CloseCode::Policy,
                    reason: "hello timeout".into(),
                },
            ))).await;
            return;
        }
    };

    let client_ip = addr.ip().to_string();

    // Check blocklist
    {
        let bl = blocklist.read().unwrap();
        if bl.iter().any(|b| b.ip == client_ip && b.device_name == sender_name) {
            info!("Rejected blocked device {sender_name} from {addr}");
            let _ = write.send(Message::Close(Some(
                tungstenite::protocol::CloseFrame {
                    code: tungstenite::protocol::frame::coding::CloseCode::Policy,
                    reason: "blocked".into(),
                },
            ))).await;
            return;
        }
    }

    // Create shutdown channel for this client
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    // Register in registry
    {
        let mut clients = client_registry.clients.write().unwrap();
        clients.push(ClientInfo {
            id: client_id.clone(),
            device_name: sender_name.clone(),
            ip: client_ip.clone(),
        });
    }
    {
        let mut txs = client_registry.shutdown_txs.write().unwrap();
        txs.insert(client_id.clone(), shutdown_tx);
    }

    // Emit clients-changed event
    {
        let clients = client_registry.clients.read().unwrap();
        let payload = serde_json::to_value(&*clients).unwrap_or(serde_json::Value::Array(vec![]));
        let _ = app_handle.emit("clients-changed", payload);
    }

    info!("Client {sender_name} ({addr}) registered as {client_id}");

    // Main message loop
    loop {
        tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match protocol::parse_client_message(&text) {
                            Ok(ClientMessage::Ping) => {
                                let pong = protocol::serialize_server_message(&ServerMessage::Pong);
                                let _ = write.send(Message::Text(pong.into())).await;
                            }
                            Ok(ClientMessage::Type { text }) => {
                                if let Err(e) = keyboard.type_text(text).await {
                                    error!("Type error: {e}");
                                }
                            }
                            Ok(ClientMessage::Diff { backspace, text }) => {
                                if backspace > 0 {
                                    if let Err(e) = keyboard.delete_chars(backspace).await {
                                        error!("Delete error: {e}");
                                    }
                                }
                                if !text.is_empty() {
                                    if let Err(e) = keyboard.type_text(text).await {
                                        error!("Type error: {e}");
                                    }
                                }
                            }
                            Ok(ClientMessage::Hello{..}) => {
                                // ignore duplicate hello
                            }
                            Err(e) => {
                                error!("Parse error: {e}");
                            }
                        }
                    }
                    Some(Ok(Message::Ping(_))) => {
                        let _ = write.send(Message::Pong(vec![])).await;
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                        break;
                    }
                    _ => {}
                }
            }
            _ = shutdown_rx.changed() => {
                info!("Disconnecting client {sender_name} ({addr})");
                let _ = write.send(Message::Close(None)).await;
                break;
            }
        }
    }

    // Deregister
    {
        let mut clients = client_registry.clients.write().unwrap();
        clients.retain(|c| c.id != client_id);
    }
    {
        let mut txs = client_registry.shutdown_txs.write().unwrap();
        txs.remove(&client_id);
    }

    // Emit updated list
    {
        let clients = client_registry.clients.read().unwrap();
        let payload = serde_json::to_value(&*clients).unwrap_or(serde_json::Value::Array(vec![]));
        let _ = app_handle.emit("clients-changed", payload);
    }

    info!("Connection closed: {addr}");
}
```

- [ ] **Step 5: Update `AppState` in `src/lib.rs`**

Import `ClientRegistry`:

```rust
use crate::core::ws::{WsServer, ClientRegistry, ClientInfo};
```

Change `AppState`:

```rust
struct AppState {
    ws_server: Arc<Mutex<WsServer>>,
    mdns: Arc<Mutex<MdnsService>>,
    http_port: u16,
    device_name: String,
    config: Config,
    client_registry: Arc<ClientRegistry>,
}
```

- [ ] **Step 6: Update setup code in `src/lib.rs`**

In the `setup` closure, update the `WsServer::start` call:

```rust
ws_server.start(ws_port_config, handle.clone()).await?;
```

After `ws_server.start()`, read the registry and set blocklist:

```rust
let client_registry = ws_server.client_registry();

// Load blocklist from config
let blocklist = config.blocklist.clone();
ws_server.set_blocklist(blocklist);
```

Update `handle.manage(AppState { ... })` — add:

```rust
client_registry,
```

- [ ] **Step 7: Add Tauri commands in `src/lib.rs`**

Before `fn run()`:

```rust
#[tauri::command]
async fn get_connected_devices(state: State<'_, AppState>) -> Result<String, String> {
    let clients = state.client_registry.clients.read().unwrap();
    serde_json::to_string(&*clients).map_err(|e| e.to_string())
}

#[tauri::command]
async fn disconnect_device(app: AppHandle, state: State<'_, AppState>, client_id: String) -> Result<(), String> {
    let tx = {
        let mut txs = state.client_registry.shutdown_txs.write().unwrap();
        txs.remove(&client_id)
    };
    match tx {
        Some(tx) => {
            let _ = tx.send(true);
            Ok(())
        }
        None => Err("Client not found".to_string()),
    }
}

#[tauri::command]
async fn block_device(app: AppHandle, state: State<'_, AppState>, client_id: String) -> Result<(), String> {
    // Find client info
    let entry = {
        let clients = state.client_registry.clients.read().unwrap();
        clients.iter().find(|c| c.id == client_id).cloned()
    };
    let Some(info) = entry else {
        return Err("Client not found".to_string());
    };

    // Add to config blocklist and persist
    let block_entry = crate::core::config::BlockEntry {
        ip: info.ip.clone(),
        device_name: info.device_name.clone(),
    };
    let mut config = state.config.clone();
    config.blocklist.push(block_entry);
    config.save().map_err(|e| format!("Failed to save blocklist: {e}"))?;

    // Update WsServer blocklist
    {
        let mut ws = state.ws_server.lock().await;
        ws.set_blocklist(config.blocklist.clone());
    }

    // Disconnect the client
    disconnect_device(app, state, client_id).await
}
```

Register the new commands in `invoke_handler`:

```rust
.invoke_handler(tauri::generate_handler![
    get_connection_info,
    get_privacy_enabled,
    toggle_privacy,
    get_connected_devices,
    disconnect_device,
    block_device,
])
```

- [ ] **Step 8: Add Tauri event permission in `capabilities/default.json`**

Add `"core:event:default"` to the permissions array:

```json
"permissions": [
    "core:default",
    "core:event:default",
    "core:window:allow-start-dragging",
    "shell:allow-open"
]
```

- [ ] **Step 9: Export `ClientInfo` and `ClientRegistry` from `mod.rs`**

In `src/core/mod.rs`, no change needed — `ws` module is already declared as `pub mod ws`, so `pub` types are accessible as `crate::core::ws::ClientInfo`.

- [ ] **Step 10: Verify compilation**

Run: `cargo check`
Expected: success. If any `Connected` destructure is broken in `ws.rs`, fix it (it was replaced in the rewrite above).

- [ ] **Step 11: Run the tail binary to verify**

Run: `make release`
Expected: builds successfully. Launch the app, verify no crashes on startup.

---

### Task 3: Desktop frontend — device list UI

**Files:**
- Modify: `web/index.html`

- [ ] **Step 1: Add device list HTML section**

After the address `<div class="address" id="address">` and before the privacy row, add:

```html
<div class="clients-section" id="clientsSection" style="display:none">
  <div class="clients-title">已连接设备</div>
  <div class="clients-list" id="clientsList"></div>
</div>
```

- [ ] **Step 2: Add CSS for device list**

Add to the `<style>` block:

```css
.clients-section {
  width:100%;
  display:flex;
  flex-direction:column;
  gap:8px;
}
.clients-title {
  font-size:13px;
  color:#888;
  text-align:center;
}
.clients-list {
  display:flex;
  flex-direction:column;
  gap:6px;
  max-height:200px;
  overflow-y:auto;
}
.client-card {
  background:rgba(255,255,255,.06);
  border-radius:10px;
  padding:10px 14px;
  display:flex;
  align-items:center;
  justify-content:space-between;
}
.client-info {
  display:flex;
  flex-direction:column;
  gap:2px;
  flex:1;
  min-width:0;
}
.client-name {
  font-size:13px;
  font-weight:500;
  color:#c4b5fd;
}
.client-ip {
  font-size:11px;
  color:#666;
}
.client-actions {
  display:flex;
  gap:6px;
  flex-shrink:0;
}
.btn-disconnect,.btn-block {
  border:none;
  border-radius:6px;
  padding:4px 10px;
  font-size:11px;
  cursor:pointer;
  transition:opacity .15s;
}
.btn-disconnect { background:#3b3b5c; color:#ccc; }
.btn-disconnect:hover { background:#4b4b6c; }
.btn-block { background:#5c2e3a; color:#f87171; }
.btn-block:hover { background:#6c3e4a; }
```

- [ ] **Step 3: Add JS for event listening and rendering**

In the `<script>` block, after the existing code:

```js
const { listen } = window.__TAURI__.event;

function renderDeviceList(clients) {
  var section = document.getElementById('clientsSection');
  var list = document.getElementById('clientsList');
  if (!clients || clients.length === 0) {
    section.style.display = 'none';
    return;
  }
  section.style.display = 'flex';
  list.innerHTML = clients.map(function(c) {
    return '<div class="client-card" data-id="' + c.id + '">' +
      '<div class="client-info">' +
        '<div class="client-name">' + escapeHtml(c.device_name) + '</div>' +
        '<div class="client-ip">' + escapeHtml(c.ip) + '</div>' +
      '</div>' +
      '<div class="client-actions">' +
        '<button class="btn-disconnect" onclick="disconnectClient(\'' + c.id + '\')">断开</button>' +
        '<button class="btn-block" onclick="blockClient(\'' + c.id + '\')">拉黑</button>' +
      '</div>' +
    '</div>';
  }).join('');
}

function escapeHtml(s) {
  var div = document.createElement('div');
  div.appendChild(document.createTextNode(s));
  return div.innerHTML;
}

function disconnectClient(id) {
  invoke('disconnect_device', { clientId: id }).catch(function(e) {
    console.error('disconnect failed', e);
  });
}

function blockClient(id) {
  invoke('block_device', { clientId: id }).catch(function(e) {
    console.error('block failed', e);
  });
}
```

- [ ] **Step 4: Wire up on load**

Add after `refresh()` and the toggle listener:

```js
// Load initial connected devices
invoke('get_connected_devices').then(function(raw) {
  renderDeviceList(JSON.parse(raw));
}).catch(function(e) {
  console.error('get_connected_devices failed', e);
});

// Listen for real-time updates
listen('clients-changed', function(event) {
  renderDeviceList(event.payload);
});
```

- [ ] **Step 5: Update window height in `tauri.conf.json`**

The device list adds vertical space (~60px per device, plus the section title). Increase window height from 460 to 520 to accommodate one device comfortably (more devices scroll inside the 200px max-height):

```json
"height": 520,
```

- [ ] **Step 6: Verify build**

Run: `make release`
Expected: builds successfully, app launches with device list section visible when a phone connects.

---

### Task 4: Phone frontend — send Hello message

**Files:**
- Modify: `web/phone.html`

- [ ] **Step 1: Add `getFriendlyName` helper and send Hello on open**

In the `<script>` block, replace the `ws.onopen` handler:

```js
function getFriendlyName() {
  var ua = navigator.userAgent;
  var browser = '浏览器';
  if (ua.indexOf('Chrome') !== -1 && ua.indexOf('Edg') === -1) browser = 'Chrome';
  else if (ua.indexOf('Safari') !== -1 && ua.indexOf('Chrome') === -1) browser = 'Safari';
  else if (ua.indexOf('Edg') !== -1) browser = 'Edge';
  else if (ua.indexOf('Firefox') !== -1) browser = 'Firefox';
  var os = '未知设备';
  if (ua.indexOf('iPhone') !== -1 || ua.indexOf('iPad') !== -1) os = 'iOS';
  else if (ua.indexOf('Android') !== -1) os = 'Android';
  else if (ua.indexOf('Mac') !== -1) os = 'macOS';
  else if (ua.indexOf('Windows') !== -1) os = 'Windows';
  else if (ua.indexOf('Linux') !== -1) os = 'Linux';
  return browser + ' · ' + os;
}
```

Then change the `ws.onopen` to send Hello:

```js
ws.onopen = function(){
  setStatus('connected','已连接');
  input.disabled=false;
  input.focus();
  ws.send(JSON.stringify({type:'hello', device_name: getFriendlyName()}));
};
```

- [ ] **Step 2: Handle `client_id` in connected message (no UI change, just parsing)**

The `phone.html` already parses `msg.type === 'connected'` and displays `msg.device`. The new `client_id` field is ignored by the phone but won't break parsing since JSON.parse handles extra fields silently. No change needed.

- [ ] **Step 3: Verify build**

Run: `make release`
Expected: builds. Open phone page in browser, connect to desktop — the `Hello` message is sent. Check server logs (`info!` line saying "Client ... registered").

---

### Verification checklist (manual)

1. Start LanType desktop app.
2. Scan QR code or manually open the phone page URL.
3. Phone connects — verify desktop shows device card with name (e.g. "Chrome · iOS") and IP.
4. Connect a second phone/tab — verify both appear in the list.
5. Click "断开" (Disconnect) on one device — verify it disappears from list, phone shows "已断开" and reconnects.
6. Reconnect, click "拉黑" (Block) — verify device disappears, phone shows "已断开" on reconnect attempt.
7. Kill and restart the app — verify blocked device is still rejected on reconnect (blocklist persisted).
8. Open a different device with same IP but different name — verify it connects (blocklist requires both IP + device_name match).