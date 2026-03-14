use std::net::SocketAddr;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use veil_crypto::Identity;
use veil_relay::protocol::{
    ForwardEnvelope, RelayMessage, RELAY_PROTOCOL_VERSION, MAX_RELAY_MESSAGE_SIZE,
};

use crate::framing;
use crate::NetError;

/// Events emitted by the relay client to the application.
#[derive(Debug)]
pub enum RelayEvent {
    Connected,
    Disconnected(String),
    Message {
        routing_tag: [u8; 32],
        payload: Vec<u8>,
    },
    MailboxDrained {
        messages: Vec<ForwardEnvelope>,
        remaining: u64,
    },
    Error {
        code: String,
        message: String,
    },
}

/// Commands sent to the relay client from the application.
#[derive(Debug)]
pub enum RelayCommand {
    Subscribe(Vec<[u8; 32]>),
    Unsubscribe(Vec<[u8; 32]>),
    Forward {
        routing_tag: [u8; 32],
        payload: Vec<u8>,
    },
    DrainMailbox,
    Shutdown,
}

/// A client that connects to a veil-relay server over QUIC.
pub struct RelayClient {
    cmd_tx: mpsc::Sender<RelayCommand>,
}

impl RelayClient {
    /// Spawn a relay client that connects to the given relay address.
    ///
    /// `identity_bytes` is the Ed25519 signing key for authenticating subscriptions.
    /// Returns the client handle and a receiver for relay events.
    pub fn spawn(
        relay_addr: SocketAddr,
        endpoint: quinn::Endpoint,
        peer_id_bytes: [u8; 32],
        identity_bytes: [u8; 32],
        initial_tags: Vec<[u8; 32]>,
    ) -> (Self, mpsc::Receiver<RelayEvent>) {
        let (cmd_tx, cmd_rx) = mpsc::channel::<RelayCommand>(256);
        let (event_tx, event_rx) = mpsc::channel::<RelayEvent>(256);

        tokio::spawn(relay_task(
            relay_addr,
            endpoint,
            peer_id_bytes,
            identity_bytes,
            initial_tags,
            cmd_rx,
            event_tx,
        ));

        (Self { cmd_tx }, event_rx)
    }

    /// Forward an encoded message to all subscribers of a routing tag.
    pub async fn forward_message(
        &self,
        routing_tag: [u8; 32],
        payload: Vec<u8>,
    ) -> Result<(), NetError> {
        self.cmd_tx
            .send(RelayCommand::Forward {
                routing_tag,
                payload,
            })
            .await
            .map_err(|_| NetError::Connection("relay client shut down".into()))
    }

    /// Subscribe to additional routing tags.
    pub async fn subscribe(&self, tags: Vec<[u8; 32]>) -> Result<(), NetError> {
        self.cmd_tx
            .send(RelayCommand::Subscribe(tags))
            .await
            .map_err(|_| NetError::Connection("relay client shut down".into()))
    }

    /// Unsubscribe from routing tags.
    pub async fn unsubscribe(&self, tags: Vec<[u8; 32]>) -> Result<(), NetError> {
        self.cmd_tx
            .send(RelayCommand::Unsubscribe(tags))
            .await
            .map_err(|_| NetError::Connection("relay client shut down".into()))
    }

    /// Request queued messages from the relay's mailbox.
    pub async fn drain_mailbox(&self) -> Result<(), NetError> {
        self.cmd_tx
            .send(RelayCommand::DrainMailbox)
            .await
            .map_err(|_| NetError::Connection("relay client shut down".into()))
    }

    /// Shut down the relay client.
    pub async fn shutdown(&self) {
        let _ = self.cmd_tx.send(RelayCommand::Shutdown).await;
    }
}

/// Sign concatenated routing tags with the identity for relay authentication.
fn sign_tags(identity_bytes: &[u8; 32], tags: &[[u8; 32]]) -> Vec<u8> {
    let identity = Identity::from_bytes(identity_bytes);
    let mut payload = Vec::with_capacity(tags.len() * 32);
    for tag in tags {
        payload.extend_from_slice(tag);
    }
    identity.sign(&payload)
}

