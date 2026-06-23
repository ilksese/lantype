# LanType — Agent Guide

## Build & Run

```bash
make release    # builds + auto-packages for current platform
```

Or step by step:

```bash
cargo build --release    # compile only
./package.sh             # macOS: wrap into .app bundle (optional, make does it)
```

- **macOS**: `make release` → produces `target/release/LanType.app` with embedded icon
- **Windows**: `make release` → `target/release/lantype.exe` (icon embedded by tauri_build, no extra step)
- **Linux**: `make release` → `target/release/lantype`
- `cargo tauri build` is NOT available — use `make release` instead.
- Proxy at `127.0.0.1:7897`. Rust crate mirror: 中科大 (system-level `.cargo/config.toml`).
- Build demands: `HTTP_PROXY`, `HTTPS_PROXY`, or direct network for crate download.

## Architecture

- Single crate `lantype`, Tauri v2 desktop app.
- `src/main.rs` → `src/lib.rs` (setup, Tauri commands) → `src/core/{ws,mdns,keyboard,protocol,config}.rs` + `src/qr.rs` + `src/tray.rs`
- `src/core/ws.rs`: tokio-tungstenite WebSocket server on `0.0.0.0:0` (random port). Port can be fixed via `config.json`.
- `src/core/keyboard.rs`: enigo 0.2 API — `Enigo::new(&Settings::default())`, call `.text()` which requires `use enigo::Keyboard`.
- `src/core/mdns.rs`: mdns-sd `_lantype._tcp` service, togglable via privacy switch.
- `src/core/protocol.rs`: JSON messages `{type: "text"|"ping"|"pong"|"connected"}`.
- `src/qr.rs`: qrcode crate + image crate → base64 PNG data URL.
- `src/main.rs` has `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` — release binary has no console window.
- App closes to system tray (not exit). Use tray menu to quit.

## Window

- Frameless, 320×460, non-resizable, centered. Decorations off.
- 3 Tauri IPC commands: `get_connection_info`, `get_privacy_enabled`, `toggle_privacy`.

## Testing

- No tests exist. No test dependencies in `Cargo.toml`. No CI.

## Notable

- `gen/` in `.gitignore` — Tauri build artifacts directory, never committed.
- `icons/icon.ico` + `icons/icon.png` (64×64 green circle) embedded via `Cargo.toml` metadata.
- `tauri.conf.json` JSON is strict — no trailing commas.
- `capabilities/default.json` defines window permissions.
- enigo 0.2 uses `Settings`, not `Default::default()` directly — must import `Keyboard` trait to call `.text()`.
- mDNS privacy: `AtomicBool` in `PrivacyState`, toggles `MdnsService::start()`/`stop()` at runtime.

## Config

- Config file `config.json` supports only the `port` field (default `"auto"`).
- Priority (ascending): defaults → `$HOME/.config/lantype/config.json` (global) → `./config.json` (local/cwd). Shallow merge — local keys override global.
- `"port": "auto"` → random port; `"port": 1234` → fixed port (fallback to random on bind failure).