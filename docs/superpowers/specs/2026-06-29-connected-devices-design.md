# Connected Devices Display with Blocklist

**Date**: 2026-06-29
**Status**: Draft

## Overview

When a sender (phone browser) connects to the receiver (desktop app), the desktop shows a list of connected devices. Each device has independent "Disconnect" and "Block" buttons. Blocked devices (by IP + device name pair) are persisted to the global config file and rejected on future connection attempts.

## Protocol Changes

### `ClientMessage` — new `hello` variant

The sender MUST send this as its first message after WebSocket handshake, before any `diff` or `type`:

```json
{ "type": "hello", "device_name": "Chrome on iPhone" }
```

`device_name` is extracted client-side from `navigator.userAgent` (e.g. `"Safari on macOS"` / `"Chrome on Android"` / `"Edge on Windows"`).

Addition to `src/core/protocol.rs`:

```rust
#[serde(rename = "hello")]
Hello { device_name: String },
```

### `ServerMessage::Connected` — add `client_id`

```json
{ "type": "connected", "device": "我的Mac", "client_id": "uuid-xxxx" }
```

`client_id` identifies this specific WS connection for disconnect/block operations.

Change in `src/core/protocol.rs`:

```rust
Connected { device: String, client_id: String },
```

## Backend

### Configuration — blocklist persistence

`config.json` (global: `$HOME/.config/lantype/config.json`) gains a new optional field:

```json
{
  "port": "auto",
  "nickname": "可爱的桃子",
  "blocklist": [
    {"ip": "192.168.1.5", "device_name": "Chrome on iPhone"}
  ]
}
```

`Config` struct in `src/core/config.rs` adds:

```rust
pub blocklist: Vec<BlockEntry>,

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockEntry {
    pub ip: String,
    pub device_name: String,
}
```

`save()` method added to `Config` — writes to global config path. `config.json` load/merge logic is unchanged (new key defaults to empty vec if absent).

### Connection state tracking

A new type in `src/core/ws.rs` (or a new file under `src/core/`):

```rust
use std::sync::RwLock;

#[derive(Debug, Clone, Serialize)]
struct ClientInfo {
    id: String,           // UUID v4
    device_name: String,  // from Hello message
    ip: String,           // from SocketAddr
}

struct ClientRegistry {
    clients: RwLock<Vec<ClientInfo>>,
    // Per-connection shutdown channels
    shutdown_txs: RwLock<HashMap<String, tokio::sync::oneshot::Sender<()>>>,
}
```

`ClientRegistry` is wrapped in `Arc` and shared between `WsServer` and `AppState`.

### Connection flow (updated `handle_client`)

1. WS handshake → accept.
2. Send `ServerMessage::Connected` with a newly generated `client_id`.
3. Read first message — must be `ClientMessage::Hello { device_name }`.
4. Check blocklist: `config.blocklist` contains `(ip, device_name)` pair → close WS with close frame, log, return.
5. Register in `ClientRegistry` — insert `ClientInfo`, store `oneshot::Sender`.
6. Emit Tauri event `"clients-changed"` → full client list serialized as JSON.
7. Normal message loop (same as current).
8. On disconnect (loop exit, close frame, error) — deregister from `ClientRegistry`, emit `"clients-changed"`.

Step 3 timeout: if the first non-ping message after connect is not `Hello` within 5 seconds, close the connection.

### New Tauri commands

All in `src/lib.rs`:

```rust
#[tauri::command]
async fn get_connected_devices(app: AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();
    let clients = state.client_registry.clients.read().unwrap();
    Ok(serde_json::to_string(&*clients).map_err(|e| e.to_string())?)
}

#[tauri::command]
async fn disconnect_device(app: AppHandle, client_id: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    // Find the oneshot sender, trigger it
    let tx = {
        let mut shutdowns = state.client_registry.shutdown_txs.write().unwrap();
        shutdowns.remove(&client_id)
    };
    if let Some(tx) = tx {
        let _ = tx.send(());
    }
    Ok(())
}

#[tauri::command]
async fn block_device(app: AppHandle, client_id: String) -> Result<(), String> {
    let state = app.state::<AppState>();

    // 1. Find client info
    let entry = {
        let clients = state.client_registry.clients.read().unwrap();
        clients.iter().find(|c| c.id == client_id).cloned()
    };
    let Some(entry) = entry else {
        return Err("Client not found".into());
    };

    // 2. Add to blocklist, persist
    let mut config = state.config.clone();
    config.blocklist.push(BlockEntry {
        ip: entry.ip.clone(),
        device_name: entry.device_name.clone(),
    });
    config.save()?;

    // 3. Disconnect the client
    disconnect_device(app, client_id).await?;

    Ok(())
}
```

