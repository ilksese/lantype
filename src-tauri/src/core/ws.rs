use std::borrow::Cow;
use std::collections::HashMap;
use std::io::IoSlice;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::{Sink, SinkExt, StreamExt};
use log::{error, info};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch, Mutex, RwLock};
use tokio::time::{sleep, timeout, Duration, Instant};
use tokio_tungstenite::accept_async_with_config;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, WebSocketConfig};
use tokio_tungstenite::tungstenite::Message;
use url::form_urlencoded;
use uuid::Uuid;

use crate::core::config::BlockEntry;
use crate::core::keyboard::KeyboardInjector;
use crate::core::protocol::{self, ClientMessage, ServerMessage};

pub const MAX_TEXT_BYTES: usize = 64 * 1024;

const MAX_MESSAGE_BYTES: usize = 128 * 1024;
const MAX_DEVICE_NAME_CHARS: usize = 80;
const MAX_DEVICE_ID_CHARS: usize = 128;
const MAX_REQUEST_ID_CHARS: usize = 128;
const MAX_KEY_CHARS: usize = 32;
const MAX_MODIFIERS: usize = 8;
const MAX_BACKSPACE: u32 = 65_536;
const MIN_INPUT_INTERVAL: Duration = Duration::from_millis(25);
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(45);
const WS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const PAIRING_TTL_SECS: u64 = 24 * 60 * 60;
const PAIRING_FAILURE_WINDOW: Duration = Duration::from_secs(60);
const MAX_PAIRING_FAILURES: u32 = 5;
const MAX_PAIRING_ATTEMPT_IPS: usize = 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub id: String,
    pub device_name: String,
    pub ip: String,
    pub device_id: String,
    pub status: String,
    pub authorized: bool,
    pub controller: bool,
    pub connected_at: u64,
    #[serde(skip)]
    protocol_version: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizedDeviceInfo {
    pub device_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityState {
    pub paused: bool,
    pub require_approval: bool,
    pub discovery_enabled: bool,
    pub controller_id: Option<String>,
    pub pending: Vec<ClientInfo>,
    pub clients: Vec<ClientInfo>,
    pub blocklist: Vec<BlockEntry>,
    pub keyboard_healthy: bool,
    pub authorized_devices: Vec<AuthorizedDeviceInfo>,
}

#[derive(Debug, Clone)]
pub struct PairingInfo {
    pub token: String,
    pub pairing_code: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub rotated: bool,
}

#[derive(Debug, Clone)]
struct PairingState {
    token: String,
    pairing_code: String,
    created_at: u64,
    expires_at: u64,
    generation: u64,
}

#[derive(Debug, Clone)]
struct Authorization {
    session_token: String,
    display_name: String,
}

#[derive(Debug)]
struct PairingAttempt {
    started_at: Instant,
    failures: u32,
}

#[derive(Debug, Clone, Copy)]
enum ClientCommand {
    Approve,
    Reject,
    Disconnect,
    Pause,
    Block,
}

#[derive(Debug, Clone)]
pub struct BlockCandidate {
    pub blocklist: Vec<BlockEntry>,
    pub device_id: String,
}

#[derive(Debug, Clone)]
pub struct UnblockCandidate {
    pub blocklist: Vec<BlockEntry>,
    pub device_ids: Vec<String>,
}

#[derive(Default)]
pub struct ClientRegistry {
    pub clients: RwLock<Vec<ClientInfo>>,
    pub shutdown_txs: RwLock<HashMap<String, watch::Sender<bool>>>,
    command_txs: RwLock<HashMap<String, mpsc::UnboundedSender<ClientCommand>>>,
}

pub struct WsServer {
    port: u16,
    keyboard: Arc<KeyboardInjector>,
    device_name: String,
    client_registry: Arc<ClientRegistry>,
    blocklist: RwLock<Vec<BlockEntry>>,
    pairing: RwLock<PairingState>,
    authorized_devices: RwLock<HashMap<String, Authorization>>,
    session_devices: RwLock<HashMap<String, String>>,
    device_ips: RwLock<HashMap<String, String>>,
    pairing_attempts: Mutex<HashMap<String, PairingAttempt>>,
    paused: AtomicBool,
    require_approval: AtomicBool,
    discovery_enabled: AtomicBool,
    controller_id: RwLock<Option<String>>,
    input_lock: Mutex<()>,
    lifecycle_lock: Mutex<()>,
}

impl WsServer {
    pub fn with_options(
        device_name: String,
        port: u16,
        require_approval: bool,
        discovery_enabled: bool,
    ) -> Self {
        Self {
            port,
            keyboard: Arc::new(KeyboardInjector::new()),
            device_name,
            client_registry: Arc::new(ClientRegistry::default()),
            blocklist: RwLock::new(Vec::new()),
            pairing: RwLock::new(new_pairing_state(1)),
            authorized_devices: RwLock::new(HashMap::new()),
            session_devices: RwLock::new(HashMap::new()),
            device_ips: RwLock::new(HashMap::new()),
            pairing_attempts: Mutex::new(HashMap::new()),
            paused: AtomicBool::new(false),
            require_approval: AtomicBool::new(require_approval),
            discovery_enabled: AtomicBool::new(discovery_enabled),
            controller_id: RwLock::new(None),
            input_lock: Mutex::new(()),
            lifecycle_lock: Mutex::new(()),
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn keyboard_healthy(&self) -> bool {
        self.keyboard.is_healthy()
    }

    pub async fn recheck_keyboard_permission(&self) -> Result<bool, String> {
        match self.keyboard.recheck().await {
            Ok(()) => Ok(true),
            Err(error) => {
                debug_assert!(!self.keyboard.is_healthy());
                Err(error)
            }
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }

    pub fn require_approval(&self) -> bool {
        self.require_approval.load(Ordering::Acquire)
    }

    pub fn discovery_enabled(&self) -> bool {
        self.discovery_enabled.load(Ordering::Acquire)
    }

    pub fn client_registry(&self) -> Arc<ClientRegistry> {
        self.client_registry.clone()
    }

    pub async fn blocklist(&self) -> Vec<BlockEntry> {
        self.blocklist.read().await.clone()
    }

    pub async fn set_blocklist(&self, blocklist: Vec<BlockEntry>) {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        *self.blocklist.write().await = deduplicate_blocklist(blocklist);
    }

    pub async fn pairing_info(&self) -> PairingInfo {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        if self.pairing.read().await.expires_at <= now_secs() {
            return self.rotate_pairing_inner().await;
        }
        let pairing = self.pairing.read().await.clone();
        pairing_info_from_state(&pairing, false)
    }

    pub async fn rotate_pairing(&self) -> PairingInfo {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.rotate_pairing_inner().await
    }

    async fn rotate_pairing_inner(&self) -> PairingInfo {
        let generation = self.pairing.read().await.generation.wrapping_add(1);
        let state = new_pairing_state(generation);
        let info = pairing_info_from_state(&state, true);
        *self.pairing.write().await = state;
        self.pairing_attempts.lock().await.clear();
        info
    }

    pub fn set_discovery_enabled(&self, enabled: bool) {
        self.discovery_enabled.store(enabled, Ordering::Release);
    }

    pub async fn set_require_approval(&self, enabled: bool) {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.require_approval.store(enabled, Ordering::Release);
        if !enabled {
            let pending_ids = {
                let clients = self.client_registry.clients.read().await;
                clients
                    .iter()
                    .filter(|client| client.status == "pending" && client.protocol_version == 2)
                    .map(|client| client.id.clone())
                    .collect::<Vec<_>>()
            };
            self.send_command_to_clients(pending_ids, ClientCommand::Approve)
                .await;
        }
    }

    pub async fn set_input_paused(&self, enabled: bool) -> bool {
        let _input_guard = self.input_lock.lock().await;
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        if self.paused.swap(enabled, Ordering::AcqRel) == enabled {
            return false;
        }

        self.rotate_pairing_inner().await;
        if enabled {
            let ids = {
                let clients = self.client_registry.clients.read().await;
                clients
                    .iter()
                    .map(|client| client.id.clone())
                    .collect::<Vec<_>>()
            };
            self.send_command_to_clients(ids, ClientCommand::Pause)
                .await;

            let shutdowns = self
                .client_registry
                .shutdown_txs
                .read()
                .await
                .values()
                .cloned()
                .collect::<Vec<_>>();
            for shutdown in shutdowns {
                let _ = shutdown.send(true);
            }

            self.client_registry.clients.write().await.clear();
            self.client_registry.shutdown_txs.write().await.clear();
            self.client_registry.command_txs.write().await.clear();
            *self.controller_id.write().await = None;
        }
        true
    }

    pub async fn disconnect_client(&self, client_id: &str) -> Result<(), String> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.disconnect_client_inner(client_id, ClientCommand::Disconnect)
            .await
    }

    async fn disconnect_client_inner(
        &self,
        client_id: &str,
        command: ClientCommand,
    ) -> Result<(), String> {
        let sender = self
            .client_registry
            .command_txs
            .read()
            .await
            .get(client_id)
            .cloned();
        if let Some(sender) = sender {
            sender
                .send(command)
                .map_err(|_| "Client is already disconnected".to_string())?;
            return Ok(());
        }

        let shutdown = self
            .client_registry
            .shutdown_txs
            .read()
            .await
            .get(client_id)
            .cloned();
        match shutdown {
            Some(shutdown) => {
                shutdown
                    .send(true)
                    .map_err(|_| "Client is already disconnected".to_string())?;
                Ok(())
            }
            None => Err("Client not found".to_string()),
        }
    }

    pub async fn disconnect_all_devices(&self) {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let ids = {
            let clients = self.client_registry.clients.read().await;
            clients
                .iter()
                .map(|client| client.id.clone())
                .collect::<Vec<_>>()
        };
        self.send_command_to_clients(ids, ClientCommand::Disconnect)
            .await;
        let shutdowns = self
            .client_registry
            .shutdown_txs
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for shutdown in shutdowns {
            let _ = shutdown.send(true);
        }
    }

    pub async fn approve_connection(&self, client_id: &str) -> Result<(), String> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.send_to_pending(client_id, ClientCommand::Approve)
            .await
    }

    pub async fn reject_connection(&self, client_id: &str) -> Result<(), String> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.send_to_pending(client_id, ClientCommand::Reject).await
    }

    pub async fn set_controller(&self, client_id: &str) -> Result<(), String> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let authorized = {
            let clients = self.client_registry.clients.read().await;
            clients
                .iter()
                .any(|client| client.id == client_id && client.authorized)
        };
        if !authorized {
            return Err("Client is not an authorized connection".to_string());
        }

        *self.controller_id.write().await = Some(client_id.to_string());
        self.refresh_controller_flags().await;
        Ok(())
    }

    pub async fn revoke_device(&self, identifier: &str) -> Result<(), String> {
        let _input_guard = self.input_lock.lock().await;
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let device_id = {
            let clients = self.client_registry.clients.read().await;
            clients
                .iter()
                .find(|client| client.id == identifier)
                .map(|client| client.device_id.clone())
                .or_else(|| {
                    clients
                        .iter()
                        .any(|client| client.device_id == identifier)
                        .then(|| identifier.to_string())
                })
        };
        let device_id = match device_id {
            Some(device_id) => device_id,
            None if self
                .authorized_devices
                .read()
                .await
                .contains_key(identifier) =>
            {
                identifier.to_string()
            }
            None => return Err("Client or device not found".to_string()),
        };

        self.revoke_device_id_inner(&device_id).await;
        self.disconnect_device_connections_inner(&device_id, ClientCommand::Disconnect)
            .await;
        Ok(())
    }

    pub async fn revoke_device_id(&self, device_id: &str) {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.revoke_device_id_inner(device_id).await;
    }

    pub async fn block_candidate(&self, client_id: &str) -> Result<BlockCandidate, String> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let info = {
            let clients = self.client_registry.clients.read().await;
            clients
                .iter()
                .find(|client| client.id == client_id)
                .cloned()
        };
        let Some(info) = info else {
            return Err("Client not found".to_string());
        };

        let device_id = info.device_id.clone();
        let entry = BlockEntry {
            ip: info.ip,
            device_name: info.device_name,
            device_id: (info.protocol_version == 2).then_some(device_id.clone()),
        };
        let mut blocklist = self.blocklist.read().await.clone();
        add_block_entry(&mut blocklist, entry);
        Ok(BlockCandidate {
            blocklist,
            device_id,
        })
    }

    pub async fn block_client(&self, client_id: &str) -> Result<Vec<BlockEntry>, String> {
        self.block_candidate(client_id)
            .await
            .map(|candidate| candidate.blocklist)
    }

    pub async fn block_client_candidate(&self, client_id: &str) -> Result<BlockCandidate, String> {
        self.block_candidate(client_id).await
    }

    pub async fn commit_block(&self, candidate: BlockCandidate) {
        let _input_guard = self.input_lock.lock().await;
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        *self.blocklist.write().await = candidate.blocklist;
        self.revoke_device_id_inner(&candidate.device_id).await;
        self.disconnect_device_connections_inner(&candidate.device_id, ClientCommand::Block)
            .await;
    }

    pub async fn unblock_candidate(&self, identifier: &str) -> Result<UnblockCandidate, String> {
        let identifier = identifier.trim();
        if identifier.is_empty() {
            return Err("Device ID or IP is required".to_string());
        }

        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let mut removed_device_ids = Vec::new();
        let mut blocklist = self.blocklist.read().await.clone();
        let before = blocklist.len();
        let related_ips = blocklist
            .iter()
            .filter(|entry| entry.device_id.as_deref() == Some(identifier))
            .map(|entry| entry.ip.clone())
            .collect::<Vec<_>>();
        blocklist.retain(|entry| {
            let matches = entry.device_id.as_deref() == Some(identifier)
                || entry.ip == identifier
                || (entry.device_id.is_none() && related_ips.iter().any(|ip| ip == &entry.ip));
            if matches {
                if let Some(device_id) = entry.device_id.as_deref() {
                    removed_device_ids.push(device_id.to_string());
                }
            }
            !matches
        });
        if blocklist.len() == before {
            return Err("Blocklist entry not found".to_string());
        }

        let known_device_ids = self
            .device_ips
            .read()
            .await
            .iter()
            .filter(|(device_id, ip)| *device_id == identifier || *ip == identifier)
            .map(|(device_id, _)| device_id.clone())
            .collect::<Vec<_>>();
        removed_device_ids.extend(known_device_ids);

        let connected_device_ids = self
            .client_registry
            .clients
            .read()
            .await
            .iter()
            .filter(|client| client.device_id == identifier || client.ip == identifier)
            .map(|client| client.device_id.clone())
            .collect::<Vec<_>>();
        removed_device_ids.extend(connected_device_ids);
        removed_device_ids.sort();
        removed_device_ids.dedup();
        Ok(UnblockCandidate {
            blocklist,
            device_ids: removed_device_ids,
        })
    }

    pub async fn unblock_device(&self, identifier: &str) -> Result<Vec<BlockEntry>, String> {
        self.unblock_candidate(identifier)
            .await
            .map(|candidate| candidate.blocklist)
    }

    pub async fn unblock_device_candidate(
        &self,
        identifier: &str,
    ) -> Result<UnblockCandidate, String> {
        self.unblock_candidate(identifier).await
    }

    pub async fn commit_unblock(&self, candidate: UnblockCandidate) {
        let _input_guard = self.input_lock.lock().await;
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        *self.blocklist.write().await = candidate.blocklist;
        for device_id in candidate.device_ids {
            self.revoke_device_id_inner(&device_id).await;
            self.disconnect_device_connections_inner(&device_id, ClientCommand::Disconnect)
                .await;
        }
    }

    pub async fn security_state(&self) -> SecurityState {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let connections = self.client_registry.clients.read().await.clone();
        let pending = connections
            .iter()
            .filter(|client| client.status == "pending")
            .cloned()
            .collect();
        let clients = connections
            .into_iter()
            .filter(|client| client.authorized && client.status == "authorized")
            .collect();
        let mut authorized_devices = self
            .authorized_devices
            .read()
            .await
            .iter()
            .map(|(device_id, authorization)| AuthorizedDeviceInfo {
                device_id: device_id.clone(),
                display_name: authorization.display_name.clone(),
            })
            .collect::<Vec<_>>();
        authorized_devices.sort_by(|left, right| left.device_id.cmp(&right.device_id));
        let controller_id = self.controller_id.read().await.clone();
        SecurityState {
            paused: self.is_paused(),
            require_approval: self.require_approval(),
            discovery_enabled: self.discovery_enabled(),
            controller_id,
            pending,
            clients,
            blocklist: self.blocklist().await,
            keyboard_healthy: self.keyboard_healthy(),
            authorized_devices,
        }
    }

    pub async fn authorized_clients(&self) -> Vec<ClientInfo> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.authorized_clients_inner().await
    }

    async fn authorized_clients_inner(&self) -> Vec<ClientInfo> {
        self.client_registry
            .clients
            .read()
            .await
            .iter()
            .filter(|client| client.authorized && client.status == "authorized")
            .cloned()
            .collect()
    }

    pub async fn emit_state<R: Runtime>(&self, app_handle: &AppHandle<R>) {
        let clients = self.authorized_clients().await;
        let payload =
            serde_json::to_value(&clients).unwrap_or_else(|_| serde_json::Value::Array(Vec::new()));
        let _ = app_handle.emit("clients-changed", payload);
        let _ = app_handle.emit("security-state-changed", self.security_state().await);
    }

    pub fn emit_keyboard_status<R: Runtime>(&self, app_handle: &AppHandle<R>) {
        let payload = serde_json::json!({ "healthy": self.keyboard_healthy() });
        let _ = app_handle.emit("keyboard-status-changed", payload);
    }

    pub async fn accept_connection<R: Runtime>(
        self: &Arc<Self>,
        stream: TcpStream,
        addr: SocketAddr,
        first_chunk: Vec<u8>,
        app_handle: AppHandle<R>,
    ) {
        let query = request_pairing(&first_chunk);
        let prepend = PrependStream::new(stream, first_chunk);
        let handshake = timeout(
            WS_HANDSHAKE_TIMEOUT,
            accept_async_with_config(prepend, Some(websocket_config())),
        )
        .await;
        let ws_stream = match handshake {
            Ok(Ok(ws)) => ws,
            Ok(Err(_)) => {
                error!("WS handshake failed from {addr}");
                return;
            }
            Err(_) => {
                error!("WS handshake timed out from {addr}");
                return;
            }
        };

        let server = self.clone();
        handle_ws_client(ws_stream, addr, server, app_handle, query).await;
    }

    async fn authenticate_locked(
        &self,
        query: &PairingQuery,
        hello: &HelloInfo,
        contract: &HelloContract,
        display_name: &str,
        ip: &str,
    ) -> Result<AuthOutcome, AuthFailure> {
        let current_pairing_generation = self.pairing.read().await.generation;
        if self.is_paused() {
            return Err(AuthFailure::Paused);
        }

        if self.is_blocked(&contract.device_id, ip).await {
            return Err(AuthFailure::Blacklisted);
        }

        let valid_session = if contract.is_v2 {
            self.valid_session_token(query, hello, &contract.device_id, ip)
                .await?
        } else {
            None
        };

        if let Some(session_token) = valid_session {
            if let Some(authorization) = self
                .authorized_devices
                .write()
                .await
                .get_mut(&contract.device_id)
            {
                authorization.display_name = display_name.to_string();
            }
            self.device_ips
                .write()
                .await
                .insert(contract.device_id.clone(), ip.to_string());
            return Ok(AuthOutcome::Authorized {
                device_id: contract.device_id.clone(),
                session_token,
                pairing_generation: current_pairing_generation,
            });
        }

        if !self.reserve_pairing_attempt(ip).await {
            return Err(AuthFailure::InvalidPairing);
        }
        let pairing = self.pairing.read().await.clone();
        let pairing_active = now_secs() < pairing.expires_at;
        let pairing_valid = pairing_active
            && (query.pin.as_deref() == Some(pairing.pairing_code.as_str())
                || query.token.as_deref() == Some(pairing.token.as_str())
                || hello.pairing_token.as_deref() == Some(pairing.token.as_str()));

        if !pairing_valid {
            return Err(AuthFailure::InvalidPairing);
        }
        self.clear_pairing_attempt(ip).await;
        self.device_ips
            .write()
            .await
            .insert(contract.device_id.clone(), ip.to_string());

        if !contract.is_v2 || self.require_approval() {
            Ok(AuthOutcome::Pending {
                device_id: contract.device_id.clone(),
                pairing_generation: current_pairing_generation,
            })
        } else {
            let session_token = self
                .authorize_device_inner(&contract.device_id, display_name)
                .await;
            Ok(AuthOutcome::Authorized {
                device_id: contract.device_id.clone(),
                session_token,
                pairing_generation: current_pairing_generation,
            })
        }
    }

    async fn valid_session_token(
        &self,
        query: &PairingQuery,
        hello: &HelloInfo,
        device_id: &str,
        ip: &str,
    ) -> Result<Option<String>, AuthFailure> {
        let mut candidates = Vec::new();
        for value in [
            hello.session_token.as_deref(),
            query.session_token.as_deref(),
            query.token.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if !value.is_empty() && !candidates.iter().any(|candidate| candidate == &value) {
                candidates.push(value);
            }
        }

        let sessions = self.session_devices.read().await;
        let authorizations = self.authorized_devices.read().await;
        for candidate in candidates {
            let Some(mapped_device_id) = sessions.get(candidate) else {
                continue;
            };
            if mapped_device_id != device_id {
                drop(authorizations);
                drop(sessions);
                self.record_pairing_failure(ip).await;
                return Err(AuthFailure::InvalidPairing);
            }
            if authorizations
                .get(device_id)
                .is_some_and(|authorization| authorization.session_token == candidate)
            {
                return Ok(Some(candidate.to_string()));
            }
        }
        Ok(None)
    }

    async fn record_pairing_failure(&self, ip: &str) -> bool {
        let mut attempts = self.pairing_attempts.lock().await;
        let now = Instant::now();
        prune_pairing_attempts(&mut attempts, now, MAX_PAIRING_ATTEMPT_IPS);
        let entry = attempts.entry(ip.to_string()).or_insert(PairingAttempt {
            started_at: now,
            failures: 0,
        });
        if entry.failures >= MAX_PAIRING_FAILURES {
            return false;
        }
        entry.failures = entry.failures.saturating_add(1);
        true
    }

    async fn reserve_pairing_attempt(&self, ip: &str) -> bool {
        self.record_pairing_failure(ip).await
    }

    async fn clear_pairing_attempt(&self, ip: &str) {
        self.pairing_attempts.lock().await.remove(ip);
    }

    async fn register_client_with_id(
        &self,
        client_id: &str,
        info: ClientInfo,
        shutdown_tx: watch::Sender<bool>,
        command_tx: mpsc::UnboundedSender<ClientCommand>,
    ) {
        {
            let mut clients = self.client_registry.clients.write().await;
            clients.push(info);
        }
        self.client_registry
            .shutdown_txs
            .write()
            .await
            .insert(client_id.to_string(), shutdown_tx);
        self.client_registry
            .command_txs
            .write()
            .await
            .insert(client_id.to_string(), command_tx);
        self.refresh_controller_flags().await;
    }

    async fn mark_client_authorized(&self, client_id: &str) {
        {
            let mut clients = self.client_registry.clients.write().await;
            if let Some(client) = clients.iter_mut().find(|client| client.id == client_id) {
                client.authorized = true;
                client.status = "authorized".to_string();
            }
        }
        self.refresh_controller_flags().await;
    }

    async fn is_client_pending(&self, client_id: &str) -> bool {
        self.client_registry
            .clients
            .read()
            .await
            .iter()
            .any(|client| client.id == client_id && client.status == "pending")
    }

    async fn update_client_name(&self, client_id: &str, device_name: String) -> bool {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let changed = {
            let mut clients = self.client_registry.clients.write().await;
            match clients.iter_mut().find(|client| client.id == client_id) {
                Some(client) if client.device_name != device_name => {
                    if client.authorized && client.protocol_version == 2 {
                        if let Some(authorization) = self
                            .authorized_devices
                            .write()
                            .await
                            .get_mut(&client.device_id)
                        {
                            authorization.display_name = device_name.clone();
                        }
                    }
                    client.device_name = device_name;
                    true
                }
                _ => false,
            }
        };
        changed
    }

    async fn remove_client(&self, client_id: &str) {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.remove_client_inner(client_id).await;
    }

    async fn remove_client_inner(&self, client_id: &str) {
        self.client_registry
            .clients
            .write()
            .await
            .retain(|client| client.id != client_id);
        self.client_registry
            .shutdown_txs
            .write()
            .await
            .remove(client_id);
        self.client_registry
            .command_txs
            .write()
            .await
            .remove(client_id);
        self.refresh_controller_flags().await;
    }

    async fn refresh_controller_flags(&self) {
        let clients_snapshot = self.client_registry.clients.read().await.clone();
        let candidate = clients_snapshot
            .iter()
            .find(|client| client.authorized)
            .map(|client| client.id.clone());
        let mut controller = self.controller_id.write().await;
        let current_is_valid = controller.as_ref().is_some_and(|id| {
            clients_snapshot
                .iter()
                .any(|client| client.id == *id && client.authorized)
        });
        if !current_is_valid {
            *controller = candidate;
        }
        let selected = controller.clone();
        drop(controller);

        let mut clients = self.client_registry.clients.write().await;
        for client in clients.iter_mut() {
            client.controller = selected.as_deref() == Some(client.id.as_str());
        }
    }

    async fn send_to_pending(&self, client_id: &str, command: ClientCommand) -> Result<(), String> {
        let pending = {
            let clients = self.client_registry.clients.read().await;
            clients
                .iter()
                .any(|client| client.id == client_id && client.status == "pending")
        };
        if !pending {
            return Err("Pending connection not found".to_string());
        }
        let sender = self
            .client_registry
            .command_txs
            .read()
            .await
            .get(client_id)
            .cloned()
            .ok_or_else(|| "Pending connection not found".to_string())?;
        sender
            .send(command)
            .map_err(|_| "Pending connection is already closed".to_string())
    }

    async fn send_command_to_clients(&self, ids: Vec<String>, command: ClientCommand) {
        let senders = {
            let commands = self.client_registry.command_txs.read().await;
            ids.iter()
                .filter_map(|id| commands.get(id).cloned())
                .collect::<Vec<_>>()
        };
        for sender in senders {
            let _ = sender.send(command);
        }
    }

    async fn revoke_device_id_inner(&self, device_id: &str) {
        self.authorized_devices.write().await.remove(device_id);
        self.session_devices
            .write()
            .await
            .retain(|_, current| current != device_id);
        self.device_ips.write().await.remove(device_id);
        {
            let mut clients = self.client_registry.clients.write().await;
            for client in clients
                .iter_mut()
                .filter(|client| client.device_id == device_id)
            {
                client.authorized = false;
                client.status = "revoked".to_string();
                client.controller = false;
            }
        }
        self.refresh_controller_flags().await;
    }

    async fn authorize_device_inner(&self, device_id: &str, display_name: &str) -> String {
        let session_token = format!("lt-session-{}", Uuid::new_v4());
        if let Some(previous) = self.authorized_devices.write().await.insert(
            device_id.to_string(),
            Authorization {
                session_token: session_token.clone(),
                display_name: display_name.to_string(),
            },
        ) {
            self.session_devices
                .write()
                .await
                .remove(&previous.session_token);
        }
        self.session_devices
            .write()
            .await
            .insert(session_token.clone(), device_id.to_string());
        session_token
    }

    async fn authorize_device(&self, device_id: &str) -> String {
        self.authorize_device_inner(device_id, device_id).await
    }

    async fn disconnect_device_connections_inner(&self, device_id: &str, command: ClientCommand) {
        let client_ids = self
            .client_registry
            .clients
            .read()
            .await
            .iter()
            .filter(|client| client.device_id == device_id)
            .map(|client| client.id.clone())
            .collect::<Vec<_>>();
        let senders = {
            let commands = self.client_registry.command_txs.read().await;
            client_ids
                .iter()
                .filter_map(|client_id| commands.get(client_id).cloned())
                .collect::<Vec<_>>()
        };
        for sender in senders {
            let _ = sender.send(command);
        }
    }

    async fn authenticate(
        &self,
        query: &PairingQuery,
        pairing_token: Option<&str>,
        session_token: Option<&str>,
        explicit_device_id: Option<&str>,
        fallback_device_id: &str,
        ip: &str,
    ) -> Result<AuthOutcome, AuthFailure> {
        let hello = HelloInfo {
            protocol_version: Some(u32::from(explicit_device_id.is_some()) + 1),
            device_id: explicit_device_id.map(str::to_string),
            device_name: fallback_device_id.to_string(),
            pairing_token: pairing_token.map(str::to_string),
            session_token: session_token.map(str::to_string),
        };
        let contract = HelloContract {
            is_v2: explicit_device_id.is_some(),
            device_id: explicit_device_id.unwrap_or(fallback_device_id).to_string(),
        };
        self.authenticate_locked(query, &hello, &contract, fallback_device_id, ip)
            .await
    }

    async fn is_blocked(&self, device_id: &str, ip: &str) -> bool {
        self.blocklist.read().await.iter().any(|entry| {
            entry.device_id.as_deref() == Some(device_id)
                || (entry.device_id.is_none() && entry.ip == ip)
        })
    }

    async fn execute_input(
        &self,
        client_id: &str,
        operation: InputOperation,
        request_id: Option<&str>,
        sequence: Option<u64>,
        protocol_version: u32,
        last_input: &mut Option<Instant>,
    ) -> Result<(), InputFailure> {
        validate_input_contract(protocol_version, request_id, sequence)?;
        validate_operation(&operation)?;
        if let Some(last) = *last_input {
            if last.elapsed() < MIN_INPUT_INTERVAL {
                return Err(InputFailure::new(
                    "rate_limited",
                    "输入过于频繁，请稍后重试",
                    true,
                ));
            }
        }
        *last_input = Some(Instant::now());

        let _input_guard = self.input_lock.lock().await;
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        if self.is_paused() {
            return Err(InputFailure::new("paused", "桌面端已暂停接收输入", true));
        }
        if !self.is_client_authorized(client_id).await {
            return Err(InputFailure::new(
                "not_authorized",
                "此设备已被撤销授权",
                false,
            ));
        }
        if !self.is_controller(client_id).await {
            return Err(InputFailure::new("busy", "当前设备没有输入控制权", true));
        }
        if !self.keyboard.is_healthy() {
            return Err(InputFailure::new(
                "permission_required",
                "请授予辅助功能权限后重试",
                true,
            ));
        }

        let result = match operation {
            InputOperation::Type(text) => self.keyboard.type_text(text).await,
            InputOperation::Diff { backspace, text } => {
                self.keyboard.apply_diff(backspace, text).await
            }
            InputOperation::Keys { modifiers, key } => {
                self.keyboard.press_chord(modifiers, key).await
            }
        };

        match result {
            Ok(()) => Ok(()),
            Err(error) => Err(map_keyboard_error(&error, self.keyboard.is_healthy())),
        }
    }

    async fn is_controller(&self, client_id: &str) -> bool {
        self.controller_id.read().await.as_deref() == Some(client_id)
    }

    async fn is_client_authorized(&self, client_id: &str) -> bool {
        self.client_registry
            .clients
            .read()
            .await
            .iter()
            .any(|client| {
                client.id == client_id && client.authorized && client.status == "authorized"
            })
    }

    pub fn requires_pairing(&self) -> bool {
        !self.is_paused()
    }
}

