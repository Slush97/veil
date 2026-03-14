use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use quinn::Endpoint;
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, info, warn};

use crate::directory::DirectoryStore;
use crate::mailbox::MailboxStore;
use crate::protocol::*;

/// Relay configuration — sensible defaults for low-resource devices.
pub struct RelayConfig {
    /// Address to bind to.
    pub bind_addr: SocketAddr,
    /// Max routing tags a single client can subscribe to.
    pub max_tags_per_client: usize,
    /// Max messages queued per routing tag for offline clients.
    pub max_mailbox_per_tag: usize,
    /// Max total mailbox messages across all tags.
    pub max_mailbox_total: usize,
    /// Max concurrent client connections.
    pub max_connections: usize,
    /// Max forward messages per second per client.
    pub max_forwards_per_second: u32,
    /// Path for the persistent mailbox database.
    pub db_path: PathBuf,
    /// Max age for mailbox messages before purging.
    pub max_age: Duration,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            bind_addr: ([0, 0, 0, 0], 4433).into(),
            max_tags_per_client: 256,
            max_mailbox_per_tag: 1000,
            max_mailbox_total: 50_000,
            max_connections: 1024,
            max_forwards_per_second: 50,
            db_path: PathBuf::from("./mailbox.redb"),
            max_age: Duration::from_secs(86400),
        }
    }
}

/// Identifies a connected client.
type ClientId = u64;

struct ClientState {
    /// Opaque peer identity bytes (Ed25519 public key).
    peer_id_bytes: [u8; 32],
    /// Routing tags this client subscribes to.
    subscribed_tags: HashSet<[u8; 32]>,
    /// Channel to send messages to this client's write task.
    tx: mpsc::Sender<RelayMessage>,
    /// Rate limiting: last time tokens were replenished.
    last_replenish: Instant,
    /// Rate limiting: available forward tokens.
    forward_tokens: u32,
}

struct RelayState {
    config_max_tags_per_client: usize,
    config_max_forwards_per_second: u32,
    /// Connected clients.
    clients: HashMap<ClientId, ClientState>,
    /// Routing tag → set of subscribed client IDs.
    tag_subscribers: HashMap<[u8; 32], HashSet<ClientId>>,
    /// Persistent mailbox store.
    mailbox_store: Arc<MailboxStore>,
    /// Next client ID.
    next_id: ClientId,
}

impl RelayState {
    fn new(
        max_tags_per_client: usize,
        max_forwards_per_second: u32,
        mailbox_store: Arc<MailboxStore>,
    ) -> Self {
        Self {
            config_max_tags_per_client: max_tags_per_client,
            config_max_forwards_per_second: max_forwards_per_second,
            clients: HashMap::new(),
            tag_subscribers: HashMap::new(),
            mailbox_store,
            next_id: 0,
        }
    }

    fn add_client(&mut self, peer_id_bytes: [u8; 32], tx: mpsc::Sender<RelayMessage>) -> ClientId {
        let id = self.next_id;
        self.next_id += 1;
        self.clients.insert(
            id,
            ClientState {
                peer_id_bytes,
                subscribed_tags: HashSet::new(),
                tx,
                last_replenish: Instant::now(),
                forward_tokens: self.config_max_forwards_per_second,
            },
        );
        id
    }

    fn remove_client(&mut self, client_id: ClientId) {
        if let Some(client) = self.clients.remove(&client_id) {
            for tag in &client.subscribed_tags {
                if let Some(subs) = self.tag_subscribers.get_mut(tag) {
                    subs.remove(&client_id);
                    if subs.is_empty() {
                        self.tag_subscribers.remove(tag);
                    }
                }
            }
        }
    }

    fn subscribe(&mut self, client_id: ClientId, tags: &[[u8; 32]]) -> Result<(), StatusCode> {
        let client = self
            .clients
            .get_mut(&client_id)
            .expect("client_id must exist in clients map");
        if client.subscribed_tags.len() + tags.len() > self.config_max_tags_per_client {
            return Err(StatusCode::TagLimitExceeded);
        }
        for tag in tags {
            client.subscribed_tags.insert(*tag);
            self.tag_subscribers
                .entry(*tag)
                .or_default()
                .insert(client_id);
        }
        Ok(())
    }

