pub mod core;
pub mod phone;
pub mod qr;
pub mod tray;

use std::future::Future;
use std::io;
use std::net::Ipv4Addr;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};
use url::form_urlencoded;

use crate::core::config::{resolve_device_name, Config};
use crate::core::mdns::MdnsService;
use crate::core::ws::{ClientRegistry, SecurityState, WsServer};
use crate::phone::{serve_phone_page_with_health, PhoneHealth, PHONE_HTML};

const MAX_HTTP_HEADER_BYTES: usize = 16 * 1024;

pub(crate) struct AppState {
    pub(crate) ws_server: Arc<WsServer>,
    pub(crate) _mdns: Arc<Mutex<MdnsService>>,
    pub(crate) port: u16,
    pub(crate) device_name: String,
    pub(crate) config: Arc<Mutex<Config>>,
    pub(crate) client_registry: Arc<ClientRegistry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct KeyboardStatus {
    healthy: bool,
}

#[tauri::command]
async fn get_connection_info(state: State<'_, AppState>) -> Result<String, String> {
    let pairing = state.ws_server.pairing_info().await;
    let port = state.port;
    let mut ips = get_lan_ips();
    let selected_ip = get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
    if ips.is_empty() {
        ips.push(selected_ip.clone());
    } else if !ips.iter().any(|ip| ip == &selected_ip) && selected_ip != "127.0.0.1" {
        ips.push(selected_ip.clone());
        ips.sort();
    }

    let addresses = ips
        .iter()
        .map(|ip| format!("ws://{ip}:{port}"))
        .collect::<Vec<_>>();
    let selected_address = format!("ws://{selected_ip}:{port}");
    let mut query = form_urlencoded::Serializer::new(String::new());
    query.append_pair("ws", &port.to_string());
    query.append_pair("token", &pairing.token);
    // Keep pin in the QR URL for clients that only understand the legacy
    // query parameter. The long token remains the primary credential.
    query.append_pair("pin", &pairing.pairing_code);
    let full_link = format!("http://{selected_ip}:{port}/?{}", query.finish());
    let qr_data_url = crate::qr::qr_data_url(&full_link)?;

    let json = serde_json::json!({
        "qrDataUrl": qr_data_url,
        "deviceName": state.device_name,
        "address": selected_address,
        "httpUrl": full_link,
        "addresses": addresses,
        "selectedAddress": selected_address,
        "fullLink": full_link,
        "pairingCode": pairing.pairing_code,
        "pin": pairing.pairing_code,
        "createdAt": pairing.created_at,
        "expiresAt": pairing.expires_at,
        "paused": state.ws_server.is_paused(),
    });

    Ok(json.to_string())
}

#[tauri::command]
async fn get_connected_devices(state: State<'_, AppState>) -> Result<String, String> {
    let clients = state.client_registry.clients.read().await;
    serde_json::to_string(&*clients).map_err(|e| e.to_string())
}

#[tauri::command]
async fn disconnect_device(
    app: AppHandle,
    state: State<'_, AppState>,
    client_id: String,
) -> Result<(), String> {
    state.ws_server.disconnect_client(&client_id).await?;
    state.ws_server.emit_state(&app).await;
    Ok(())
}

#[tauri::command]
async fn block_device(
    app: AppHandle,
    state: State<'_, AppState>,
    client_id: String,
) -> Result<(), String> {
    let previous_blocklist = state.ws_server.blocklist().await;
    let blocklist = state.ws_server.block_client(&client_id).await?;
    let save_result = {
        let mut config = state.config.lock().await;
        config.blocklist = blocklist;
        config.save()
    };
    if let Err(error) = save_result {
        state
            .ws_server
            .set_blocklist(previous_blocklist.clone())
            .await;
        state.config.lock().await.blocklist = previous_blocklist;
        return Err(format!("Failed to save blocklist: {error}"));
    }
    state.ws_server.emit_state(&app).await;
    Ok(())
}

#[tauri::command]
async fn get_security_state(state: State<'_, AppState>) -> Result<SecurityState, String> {
    Ok(state.ws_server.security_state().await)
}

#[tauri::command]
async fn set_input_paused(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    state.ws_server.set_input_paused(enabled).await;
    state.ws_server.emit_state(&app).await;
    Ok(())
}

#[tauri::command]
async fn set_require_approval(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    let save_result = {
        let mut config = state.config.lock().await;
        let previous = config.require_approval;
        config.require_approval = enabled;
        if let Err(error) = config.save() {
            config.require_approval = previous;
            return Err(format!("Failed to save approval setting: {error}"));
        }
        Ok::<(), String>(())
    };
    save_result?;
    state.ws_server.set_require_approval(enabled).await;
    state.ws_server.emit_state(&app).await;
    Ok(())
}

#[tauri::command]
async fn approve_connection(
    app: AppHandle,
    state: State<'_, AppState>,
    client_id: String,
) -> Result<(), String> {
    state.ws_server.approve_connection(&client_id).await?;
    state.ws_server.emit_state(&app).await;
    Ok(())
}

#[tauri::command]
async fn reject_connection(
    app: AppHandle,
    state: State<'_, AppState>,
    client_id: String,
) -> Result<(), String> {
    state.ws_server.reject_connection(&client_id).await?;
    state.ws_server.emit_state(&app).await;
    Ok(())
}

#[tauri::command]
async fn set_controller(
    app: AppHandle,
    state: State<'_, AppState>,
    client_id: String,
) -> Result<(), String> {
    state.ws_server.set_controller(&client_id).await?;
    state.ws_server.emit_state(&app).await;
    Ok(())
}

#[tauri::command]
async fn disconnect_all_devices(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.ws_server.disconnect_all_devices().await;
    state.ws_server.emit_state(&app).await;
    Ok(())
}

#[tauri::command]
async fn unblock_device(
    app: AppHandle,
    state: State<'_, AppState>,
    device_id: String,
) -> Result<(), String> {
    let previous_blocklist = state.ws_server.blocklist().await;
    let blocklist = state.ws_server.unblock_device(&device_id).await?;
    let save_result = {
        let mut config = state.config.lock().await;
        config.blocklist = blocklist;
        config.save()
    };
    if let Err(error) = save_result {
        state
            .ws_server
            .set_blocklist(previous_blocklist.clone())
            .await;
        state.config.lock().await.blocklist = previous_blocklist;
        return Err(format!("Failed to save blocklist: {error}"));
    }
    state.ws_server.emit_state(&app).await;
    Ok(())
}

#[tauri::command]
async fn revoke_device(
    app: AppHandle,
    state: State<'_, AppState>,
    client_id: String,
) -> Result<(), String> {
    state.ws_server.revoke_device(&client_id).await?;
    state.ws_server.emit_state(&app).await;
    Ok(())
}

#[tauri::command]
async fn rotate_pairing(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.ws_server.rotate_pairing().await;
    state.ws_server.emit_state(&app).await;
    Ok(())
}

#[tauri::command]
async fn get_keyboard_status(state: State<'_, AppState>) -> Result<KeyboardStatus, String> {
    Ok(KeyboardStatus {
        healthy: state.ws_server.keyboard_healthy(),
    })
}

#[tauri::command]
async fn recheck_keyboard_permission(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<KeyboardStatus, String> {
    let healthy = state.ws_server.recheck_keyboard_permission().await?;
    let status = KeyboardStatus { healthy };
    state.ws_server.emit_keyboard_status(&app);
    state.ws_server.emit_state(&app).await;
    Ok(status)
}

#[tauri::command]
fn open_keyboard_permission_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .status()
            .map_err(|error| error.to_string())?
            .success()
            .then_some(())
            .ok_or_else(|| "无法打开辅助功能设置".to_string())
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", "ms-settings:easeofaccess-keyboard"])
            .status()
            .map_err(|error| error.to_string())?
            .success()
            .then_some(())
            .ok_or_else(|| "无法打开键盘权限设置".to_string())
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg("settings://accessibility")
            .status()
            .map_err(|error| error.to_string())?
            .success()
            .then_some(())
            .ok_or_else(|| "无法打开辅助功能设置".to_string())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err("当前平台不支持自动打开辅助功能设置".to_string())
    }
}

