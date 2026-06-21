use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use log::{error, info};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

use crate::core::keyboard::KeyboardInjector;
use crate::core::protocol::{self, ClientMessage, ServerMessage};

pub struct WsServer {
    port: u16,
    keyboard: Arc<KeyboardInjector>,
    device_name: String,
    listener: Option<Arc<TcpListener>>,
}

impl WsServer {
    pub fn new(device_name: String) -> Self {
        Self {
            port: 0,
            keyboard: Arc::new(KeyboardInjector::new()),
            device_name,
            listener: None,
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn start(&mut self) -> Result<(), String> {
        let listener = TcpListener::bind("0.0.0.0:0")
            .await
            .map_err(|e| format!("bind: {e}"))?;
        self.port = listener.local_addr().map_err(|e| format!("local addr: {e}"))?.port();
        let listener = Arc::new(listener);
        self.listener = Some(listener.clone());

        let keyboard = self.keyboard.clone();
        let device_name = self.device_name.clone();

        tokio::spawn(async move {
            info!("WS server listening on port {}", listener.local_addr().unwrap().port());
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        info!("Connection from {}", addr);
                        let keyboard = keyboard.clone();
                        let device_name = device_name.clone();
                        tokio::spawn(handle_client(stream, addr, keyboard, device_name));
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
) {
    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            error!("WS handshake error from {addr}: {e}");
            return;
        }
    };

    let (mut write, mut read) = ws_stream.split();

    let connected_msg = protocol::serialize_server_message(&ServerMessage::Connected {
        device: device_name,
    });
    if let Err(e) = write.send(Message::Text(connected_msg.into())).await {
        error!("Send error: {e}");
        return;
    }

    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                match protocol::parse_client_message(&text) {
                    Ok(ClientMessage::Ping) => {
                        let pong = protocol::serialize_server_message(&ServerMessage::Pong);
                        let _ = write.send(Message::Text(pong.into())).await;
                    }
                    Ok(ClientMessage::Type { text }) => {
                        if let Err(e) = keyboard.type_text(&text).await {
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
                            if let Err(e) = keyboard.type_text(&text).await {
                                error!("Type error: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        error!("Parse error: {e}");
                    }
                }
            }
            Ok(Message::Ping(_)) => {
                let _ = write.send(Message::Pong(vec![])).await;
            }
            Ok(Message::Close(_)) => {
                info!("Client {addr} disconnected");
                break;
            }
            Err(e) => {
                error!("WS error from {addr}: {e}");
                break;
            }
            _ => {}
        }
    }

    info!("Connection closed: {addr}");
}