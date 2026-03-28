//! WebRTC SFU voice/video module.
//!
//! Runs on a separate UDP socket from the QUIC chat endpoint.
//! Each participant gets a dedicated `str0m::Rtc` instance.
//! Media frames are forwarded opaquely — the SFU never decrypts content
//! (E2E encryption is applied by clients via Insertable Streams).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use str0m::change::SdpAnswer;
use str0m::channel::ChannelId;
use str0m::media::{Direction, MediaKind, Mid};
use str0m::net::{Protocol, Receive};
use str0m::{Event, IceConnectionState, Input, Output, RtcConfig};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

use crate::server::ClientId;

// ── Configuration ──────────────────────────────────────────────────────

/// Voice module configuration.
#[derive(Clone, Debug)]
pub struct VoiceConfig {
    /// UDP address to bind for WebRTC media (default `0.0.0.0:4434`).
    pub udp_bind_addr: SocketAddr,
    /// Maximum number of concurrent voice rooms.
    pub max_rooms: usize,
    /// Maximum participants per room.
    pub max_participants_per_room: usize,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            udp_bind_addr: "0.0.0.0:4434".parse().expect("valid addr"),
            max_rooms: 100,
            max_participants_per_room: 25,
        }
    }
}

// ── Signaling types (relay ↔ voice module) ────────────────────────────

pub type VoiceRoomId = [u8; 32];
pub type ParticipantId = u64;

/// Commands flowing from the QUIC relay into the voice module.
#[derive(Debug)]
pub enum VoiceSignal {
    Join {
        room_id: VoiceRoomId,
        group_id: [u8; 32],
        client_id: ClientId,
        peer_id_bytes: [u8; 32],
    },
    Answer {
        room_id: VoiceRoomId,
        participant_id: ParticipantId,
        sdp: String,
    },
    IceCandidate {
        room_id: VoiceRoomId,
        participant_id: ParticipantId,
        candidate: String,
    },
    Leave {
        room_id: VoiceRoomId,
        client_id: ClientId,
    },
    /// Internal: client disconnected from QUIC, clean up any voice sessions.
    ClientDisconnected { client_id: ClientId },
}

/// Signals flowing from the voice module back to the QUIC relay.
#[derive(Debug)]
pub enum VoiceSignalOut {
    Offer {
        client_id: ClientId,
        room_id: VoiceRoomId,
        participant_id: ParticipantId,
        sdp: String,
        voice_endpoint: String,
        participants: Vec<[u8; 32]>,
    },
    ParticipantJoined {
        room_id: VoiceRoomId,
        peer_id_bytes: [u8; 32],
        /// Notify these client_ids.
        notify: Vec<ClientId>,
    },
    ParticipantLeft {
        room_id: VoiceRoomId,
        peer_id_bytes: [u8; 32],
        /// Notify these client_ids.
        notify: Vec<ClientId>,
    },
    Speaking {
        room_id: VoiceRoomId,
        peer_id_bytes: [u8; 32],
        audio_level: u8,
        /// Notify these client_ids.
        notify: Vec<ClientId>,
    },
}

// ── Internal state ────────────────────────────────────────────────────

/// Pending SDP offer that we need to keep until the answer arrives.
struct PendingOffer {
    offer: str0m::change::SdpPendingOffer,
}

struct VoiceParticipant {
    id: ParticipantId,
    client_id: ClientId,
    peer_id_bytes: [u8; 32],
    rtc: str0m::Rtc,
    /// Remote socket address discovered via ICE.
    remote_addr: Option<SocketAddr>,
    /// Mids this participant is sending to us (we receive from them).
    sending_mids: Vec<Mid>,
    /// Map: sendonly mid on this participant → source participant whose audio it carries.
    forwarding_map: HashMap<Mid, ParticipantId>,
    /// Data channel for SDP renegotiation (if established).
    data_channel: Option<ChannelId>,
    /// Pending SDP offer awaiting answer.
    pending_offer: Option<PendingOffer>,
    /// Queue of participant IDs that need renegotiation (sendonly track added) once
    /// the current pending offer is resolved.
    pending_renegotiations: Vec<ParticipantId>,
}

