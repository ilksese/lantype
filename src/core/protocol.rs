use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ConnectInfo {
    pub device_name: String,
    pub ip: String,
    pub port: u16,
    pub key: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    #[serde(rename = "type")]
    Type { text: String },
    #[serde(rename = "ping")]
    Ping,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    #[serde(rename = "pong")]
    Pong,
    #[serde(rename = "connected")]
    Connected { device: String },
}

pub fn parse_client_message(data: &str) -> Result<ClientMessage, serde_json::Error> {
    serde_json::from_str(data)
}

pub fn serialize_server_message(msg: &ServerMessage) -> String {
    serde_json::to_string(msg).unwrap_or_default()
}