    fn unsubscribe(&mut self, client_id: ClientId, tags: &[[u8; 32]]) {
        if let Some(client) = self.clients.get_mut(&client_id) {
            for tag in tags {
                client.subscribed_tags.remove(tag);
                if let Some(subs) = self.tag_subscribers.get_mut(tag) {
                    subs.remove(&client_id);
                    if subs.is_empty() {
                        self.tag_subscribers.remove(tag);
                    }
                }
            }
        }
    }

    /// Check and consume a forward token for rate limiting.
    /// Returns true if the forward is allowed.
    fn check_rate_limit(&mut self, client_id: ClientId) -> bool {
        let max_tokens = self.config_max_forwards_per_second;
        let client = match self.clients.get_mut(&client_id) {
            Some(c) => c,
            None => return false,
        };

        // Replenish tokens based on elapsed time
        let now = Instant::now();
        let elapsed = now.duration_since(client.last_replenish);
        let new_tokens = (elapsed.as_secs_f64() * max_tokens as f64) as u32;
        if new_tokens > 0 {
            client.forward_tokens = (client.forward_tokens + new_tokens).min(max_tokens);
            client.last_replenish = now;
        }

        if client.forward_tokens > 0 {
            client.forward_tokens -= 1;
            true
        } else {
            false
        }
    }

    /// Forward a message to all online subscribers of a tag, and queue for offline.
    fn forward(
        &mut self,
        sender_id: ClientId,
        routing_tag: [u8; 32],
        payload: Vec<u8>,
    ) -> Vec<(ClientId, mpsc::Sender<RelayMessage>)> {
        let mut targets = Vec::new();

        if let Some(subscribers) = self.tag_subscribers.get(&routing_tag) {
            for &sub_id in subscribers {
                if sub_id == sender_id {
                    continue; // Don't echo back to sender.
                }
                if let Some(client) = self.clients.get(&sub_id) {
                    targets.push((sub_id, client.tx.clone()));
                }
            }
        }

        // Queue in persistent mailbox
        let envelope = ForwardEnvelope {
            routing_tag,
            payload,
            received_at: chrono_timestamp(),
        };
        if let Err(e) = self.mailbox_store.push(&envelope) {
            warn!("failed to persist mailbox message: {e}");
        }

        targets
    }

    /// Drain mailbox for a client's subscribed tags.
    fn drain_mailbox(&mut self, client_id: ClientId) -> (Vec<ForwardEnvelope>, u64) {
        let tags: Vec<[u8; 32]> = match self.clients.get(&client_id) {
            Some(c) => c.subscribed_tags.iter().copied().collect(),
            None => return (vec![], 0),
        };

        match self.mailbox_store.drain(&tags, 100) {
            Ok((batch, remaining)) => (batch, remaining),
            Err(e) => {
                warn!("failed to drain mailbox: {e}");
                (vec![], 0)
            }
        }
    }
}

fn chrono_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Verify an Ed25519 signature over concatenated routing tags.
fn verify_subscribe_signature(
    peer_id_bytes: &[u8; 32],
    routing_tags: &[[u8; 32]],
    signature: &[u8],
) -> bool {
    let Ok(vk) = VerifyingKey::from_bytes(peer_id_bytes) else {
        return false;
    };
    let Ok(sig_bytes): Result<[u8; 64], _> = signature.try_into() else {
        return false;
    };
    let sig = Signature::from_bytes(&sig_bytes);

    let mut payload = Vec::with_capacity(routing_tags.len() * 32);
    for tag in routing_tags {
        payload.extend_from_slice(tag);
    }

    vk.verify(&payload, &sig).is_ok()
}

