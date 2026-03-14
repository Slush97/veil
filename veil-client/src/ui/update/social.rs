use std::net::SocketAddr;

use veil_core::{MessageContent, MessageKind};
use veil_crypto::PeerId;

use crate::ui::app::App;
use crate::ui::message::NetCommand;
use crate::ui::types::*;

impl App {
    pub(crate) fn update_typing_started(&mut self, peer_id: PeerId) {
        // Update or insert typing peer with current timestamp
        if let Some(entry) = self.typing_peers.iter_mut().find(|(p, _)| *p == peer_id) {
            entry.1 = std::time::Instant::now();
        } else {
            self.typing_peers.push((peer_id, std::time::Instant::now()));
        }
    }

    pub(crate) fn update_typing_stopped(&mut self, peer_id: PeerId) {
        self.typing_peers.retain(|(p, _)| *p != peer_id);
    }

    pub(crate) fn update_set_display_name(&mut self) {
        if !self.display_name_input.trim().is_empty() {
            let fp = self.master_peer_id().fingerprint();
            let name = self.display_name_input.trim().to_string();
            self.display_names.insert(fp.clone(), name.clone());
            if let Some(ref store) = self.store
                && let Err(e) = store.store_display_name(&fp, &name) {
                    tracing::warn!("failed to persist display name: {e}");
                }
            self.display_name_input.clear();
        }
    }

    pub(crate) fn update_reply_to(&mut self, idx: usize) {
        if idx < self.messages.len() {
            self.replying_to = Some(idx);
        }
    }

    pub(crate) fn update_react(&mut self, idx: usize, emoji: String) {
        if idx < self.messages.len() {
            let msg_id = self.messages[idx].id.clone();
            if let Some(msg_id) = msg_id {
                let channel_id = self
                    .current_group
                    .as_ref()
                    .map(|g| self.current_channel_id(g))
                    .unwrap_or_default();

                let content = MessageContent {
                    kind: MessageKind::Reaction {
                        target_id: msg_id.clone(),
                        emoji: emoji.clone(),
                    },
                    timestamp: chrono::Utc::now(),
                    channel_id,
                };
                if self.seal_send_persist(&content).is_some() {
                    let key = msg_id.0;
                    let our_pid = self.master_peer_id();
                    self.reactions
                        .entry(key)
                        .or_default()
                        .push((our_pid, emoji));
                }
            }
        }
    }

    pub(crate) fn update_toggle_search(&mut self) {
        self.search_active = !self.search_active;
        if !self.search_active {
            self.search_query.clear();
            self.search_results.clear();
        }
    }

    pub(crate) fn update_search_query(&mut self, query: String) {
        self.search_query = query;
        // Search in-memory messages by substring
        self.search_results = self
            .messages
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                !m.deleted
                    && m.sender != "system"
                    && m.content
                        .to_lowercase()
                        .contains(&self.search_query.to_lowercase())
            })
            .map(|(i, _)| i)
            .collect();
    }

    pub(crate) fn update_connect_discovered_peer(&mut self, addr: SocketAddr) {
        self.connection_state =
            ConnectionState::Connecting(format!("Connecting to LAN peer {addr}..."));
        if let Some(ref mut tx) = self.net_cmd_tx
            && let Err(e) = tx.try_send(NetCommand::Connect(addr)) {
                tracing::warn!("failed to send connect command: {e}");
            }
    }

    pub(crate) fn update_lan_peer_discovered(
        &mut self,
        name: String,
        addr: SocketAddr,
        fingerprint: String,
    ) {
        // Don't add ourselves
        let our_fp = self
            .master
            .as_ref()
            .map(|m| m.peer_id().fingerprint())
            .unwrap_or_default();
        if fingerprint != our_fp && !self.discovered_peers.iter().any(|(_, a, _)| *a == addr) {
            self.discovered_peers.push((name, addr, fingerprint));
        }
    }
}
