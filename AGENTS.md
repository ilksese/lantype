# LanType — Agent Guide

## Build & Run

```bash
npm install
npm run dev      # Tauri dev mode with Vite dev server
npm run build    # Tauri production build
```

Useful direct commands:

```bash
npm run build:phone
npm run tauri -- build --target <target-triple>
cargo check --manifest-path src-tauri/Cargo.toml
```

- Build artifacts live under `src-tauri/target/`; bundles under `src-tauri/target/release/bundle/`.
- Tauri CLI is installed locally via `@tauri-apps/cli`; use `npm run tauri -- ...`.
- Cross-compile dependencies vary by Tauri target and platform.
- Proxy at `127.0.0.1:7897`. Rust crate mirror: 中科大 (system-level `.cargo/config.toml`).
- Build demands: `HTTP_PROXY`, `HTTPS_PROXY`, or direct network for crate download.

## Architecture

- Standard Tauri layout: `src-tauri/` contains the Rust crate, Tauri config, capabilities, and icons; `web/` contains frontend files.
- Single crate `lantype`, Tauri v2 desktop app.
- `src-tauri/src/main.rs` → `src-tauri/src/lib.rs` (setup, Tauri commands) → `src-tauri/src/core/{ws,mdns,keyboard,protocol,config}.rs` + `src-tauri/src/qr.rs` + `src-tauri/src/tray.rs`
- `src-tauri/src/core/ws.rs`: tokio-tungstenite WebSocket server starts at the configured port, defaulting to 2777, and increments until a free port is found.
- `src-tauri/src/core/keyboard.rs`: enigo 0.2 API — `Enigo::new(&Settings::default())`, call `.text()` which requires `use enigo::Keyboard`.
- `src-tauri/src/core/mdns.rs`: mdns-sd `_lantype._tcp` service, togglable via privacy switch.
- `src-tauri/src/core/protocol.rs`: JSON messages `{type: "text"|"ping"|"pong"|"connected"}`.
- `src-tauri/src/qr.rs`: qrcode crate + image crate → base64 PNG data URL.
- `src-tauri/src/main.rs` has `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` — release binary has no console window.
- App closes to system tray (not exit). Use tray menu to quit.

## Window

- Frameless, 320×460, non-resizable, centered. Decorations off.
- 3 Tauri IPC commands: `get_connection_info`, `get_privacy_enabled`, `toggle_privacy`.

## Testing

- Run `cargo fmt` only for Rust code files changed in the current task.
- No tests exist. No test dependencies in `src-tauri/Cargo.toml`. No CI.

## Notable

- `src-tauri/target/` and `src-tauri/gen/` in `.gitignore` — Tauri build artifacts, never committed.
- `src-tauri/icons/icon.ico` + `src-tauri/icons/icon.png` (64×64 green circle) embedded via `src-tauri/Cargo.toml` metadata.
- `src-tauri/tauri.conf.json` JSON is strict — no trailing commas.
- `src-tauri/capabilities/default.json` defines window permissions.
- enigo 0.2 uses `Settings`, not `Default::default()` directly — must import `Keyboard` trait to call `.text()`.
- mDNS privacy: `AtomicBool` in `PrivacyState`, toggles `MdnsService::start()`/`stop()` at runtime.

## Config

- Config file `config.json` supports only the `port` field (default `"auto"`).
- Priority (ascending): defaults → `$HOME/.config/lantype/config.json` (global) → `./config.json` (local/cwd). Shallow merge — local keys override global.
- `"port": "auto"` → start at 2777 and increment until free; `"port": 1234` → start at 1234 and increment until free.