/// Verify an Ed25519 signature for username registration.
fn verify_register_signature(
    public_key: &[u8; 32],
    message: &[u8],
    signature: &[u8],
) -> bool {
    let Ok(vk) = VerifyingKey::from_bytes(public_key) else {
        return false;
    };
    let Ok(sig_bytes): Result<[u8; 64], _> = signature.try_into() else {
        return false;
    };
    let sig = Signature::from_bytes(&sig_bytes);
    vk.verify(message, &sig).is_ok()
}

pub struct RelayServer {
    state: Arc<RwLock<RelayState>>,
    mailbox_store: Arc<MailboxStore>,
    directory_store: Arc<DirectoryStore>,
    config_bind_addr: SocketAddr,
    #[allow(dead_code)]
    config_max_age: Duration,
}

impl RelayServer {
    #[allow(clippy::needless_pass_by_value)]
    pub fn new(config: RelayConfig) -> Self {
        let mailbox_store = Arc::new(
            MailboxStore::open(
                &config.db_path,
                config.max_mailbox_per_tag,
                config.max_mailbox_total,
                config.max_age,
            )
            .expect("failed to open mailbox database"),
        );

        // Share the same database with the directory store
        let directory_store = Arc::new(
            DirectoryStore::new(mailbox_store.database())
                .expect("failed to initialize directory store"),
        );

        let bind_addr = config.bind_addr;
        let max_age = config.max_age;
        Self {
            state: Arc::new(RwLock::new(RelayState::new(
                config.max_tags_per_client,
                config.max_forwards_per_second,
                mailbox_store.clone(),
            ))),
            mailbox_store,
            directory_store,
            config_bind_addr: bind_addr,
            config_max_age: max_age,
        }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        let endpoint = create_relay_endpoint(self.config_bind_addr)?;
        let local_addr = endpoint.local_addr()?;
        info!("veil-relay listening on {local_addr}");

        // Spawn TTL cleanup task — purge expired messages every 5 minutes
        let mailbox = self.mailbox_store.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                match mailbox.purge_expired() {
                    Ok(0) => {}
                    Ok(n) => info!("TTL cleanup: purged {n} expired messages"),
                    Err(e) => warn!("TTL cleanup error: {e}"),
                }
            }
        });

        loop {
            let incoming = match endpoint.accept().await {
                Some(conn) => conn,
                None => break,
            };

            let state = self.state.clone();
            let directory = self.directory_store.clone();
            tokio::spawn(async move {
                match incoming.await {
                    Ok(conn) => {
                        let addr = conn.remote_address();
                        debug!("client connected: {addr}");
                        if let Err(e) = handle_client(state, directory, conn).await {
                            debug!("client {addr} disconnected: {e}");
                        }
                    }
                    Err(e) => {
                        debug!("incoming connection failed: {e}");
                    }
                }
            });
        }

        Ok(())
    }
}

async fn handle_client(
    state: Arc<RwLock<RelayState>>,
    directory: Arc<DirectoryStore>,
    conn: quinn::Connection,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = conn.remote_address();

    // Wait for Hello message.
    let hello = recv_relay_message(&conn).await?;
    let (peer_id_bytes, initial_tags, version) = match hello {
        RelayMessage::Hello {
            peer_id_bytes,
            routing_tags,
            version,
        } => (peer_id_bytes, routing_tags, version),
        _ => {
            warn!("client {addr} sent non-Hello as first message");
            return Ok(());
        }
    };

    // Accept current version and one prior for backward compatibility
    if version != RELAY_PROTOCOL_VERSION && version != RELAY_PROTOCOL_VERSION - 1 {
        send_relay_message(
            &conn,
            &RelayMessage::Status {
                code: StatusCode::BadVersion,
                message: format!("expected version {RELAY_PROTOCOL_VERSION}, got {version}"),
            },
        )
        .await?;
        return Ok(());
    }

    // Set up outbound channel for this client.
    let (tx, mut rx) = mpsc::channel::<RelayMessage>(256);

    let client_id = {
        let mut s = state.write().await;
        let id = s.add_client(peer_id_bytes, tx);
        if let Err(code) = s.subscribe(id, &initial_tags) {
            send_relay_message(
                &conn,
                &RelayMessage::Status {
                    code,
                    message: "subscription failed".into(),
                },
            )
            .await?;
            s.remove_client(id);
            return Ok(());
        }
        id
    };

    info!(
        "client {addr} registered (id={client_id}, tags={})",
        initial_tags.len()
    );

    // Send OK status.
    send_relay_message(
        &conn,
        &RelayMessage::Status {
            code: StatusCode::Ok,
            message: "connected".into(),
        },
    )
    .await?;

    // Spawn writer task: reads from channel, sends to client.
    let conn_write = conn.clone();
    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if send_relay_message(&conn_write, &msg).await.is_err() {
                break;
            }
        }
    });

    // Reader loop: process incoming messages from client.
    let result = reader_loop(state.clone(), directory, &conn, client_id).await;

    // Cleanup.
    writer.abort();
    {
        let mut s = state.write().await;
        s.remove_client(client_id);
    }
    info!("client {addr} disconnected (id={client_id})");

    result
}

