pub mod core;
pub mod phone;
pub mod qr;
pub mod tray;

use log::info;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::core::config::{resolve_device_name, Config, PortConfig};
use crate::core::mdns::MdnsService;
use crate::core::ws::{generate_pin, ClientRegistry, WsServer};
use crate::phone::{serve_phone_page, PHONE_HTML};
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

struct AppState {
    ws_server: Arc<Mutex<WsServer>>,
    _mdns: Arc<Mutex<MdnsService>>,
    port: u16,
    device_name: String,
    config: Config,
    client_registry: Arc<ClientRegistry>,
}

#[tauri::command]
async fn get_connection_info(
    _app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let port = state.port;
    let device_name = state.device_name.clone();
    let local_ip = get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());

    // Generate a fresh one-time pairing PIN and store it on the server so only
    // the phone that scanned this QR can connect.
    let pin = generate_pin();
    state.ws_server.lock().await.set_pin(pin.clone()).await;

    let url = format!("http://{local_ip}:{port}/?ws={port}&pin={pin}");
    info!("Phone page URL: {url}");

    let data_url = qr::qr_data_url(&url)?;

    let json = serde_json::json!({
        "qrDataUrl": data_url,
        "deviceName": device_name,
        "address": format!("ws://{}:{}", local_ip, port),
        "httpUrl": url,
        "pin": pin,
    });

    Ok(json.to_string())
}

/// Pick the IP phones on the same LAN can reach: the address of the interface
/// that owns the default route (robust, portable), falling back to any private
/// IPv4 on this host.
fn get_local_ip() -> Option<String> {
    if let Ok(iface) = default_net::get_default_interface() {
        if let Some(ip) = iface.ipv4.first().map(|n| n.addr) {
            if is_lan_ipv4(&ip) {
                return Some(ip.to_string());
            }
        }
    }
    get_lan_ips().into_iter().next()
}

/// A usable LAN IPv4: private (RFC1918), non-loopback, non-link-local.
fn is_lan_ipv4(ip: &std::net::Ipv4Addr) -> bool {
    !ip.is_loopback() && !ip.is_link_local() && is_private(ip)
}

fn is_private(ip: &std::net::Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 10 || (o[0] == 172 && (16..=31).contains(&o[1])) || (o[0] == 192 && o[1] == 168)
}

/// Collect private, non-loopback IPv4 addresses on this host.
fn get_lan_ips() -> Vec<String> {
    let mut ips: Vec<String> = Vec::new();
    for iface in default_net::get_interfaces() {
        for net in iface.ipv4 {
            if is_lan_ipv4(&net.addr) {
                ips.push(net.addr.to_string());
            }
        }
    }
    ips
}

#[tauri::command]
async fn get_connected_devices(state: State<'_, AppState>) -> Result<String, String> {
    let clients = state.client_registry.clients.read().await;
    serde_json::to_string(&*clients).map_err(|e| e.to_string())
}