async fn relay_task(
    relay_addr: SocketAddr,
    endpoint: quinn::Endpoint,
    peer_id_bytes: [u8; 32],
    identity_bytes: [u8; 32],
    initial_tags: Vec<[u8; 32]>,
    mut cmd_rx: mpsc::Receiver<RelayCommand>,
    event_tx: mpsc::Sender<RelayEvent>,
) {
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(60);
    let mut pending_commands: Vec<RelayCommand> = Vec::new();
    const MAX_PENDING: usize = 500;

    loop {
        match connect_and_run(
            &relay_addr,
            &endpoint,
            &peer_id_bytes,
            &identity_bytes,
            &initial_tags,
            &mut cmd_rx,
            &event_tx,
            &mut pending_commands,
        )
        .await
        {
            Ok(ShutdownReason::CommandShutdown) => {
                debug!("relay client shutting down by command");
                return;
            }
            Ok(ShutdownReason::Disconnected(reason)) => {
                warn!("relay disconnected: {reason}, reconnecting in {backoff:?}");
                let _ = event_tx
                    .send(RelayEvent::Disconnected(reason))
                    .await;
            }
            Err(e) => {
                warn!("relay connection failed: {e}, retrying in {backoff:?}");
                let _ = event_tx
                    .send(RelayEvent::Disconnected(e.to_string()))
                    .await;
            }
        }

        // Buffer commands during reconnect backoff instead of discarding
        let sleep = tokio::time::sleep(backoff);
        tokio::pin!(sleep);

        loop {
            tokio::select! {
                _ = &mut sleep => break,
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(RelayCommand::Shutdown) | None => return,
                        Some(cmd) => {
                            if pending_commands.len() < MAX_PENDING {
                                pending_commands.push(cmd);
                            }
                        }
                    }
                }
            }
        }

        // Exponential backoff
        backoff = (backoff * 2).min(max_backoff);
    }
}

enum ShutdownReason {
    CommandShutdown,
    Disconnected(String),
}

