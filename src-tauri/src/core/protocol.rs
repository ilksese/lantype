use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ConnectInfo {
    pub device_name: String,
    pub ip: String,
    pub port: u16,
    pub key: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    #[serde(rename = "type")]
    Type {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sequence: Option<u64>,
    },
    #[serde(rename = "diff")]
    Diff {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sequence: Option<u64>,
        backspace: u32,
        text: String,
    },
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "hello")]
    Hello {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        protocol_version: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        device_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pairing_token: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_token: Option<String>,
    },
    #[serde(rename = "keys")]
    Keys {
        modifiers: Vec<String>,
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sequence: Option<u64>,
    },
}

impl ClientMessage {
    pub fn request_meta(&self) -> (Option<&str>, Option<u64>) {
        match self {
            Self::Type {
                request_id,
                sequence,
                ..
            }
            | Self::Diff {
                request_id,
                sequence,
                ..
            }
            | Self::Keys {
                request_id,
                sequence,
                ..
            } => (request_id.as_deref(), *sequence),
            _ => (None, None),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    #[serde(rename = "pong")]
    Pong,
    #[serde(rename = "connected")]
    Connected { device: String, client_id: String },
    #[serde(rename = "pending")]
    Pending { client_id: String, message: String },
    #[serde(rename = "paused")]
    Paused { message: String },
    #[serde(rename = "ready")]
    Ready {
        device: String,
        client_id: String,
        session_id: String,
    },
    #[serde(rename = "ack")]
    Ack {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sequence: Option<u64>,
    },
    #[serde(rename = "error")]
    Error {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        #[serde(default)]
        code: String,
        message: String,
        #[serde(default)]
        retryable: bool,
    },
}

pub fn parse_client_message(data: &str) -> Result<ClientMessage, serde_json::Error> {
    serde_json::from_str(data)
}

pub fn serialize_server_message(msg: &ServerMessage) -> String {
    serde_json::to_string(msg).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_message_round_trip_preserves_optional_fields() {
        let message = ClientMessage::Diff {
            request_id: Some("request-7".to_string()),
            sequence: Some(12),
            backspace: 1,
            text: "你好".to_string(),
        };

        let encoded = serde_json::to_string(&message).unwrap();
        let decoded = parse_client_message(&encoded).unwrap();

        assert_eq!(decoded, message);
    }

    #[test]
    fn old_client_message_without_optional_fields_still_deserializes() {
        let decoded = parse_client_message(r#"{"type":"type","text":"legacy"}"#).unwrap();

        assert_eq!(
            decoded,
            ClientMessage::Type {
                text: "legacy".to_string(),
                request_id: None,
                sequence: None,
            }
        );
    }

    #[test]
    fn server_messages_round_trip() {
        let messages = [
            ServerMessage::Ready {
                device: "桌面".to_string(),
                client_id: "client-1".to_string(),
                session_id: "session-1".to_string(),
            },
            ServerMessage::Ack {
                request_id: "request-7".to_string(),
                sequence: Some(12),
            },
            ServerMessage::Pending {
                client_id: "client-2".to_string(),
                message: "等待批准".to_string(),
            },
            ServerMessage::Paused {
                message: "已暂停".to_string(),
            },
            ServerMessage::Error {
                request_id: Some("request-8".to_string()),
                code: "permission_required".to_string(),
                message: "请授予辅助功能权限".to_string(),
                retryable: true,
            },
        ];

        for message in messages {
            let encoded = serialize_server_message(&message);
            let decoded: ServerMessage = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, message);
        }
    }
}
