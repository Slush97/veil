use std::sync::Arc;

use veil_core::BlobId;
use veil_net::ConnectionId;

use crate::ui::app::App;
use crate::ui::message::NetCommand;
use crate::ui::types::*;

impl App {
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
                })
            {
                tracing::warn!("failed to send file command: {e}");
            }
        }
    }

    pub(crate) fn update_file_sent(&mut self, filename: String) {
        let mut cm = ChatMessage::user(
            veil_core::MessageId([0; 32]),
            self.master_peer_id(),
            format!("Sent file: {filename}"),
            chrono::Utc::now(),
        );
        cm.sender = self.resolve_display_name(&self.master_peer_id());
        cm.status = Some(MessageStatus::Sent);
        self.messages.push(cm);
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
            })
        {
            tracing::warn!("failed to send blob response: {e}");
        }
    }

    pub(crate) fn update_blob_received(&mut self, blob_id: BlobId) {
        // Find matching file/image/video messages and flip status to Available
        for msg in &mut self.messages {
            if let Some(ref mut fi) = msg.file_info
                && fi.blob_id == blob_id
                && fi.status == FileStatus::Downloading
            {
                fi.status = FileStatus::Available;
                if msg.thumbnail.is_none() && msg.audio_info.is_none() {
                    msg.content = format!("[file: {} ({})]", fi.filename, fi.size_str);
                }
            }
            if let Some(ref mut ai) = msg.audio_info
                && ai.blob_id == blob_id
                && ai.status == FileStatus::Downloading
            {
                ai.status = FileStatus::Available;
            }
        }
    }

    pub(crate) fn update_save_file(&mut self, blob_id: BlobId, filename: String) {
        if let Some(ref store) = self.store {
            match store.get_blob_full(&blob_id) {
                Ok(Some(encrypted_data)) => {
                    // Decrypt and decompress with group key
                    let decrypted = self.current_group.as_ref().and_then(|group| {
                        let ring = group.key_ring.lock().ok()?;
                        let raw = ring.current().decrypt(&encrypted_data).ok()?;
                        veil_core::decompress(&raw, 256 * 1024 * 1024).ok()
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