async fn connect_and_run(
    relay_addr: &SocketAddr,
    endpoint: &quinn::Endpoint,
    peer_id_bytes: &[u8; 32],
    identity_bytes: &[u8; 32],
    initial_tags: &[[u8; 32]],
    cmd_rx: &mut mpsc::Receiver<RelayCommand>,
    event_tx: &mpsc::Sender<RelayEvent>,
    pending_commands: &mut Vec<RelayCommand>,
) -> Result<ShutdownReason, Box<dyn std::error::Error + Send + Sync>> {
    // Connect to relay
    let conn = endpoint
        .connect(*relay_addr, "veil-relay")?
        .await?;

    info!("connected to relay at {relay_addr}");

    // Send Hello
    send_relay_msg(
        &conn,
        &RelayMessage::Hello {
            peer_id_bytes: *peer_id_bytes,
            routing_tags: initial_tags.to_vec(),
            version: RELAY_PROTOCOL_VERSION,
        },
    )
    .await?;

    // Wait for Status OK
    let status = recv_relay_msg(&conn).await?;
    match status {
        RelayMessage::Status {
            code: veil_relay::protocol::StatusCode::Ok,
            ..
        } => {}
        RelayMessage::Status { code, message } => {
            return Err(format!("relay rejected connection: {code:?}: {message}").into());
        }
        other => {
            return Err(format!("unexpected relay message: {other:?}").into());
        }
    }

    let _ = event_tx.send(RelayEvent::Connected).await;

    // Drain mailbox on connect
    send_relay_msg(&conn, &RelayMessage::DrainMailbox).await?;

    // Flush any commands buffered during reconnect
    let buffered: Vec<RelayCommand> = pending_commands.drain(..).collect();
    for cmd in buffered {
        match cmd {
            RelayCommand::Forward { routing_tag, payload } => {
                send_relay_msg(&conn, &RelayMessage::Forward { routing_tag, payload }).await?;
            }
            RelayCommand::Subscribe(tags) => {
                let signature = sign_tags(identity_bytes, &tags);
                send_relay_msg(&conn, &RelayMessage::Subscribe { routing_tags: tags, signature }).await?;
            }
            RelayCommand::Unsubscribe(tags) => {
                send_relay_msg(&conn, &RelayMessage::Unsubscribe { routing_tags: tags }).await?;
            }
            RelayCommand::DrainMailbox => {
                send_relay_msg(&conn, &RelayMessage::DrainMailbox).await?;
            }
            RelayCommand::Shutdown => {
                return Ok(ShutdownReason::CommandShutdown);
            }
        }
    }

    // Main loop
    let mut ping_interval = tokio::time::interval(Duration::from_secs(30));
    ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut ping_seq: u64 = 0;

    loop {
        tokio::select! {
            // Incoming relay messages
            result = recv_relay_msg(&conn) => {
                match result {
                    Ok(msg) => match msg {
                        RelayMessage::Forward { routing_tag, payload } => {
                            let _ = event_tx.send(RelayEvent::Message { routing_tag, payload }).await;
                        }
                        RelayMessage::MailboxBatch { messages, remaining } => {
                            let _ = event_tx.send(RelayEvent::MailboxDrained { messages, remaining }).await;
                        }
                        RelayMessage::Pong(_) => {}
                        RelayMessage::Status { code, message } => {
                            match code {
                                veil_relay::protocol::StatusCode::Ok => {}
                                veil_relay::protocol::StatusCode::RateLimited
                                | veil_relay::protocol::StatusCode::MailboxFull
                                | veil_relay::protocol::StatusCode::TagLimitExceeded
                                | veil_relay::protocol::StatusCode::BadVersion => {
                                    let _ = event_tx.send(RelayEvent::Error {
                                        code: format!("{code:?}"),
                                        message,
                                    }).await;
                                }
                            }
                        }
                        _ => {}
                    },
                    Err(e) => {
                        return Ok(ShutdownReason::Disconnected(e.to_string()));
                    }
                }
            }

            // Outbound commands
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(RelayCommand::Forward { routing_tag, payload }) => {
                        send_relay_msg(&conn, &RelayMessage::Forward { routing_tag, payload }).await?;
                    }
                    Some(RelayCommand::Subscribe(tags)) => {
                        let signature = sign_tags(identity_bytes, &tags);
                        send_relay_msg(&conn, &RelayMessage::Subscribe { routing_tags: tags, signature }).await?;
                    }
                    Some(RelayCommand::Unsubscribe(tags)) => {
                        send_relay_msg(&conn, &RelayMessage::Unsubscribe { routing_tags: tags }).await?;
                    }
                    Some(RelayCommand::DrainMailbox) => {
                        send_relay_msg(&conn, &RelayMessage::DrainMailbox).await?;
                    }
                    Some(RelayCommand::Shutdown) | None => {
                        return Ok(ShutdownReason::CommandShutdown);
                    }
                }
            }

            // Keepalive ping
            _ = ping_interval.tick() => {
                ping_seq += 1;
                if let Err(e) = send_relay_msg(&conn, &RelayMessage::Ping(ping_seq)).await {
                    return Ok(ShutdownReason::Disconnected(format!("ping failed: {e}")));
                }
            }
        }
    }
}

// --- Transport helpers using shared framing ---

async fn send_relay_msg(
    conn: &quinn::Connection,
    msg: &RelayMessage,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let data = msg.encode()?;
    framing::send_framed(conn, &data).await?;
    Ok(())
}

async fn recv_relay_msg(
    conn: &quinn::Connection,
) -> Result<RelayMessage, Box<dyn std::error::Error + Send + Sync>> {
    let data = framing::recv_framed(conn, MAX_RELAY_MESSAGE_SIZE).await?;
    Ok(RelayMessage::decode(&data)?)
}