### Tauri event emission

After any client add/remove, emit:

```rust
app.emit("clients-changed", &*client_registry.clients.read().unwrap())?;
```

Frontend listens via `window.__TAURI__.event.listen("clients-changed", callback)`.

### `AppState` changes

```rust
struct AppState {
    ws_server: Arc<Mutex<WsServer>>,
    mdns: Arc<Mutex<MdnsService>>,
    http_port: u16,
    device_name: String,
    config: Config,
    client_registry: Arc<ClientRegistry>,   // NEW
}
```

## Frontend (`web/index.html`)

### Layout insertion point

Between the address display and the privacy toggle, add a device list section:

```html
<div class="clients-section" id="clientsSection" style="display:none">
  <div class="clients-title">已连接设备</div>
  <div class="clients-list" id="clientsList"></div>
</div>
```

### Card template (per device, generated in JS)

```html
<div class="client-card" data-id="UUID">
  <div class="client-info">
    <span class="client-name">Chrome</span>
    <span class="client-platform">iPhone</span>
  </div>
  <div class="client-ip">192.168.1.5</div>
  <div class="client-actions">
    <button class="btn-disconnect">断开</button>
    <button class="btn-block">拉黑</button>
  </div>
</div>
```

### Layout on QR code

The whole page scrolls when content exceeds 460px (fixed window height). If device list + QR don't fit, the container scrolls.

### Event flow

```
Page load
  → invoke("get_connection_info")  [existing, for QR/address]
  → invoke("get_connected_devices")  [NEW, initial state]
  → listen("clients-changed", renderDeviceList)  [NEW, realtime updates]

Button click → invoke("disconnect_device", {clientId}) or invoke("block_device", {clientId})
```

## Sender Frontend (`web/phone.html`)

After `ws.onopen`, send `Hello` before any other message:

```js
ws.onopen = function() {
  var ua = navigator.userAgent;
  var name = getFriendlyName(ua);  // e.g. "Safari on macOS"
  ws.send(JSON.stringify({type:'hello', device_name: name}));
  // ... rest of existing logic
};
```

`getFriendlyName` extracts browser + OS from user agent string client-side.

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Blocked device connects | WS accepts handshake, sends `Connected`, then closes after `Hello` received |
| `Hello` timeout (5s) | Close connection without registering |
| `Hello` never sent | Close after timeout, log warning |
| `disconnect_device` for missing ID | Return error, frontend silently ignores |
| `block_device` for missing ID | Return error |
| Config save fails (disk full, permissions) | Return error to frontend, blocklist may be lost on restart |
| Multiple connections from same IP+name (but not blocked) | Allowed; each gets unique `client_id` |

## Non-goals

- Unblock UI (no way to remove from blocklist via GUI; file edit only)
- Blocklist management page
- Connection duration display
- Last input time display

## Files Modified

| File | Change |
|------|--------|
| `src/core/protocol.rs` | Add `Hello` variant, add `client_id` to `Connected` |
| `src/core/ws.rs` | Add `ClientInfo`, `ClientRegistry`, blocklist check, shutdown channels, event emission |
| `src/core/config.rs` | Add `BlockEntry`, `blocklist` field, `save()` method |
| `src/lib.rs` | Add 3 Tauri commands, `client_registry` field in `AppState` |
| `web/index.html` | Add device list UI, Tauri event listener, button handlers |
| `web/phone.html` | Send `Hello` message on connect |
| `Cargo.toml` | Add `uuid` dependency (for `client_id` generation) |