#[tauri::command]
async fn disconnect_device(
    _app: AppHandle,
    state: State<'_, AppState>,
    client_id: String,
) -> Result<(), String> {
    let tx = {
        let mut txs = state.client_registry.shutdown_txs.write().await;
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
async fn block_device(
    app: AppHandle,
    state: State<'_, AppState>,
    client_id: String,
) -> Result<(), String> {
    // Find client info
    let entry = {
        let clients = state.client_registry.clients.read().await;
        clients.iter().find(|c| c.id == client_id).cloned()
    };
    let Some(info) = entry else {
        return Err("Client not found".to_string());
    };

    // Update WsServer blocklist (source of truth) and save to disk
    let block_entry = crate::core::config::BlockEntry {
        ip: info.ip.clone(),
        device_name: info.device_name.clone(),
    };
    let blocklist = {
        let mut ws = state.ws_server.lock().await;
        let mut current = ws.blocklist().await;
        current.push(block_entry);
        ws.set_blocklist(current.clone()).await;
        current
    };

    let mut config = state.config.clone();
    config.blocklist = blocklist;
    config
        .save()
        .map_err(|e| format!("Failed to save blocklist: {e}"))?;

    // Disconnect the client
    disconnect_device(app, state, client_id).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    let config = Config::load();
    let device_name = resolve_device_name(&config);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(move |app| {
            let handle = app.handle().clone();

            let device_name_clone = device_name.clone();
            let config = config.clone();

            tauri::async_runtime::spawn(async move {
                // Determine the listen port:
                //   http_port > port > random
                let listen_port = match &config.http_port {
                    PortConfig::Fixed(p) => *p,
                    PortConfig::Auto => match &config.port {
                        PortConfig::Fixed(p) => *p,
                        PortConfig::Auto => 0,
                    },
                };

                // Single TcpListener for both HTTP and WebSocket
                let listener = if listen_port == 0 {
                    match TcpListener::bind("0.0.0.0:0").await {
                        Ok(l) => l,
                        Err(e) => {
                            log::error!("Failed to bind random port: {e}");
                            return;
                        }
                    }
                } else {
                    match TcpListener::bind(("0.0.0.0", listen_port)).await {
                        Ok(l) => {
                            log::info!("Server bound to configured port {listen_port}");
                            l
                        }
                        Err(e) => {
                            log::warn!("Failed to bind to configured port {listen_port} ({e}), falling back to random port");
                            match TcpListener::bind("0.0.0.0:0").await {
                                Ok(l) => l,
                                Err(e) => {
                                    log::error!("Failed to bind fallback port: {e}");
                                    return;
                                }
                            }
                        }
                    }
                };

                let actual_port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
                log::info!("Server listening on port {actual_port}");

                let ws_server = Arc::new(Mutex::new(WsServer::new(
                    device_name_clone.clone(),
                    actual_port,
                )));

                // Load blocklist
                let blocklist = config.blocklist.clone();
                ws_server.lock().await.set_blocklist(blocklist).await;

                if !ws_server.lock().await.keyboard_healthy() {
                    log::warn!("键盘输入不可用：请授予辅助功能权限");
                    let _ = handle.emit("keyboard-permission-needed", ());
                }

                let client_registry = ws_server.lock().await.client_registry();

                // Unified accept loop
                let listener = Arc::new(listener);
                let ws_server_ref = ws_server.clone();
                let handle_ref = handle.clone();
                let html = PHONE_HTML.to_owned();

                tokio::spawn(async move {
                    loop {
                        let (mut stream, addr) = match listener.accept().await {
                            Ok(v) => v,
                            Err(e) => {
                                log::error!("Accept error: {e}");
                                continue;
                            }
                        };

                        let ws = ws_server_ref.clone();
                        let h = handle_ref.clone();
                        let html = html.clone();

                        tokio::spawn(async move {
                            // Read first chunk to classify the connection
                            let mut buf = vec![0u8; 4096];
                            let n = match stream.read(&mut buf).await {
                                Ok(0) => return,
                                Ok(n) => n,
                                Err(e) => {
                                    log::error!("Read error from {addr}: {e}");
                                    return;
                                }
                            };
                            buf.truncate(n);

                            let is_ws = buf
                                .windows(b"Upgrade: websocket".len())
                                .any(|w| w.eq_ignore_ascii_case(b"Upgrade: websocket"));

                            if is_ws {
                                ws.lock()
                                    .await
                                    .accept_connection(stream, addr, buf, h)
                                    .await;
                            } else {
                                serve_phone_page(stream, addr, buf, html).await;
                            }
                        });
                    }
                });

                let mut mdns = MdnsService::new(device_name_clone.clone(), actual_port);
                let mdns_addresses = get_lan_ips();
                if let Err(e) = mdns.start(mdns_addresses) {
                    log::error!("Failed to start mDNS: {e}");
                }

                handle.manage(AppState {
                    ws_server,
                    _mdns: Arc::new(Mutex::new(mdns)),
                    port: actual_port,
                    device_name: device_name_clone,
                    config,
                    client_registry,
                });
            });

            tray::setup_tray(app.handle())?;

            #[cfg(target_os = "macos")]
            log::warn!(
                "macOS: 请确保 LanType 已在 系统设置 → 隐私与安全性 → 辅助功能 中被授权，否则键盘输入将不生效。"
            );

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_connection_info,
            get_connected_devices,
            disconnect_device,
            block_device,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