struct VoiceRoom {
    room_id: VoiceRoomId,
    #[allow(dead_code)]
    group_id: [u8; 32],
    participants: HashMap<ParticipantId, VoiceParticipant>,
    next_participant_id: ParticipantId,
}

impl VoiceRoom {
    fn new(room_id: VoiceRoomId, group_id: [u8; 32]) -> Self {
        Self {
            room_id,
            group_id,
            participants: HashMap::new(),
            next_participant_id: 1,
        }
    }

    fn participant_peer_ids(&self) -> Vec<[u8; 32]> {
        self.participants
            .values()
            .map(|p| p.peer_id_bytes)
            .collect()
    }

    fn client_ids(&self) -> Vec<ClientId> {
        self.participants.values().map(|p| p.client_id).collect()
    }

    fn client_ids_except(&self, exclude: ClientId) -> Vec<ClientId> {
        self.participants
            .values()
            .filter(|p| p.client_id != exclude)
            .map(|p| p.client_id)
            .collect()
    }

    fn find_by_client(&self, client_id: ClientId) -> Option<ParticipantId> {
        self.participants
            .values()
            .find(|p| p.client_id == client_id)
            .map(|p| p.id)
    }
}

// ── Voice Module ──────────────────────────────────────────────────────

pub struct VoiceModule {
    udp_socket: UdpSocket,
    local_addr: SocketAddr,
    rooms: HashMap<VoiceRoomId, VoiceRoom>,
    /// Reverse lookup: socket address → (room_id, participant_id).
    addr_to_participant: HashMap<SocketAddr, (VoiceRoomId, ParticipantId)>,
    /// Reverse lookup: client_id → (room_id, participant_id).
    client_to_participant: HashMap<ClientId, (VoiceRoomId, ParticipantId)>,
    config: VoiceConfig,
}

impl VoiceModule {
    pub async fn new(
        config: VoiceConfig,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let udp_socket = UdpSocket::bind(config.udp_bind_addr).await?;
        let local_addr = udp_socket.local_addr()?;
        info!("Voice module listening on UDP {local_addr}");
        Ok(Self {
            udp_socket,
            local_addr,
            rooms: HashMap::new(),
            addr_to_participant: HashMap::new(),
            client_to_participant: HashMap::new(),
            config,
        })
    }

