use log::error;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

pub const PHONE_HTML: &str = include_str!("../web/phone/dist/index.html");

pub async fn serve_phone_page(
    mut stream: TcpStream,
    addr: std::net::SocketAddr,
    first_chunk: Vec<u8>,
    html: String,
) {
    let is_get = first_chunk.starts_with(b"GET");
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
    }
}