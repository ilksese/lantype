pub mod core;
pub mod qr;
pub mod tray;

use log::info;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

use crate::core::mdns::MdnsService;
use crate::core::ws::WsServer;
use crate::tray::PrivacyState;
use tokio::sync::Mutex;

struct AppState {
    ws_server: Arc<Mutex<WsServer>>,
    mdns: Arc<Mutex<MdnsService>>,
}

#[tauri::command]
async fn get_connection_info(
    _app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let ws = state.ws_server.lock().await;
    let port = ws.port();

    let device_name = hostname();

    let local_ip = get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
    let key = uuid::Uuid::new_v4().to_string();

    let url = format!("lantype://{device_name}@{local_ip}:{port}?key={key}");
    info!("Connection URL: {url}");

    let data_url = qr::qr_data_url(&url)?;

    let json = serde_json::json!({
        "qrDataUrl": data_url,
        "deviceName": device_name,
        "address": format!("ws://{}:{}", local_ip, port),
        "url": url,
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
    let _device_name = hostname();

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

fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "LanType".to_string())
}

fn get_local_ip() -> Option<String> {
    for iface in local_ip_address::list_afinet_netifas().ok()? {
        if !iface.0.starts_with("lo") && !iface.1.is_loopback() {
            if let std::net::IpAddr::V4(ip) = iface.1 {
                return Some(ip.to_string());
            }
        }
    }
    None
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    let device_name = hostname();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(PrivacyState {
            enabled: AtomicBool::new(true),
        })
        .setup(move |app| {
            let handle = app.handle().clone();

            let device_name_clone = device_name.clone();

            tauri::async_runtime::spawn(async move {
                let mut ws_server = WsServer::new(device_name_clone.clone());
                if let Err(e) = ws_server.start().await {
                    log::error!("Failed to start WS server: {e}");
                    return;
                }

                let port = ws_server.port();

                let mut mdns = MdnsService::new(device_name_clone.clone(), port);
                if let Err(e) = mdns.start() {
                    log::error!("Failed to start mDNS: {e}");
                }

                handle.manage(AppState {
                    ws_server: Arc::new(Mutex::new(ws_server)),
                    mdns: Arc::new(Mutex::new(mdns)),
                });
            });

            tray::setup_tray(app.handle())?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_connection_info,
            get_privacy_enabled,
            toggle_privacy,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}