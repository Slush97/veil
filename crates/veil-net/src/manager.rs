use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use quinn::Endpoint;
use tokio::sync::{mpsc, Mutex};
use veil_crypto::{EphemeralKeyPair, Identity, PeerId};

use crate::peer::PeerConnection;
use crate::protocol::{WireMessage, challenge_sign_payload};
use crate::NetError;

/// Identifies a connected peer.
pub type ConnectionId = u64;

/// An event from the peer manager.
#[derive(Debug)]
pub enum PeerEvent {
    /// A new peer connected and completed the authenticated handshake.
    Connected {
        conn_id: ConnectionId,
        peer_id: PeerId,
        /// Ephemeral DH-derived pairwise session key for this connection.
        session_key: [u8; 32],
    },
    /// A peer disconnected.
    Disconnected { conn_id: ConnectionId },
    /// Received a wire message from a peer.
    Message {
        conn_id: ConnectionId,
        peer_id: PeerId,
        message: WireMessage,
    },
}

/// Manages all peer connections, spawns recv tasks, exposes an event channel.
pub struct PeerManager {
    endpoint: Endpoint,
    connections: Arc<Mutex<HashMap<ConnectionId, Arc<PeerConnection>>>>,
    next_id: ConnectionId,
    event_tx: mpsc::Sender<PeerEvent>,
    event_rx: Option<mpsc::Receiver<PeerEvent>>,
    our_peer_id: PeerId,
    identity_bytes: [u8; 32],
}

impl PeerManager {
    pub fn new(endpoint: Endpoint, our_peer_id: PeerId, identity_bytes: [u8; 32]) -> Self {
        let (event_tx, event_rx) = mpsc::channel(256);
        Self {
            endpoint,
            connections: Arc::new(Mutex::new(HashMap::new())),
            next_id: 0,
            event_tx,
            event_rx: Some(event_rx),
            our_peer_id,
            identity_bytes,
        }
    }

    /// Take the event receiver. Can only be called once.
    pub fn take_event_receiver(&mut self) -> Option<mpsc::Receiver<PeerEvent>> {
        self.event_rx.take()
    }

    /// Get a handle to the shared connections map for accept_loop.
    pub fn connections_handle(&self) -> Arc<Mutex<HashMap<ConnectionId, Arc<PeerConnection>>>> {
        self.connections.clone()
    }

    /// Get a clone of the event sender for accept_loop.
    pub fn event_sender(&self) -> mpsc::Sender<PeerEvent> {
        self.event_tx.clone()
    }

    /// Connect to a peer at the given address and perform the authenticated handshake.
    pub async fn connect(&mut self, addr: SocketAddr) -> Result<ConnectionId, NetError> {
        let connection = self
            .endpoint
            .connect(addr, "veil-peer")
            .map_err(|e| NetError::Connection(e.to_string()))?
            .await
            .map_err(|e| NetError::Connection(e.to_string()))?;

        let peer_conn = PeerConnection::new(connection, addr);
        let conn_id = self.next_id;
        self.next_id += 1;

        // Generate challenge nonce
        let mut our_challenge = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut our_challenge);

        // Step 1: Send Hello with challenge, empty signature (initiator)
        let hello = WireMessage::Hello {
            peer_id: self.our_peer_id.clone(),
            version: 1,
            challenge: our_challenge,
            signature: vec![],
        };
        peer_conn.send(&hello).await?;

        // Step 2: Receive Hello response with their challenge + signature
        let response = peer_conn.recv().await?;
        let (peer_id, their_challenge) = match response {
            WireMessage::Hello {
                peer_id,
                version,
                challenge,
                signature,
            } => {
                if version != 1 {
                    return Err(NetError::Protocol(format!(
                        "unsupported protocol version: {version}"
                    )));
                }
                // Verify responder's signature over (our_challenge || their_peer_id)
                let sig_payload = challenge_sign_payload(&our_challenge, &peer_id);
                if !peer_id.verify(&sig_payload, &signature) {
                    return Err(NetError::Protocol(
                        "peer failed challenge-response authentication".into(),
                    ));
                }
                (peer_id, challenge)
            }
            _ => {
                return Err(NetError::Protocol(
                    "expected Hello response, got something else".into(),
                ));
            }
        };

