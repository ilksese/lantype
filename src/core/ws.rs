use std::borrow::Cow;
use std::collections::HashMap;
use std::io::IoSlice;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::{watch, RwLock};
use tokio::time::{Duration, Instant};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
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
    client_registry: Arc<ClientRegistry>,
    blocklist: Arc<RwLock<Vec<BlockEntry>>>,
    pin: RwLock<String>,
}

impl WsServer {
    pub fn new(device_name: String, port: u16) -> Self {
        Self {
            port,
            keyboard: Arc::new(KeyboardInjector::new()),
            device_name,
            client_registry: Arc::new(ClientRegistry::new()),
            blocklist: Arc::new(RwLock::new(Vec::new())),
            pin: RwLock::new(String::new()),
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn keyboard_healthy(&self) -> bool {
        self.keyboard.is_healthy()
    }

    pub fn client_registry(&self) -> Arc<ClientRegistry> {
        self.client_registry.clone()
    }

    pub async fn blocklist(&self) -> Vec<BlockEntry> {
        self.blocklist.read().await.clone()
    }

    pub async fn set_blocklist(&mut self, blocklist: Vec<BlockEntry>) {
        *self.blocklist.write().await = blocklist;
    }

    /// Rotate the pairing PIN. Every subsequent new connection must present the
    /// new PIN in its handshake URI (`?pin=...`) or it is rejected.
    pub async fn set_pin(&self, pin: String) {
        *self.pin.write().await = pin;
    }

    pub async fn accept_connection(
        &self,
        stream: TcpStream,
        addr: SocketAddr,
        first_chunk: Vec<u8>,
        app_handle: AppHandle,
    ) {
        // Grab the PIN presented in the WebSocket handshake before the chunk is
        // consumed by the upgrade.
        let presented_pin = request_pin(&first_chunk);

        let prepend = PrependStream::new(stream, first_chunk);
        let ws_stream = match accept_async(prepend).await {
            Ok(ws) => ws,
            Err(e) => {
                error!("WS handshake error from {addr}: {e}");
                return;
            }
        };

        // Pairing gate: without a valid one-time PIN a stranger can neither type
        // nor stay connected, even if they discover the port (e.g. via mDNS).
        let expected_pin = self.pin.read().await.clone();
        if expected_pin.is_empty() || presented_pin.as_deref() != Some(expected_pin.as_str()) {
            info!("Rejected connection from {addr}: missing or invalid pairing pin");
            let (mut write, _read) = ws_stream.split();
            let err = protocol::serialize_server_message(&ServerMessage::Error {
                message: "配对码无效，请重新扫码后连接".into(),
            });
            let _ = write.send(Message::Text(err.into())).await;
            let _ = write
                .send(Message::Close(Some(CloseFrame {
                    code: CloseCode::Library(1008),
                    reason: Cow::Borrowed("invalid pin"),
                })))
                .await;
            return;
        }

        let keyboard = self.keyboard.clone();
        let device_name = self.device_name.clone();
        let client_registry = self.client_registry.clone();
        let blocklist = self.blocklist.clone();

        handle_ws_client(ws_stream, addr, keyboard, device_name, client_registry, blocklist, app_handle).await;
    }
}

async fn handle_ws_client<S>(
    ws_stream: tokio_tungstenite::WebSocketStream<S>,
    addr: SocketAddr,
    keyboard: Arc<KeyboardInjector>,
    device_name: String,
    client_registry: Arc<ClientRegistry>,
    blocklist: Arc<RwLock<Vec<BlockEntry>>>,
    app_handle: AppHandle,
) where
    S: AsyncRead + AsyncWrite + Unpin,
{
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
            let _ = write
                .send(Message::Close(Some(CloseFrame {
                    code: CloseCode::Library(1008),
                    reason: Cow::Borrowed("hello timeout"),
                })))
                .await;
            return;
        }
    };

    let client_ip = addr.ip().to_string();