#[derive(Debug)]
enum AuthFailure {
    InvalidPairing,
    Paused,
    Blacklisted,
}

#[derive(Debug)]
enum AuthOutcome {
    Authorized {
        device_id: String,
        session_token: String,
        pairing_generation: u64,
    },
    Pending {
        device_id: String,
        pairing_generation: u64,
    },
}

#[derive(Debug, Clone, Default)]
struct PairingQuery {
    token: Option<String>,
    pin: Option<String>,
    session_token: Option<String>,
}

#[derive(Debug)]
struct HelloInfo {
    protocol_version: Option<u32>,
    device_id: Option<String>,
    device_name: String,
    pairing_token: Option<String>,
    session_token: Option<String>,
}

#[derive(Debug)]
struct HelloContract {
    is_v2: bool,
    device_id: String,
}

#[derive(Debug)]
enum InputOperation {
    Type(String),
    Diff { backspace: u32, text: String },
    Keys { modifiers: Vec<String>, key: String },
}

#[derive(Debug)]
struct InputFailure {
    code: &'static str,
    message: &'static str,
    retryable: bool,
}

impl InputFailure {
    fn new(code: &'static str, message: &'static str, retryable: bool) -> Self {
        Self {
            code,
            message,
            retryable,
        }
    }
}

async fn handle_ws_client<S, R>(
    ws_stream: tokio_tungstenite::WebSocketStream<S>,
    addr: SocketAddr,
    server: Arc<WsServer>,
    app_handle: AppHandle<R>,
    query: PairingQuery,
) where
    S: AsyncRead + AsyncWrite + Unpin,
    R: Runtime,
{
    let (mut write, mut read) = ws_stream.split();
    let client_id = Uuid::new_v4().to_string();

    if server.is_paused() {
        let _ = send_server(
            &mut write,
            &ServerMessage::Paused {
                message: "桌面端已暂停接收输入".to_string(),
            },
        )
        .await;
        close_writer(&mut write, "paused").await;
        return;
    }

    let hello = match read_hello(&mut write, &mut read).await {
        Ok(hello) => hello,
        Err(reason) => {
            info!("Client {addr} rejected during hello: {reason}");
            close_writer(&mut write, "hello required").await;
            return;
        }
    };

    if hello.protocol_version.is_some_and(|version| version > 2) {
        send_error(&mut write, None, "invalid_input", "不支持的协议版本", false).await;
        close_writer(&mut write, "unsupported protocol").await;
        return;
    }

    let sender_name = normalize_device_name(&hello.device_name);
    let client_ip = addr.ip().to_string();
    let explicit_device_id = match hello.device_id.as_deref() {
        Some(device_id) if valid_identity(device_id, MAX_DEVICE_ID_CHARS) => Some(device_id),
        Some(_) => {
            send_error(&mut write, None, "invalid_input", "设备 ID 无效", false).await;
            close_writer(&mut write, "invalid device id").await;
            return;
        }
        None => None,
    };
    if hello.protocol_version.unwrap_or(1) >= 2 && explicit_device_id.is_none() {
        send_error(
            &mut write,
            None,
            "invalid_input",
            "V2 连接必须包含设备 ID",
            false,
        )
        .await;
        close_writer(&mut write, "invalid device id").await;
        return;
    }
    let fallback_device_id = format!("ip:{client_ip}|name:{sender_name}");

    let auth = match server
        .authenticate(
            &query,
            hello.pairing_token.as_deref(),
            hello.session_token.as_deref(),
            explicit_device_id,
            &fallback_device_id,
            &client_ip,
        )
        .await
    {
        Ok(auth) => auth,
        Err(AuthFailure::InvalidPairing) => {
            send_error(
                &mut write,
                None,
                "invalid_pairing",
                "配对信息无效或已过期，请重新扫码",
                true,
            )
            .await;
            close_writer(&mut write, "invalid pairing").await;
            return;
        }
        Err(AuthFailure::Paused) => {
            send_server(
                &mut write,
                &ServerMessage::Paused {
                    message: "桌面端已暂停接收输入".to_string(),
                },
            )
            .await
            .ok();
            close_writer(&mut write, "paused").await;
            return;
        }
        Err(AuthFailure::Blacklisted) => {
            send_error(&mut write, None, "blacklisted", "此设备已被拉黑", false).await;
            close_writer(&mut write, "blocked").await;
            return;
        }
    };

    let lifecycle_guard = server.lifecycle_lock.lock().await;
    let (device_id, pairing_generation, authorized, session_token) = match &auth {
        AuthOutcome::Authorized {
            device_id,
            pairing_generation,
            session_token,
        } => (
            device_id.clone(),
            Some(*pairing_generation),
            true,
            Some(session_token.clone()),
        ),
        AuthOutcome::Pending {
            device_id,
            pairing_generation,
        } => (device_id.clone(), Some(*pairing_generation), false, None),
    };

    if server.is_blocked(&device_id, &client_ip).await {
        drop(lifecycle_guard);
        if authorized {
            server.revoke_device_id(&device_id).await;
        }
        send_error(&mut write, None, "blacklisted", "此设备已被拉黑", false).await;
        close_writer(&mut write, "blocked").await;
        return;
    }

    let status = if authorized { "authorized" } else { "pending" };
    if server.is_paused() {
        drop(lifecycle_guard);
        let _ = send_server(
            &mut write,
            &ServerMessage::Paused {
                message: "桌面端已暂停接收输入".to_string(),
            },
        )
        .await;
        close_writer(&mut write, "paused").await;
        return;
    }
    let current_pairing_generation = server.pairing.read().await.generation;
    if pairing_generation.is_some_and(|generation| generation != current_pairing_generation) {
        drop(lifecycle_guard);
        send_error(
            &mut write,
            None,
            "invalid_pairing",
            "配对信息已轮换，请重新扫码",
            true,
        )
        .await;
        close_writer(&mut write, "invalid pairing").await;
        return;
    }

    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let (command_tx, mut command_rx) = mpsc::unbounded_channel();
    let info = ClientInfo {
        id: client_id.clone(),
        device_name: sender_name.clone(),
        ip: client_ip.clone(),
        device_id: device_id.clone(),
        status: status.to_string(),
        authorized,
        controller: false,
        connected_at: now_secs(),
        protocol_version: hello.protocol_version.unwrap_or(1),
    };
    server
        .register_client_with_id(&client_id, info.clone(), shutdown_tx, command_tx)
        .await;
    drop(lifecycle_guard);
    server.emit_state(&app_handle).await;

    if !authorized {
        let _ = app_handle.emit("connection-requested", &info);
        let _ = send_server(
            &mut write,
            &ServerMessage::Pending {
                client_id: client_id.clone(),
                message: "请在桌面端批准此设备".to_string(),
            },
        )
        .await;
        send_error(
            &mut write,
            None,
            "approval_required",
            "等待桌面端批准此设备",
            true,
        )
        .await;
    } else if let Some(session_token) = session_token.as_deref() {
        let _ = send_server(
            &mut write,
            &ServerMessage::Ready {
                device: server.device_name.clone(),
                client_id: client_id.clone(),
                session_id: session_token.to_string(),
            },
        )
        .await;
    }

    info!(
        "Client {} ({}) registered as {}",
        sender_name, client_ip, client_id
    );

    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    let mut last_seen = Instant::now();
    let mut last_input = None;
    let mut approval_pending = !authorized;
    let approval_timer = sleep(APPROVAL_TIMEOUT);
    tokio::pin!(approval_timer);

    loop {
        tokio::select! {
            command = command_rx.recv() => {
                match command {
                    Some(ClientCommand::Approve) if approval_pending => {
                        if !server.is_client_pending(&client_id).await
                            || server.is_blocked(&device_id, &client_ip).await
                        {
                            send_error(
                                &mut write,
                                None,
                                "not_authorized",
                                "此设备已被撤销或拒绝",
                                false,
                            )
                            .await;
                            close_writer(&mut write, "not authorized").await;
                            break;
                        }
                        let session_token = server.authorize_device(&device_id).await;
                        server.mark_client_authorized(&client_id).await;
                        approval_pending = false;
                        let _ = send_server(
                            &mut write,
                            &ServerMessage::Ready {
                                device: server.device_name.clone(),
                                client_id: client_id.clone(),
                                session_id: session_token,
                            },
                        )
                        .await;
                        server.emit_state(&app_handle).await;
                    }
                    Some(ClientCommand::Reject) if approval_pending => {
                        send_error(
                            &mut write,
                            None,
                            "not_authorized",
                            "桌面端拒绝了此设备",
                            false,
                        )
                        .await;
                        close_writer(&mut write, "not authorized").await;
                        break;
                    }
                    Some(ClientCommand::Pause) => {
                        let _ = send_server(
                            &mut write,
                            &ServerMessage::Paused {
                                message: "桌面端已暂停接收输入".to_string(),
                            },
                        )
                        .await;
                        close_writer(&mut write, "paused").await;
                        break;
                    }
                    Some(ClientCommand::Disconnect) | None => {
                        close_writer(&mut write, "disconnected").await;
                        break;
                    }
                    _ => {}
                }
            }
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        last_seen = Instant::now();
        if text.len() > MAX_MESSAGE_BYTES {
                            send_error(
                                &mut write,
                                None,
                                "payload_too_large",
                                "消息超过大小限制",
                                false,
                            )
                            .await;
                            continue;
                        }
                        let parsed = match protocol::parse_client_message(&text) {
                            Ok(message) => message,
                            Err(_) => {
                                send_error(
                                    &mut write,
                                    request_id_from_raw(&text),
                                    "invalid_input",
                                    "消息格式无效",
                                    false,
                                )
                                .await;
                                continue;
                            }
                        };

                        match parsed {
                            ClientMessage::Ping => {
                                let _ = send_server(&mut write, &ServerMessage::Pong).await;
                            }
                            ClientMessage::Hello { device_name, .. } => {
                                let name = normalize_device_name(&device_name);
                                if server.update_client_name(&client_id, name).await {
                                    server.emit_state(&app_handle).await;
                                }
                            }
                            message if approval_pending => {
                                let (request_id, _) = message.request_meta();
                                send_error(
                                    &mut write,
                                    normalize_request_id(request_id.map(str::to_string)),
                                    "approval_required",
                                    "请等待桌面端批准此设备",
                                    true,
                                )
                                .await;
                            }
                            ClientMessage::Type {
                                text,
                                request_id,
                                sequence,
                            } => {
                                let result = server
                                    .execute_input(
                                        &client_id,
                                        InputOperation::Type(text),
                                        request_id.as_deref(),
                                        sequence,
                                        hello.protocol_version.unwrap_or(1),
                                        &mut last_input,
                                    )
                                    .await;
                                respond_input(&mut write, request_id, sequence, result).await;
                            }
                            ClientMessage::Diff {
                                backspace,
                                text,
                                request_id,
                                sequence,
                            } => {
                                let result = server
                                    .execute_input(
                                        &client_id,
                                        InputOperation::Diff { backspace, text },
                                        request_id.as_deref(),
                                        sequence,
                                        hello.protocol_version.unwrap_or(1),
                                        &mut last_input,
                                    )
                                    .await;
                                respond_input(&mut write, request_id, sequence, result).await;
                            }
                            ClientMessage::Keys {
                                modifiers,
                                key,
                                request_id,
                                sequence,
                            } => {
                                let result = server
                                    .execute_input(
                                        &client_id,
                                        InputOperation::Keys { modifiers, key },
                                        request_id.as_deref(),
                                        sequence,
                                        hello.protocol_version.unwrap_or(1),
                                        &mut last_input,
                                    )
                                    .await;
                                respond_input(&mut write, request_id, sequence, result).await;
                            }
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        last_seen = Instant::now();
                        let _ = write.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Pong(_))) => {
                        last_seen = Instant::now();
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Binary(_))) => {
                        send_error(
                            &mut write,
                            None,
                            "invalid_input",
                            "仅支持文本消息",
                            false,
                        )
                        .await;
                    }
                    _ => {}
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    close_writer(&mut write, "disconnected").await;
                    break;
                }
            }
            _ = heartbeat.tick() => {
                if last_seen.elapsed() > CLIENT_TIMEOUT {
                    close_writer(&mut write, "heartbeat timeout").await;
                    break;
                }
                if write.send(Message::Ping(Vec::new())).await.is_err() {
                    break;
                }
            }
            _ = &mut approval_timer, if approval_pending => {
                send_error(
                    &mut write,
                    None,
                    "not_authorized",
                    "设备审批已超时，请重新配对",
                    false,
                )
                .await;
                close_writer(&mut write, "approval timeout").await;
                break;
            }
        }
    }

    server.remove_client(&client_id).await;
    server.emit_state(&app_handle).await;
    info!("Connection closed: {addr}");
}