async fn reader_loop(
    state: Arc<RwLock<RelayState>>,
    directory: Arc<DirectoryStore>,
    conn: &quinn::Connection,
    client_id: ClientId,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    loop {
        let msg = recv_relay_message(conn).await?;

        match msg {
            RelayMessage::Subscribe {
                routing_tags,
                signature,
            } => {
                // Verify signature before subscribing
                let peer_id_bytes = {
                    let s = state.read().await;
                    match s.clients.get(&client_id) {
                        Some(c) => c.peer_id_bytes,
                        None => continue,
                    }
                };

                if !verify_subscribe_signature(&peer_id_bytes, &routing_tags, &signature) {
                    send_relay_message(
                        conn,
                        &RelayMessage::Status {
                            code: StatusCode::RateLimited,
                            message: "invalid subscription signature".into(),
                        },
                    )
                    .await?;
                    continue;
                }

                let mut s = state.write().await;
                if let Err(code) = s.subscribe(client_id, &routing_tags) {
                    drop(s);
                    send_relay_message(
                        conn,
                        &RelayMessage::Status {
                            code,
                            message: "subscribe failed".into(),
                        },
                    )
                    .await?;
                }
            }

            RelayMessage::Unsubscribe { routing_tags } => {
                let mut s = state.write().await;
                s.unsubscribe(client_id, &routing_tags);
            }

            RelayMessage::Forward {
                routing_tag,
                payload,
            } => {
                // Rate limit check
                let (allowed, targets) = {
                    let mut s = state.write().await;
                    if !s.check_rate_limit(client_id) {
                        // Rate limited — send status but don't disconnect
                        drop(s);
                        send_relay_message(
                            conn,
                            &RelayMessage::Status {
                                code: StatusCode::RateLimited,
                                message: "forward rate limit exceeded".into(),
                            },
                        )
                        .await?;
                        continue;
                    }
                    let targets = s.forward(client_id, routing_tag, payload.clone());
                    (true, targets)
                };

                if allowed {
                    // Fan out to online subscribers.
                    for (_sub_id, tx) in targets {
                        let _ = tx.try_send(RelayMessage::Forward {
                            routing_tag,
                            payload: payload.clone(),
                        });
                    }
                }
            }

            RelayMessage::DrainMailbox => {
                let (messages, remaining) = {
                    let mut s = state.write().await;
                    s.drain_mailbox(client_id)
                };
                send_relay_message(
                    conn,
                    &RelayMessage::MailboxBatch {
                        messages,
                        remaining,
                    },
                )
                .await?;
            }

            RelayMessage::Register {
                username,
                public_key,
                signature,
            } => {
                // Verify Ed25519 signature: signs b"veil-register-v1:" || username_lowercase
                let valid = {
                    let msg = format!("veil-register-v1:{}", username.to_lowercase());
                    verify_register_signature(&public_key, msg.as_bytes(), &signature)
                };

                if !valid {
                    send_relay_message(
                        conn,
                        &RelayMessage::RegisterResult {
                            success: false,
                            message: "invalid signature".into(),
                        },
                    )
                    .await?;
                    continue;
                }

                match directory.register(&username, &public_key) {
                    Ok(crate::directory::RegisterOutcome::Success) => {
                        send_relay_message(
                            conn,
                            &RelayMessage::RegisterResult {
                                success: true,
                                message: "registered".into(),
                            },
                        )
                        .await?;
                    }
                    Ok(crate::directory::RegisterOutcome::UsernameTaken) => {
                        send_relay_message(
                            conn,
                            &RelayMessage::RegisterResult {
                                success: false,
                                message: "username taken".into(),
                            },
                        )
                        .await?;
                    }
                    Ok(crate::directory::RegisterOutcome::KeyAlreadyRegistered(existing)) => {
                        send_relay_message(
                            conn,
                            &RelayMessage::RegisterResult {
                                success: false,
                                message: format!("key already registered as @{existing}"),
                            },
                        )
                        .await?;
                    }
                    Ok(crate::directory::RegisterOutcome::InvalidUsername) => {
                        send_relay_message(
                            conn,
                            &RelayMessage::RegisterResult {
                                success: false,
                                message: "invalid username (3-20 chars, alphanumeric + underscore)"
                                    .into(),
                            },
                        )
                        .await?;
                    }
                    Err(e) => {
                        warn!("directory register error: {e}");
                        send_relay_message(
                            conn,
                            &RelayMessage::RegisterResult {
                                success: false,
                                message: "internal error".into(),
                            },
                        )
                        .await?;
                    }
                }
            }

            RelayMessage::Lookup { username } => {
                let result = directory.lookup(&username);
                match result {
                    Ok(public_key) => {
                        send_relay_message(
                            conn,
                            &RelayMessage::LookupResult {
                                username,
                                public_key,
                            },
                        )
                        .await?;
                    }
                    Err(e) => {
                        warn!("directory lookup error: {e}");
                        send_relay_message(
                            conn,
                            &RelayMessage::LookupResult {
                                username,
                                public_key: None,
                            },
                        )
                        .await?;
                    }
                }
            }

            RelayMessage::Ping(seq) => {
                send_relay_message(conn, &RelayMessage::Pong(seq)).await?;
            }

            // Ignore messages that only the relay sends.
            _ => {}
        }
    }
}

