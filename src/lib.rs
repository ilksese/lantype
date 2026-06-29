pub mod core;
pub mod phone;
pub mod qr;
pub mod tray;

use log::info;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::core::config::{resolve_device_name, Config};
use crate::core::mdns::MdnsService;
use crate::core::ws::{ClientRegistry, WsServer};
use crate::phone::PhoneServer;
use crate::tray::PrivacyState;
use tokio::sync::Mutex;

struct AppState {
    ws_server: Arc<Mutex<WsServer>>,
    mdns: Arc<Mutex<MdnsService>>,
    http_port: u16,
    device_name: String,
    config: Config,
    client_registry: Arc<ClientRegistry>,
}

#[tauri::command]
async fn get_connection_info(
    _app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let ws = state.ws_server.lock().await;
    let ws_port = ws.port();
    let http_port = state.http_port;

    let device_name = state.device_name.clone();

    let local_ip = get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());

    let url = format!("http://{local_ip}:{http_port}/?ws={ws_port}");
    info!("Phone page URL: {url}");

    let data_url = qr::qr_data_url(&url)?;

    let json = serde_json::json!({
        "qrDataUrl": data_url,
        "deviceName": device_name,
        "address": format!("ws://{}:{}", local_ip, ws_port),
        "httpUrl": url,
    });

    Ok(json.to_string())
}

#[tauri::command]
fn get_privacy_enabled(app: AppHandle) -> bool {
    app.state::<PrivacyState>().enabled.load(Ordering::Relaxed)
}

#[tauri::command]
async fn toggle_privacy(app: AppHandle) -> Result<bool, String> {
    let state = app.state::<PrivacyState>();
    let new_val = !state.enabled.load(Ordering::Relaxed);
    state.enabled.store(new_val, Ordering::Relaxed);

    let app_state = app.state::<AppState>();
    let ws = app_state.ws_server.lock().await;
    let _port = ws.port();

    let mut mdns = app_state.mdns.lock().await;
    if new_val {
        mdns.start()?;
        info!("mDNS broadcast started");
    } else {
        mdns.stop();
        info!("mDNS broadcast stopped");
    }

    Ok(new_val)
}

fn get_local_ip() -> Option<String> {
    for iface in local_ip_address::list_afinet_netifas().ok()? {
        if let std::net::IpAddr::V4(ip) = iface.1 {
            if !ip.is_loopback() && !ip.is_link_local()
                && (ip.octets()[0] == 10
                    || (ip.octets()[0] == 172 && (16..=31).contains(&ip.octets()[1]))
                    || (ip.octets()[0] == 192 && ip.octets()[1] == 168))
            {
                return Some(ip.to_string());
            }
        }
    }
    None
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
        .manage(PrivacyState {
            enabled: AtomicBool::new(true),
        })
        .setup(move |app| {
            let handle = app.handle().clone();

            let device_name_clone = device_name.clone();
            let config = config.clone();
            let ws_port_config = match config.port {
                crate::core::config::PortConfig::Auto => None,
                crate::core::config::PortConfig::Fixed(port) => Some(port),
            };

            tauri::async_runtime::spawn(async move {
                let mut ws_server = WsServer::new(device_name_clone.clone());
                if let Err(e) = ws_server.start(ws_port_config, handle.clone()).await {
                    log::error!("Failed to start WS server: {e}");
                    return;
                }

                let ws_port = ws_server.port();

                if !ws_server.keyboard_healthy() {
                    log::warn!("键盘输入不可用：请授予辅助功能权限");
                    let _ = handle.emit("keyboard-permission-needed", ());
                }

                // Load blocklist from config
                let blocklist = config.blocklist.clone();
                ws_server.set_blocklist(blocklist).await;

                let client_registry = ws_server.client_registry();

                let phone_server = match PhoneServer::start().await {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("Failed to start HTTP server: {e}");
                        return;
                    }
                };
                let http_port = phone_server.lock().await.port();

                let mut mdns = MdnsService::new(device_name_clone.clone(), ws_port);
                if let Err(e) = mdns.start() {
                    log::error!("Failed to start mDNS: {e}");
                }

                handle.manage(AppState {
                    ws_server: Arc::new(Mutex::new(ws_server)),
                    mdns: Arc::new(Mutex::new(mdns)),
                    http_port,
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
            get_privacy_enabled,
            toggle_privacy,
            get_connected_devices,
            disconnect_device,
            block_device,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}