        // Step 3: Send our ChallengeResponse (sign their_challenge || our_peer_id)
        let identity = Identity::from_bytes(&self.identity_bytes);
        let sig_payload = challenge_sign_payload(&their_challenge, &self.our_peer_id);
        let our_sig = identity.sign(&sig_payload);
        peer_conn
            .send(&WireMessage::ChallengeResponse {
                signature: our_sig,
            })
            .await?;

        // Step 4: DH key exchange - initiator sends first
        let eph = EphemeralKeyPair::generate();
        let eph_pub_bytes = *eph.public_key().as_bytes();
        peer_conn
            .send(&WireMessage::KeyExchange {
                ephemeral_public: eph_pub_bytes,
            })
            .await?;

        // Receive peer's ephemeral public key
        let their_eph_pub = match peer_conn.recv().await? {
            WireMessage::KeyExchange { ephemeral_public } => ephemeral_public,
            _ => {
                return Err(NetError::Protocol(
                    "expected KeyExchange, got something else".into(),
                ));
            }
        };

        // Derive session key
        let peer_pub = x25519_dalek::PublicKey::from(their_eph_pub);
        let shared_secret = eph.exchange(&peer_pub);
        let session_key = derive_session_key(&shared_secret, &self.our_peer_id, &peer_id);

        let peer_conn = Arc::new(peer_conn);

        // Notify connected with session key
        let _ = self
            .event_tx
            .send(PeerEvent::Connected {
                conn_id,
                peer_id: peer_id.clone(),
                session_key,
            })
            .await;

        // Spawn recv task
        let tx = self.event_tx.clone();
        let conn = peer_conn.clone();
        let pid = peer_id;
        tokio::spawn(async move {
            loop {
                match conn.recv().await {
                    Ok(msg) => {
                        if tx
                            .send(PeerEvent::Message {
                                conn_id,
                                peer_id: pid.clone(),
                                message: msg,
                            })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(_) => {
                        let _ = tx.send(PeerEvent::Disconnected { conn_id }).await;
                        break;
                    }
                }
            }
        });

        self.connections.lock().await.insert(conn_id, peer_conn);
        Ok(conn_id)
    }

    /// Send a message to a specific peer.
    pub async fn send_to(
        &self,
        conn_id: ConnectionId,
        msg: &WireMessage,
    ) -> Result<(), NetError> {
        let connections = self.connections.lock().await;
        let conn = connections
            .get(&conn_id)
            .ok_or_else(|| NetError::Connection("unknown connection".into()))?;
        conn.send(msg).await
    }

    /// Broadcast a message to all connected peers.
    pub async fn broadcast(&self, msg: &WireMessage) {
        let connections = self.connections.lock().await;
        for conn in connections.values() {
            let _ = conn.send(msg).await;
        }
    }

    /// Accept incoming connections in a loop. Call this from a spawned task.
    pub async fn accept_loop(
        endpoint: Endpoint,
        our_peer_id: PeerId,
        identity_bytes: [u8; 32],
        event_tx: mpsc::Sender<PeerEvent>,
        connections: Arc<Mutex<HashMap<ConnectionId, Arc<PeerConnection>>>>,
    ) {
        let mut next_id: ConnectionId = 1_000_000; // Offset from outbound IDs
        while let Some(incoming) = endpoint.accept().await {
            let connection = match incoming.await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("failed to accept connection: {e}");
                    continue;
                }
            };

            let addr = connection.remote_address();
            let peer_conn = PeerConnection::new(connection, addr);