// --- Transport helpers (same framing as peer.rs) ---

async fn send_relay_message(
    conn: &quinn::Connection,
    msg: &RelayMessage,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let data = msg.encode()?;
    let mut stream = conn.open_uni().await?;
    let len = (data.len() as u32).to_be_bytes();
    stream.write_all(&len).await?;
    stream.write_all(&data).await?;
    stream.finish()?;
    Ok(())
}

async fn recv_relay_message(
    conn: &quinn::Connection,
) -> Result<RelayMessage, Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = conn.accept_uni().await?;

    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as u64;

    if len > MAX_RELAY_MESSAGE_SIZE {
        return Err(format!("message too large: {len} bytes").into());
    }

    let mut data = vec![0u8; len as usize];
    stream.read_exact(&mut data).await?;

    Ok(RelayMessage::decode(&data)?)
}

// --- QUIC endpoint setup (minimal, no app-layer crypto needed) ---

fn create_relay_endpoint(bind_addr: SocketAddr) -> Result<Endpoint, Box<dyn std::error::Error>> {
    use std::sync::Arc;

    let mut cn_bytes = [0u8; 8];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut cn_bytes);
    let cn = format!("veil-relay-{}", hex::encode(cn_bytes));

    let cert = rcgen::generate_simple_self_signed(vec![cn])?;
    let cert_der = cert.cert.der().clone();
    let key_der = cert.key_pair.serialize_der();

    let cert_chain = vec![rustls::pki_types::CertificateDer::from(cert_der.to_vec())];
    let key = rustls::pki_types::PrivateKeyDer::try_from(key_der)
        .map_err(|e| format!("invalid key: {e}"))?;

    let server_crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)?;

    let server_config = quinn::ServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)?,
    ));

    let endpoint = Endpoint::server(server_config, bind_addr)?;
    Ok(endpoint)
}
