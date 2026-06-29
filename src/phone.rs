use std::net::SocketAddr;
use std::sync::Arc;

use log::{error, info};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

const PHONE_HTML: &str = include_str!("../web/phone/dist/index.html");

pub struct PhoneServer {
    port: u16,
}

impl PhoneServer {
    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn start() -> Result<Arc<Mutex<Self>>, String> {
        let listener = TcpListener::bind("0.0.0.0:0")
            .await
            .map_err(|e| format!("http bind: {e}"))?;
        let port = listener.local_addr().map_err(|e| format!("local addr: {e}"))?.port();
        info!("Phone page HTTP server on port {port}");

        let server = Arc::new(Mutex::new(Self { port }));

        let html = PHONE_HTML.to_owned();

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        let html = html.clone();
                        tokio::spawn(handle_http(stream, addr, html));
                    }
                    Err(e) => {
                        error!("HTTP accept error: {e}");
                    }
                }
            }
        });

        Ok(server)
    }
}

async fn handle_http(mut stream: TcpStream, addr: SocketAddr, html: String) {
    let mut buf = [0u8; 4096];
    let _n = match stream.read(&mut buf).await {
        Ok(n) if n == 0 => return,
        Ok(n) => n,
        Err(e) => {
            error!("HTTP read error from {addr}: {e}");
            return;
        }
    };

    let is_get = buf.starts_with(b"GET");
    if !is_get {
        return;
    }

    let body = html.as_bytes();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );

    if let Err(e) = stream.write_all(response.as_bytes()).await {
        error!("HTTP write header error to {addr}: {e}");
        return;
    }
    if let Err(e) = stream.write_all(body).await {
        error!("HTTP write body error to {addr}: {e}");
        return;
    }
}