#[tauri::command]
async fn set_discovery_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    let old_enabled = state.ws_server.discovery_enabled();
    {
        let mut mdns = state._mdns.lock().await;
        mdns.set_discovery_enabled(enabled, get_lan_ips())?;
    }
    state.ws_server.set_discovery_enabled(enabled);

    let save_result = {
        let mut config = state.config.lock().await;
        config.discovery_enabled = enabled;
        config.save()
    };
    if let Err(error) = save_result {
        {
            let mut mdns = state._mdns.lock().await;
            let _ = mdns.set_discovery_enabled(old_enabled, get_lan_ips());
        }
        state.ws_server.set_discovery_enabled(old_enabled);
        return Err(format!("Failed to save discovery setting: {error}"));
    }

    state.ws_server.emit_state(&app).await;
    Ok(())
}

async fn bind_listener_from(start_port: u16) -> io::Result<(TcpListener, u16)> {
    bind_port_with(start_port, |port| async move { TcpListener::bind(("0.0.0.0", port)).await })
        .await
}

async fn bind_port_with<T, F, Fut>(start_port: u16, mut try_bind: F) -> io::Result<(T, u16)>
where
    F: FnMut(u16) -> Fut,
    Fut: Future<Output = io::Result<T>>,
{
    for port in start_port..=u16::MAX {
        match try_bind(port).await {
            Ok(value) => return Ok((value, port)),
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AddrNotAvailable,
        "no free port found",
    ))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    let config = Config::load();
    let device_name = resolve_device_name(&config);
    let config = Arc::new(Mutex::new(config));

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            let device_name_clone = device_name.clone();
            let config = config.clone();

            tauri::async_runtime::spawn(async move {
                let config_snapshot = config.lock().await.clone();
                let listen_port = config_snapshot.listen_port();
                let (listener, actual_port) = match bind_listener_from(listen_port).await {
                    Ok(result) => result,
                    Err(error) => {
                        log::error!("Failed to bind port range starting at {listen_port}: {error}");
                        return;
                    }
                };
                log::info!("Server listening on port {actual_port}");

                let ws_server = Arc::new(WsServer::with_options(
                    device_name_clone.clone(),
                    actual_port,
                    config_snapshot.require_approval,
                    config_snapshot.discovery_enabled,
                ));
                ws_server.set_blocklist(config_snapshot.blocklist).await;

                if !ws_server.keyboard_healthy() {
                    log::warn!("键盘输入不可用：请授予辅助功能权限");
                    let _ = handle.emit("keyboard-permission-needed", ());
                }
                ws_server.emit_keyboard_status(&handle);

                let client_registry = ws_server.client_registry();
                let listener = Arc::new(listener);
                let ws_server_ref = ws_server.clone();
                let handle_ref = handle.clone();
                let html = PHONE_HTML.to_owned();

                tokio::spawn(async move {
                    loop {
                        let (mut stream, addr) = match listener.accept().await {
                            Ok(value) => value,
                            Err(error) => {
                                log::error!("Accept error: {error}");
                                continue;
                            }
                        };

                        let ws = ws_server_ref.clone();
                        let app_handle = handle_ref.clone();
                        let html = html.clone();

                        tokio::spawn(async move {
                            let buffer = match timeout(
                                Duration::from_secs(5),
                                read_request_head(&mut stream),
                            )
                            .await
                            {
                                Ok(Ok(buffer)) if buffer.is_empty() => return,
                                Ok(Ok(buffer)) => buffer,
                                Ok(Err(error)) => {
                                    log::error!("Read error from {addr}: {error}");
                                    return;
                                }
                                Err(_) => return,
                            };

                            let is_websocket = buffer
                                .windows(b"Upgrade: websocket".len())
                                .any(|window| window.eq_ignore_ascii_case(b"Upgrade: websocket"));
                            if is_websocket {
                                ws.accept_connection(stream, addr, buffer, app_handle).await;
                            } else {
                                let health = PhoneHealth {
                                    ok: true,
                                    device: ws.device_name().to_string(),
                                    paused: ws.is_paused(),
                                    requires_pairing: ws.requires_pairing(),
                                    port: Some(ws.port()),
                                };
                                serve_phone_page_with_health(
                                    stream,
                                    addr,
                                    buffer,
                                    html,
                                    health,
                                )
                                .await;
                            }
                        });
                    }
                });

                let mut mdns = MdnsService::new(device_name_clone.clone(), actual_port);
                if let Err(error) = mdns.set_discovery_enabled(
                    config_snapshot.discovery_enabled,
                    get_lan_ips(),
                ) {
                    log::error!("Failed to start mDNS: {error}");
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
            get_security_state,
            set_input_paused,
            set_require_approval,
            approve_connection,
            reject_connection,
            set_controller,
            disconnect_all_devices,
            unblock_device,
            revoke_device,
            rotate_pairing,
            get_keyboard_status,
            recheck_keyboard_permission,
            open_keyboard_permission_settings,
            set_discovery_enabled,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn get_local_ip() -> Option<String> {
    if let Ok(interface) = default_net::get_default_interface() {
        if let Some(ip) = interface.ipv4.first().map(|network| network.addr) {
            if is_lan_ipv4(&ip) {
                return Some(ip.to_string());
            }
        }
    }
    get_lan_ips().into_iter().next()
}

async fn read_request_head(stream: &mut tokio::net::TcpStream) -> std::io::Result<Vec<u8>> {
    let mut request = Vec::with_capacity(4096);
    while request.len() < MAX_HTTP_HEADER_BYTES {
        let remaining = MAX_HTTP_HEADER_BYTES - request.len();
        let mut chunk = [0_u8; 4096];
        let read_len = remaining.min(chunk.len());
        let size = stream.read(&mut chunk[..read_len]).await?;
        if size == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..size]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    Ok(request)
}

fn is_lan_ipv4(ip: &Ipv4Addr) -> bool {
    !ip.is_loopback() && !ip.is_link_local() && is_private(ip)
}

fn is_private(ip: &Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 10
        || (octets[0] == 172 && (16..=31).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 168)
}

fn get_lan_ips() -> Vec<String> {
    let mut ips = default_net::get_interfaces()
        .into_iter()
        .flat_map(|interface| interface.ipv4.into_iter().map(|network| network.addr))
        .filter(is_lan_ipv4)
        .map(|ip| ip.to_string())
        .collect::<Vec<_>>();
    ips.sort();
    ips.dedup();
    ips
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bind_port_with_skips_ports_in_use() {
        let seen = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let calls = seen.clone();

        let result = bind_port_with(2777, move |port| {
            let calls = calls.clone();
            async move {
                calls.lock().await.push(port);
                if port < 2779 {
                    Err(io::Error::new(io::ErrorKind::AddrInUse, "occupied"))
                } else {
                    Ok(port)
                }
            }
        })
        .await
        .unwrap();

        assert_eq!(*seen.lock().await, vec![2777, 2778, 2779]);
        assert_eq!(result, (2779, 2779));
    }
}