            // Step 1: Receive Hello from the connecting peer (initiator)
            let (peer_id, their_challenge) = match peer_conn.recv().await {
                Ok(WireMessage::Hello {
                    peer_id,
                    version,
                    challenge,
                    ..
                }) => {
                    if version != 1 {
                        tracing::warn!("rejecting peer with unsupported version {version}");
                        continue;
                    }
                    (peer_id, challenge)
                }
                _ => {
                    tracing::warn!("peer didn't send Hello, disconnecting");
                    continue;
                }
            };

            // Step 2: Send our Hello back with signature over their challenge
            let mut our_challenge = [0u8; 32];
            rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut our_challenge);

            let identity = Identity::from_bytes(&identity_bytes);
            let sig_payload = challenge_sign_payload(&their_challenge, &our_peer_id);
            let our_sig = identity.sign(&sig_payload);

            let hello = WireMessage::Hello {
                peer_id: our_peer_id.clone(),
                version: 1,
                challenge: our_challenge,
                signature: our_sig,
            };
            if peer_conn.send(&hello).await.is_err() {
                continue;
            }

            // Step 3: Receive ChallengeResponse from initiator
            match peer_conn.recv().await {
                Ok(WireMessage::ChallengeResponse { signature }) => {
                    let sig_payload = challenge_sign_payload(&our_challenge, &peer_id);
                    if !peer_id.verify(&sig_payload, &signature) {
                        tracing::warn!(
                            "peer {} failed challenge-response authentication",
                            peer_id.fingerprint()
                        );
                        continue;
                    }
                }
                _ => {
                    tracing::warn!("expected ChallengeResponse, disconnecting");
                    continue;
                }
            }

            // Step 4: DH key exchange - receive initiator's key first, then send ours
            let their_eph_pub = match peer_conn.recv().await {
                Ok(WireMessage::KeyExchange { ephemeral_public }) => ephemeral_public,
                _ => {
                    tracing::warn!("expected KeyExchange, disconnecting");
                    continue;
                }
            };

            let eph = EphemeralKeyPair::generate();
            let eph_pub_bytes = *eph.public_key().as_bytes();
            if peer_conn
                .send(&WireMessage::KeyExchange {
                    ephemeral_public: eph_pub_bytes,
                })
                .await
                .is_err()
            {
                continue;
            }

            // Derive session key
            let peer_pub = x25519_dalek::PublicKey::from(their_eph_pub);
            let shared_secret = eph.exchange(&peer_pub);
            let session_key = derive_session_key(&shared_secret, &our_peer_id, &peer_id);

            let conn_id = next_id;
            next_id += 1;

            let peer_conn = Arc::new(peer_conn);

            // Insert into shared connections map
            connections.lock().await.insert(conn_id, peer_conn.clone());

            let _ = event_tx
                .send(PeerEvent::Connected {
                    conn_id,
                    peer_id: peer_id.clone(),
                    session_key,
                })
                .await;

            // Spawn recv task
            let tx = event_tx.clone();
            let conn = peer_conn;
            let pid = peer_id;
            tokio::spawn(async move {
                loop {
                    match conn.recv().await {
                        Ok(msg) => {
                            if tx
                                .send(PeerEvent::Message {
                                    conn_id,
                                    peer_id: pid.clone(),
                                    message: msg,
                                })
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(_) => {
                            let _ = tx.send(PeerEvent::Disconnected { conn_id }).await;
                            break;
                        }
                    }
                }
            });
        }
    }
}

/// Derive a deterministic session key from the DH shared secret and sorted peer IDs.
fn derive_session_key(
    shared_secret: &[u8; 32],
    peer_a: &PeerId,
    peer_b: &PeerId,
) -> [u8; 32] {
    let (first, second) = if peer_a.verifying_key <= peer_b.verifying_key {
        (&peer_a.verifying_key, &peer_b.verifying_key)
    } else {
        (&peer_b.verifying_key, &peer_a.verifying_key)
    };

    let mut context = Vec::with_capacity(32 + first.len() + second.len());
    context.extend_from_slice(shared_secret);
    context.extend_from_slice(first);
    context.extend_from_slice(second);
    blake3::derive_key("veil-pairwise-session", &context)
}
