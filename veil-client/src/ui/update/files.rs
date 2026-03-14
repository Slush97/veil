use std::sync::Arc;

use veil_core::BlobId;
use veil_net::ConnectionId;

use crate::ui::app::App;
use crate::ui::message::{Message, NetCommand};
use crate::ui::types::*;

impl App {
    pub(crate) fn update_pick_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_file() {
            self.update(Message::SendFile(path));
        }
    }

    pub(crate) fn update_send_file(&mut self, path: std::path::PathBuf) {
        if let Some(ref group) = self.current_group {
            let Some(device) = self.device.as_ref() else {
                return;
            };
            let Ok(ring) = group.key_ring.lock() else {
                tracing::error!("key ring lock poisoned");
                return;
            };
            let group_key = Arc::new(ring.current().duplicate());
            drop(ring);

            if let Some(ref store) = self.store
                && let Some(ref mut tx) = self.net_cmd_tx
                && let Err(e) = tx.try_send(NetCommand::SendFile {
                    path,
                    group_id: group.id.clone(),
                    group_key,
                    store: store.clone(),
                    identity_bytes: device.device_key_bytes(),
                }) {
                    tracing::warn!("failed to send file command: {e}");
                }
        }
    }

    pub(crate) fn update_file_sent(&mut self, filename: String) {
        self.messages.push(ChatMessage {
            id: None,
            sender: self.resolve_display_name(&self.master_peer_id()),
            sender_id: Some(self.master_peer_id()),
            content: format!("Sent file: {filename}"),
            timestamp: chrono::Utc::now().format("%H:%M").to_string(),
            datetime: Some(chrono::Utc::now()),
            edited: false,
            deleted: false,
            status: Some(MessageStatus::Sent),
            reply_to_content: None,
            reply_to_sender: None,
            channel_id: None,
            file_info: None,
        });
    }

    pub(crate) fn update_file_failed(&mut self, err: String) {
        self.messages
            .push(ChatMessage::system(format!("File send failed: {err}")));
    }

    pub(crate) fn update_blob_requested(&mut self, conn_id: ConnectionId, blob_id: BlobId) {
        // Look up the full blob in our store and send it back
        if let Some(ref store) = self.store
            && let Ok(Some(data)) = store.get_blob_full(&blob_id)
            && let Some(ref mut tx) = self.net_cmd_tx
            && let Err(e) = tx.try_send(NetCommand::BlobResponse {
                conn_id,
                blob_id,
                data,
            }) {
                tracing::warn!("failed to send blob response: {e}");
            }
    }

    pub(crate) fn update_blob_received(&mut self, blob_id: BlobId) {
        // Find matching file messages and flip status to Available
        for msg in &mut self.messages {
            if let Some(ref mut fi) = msg.file_info
                && fi.blob_id == blob_id
                && fi.status == FileStatus::Downloading
            {
                fi.status = FileStatus::Available;
                msg.content = format!("[file: {} ({})]", fi.filename, fi.size_str);
            }
        }
    }

    pub(crate) fn update_save_file(&mut self, blob_id: BlobId, filename: String) {
        if let Some(ref store) = self.store {
            match store.get_blob_full(&blob_id) {
                Ok(Some(encrypted_data)) => {
                    // Decrypt with group key
                    let decrypted = self.current_group.as_ref().and_then(|group| {
                        let ring = group.key_ring.lock().ok()?;
                        ring.current().decrypt(&encrypted_data).ok()
                    });

                    match decrypted {
                        Some(data) => {
                            if let Some(path) =
                                rfd::FileDialog::new().set_file_name(&filename).save_file()
                            {
                                if let Err(e) = std::fs::write(&path, &data) {
                                    self.messages.push(ChatMessage::system(format!(
                                        "Failed to save file: {e}"
                                    )));
                                } else {
                                    self.messages.push(ChatMessage::system(format!(
                                        "File saved to {}",
                                        path.display()
                                    )));
                                }
                            }
                        }
                        None => {
                            self.messages
                                .push(ChatMessage::system("Failed to decrypt file".into()));
                        }
                    }
                }
                Ok(None) => {
                    self.messages
                        .push(ChatMessage::system("File data not available".into()));
                }
                Err(e) => {
                    self.messages
                        .push(ChatMessage::system(format!("Failed to load file: {e}")));
                }
            }
        }
    }
}