async fn read_hello<S, R>(write: &mut S, read: &mut R) -> Result<HelloInfo, String>
where
    S: Sink<Message> + Unpin,
    R: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    timeout(HELLO_TIMEOUT, async {
        while let Some(message) = read.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    if text.len() > MAX_MESSAGE_BYTES {
                        return Err("hello payload too large".to_string());
                    }
                    match protocol::parse_client_message(&text) {
                        Ok(ClientMessage::Hello {
                            protocol_version,
                            device_id,
                            device_name,
                            pairing_token,
                            session_token,
                        }) => {
                            return Ok(HelloInfo {
                                protocol_version,
                                device_id,
                                device_name,
                                pairing_token,
                                session_token,
                            });
                        }
                        Ok(ClientMessage::Ping) => {
                            send_server(write, &ServerMessage::Pong)
                                .await
                                .map_err(|_| "failed to send pong".to_string())?;
                        }
                        Ok(_) => return Err("expected hello first".to_string()),
                        Err(_) => return Err("invalid hello".to_string()),
                    }
                }
                Ok(Message::Ping(payload)) => {
                    write
                        .send(Message::Pong(payload))
                        .await
                        .map_err(|_| "failed to send pong".to_string())?;
                }
                Ok(Message::Close(_)) => return Err("connection closed before hello".to_string()),
                Ok(Message::Pong(_)) => {}
                Ok(Message::Binary(_)) => return Err("expected hello first".to_string()),
                Ok(Message::Frame(_)) => {}
                Err(_) => return Err("connection read failed".to_string()),
            }
        }
        Err("stream ended before hello".to_string())
    })
    .await
    .map_err(|_| "hello timeout".to_string())?
}