    // Check blocklist (by IP, which cannot be spoofed by the client)
    let is_blocked = {
        let bl = blocklist.read().await;
        bl.iter().any(|b| b.ip == client_ip)
    };
    if is_blocked {
        info!("Rejected blocked device {sender_name} from {addr}");
        let _ = write
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::Library(1008),
                reason: Cow::Borrowed("blocked"),
            })))
            .await;
        return;
    }

    // Create shutdown channel for this client
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    // Register in registry
    {
        let mut clients = client_registry.clients.write().await;
        clients.push(ClientInfo {
            id: client_id.clone(),
            device_name: sender_name.clone(),
            ip: client_ip.clone(),
        });
    }
    {
        let mut txs = client_registry.shutdown_txs.write().await;
        txs.insert(client_id.clone(), shutdown_tx);
    }

    // Emit clients-changed event
    {
        let clients = client_registry.clients.read().await;
        let payload =
            serde_json::to_value(&*clients).unwrap_or(serde_json::Value::Array(vec![]));
        let _ = app_handle.emit("clients-changed", payload);
    }

    info!("Client {sender_name} ({addr}) registered as {client_id}");

    // Main message loop
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    let mut last_seen = Instant::now();

    loop {
        tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        last_seen = Instant::now();
                        match protocol::parse_client_message(&text) {
                            Ok(ClientMessage::Ping) => {
                                let pong = protocol::serialize_server_message(&ServerMessage::Pong);
                                let _ = write.send(Message::Text(pong.into())).await;
                            }
                            Ok(ClientMessage::Type { text }) => {
                                if !keyboard.is_healthy() {
                                    let err_msg = protocol::serialize_server_message(
                                        &ServerMessage::Error {
                                            message: "辅助功能权限未授予，请在桌面端授权后重试".into(),
                                        },
                                    );
                                    let _ = write.send(Message::Text(err_msg.into())).await;
                                    continue;
                                }
                                if let Err(e) = keyboard.type_text(text).await {
                                    error!("Type error: {e}");
                                }
                            }
                            Ok(ClientMessage::Diff { backspace, text }) => {
                                if !keyboard.is_healthy() {
                                    let err_msg = protocol::serialize_server_message(
                                        &ServerMessage::Error {
                                            message: "辅助功能权限未授予，请在桌面端授权后重试".into(),
                                        },
                                    );
                                    let _ = write.send(Message::Text(err_msg.into())).await;
                                    continue;
                                }
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
                            Ok(ClientMessage::Keys { modifiers, key }) => {
                                if !keyboard.is_healthy() {
                                    let err_msg = protocol::serialize_server_message(
                                        &ServerMessage::Error {
                                            message: "辅助功能权限未授予，请在桌面端授权后重试".into(),
                                        },
                                    );
                                    let _ = write.send(Message::Text(err_msg.into())).await;
                                    continue;
                                }
                                if let Err(e) = keyboard.press_chord(modifiers, key).await {
                                    error!("Press chord error: {e}");
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
                        last_seen = Instant::now();
                        let _ = write.send(Message::Pong(vec![])).await;
                    }
                    Some(Ok(Message::Pong(_))) => {
                        last_seen = Instant::now();
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
            _ = heartbeat.tick() => {
                if last_seen.elapsed() > Duration::from_secs(45) {
                    info!("Client {sender_name} ({addr}) timed out");
                    let _ = write.send(Message::Close(None)).await;
                    break;
                }
                if let Err(e) = write.send(Message::Ping(vec![])).await {
                    error!("Heartbeat error to {addr}: {e}");
                    break;
                }
            }
        }
    }

    // Deregister
    {
        let mut clients = client_registry.clients.write().await;
        clients.retain(|c| c.id != client_id);
    }
    {
        let mut txs = client_registry.shutdown_txs.write().await;
        txs.remove(&client_id);
    }

    // Emit updated list
    {
        let clients = client_registry.clients.read().await;
        let payload =
            serde_json::to_value(&*clients).unwrap_or(serde_json::Value::Array(vec![]));
        let _ = app_handle.emit("clients-changed", payload);
    }

    info!("Connection closed: {addr}");
}

/// Generate a fresh 6-digit pairing PIN for the current session.
pub fn generate_pin() -> String {
    let n = Uuid::new_v4().as_u128() % 1_000_000;
    format!("{n:06}")
}

/// Extract the `pin` query parameter from the initial HTTP/WebSocket request
/// line (e.g. `GET /?ws=1234&pin=482913 HTTP/1.1`).
fn request_pin(first_chunk: &[u8]) -> Option<String> {
    let request = std::str::from_utf8(first_chunk).ok()?;
    let line = request.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    if method != "GET" {
        return None;
    }
    let target = parts.next()?;
    let query = target.split_once('?')?.1;
    for pair in query.split('&') {
        let Some((k, v)) = pair.split_once('=') else { continue };
        if k == "pin" {
            return Some(v.to_string());
        }
    }
    None
}

/// Wraps a `TcpStream` and prepends already-read bytes so that
/// downstream consumers (e.g. tokio-tungstenite) see the complete
/// initial request as if it had not been consumed.
struct PrependStream {
    stream: TcpStream,
    buf: Vec<u8>,
    pos: usize,
}

impl PrependStream {
    fn new(stream: TcpStream, buf: Vec<u8>) -> Self {
        Self { stream, buf, pos: 0 }
    }
}

impl AsyncRead for PrependStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        // Serve from the prepend buffer first
        if self.pos < self.buf.len() {
            let avail = &self.buf[self.pos..];
            let len = std::cmp::min(avail.len(), out.remaining());
            out.put_slice(&avail[..len]);
            self.pos += len;
            return Poll::Ready(Ok(()));
        }
        // Then delegate to the underlying stream
        Pin::new(&mut self.stream).poll_read(cx, out)
    }
}

impl AsyncWrite for PrependStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.stream.is_write_vectored()
    }
}

impl Unpin for PrependStream {}