    /// Main event loop. Runs until the signal channel closes.
    pub async fn run(
        mut self,
        mut signal_rx: mpsc::Receiver<VoiceSignal>,
        signal_tx: mpsc::Sender<VoiceSignalOut>,
    ) {
        let mut buf = vec![0u8; 65535]; // Max UDP datagram
        let mut next_timeout = Instant::now() + Duration::from_millis(100);

        loop {
            tokio::select! {
                // 1. Incoming UDP packet
                result = self.udp_socket.recv_from(&mut buf) => {
                    match result {
                        Ok((len, source)) => {
                            self.handle_udp_packet(&buf[..len], source);
                        }
                        Err(e) => {
                            warn!("UDP recv error: {e}");
                        }
                    }
                }

                // 2. Signaling from the QUIC relay
                signal = signal_rx.recv() => {
                    match signal {
                        Some(s) => self.handle_signal(s, &signal_tx).await,
                        None => {
                            info!("Voice signal channel closed, shutting down");
                            return;
                        }
                    }
                }

                // 3. Timer tick — drive str0m state machines
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(next_timeout)) => {}
            }

            // After any event, poll all Rtc instances for outputs.
            next_timeout = self.poll_all_rtc(&signal_tx).await;
        }
    }

    // ── UDP packet routing ────────────────────────────────────────────

    fn handle_udp_packet(&mut self, data: &[u8], source: SocketAddr) {
        let now = Instant::now();
        let dest = self.local_addr;

        // Try to route to known participant first (fast path).
        if let Some(&(room_id, participant_id)) = self.addr_to_participant.get(&source) {
            if let Some(room) = self.rooms.get_mut(&room_id) {
                if let Some(participant) = room.participants.get_mut(&participant_id) {
                    if let Ok(receive) = Receive::new(Protocol::Udp, source, dest, data) {
                        let input = Input::Receive(now, receive);
                        if let Err(e) = participant.rtc.handle_input(input) {
                            debug!("Rtc input error for participant {participant_id}: {e}");
                        }
                    }
                    return;
                }
            }
        }

        // Slow path: check all Rtc instances (new ICE candidate pairs).
        for room in self.rooms.values_mut() {
            for participant in room.participants.values_mut() {
                // Create a Receive to test with accepts().
                let Ok(test_receive) = Receive::new(Protocol::Udp, source, dest, data) else {
                    return;
                };
                let test_input = Input::Receive(now, test_receive);
                if participant.rtc.accepts(&test_input) {
                    // It accepted — feed it.
                    if let Err(e) = participant.rtc.handle_input(test_input) {
                        debug!("Rtc input error: {e}");
                    }
                    // Update address mapping for fast path next time.
                    self.addr_to_participant
                        .insert(source, (room.room_id, participant.id));
                    participant.remote_addr = Some(source);
                    return;
                }
            }
        }

        trace!("Unroutable UDP packet from {source} ({} bytes)", data.len());
    }

    // ── Signaling handlers ────────────────────────────────────────────

    async fn handle_signal(
        &mut self,
        signal: VoiceSignal,
        signal_tx: &mpsc::Sender<VoiceSignalOut>,
    ) {
        match signal {
            VoiceSignal::Join {
                room_id,
                group_id,
                client_id,
                peer_id_bytes,
            } => {
                self.handle_join(room_id, group_id, client_id, peer_id_bytes, signal_tx)
                    .await;
            }
            VoiceSignal::Answer {
                room_id,
                participant_id,
                sdp,
            } => {
                if let Some((room_id, existing_pid, new_pid)) =
                    self.handle_answer(room_id, participant_id, &sdp)
                {
                    self.renegotiate_existing_participant(
                        room_id,
                        existing_pid,
                        new_pid,
                        signal_tx,
                    )
                    .await;
                }
            }
            VoiceSignal::IceCandidate {
                room_id,
                participant_id,
                candidate,
            } => {
                self.handle_ice_candidate(room_id, participant_id, &candidate);
            }
            VoiceSignal::Leave { room_id, client_id } => {
                self.handle_leave(room_id, client_id, signal_tx).await;
            }
            VoiceSignal::ClientDisconnected { client_id } => {
                self.handle_client_disconnect(client_id, signal_tx).await;
            }
        }
    }

    async fn handle_join(
        &mut self,
        room_id: VoiceRoomId,
        group_id: [u8; 32],
        client_id: ClientId,
        peer_id_bytes: [u8; 32],
        signal_tx: &mpsc::Sender<VoiceSignalOut>,
    ) {
        // Check limits.
        if self.rooms.len() >= self.config.max_rooms && !self.rooms.contains_key(&room_id) {
            warn!("Max rooms reached, rejecting join");
            return;
        }

        // If client is already in a room, leave it first.
        if let Some((old_room_id, _)) = self.client_to_participant.get(&client_id).copied() {
            self.handle_leave(old_room_id, client_id, signal_tx).await;
        }

        let room = self
            .rooms
            .entry(room_id)
            .or_insert_with(|| VoiceRoom::new(room_id, group_id));

        if room.participants.len() >= self.config.max_participants_per_room {
            warn!("Room full, rejecting join");
            return;
        }

        let participant_id = room.next_participant_id;
        room.next_participant_id += 1;

        // Create str0m Rtc instance in ICE-lite mode (server has known address).
        let mut rtc = RtcConfig::new().set_ice_lite(true).build(Instant::now());

        // Register our local address as an ICE candidate.
        if let Ok(candidate) = str0m::Candidate::host(self.local_addr, Protocol::Udp) {
            rtc.add_local_candidate(candidate);
        }

        // Collect existing participant IDs before mutating.
        let existing_pids: Vec<ParticipantId> = room.participants.keys().copied().collect();

        // Create SDP offer: receive audio from this peer, send audio for
        // each existing participant.
        let mut sdp_api = rtc.sdp_api();

        // Audio track: we receive audio from this participant.
        let _recv_mid = sdp_api.add_media(MediaKind::Audio, Direction::RecvOnly, None, None, None);

        // For each existing participant, add a sendonly audio track so we can
        // forward their audio to this new joiner. Capture the Mid for each.
        let mut forwarding_map = HashMap::new();
        for &existing_pid in &existing_pids {
            let mid = sdp_api.add_media(MediaKind::Audio, Direction::SendOnly, None, None, None);
            forwarding_map.insert(mid, existing_pid);
        }

        // Add data channel for potential renegotiation.
        sdp_api.add_channel("signaling".to_string());

        let Some((offer, pending_offer)) = sdp_api.apply() else {
            error!("Failed to create SDP offer: no changes");
            return;
        };

        let offer_sdp = offer.to_sdp_string();
        let current_participants = room.participant_peer_ids();
        let notify_clients = room.client_ids_except(client_id);

        let participant = VoiceParticipant {
            id: participant_id,
            client_id,
            peer_id_bytes,
            rtc,
            remote_addr: None,
            sending_mids: Vec::new(),
            forwarding_map,
            data_channel: None,
            pending_offer: Some(PendingOffer {
                offer: pending_offer,
            }),
            pending_renegotiations: Vec::new(),
        };

        room.participants.insert(participant_id, participant);
        self.client_to_participant
            .insert(client_id, (room_id, participant_id));

        info!(
            "Participant {participant_id} (client {client_id}) joined voice room {}",
            hex::encode(&room_id[..8])
        );

        // Send offer to the joining client.
        let _ = signal_tx
            .send(VoiceSignalOut::Offer {
                client_id,
                room_id,
                participant_id,
                sdp: offer_sdp,
                voice_endpoint: self.local_addr.to_string(),
                participants: current_participants,
            })
            .await;

        // Notify existing participants about the new joiner.
        if !notify_clients.is_empty() {
            let _ = signal_tx
                .send(VoiceSignalOut::ParticipantJoined {
                    room_id,
                    peer_id_bytes,
                    notify: notify_clients,
                })
                .await;
        }

        // Renegotiate existing participants so they get a sendonly track
        // carrying the new joiner's audio.
        for &existing_pid in &existing_pids {
            self.renegotiate_existing_participant(room_id, existing_pid, participant_id, signal_tx)
                .await;
        }
    }

    /// Handle an SDP answer. Returns `Some((room_id, participant_id, next_pid))` if
    /// a queued renegotiation should be triggered next.
    fn handle_answer(
        &mut self,
        room_id: VoiceRoomId,
        participant_id: ParticipantId,
        sdp: &str,
    ) -> Option<(VoiceRoomId, ParticipantId, ParticipantId)> {
        let Some(room) = self.rooms.get_mut(&room_id) else {
            warn!("Answer for unknown room");
            return None;
        };
        let Some(participant) = room.participants.get_mut(&participant_id) else {
            warn!("Answer for unknown participant {participant_id}");
            return None;
        };

        let Some(pending) = participant.pending_offer.take() else {
            warn!("Answer without pending offer for participant {participant_id}");
            return None;
        };

        let answer = match SdpAnswer::from_sdp_string(sdp) {
            Ok(a) => a,
            Err(e) => {
                error!("Failed to parse SDP answer: {e}");
                return None;
            }
        };

        let sdp_api = participant.rtc.sdp_api();
        if let Err(e) = sdp_api.accept_answer(pending.offer, answer) {
            error!("Failed to accept SDP answer: {e}");
            return None;
        }

        // Drain one queued renegotiation if any.
        if !participant.pending_renegotiations.is_empty() {
            let next_pid = participant.pending_renegotiations.remove(0);
            Some((room_id, participant_id, next_pid))
        } else {
            None
        }
    }

    fn handle_ice_candidate(
        &mut self,
        room_id: VoiceRoomId,
        participant_id: ParticipantId,
        candidate: &str,
    ) {
        let Some(room) = self.rooms.get_mut(&room_id) else {
            return;
        };
        let Some(participant) = room.participants.get_mut(&participant_id) else {
            return;
        };

        if let Ok(cand) = str0m::Candidate::from_sdp_string(candidate) {
            participant.rtc.add_remote_candidate(cand);
        } else {
            debug!("Failed to parse ICE candidate: {candidate}");
        }
    }

    async fn handle_leave(
        &mut self,
        room_id: VoiceRoomId,
        client_id: ClientId,
        signal_tx: &mpsc::Sender<VoiceSignalOut>,
    ) {
        let Some(room) = self.rooms.get_mut(&room_id) else {
            return;
        };
        let Some(participant_id) = room.find_by_client(client_id) else {
            return;
        };

        let peer_id_bytes = room
            .participants
            .get(&participant_id)
            .map(|p| p.peer_id_bytes)
            .unwrap_or_default();

        room.participants.remove(&participant_id);
        self.client_to_participant.remove(&client_id);

        // Clean up forwarding maps and renegotiation queues on remaining participants.
        for remaining in room.participants.values_mut() {
            remaining
                .forwarding_map
                .retain(|_, &mut src_pid| src_pid != participant_id);
            remaining
                .pending_renegotiations
                .retain(|&pid| pid != participant_id);
        }

        // Clean up address mapping.
        self.addr_to_participant
            .retain(|_, (rid, pid)| !(*rid == room_id && *pid == participant_id));

        let notify = room.client_ids();

        info!(
            "Participant {participant_id} left voice room {}",
            hex::encode(&room_id[..8])
        );

        // Remove empty rooms.
        if room.participants.is_empty() {
            self.rooms.remove(&room_id);
            debug!("Removed empty voice room {}", hex::encode(&room_id[..8]));
        }

        if !notify.is_empty() {
            let _ = signal_tx
                .send(VoiceSignalOut::ParticipantLeft {
                    room_id,
                    peer_id_bytes,
                    notify,
                })
                .await;
        }
    }

    async fn handle_client_disconnect(
        &mut self,
        client_id: ClientId,
        signal_tx: &mpsc::Sender<VoiceSignalOut>,
    ) {
        if let Some((room_id, _)) = self.client_to_participant.get(&client_id).copied() {
            self.handle_leave(room_id, client_id, signal_tx).await;
        }
    }

    /// Renegotiate an existing participant's connection to add a sendonly track
    /// for a newly joined participant's audio.
    async fn renegotiate_existing_participant(
        &mut self,
        room_id: VoiceRoomId,
        existing_pid: ParticipantId,
        new_pid: ParticipantId,
        signal_tx: &mpsc::Sender<VoiceSignalOut>,
    ) {
        let Some(room) = self.rooms.get_mut(&room_id) else {
            return;
        };
        let Some(participant) = room.participants.get_mut(&existing_pid) else {
            return;
        };

        // If there's already a pending offer, queue this renegotiation.
        if participant.pending_offer.is_some() {
            debug!("Queueing renegotiation for participant {existing_pid} (pending offer exists)");
            participant.pending_renegotiations.push(new_pid);
            return;
        }

        let mut sdp_api = participant.rtc.sdp_api();
        let mid = sdp_api.add_media(MediaKind::Audio, Direction::SendOnly, None, None, None);

        let Some((offer, pending_offer)) = sdp_api.apply() else {
            warn!("No SDP changes for renegotiation of participant {existing_pid}");
            return;
        };

        // This sendonly mid carries audio from new_pid.
        participant.forwarding_map.insert(mid, new_pid);
        participant.pending_offer = Some(PendingOffer {
            offer: pending_offer,
        });

        let client_id = participant.client_id;
        let offer_sdp = offer.to_sdp_string();
        let current_participants = room.participant_peer_ids();

        info!("Renegotiating participant {existing_pid} for new joiner {new_pid}");

        let _ = signal_tx
            .send(VoiceSignalOut::Offer {
                client_id,
                room_id,
                participant_id: existing_pid,
                sdp: offer_sdp,
                voice_endpoint: self.local_addr.to_string(),
                participants: current_participants,
            })
            .await;
    }

    // ── Polling all Rtc instances ─────────────────────────────────────

    /// Poll all Rtc instances for outputs. Returns the next timeout.
    async fn poll_all_rtc(&mut self, signal_tx: &mpsc::Sender<VoiceSignalOut>) -> Instant {
        let mut earliest_timeout = Instant::now() + Duration::from_millis(100);

        // Collect media events to forward after iterating.
        let mut media_to_forward: Vec<(VoiceRoomId, ParticipantId, str0m::media::MediaData)> =
            Vec::new();
        let mut disconnected: Vec<(VoiceRoomId, ClientId)> = Vec::new();

        for room in self.rooms.values_mut() {
            let room_id = room.room_id;

            for participant in room.participants.values_mut() {
                let now = Instant::now();

                // Feed a timeout input to drive internal timers.
                let _ = participant.rtc.handle_input(Input::Timeout(now));

                // Poll for outputs.
                loop {
                    match participant.rtc.poll_output() {
                        Ok(Output::Timeout(t)) => {
                            if t < earliest_timeout {
                                earliest_timeout = t;
                            }
                            break;
                        }
                        Ok(Output::Transmit(transmit)) => {
                            if let Err(e) = self
                                .udp_socket
                                .send_to(&transmit.contents, transmit.destination)
                                .await
                            {
                                debug!("UDP send error: {e}");
                            }
                        }
                        Ok(Output::Event(event)) => {
                            match event {
                                Event::Connected => {
                                    info!("Participant {} WebRTC connected", participant.id);
                                }
                                Event::IceConnectionStateChange(state) => {
                                    debug!("Participant {} ICE state: {state:?}", participant.id);
                                    if state == IceConnectionState::Disconnected {
                                        disconnected.push((room_id, participant.client_id));
                                    }
                                }
                                Event::MediaAdded(media_added) => {
                                    let mid = media_added.mid;
                                    let direction = media_added.direction;
                                    debug!(
                                        "Participant {} media added: mid={mid:?} dir={direction:?} kind={:?}",
                                        participant.id, media_added.kind
                                    );
                                    // If we receive from them, track it as a sending mid.
                                    if direction == Direction::RecvOnly
                                        || direction == Direction::SendRecv
                                    {
                                        participant.sending_mids.push(mid);
                                    }
                                }
                                Event::MediaData(media_data) => {
                                    media_to_forward.push((room_id, participant.id, media_data));
                                }
                                Event::ChannelOpen(ch_id, _label) => {
                                    participant.data_channel = Some(ch_id);
                                    debug!("Participant {} data channel opened", participant.id);
                                }
                                Event::EgressBitrateEstimate(_bwe) => {
                                    // Bandwidth estimation — can use for quality adaptation.
                                }
                                _ => {}
                            }
                        }
                        Err(e) => {
                            debug!("Rtc poll error for participant {}: {e}", participant.id);
                            disconnected.push((room_id, participant.client_id));
                            break;
                        }
                    }
                }
            }
        }

        // Forward media to other participants in the same room.
        for (room_id, sender_id, media_data) in media_to_forward {
            self.forward_media(room_id, sender_id, media_data).await;
        }

        // Handle disconnections.
        for (room_id, client_id) in disconnected {
            self.handle_leave(room_id, client_id, signal_tx).await;
        }

        earliest_timeout
    }

    // ── Media forwarding (the SFU hot path) ───────────────────────────

    async fn forward_media(
        &mut self,
        room_id: VoiceRoomId,
        sender_id: ParticipantId,
        media_data: str0m::media::MediaData,
    ) {
        let Some(room) = self.rooms.get_mut(&room_id) else {
            return;
        };

        // Collect the target participant IDs (everyone except the sender).
        let targets: Vec<ParticipantId> = room
            .participants
            .keys()
            .filter(|&&pid| pid != sender_id)
            .copied()
            .collect();

        for target_id in targets {
            let Some(target) = room.participants.get_mut(&target_id) else {
                continue;
            };

            // Find a forwarding track for this sender on this target.
            let writer_mid = target
                .forwarding_map
                .iter()
                .find(|&(_, &src_pid)| src_pid == sender_id)
                .map(|(&mid, _)| mid);

            if let Some(mid) = writer_mid {
                if let Some(writer) = target.rtc.writer(mid) {
                    if let Err(e) = writer.write(
                        media_data.pt,
                        media_data.network_time,
                        media_data.time,
                        media_data.data.clone(),
                    ) {
                        trace!("Media write error to participant {target_id}: {e}");
                    }
                }
            }
        }
    }
}