async fn respond_input<S>(
    write: &mut S,
    request_id: Option<String>,
    sequence: Option<u64>,
    result: Result<(), InputFailure>,
) where
    S: Sink<Message> + Unpin,
{
    let request_id = normalize_request_id(request_id);
    match result {
        Ok(()) => {
            if let Some(request_id) = request_id.filter(|id| !id.is_empty()) {
                let _ = send_server(
                    write,
                    &ServerMessage::Ack {
                        request_id,
                        sequence,
                    },
                )
                .await;
            }
        }
        Err(failure) => {
            send_error(
                write,
                request_id.filter(|id| !id.is_empty()),
                failure.code,
                failure.message,
                failure.retryable,
            )
            .await;
        }
    }
}

async fn send_server<S>(write: &mut S, message: &ServerMessage) -> Result<(), S::Error>
where
    S: Sink<Message> + Unpin,
{
    let encoded = protocol::serialize_server_message(message);
    write.send(Message::Text(encoded)).await
}

async fn send_error<S>(
    write: &mut S,
    request_id: Option<String>,
    code: &'static str,
    message: &'static str,
    retryable: bool,
) where
    S: Sink<Message> + Unpin,
{
    let _ = send_server(
        write,
        &ServerMessage::Error {
            request_id,
            code: code.to_string(),
            message: message.to_string(),
            retryable,
        },
    )
    .await;
}

