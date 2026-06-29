use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use crate::core::config::BlockEntry;
use crate::core::keyboard::KeyboardInjector;
use crate::core::protocol::{self, ClientMessage, ServerMessage};

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

pub struct WsServer {
    port: u16,
    keyboard: Arc<KeyboardInjector>,
    device_name: String,
    listener: Option<Arc<TcpListener>>,
    client_registry: Arc<ClientRegistry>,
    blocklist: Arc<RwLock<Vec<BlockEntry>>>,
}

impl WsServer {
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

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn client_registry(&self) -> Arc<ClientRegistry> {
        self.client_registry.clone()
    }

    pub fn blocklist(&self) -> Vec<BlockEntry> {
        self.blocklist.read().unwrap().clone()
    }

    pub fn set_blocklist(&mut self, blocklist: Vec<BlockEntry>) {
        *self.blocklist.write().unwrap() = blocklist;
    }

    /// Start the WebSocket server.
    ///
    /// If `port_override` is `Some(port)`, try binding to that port first;
    /// on failure, fall back to a random port with a warning.
    /// If `None`, bind to a random port (existing behaviour).
    pub async fn start(
        &mut self,
        port_override: Option<u16>,
        app_handle: AppHandle,
    ) -> Result<(), String> {
        let listener = match port_override {
            Some(port) => {
                match TcpListener::bind(("0.0.0.0", port)).await {
                    Ok(l) => {
                        log::info!("WS server bound to configured port {port}");
                        l
                    }
                    Err(e) => {
                        log::warn!("Failed to bind to configured port {port} ({e}), falling back to random port");
                        TcpListener::bind("0.0.0.0:0")
                            .await
                            .map_err(|e| format!("bind fallback: {e}"))?
                    }
                }
            }
            None => {
                TcpListener::bind("0.0.0.0:0")
                    .await
                    .map_err(|e| format!("bind: {e}"))?
            }
        };
        self.port = listener.local_addr().map_err(|e| format!("local addr: {e}"))?.port();
        let listener = Arc::new(listener);
        self.listener = Some(listener.clone());

        let keyboard = self.keyboard.clone();
        let device_name = self.device_name.clone();
        let client_registry = self.client_registry.clone();
        let blocklist = self.blocklist.clone();

        tokio::spawn(async move {
            info!("WS server listening on port {}", listener.local_addr().unwrap().port());
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        info!("Connection from {}", addr);
                        let keyboard = keyboard.clone();
                        let device_name = device_name.clone();
                        let client_registry = client_registry.clone();
                        let blocklist = blocklist.clone();
                        let app_handle = app_handle.clone();
                        tokio::spawn(handle_client(
                            stream,
                            addr,
                            keyboard,
                            device_name,
                            client_registry,
                            blocklist,
                            app_handle,
                        ));
                    }
                    Err(e) => {
                        error!("Accept error: {e}");
                    }
                }
            }
        });

        Ok(())
    }
}

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
    })
    .await;

    let sender_name = match hello_result {
        Ok(Ok(name)) => name,
        Ok(Err(e)) => {
            info!("Client {addr} rejected during hello: {e}");
            return;
        }
        Err(_) => {
            info!("Client {addr} timed out waiting for hello");
            let _ = write.send(Message::Close(None)).await;
            return;
        }
    };

    let client_ip = addr.ip().to_string();

    // Check blocklist
    let is_blocked = {
        let bl = blocklist.read().unwrap();
        bl.iter().any(|b| b.ip == client_ip && b.device_name == sender_name)
    };
    if is_blocked {
        info!("Rejected blocked device {sender_name} from {addr}");
        let _ = write.send(Message::Close(None)).await;
        return;
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
        let payload =
            serde_json::to_value(&*clients).unwrap_or(serde_json::Value::Array(vec![]));
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
        let payload =
            serde_json::to_value(&*clients).unwrap_or(serde_json::Value::Array(vec![]));
        let _ = app_handle.emit("clients-changed", payload);
    }

    info!("Connection closed: {addr}");
}
