use log::error;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

pub const PHONE_HTML: &str = include_str!("../../web/phone/dist/index.html");
const PHONE_JS: &str = include_str!("../../web/phone/dist/assets/app.js");
const PHONE_CSS: &str = include_str!("../../web/phone/dist/assets/style.css");

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneHealth {
    pub ok: bool,
    pub device: String,
    pub paused: bool,
    pub requires_pairing: bool,
    pub port: Option<u16>,
}

pub async fn serve_phone_page_with_health(
    mut stream: TcpStream,
    addr: std::net::SocketAddr,
    first_chunk: Vec<u8>,
    html: String,
    health: PhoneHealth,
) {
    let is_get = first_chunk.starts_with(b"GET");
    if !is_get {
        return;
    }

    let path = request_path(&first_chunk);
    let (body, content_type) = match path.as_deref() {
        Some("/health") => (
            serde_json::to_string(&health).unwrap_or_else(|_| "{\"ok\":false}".to_string()),
            "application/json; charset=utf-8",
        ),
        Some("/assets/app.js") => (
            PHONE_JS.to_string(),
            "application/javascript; charset=utf-8",
        ),
        Some("/assets/style.css") => (PHONE_CSS.to_string(), "text/css; charset=utf-8"),
        _ => (html, "text/html; charset=utf-8"),
    };

    let body = body.as_bytes();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len(),
    );

    if let Err(e) = stream.write_all(response.as_bytes()).await {
        error!("HTTP write header error to {addr}: {e}");
        return;
    }
    if let Err(e) = stream.write_all(body).await {
        error!("HTTP write body error to {addr}: {e}");
    }
}

fn request_path(first_chunk: &[u8]) -> Option<String> {
    let request = std::str::from_utf8(first_chunk).ok()?;
    let line = request.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    if method != "GET" {
        return None;
    }
    let path = parts.next()?;
    Some(path.split('?').next().unwrap_or(path).to_string())
}