async fn close_writer<S>(write: &mut S, reason: &'static str)
where
    S: Sink<Message> + Unpin,
{
    let _ = write
        .send(Message::Close(Some(CloseFrame {
            code: CloseCode::Policy,
            reason: Cow::Borrowed(reason),
        })))
        .await;
}

fn validate_operation(operation: &InputOperation) -> Result<(), InputFailure> {
    match operation {
        InputOperation::Type(text) => {
            if text.len() > MAX_TEXT_BYTES {
                return Err(InputFailure::new(
                    "payload_too_large",
                    "文本超过 64 KiB 限制",
                    false,
                ));
            }
        }
        InputOperation::Diff { backspace, text } => {
            if text.len() > MAX_TEXT_BYTES {
                return Err(InputFailure::new(
                    "payload_too_large",
                    "文本超过 64 KiB 限制",
                    false,
                ));
            }
            if *backspace > MAX_BACKSPACE {
                return Err(InputFailure::new(
                    "payload_too_large",
                    "删除数量超过限制",
                    false,
                ));
            }
        }
        InputOperation::Keys { modifiers, key } => {
            if modifiers.len() > MAX_MODIFIERS
                || key.is_empty()
                || key.chars().count() > MAX_KEY_CHARS
                || key.chars().any(char::is_control)
                || modifiers.iter().any(|modifier| {
                    modifier.is_empty()
                        || modifier.chars().count() > MAX_KEY_CHARS
                        || modifier.chars().any(char::is_control)
                })
            {
                return Err(InputFailure::new("invalid_input", "快捷键参数无效", false));
            }
        }
    }
    Ok(())
}

fn map_keyboard_error(error: &str, healthy: bool) -> InputFailure {
    let normalized = error.to_ascii_lowercase();
    if normalized.contains("unknown key") || normalized.contains("unknown modifier") {
        return InputFailure::new("invalid_input", "快捷键参数无效", false);
    }
    if !healthy || normalized.contains("permission") || normalized.contains("unavailable") {
        return InputFailure::new("permission_required", "请授予辅助功能权限后重试", true);
    }
    InputFailure::new("injection_failed", "键盘操作执行失败，请重试", true)
}

fn normalize_request_id(request_id: Option<String>) -> Option<String> {
    request_id.filter(|id| {
        !id.is_empty()
            && id.chars().count() <= MAX_REQUEST_ID_CHARS
            && id.chars().all(|character| !character.is_control())
    })
}

fn validate_input_contract(
    protocol_version: u32,
    request_id: Option<&str>,
    sequence: Option<u64>,
) -> Result<(), InputFailure> {
    if protocol_version >= 2 && (request_id.is_none() || sequence.is_none()) {
        return Err(InputFailure::new(
            "invalid_input",
            "V2 输入必须包含 request_id 和 sequence",
            false,
        ));
    }
    Ok(())
}

fn websocket_config() -> WebSocketConfig {
    WebSocketConfig {
        max_message_size: Some(MAX_MESSAGE_BYTES),
        max_frame_size: Some(MAX_MESSAGE_BYTES),
        accept_unmasked_frames: false,
        write_buffer_size: MAX_MESSAGE_BYTES,
        max_write_buffer_size: MAX_MESSAGE_BYTES * 2,
        ..WebSocketConfig::default()
    }
}

fn request_pairing(first_chunk: &[u8]) -> PairingQuery {
    let Some(request) = std::str::from_utf8(first_chunk).ok() else {
        return PairingQuery::default();
    };
    let Some(line) = request.lines().next() else {
        return PairingQuery::default();
    };
    let mut parts = line.split_whitespace();
    if parts.next() != Some("GET") {
        return PairingQuery::default();
    }
    let Some(target) = parts.next() else {
        return PairingQuery::default();
    };
    let Some(query) = target.split_once('?').map(|(_, query)| query) else {
        return PairingQuery::default();
    };

    let mut result = PairingQuery::default();
    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "token" | "pairing_token" => result.token = Some(value.into_owned()),
            "session" | "session_token" => result.session_token = Some(value.into_owned()),
            "pin" => result.pin = Some(value.into_owned()),
            _ => {}
        }
    }
    result
}

fn new_pairing_state(generation: u64) -> PairingState {
    let created_at = now_secs();
    PairingState {
        token: format!("lt-pair-{}", Uuid::new_v4()),
        pairing_code: generate_pin(),
        created_at,
        expires_at: created_at.saturating_add(PAIRING_TTL_SECS),
        generation,
    }
}

fn pairing_info_from_state(state: &PairingState, rotated: bool) -> PairingInfo {
    PairingInfo {
        token: state.token.clone(),
        pairing_code: state.pairing_code.clone(),
        created_at: state.created_at,
        expires_at: state.expires_at,
        rotated,
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn request_id_from_raw(data: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(data).ok()?;
    let request_id = value.get("request_id")?.as_str()?;
    if request_id.is_empty() || request_id.chars().count() > MAX_REQUEST_ID_CHARS {
        return None;
    }
    Some(request_id.to_string())
}

fn normalize_device_name(value: &str) -> String {
    let value = value.trim();
    let mut result = value
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_DEVICE_NAME_CHARS)
        .collect::<String>();
    if result.is_empty() {
        result = "未命名设备".to_string();
    }
    result
}

fn valid_identity(value: &str, max_chars: usize) -> bool {
    !value.is_empty()
        && value.chars().count() <= max_chars
        && value.chars().all(|character| !character.is_control())
}

fn deduplicate_blocklist(blocklist: Vec<BlockEntry>) -> Vec<BlockEntry> {
    let mut result = Vec::with_capacity(blocklist.len());
    for entry in blocklist {
        let duplicate = match entry.device_id.as_deref() {
            Some(device_id) => result
                .iter()
                .any(|current: &BlockEntry| current.device_id.as_deref() == Some(device_id)),
            None => result
                .iter()
                .any(|current: &BlockEntry| current.device_id.is_none() && current.ip == entry.ip),
        };
        if !duplicate {
            result.push(entry);
        }
    }
    result
}

fn add_block_entry(blocklist: &mut Vec<BlockEntry>, entry: BlockEntry) {
    let duplicate = blocklist
        .iter()
        .any(|current| match entry.device_id.as_deref() {
            Some(device_id) => current.device_id.as_deref() == Some(device_id),
            None => current.device_id.is_none() && current.ip == entry.ip,
        });
    if !duplicate {
        blocklist.push(entry);
    }
}

fn prune_pairing_attempts(
    attempts: &mut HashMap<String, PairingAttempt>,
    now: Instant,
    max_entries: usize,
) {
    attempts.retain(|_, attempt| now.duration_since(attempt.started_at) <= PAIRING_FAILURE_WINDOW);
    while attempts.len() > max_entries {
        let Some(oldest_ip) = attempts
            .iter()
            .min_by_key(|(_, attempt)| attempt.started_at)
            .map(|(ip, _)| ip.clone())
        else {
            break;
        };
        attempts.remove(&oldest_ip);
    }
}

/// Generate a fresh 6-digit pairing PIN for the current process.
pub fn generate_pin() -> String {
    let n = Uuid::new_v4().as_u128() % 1_000_000;
    format!("{n:06}")
}

/// Wraps a `TcpStream` and prepends already-read bytes so that downstream
/// consumers see the complete initial request.
struct PrependStream {
    stream: TcpStream,
    buf: Vec<u8>,
    pos: usize,
}

impl PrependStream {
    fn new(stream: TcpStream, buf: Vec<u8>) -> Self {
        Self {
            stream,
            buf,
            pos: 0,
        }
    }
}

impl AsyncRead for PrependStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.pos < self.buf.len() {
            let avail = &self.buf[self.pos..];
            let len = std::cmp::min(avail.len(), out.remaining());
            out.put_slice(&avail[..len]);
            self.pos += len;
            return Poll::Ready(Ok(()));
        }
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

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_query_accepts_long_token_and_short_pin() {
        let query = request_pairing(
            b"GET /?ws=2777&token=lt-pair-abc%20123&pin=004209&session=lt-session-x HTTP/1.1\r\nHost: test\r\n\r\n",
        );

        assert_eq!(query.token.as_deref(), Some("lt-pair-abc 123"));
        assert_eq!(query.pin.as_deref(), Some("004209"));
        assert_eq!(query.session_token.as_deref(), Some("lt-session-x"));
    }

    #[test]
    fn pairing_rotation_changes_credentials() {
        let first = new_pairing_state(1);
        let second = new_pairing_state(2);

        assert_ne!(first.token, second.token);
        assert_eq!(first.generation, 1);
        assert_eq!(second.generation, 2);
        assert_eq!(first.pairing_code.len(), 6);
        assert!(first.pairing_code.bytes().all(|byte| byte.is_ascii_digit()));
    }

    #[test]
    fn v2_input_requires_request_id_and_sequence() {
        let failure = validate_input_contract(2, Some("request-1"), None).unwrap_err();

        assert_eq!(failure.code, "invalid_input");
        assert!(validate_input_contract(2, Some("request-1"), Some(1)).is_ok());
        assert!(validate_input_contract(1, None, None).is_ok());
    }

    #[test]
    fn input_payload_limit_is_enforced() {
        let failure =
            validate_operation(&InputOperation::Type("x".repeat(MAX_TEXT_BYTES + 1))).unwrap_err();

        assert_eq!(failure.code, "payload_too_large");
    }

    #[test]
    fn blocklist_deduplicates_device_ids_and_legacy_ips() {
        let entries = deduplicate_blocklist(vec![
            BlockEntry {
                ip: "192.168.1.2".to_string(),
                device_name: "one".to_string(),
                device_id: Some("device-1".to_string()),
            },
            BlockEntry {
                ip: "192.168.1.3".to_string(),
                device_name: "same device".to_string(),
                device_id: Some("device-1".to_string()),
            },
            BlockEntry {
                ip: "192.168.1.4".to_string(),
                device_name: "legacy".to_string(),
                device_id: None,
            },
            BlockEntry {
                ip: "192.168.1.4".to_string(),
                device_name: "legacy duplicate".to_string(),
                device_id: None,
            },
        ]);

        assert_eq!(entries.len(), 2);
    }
}
