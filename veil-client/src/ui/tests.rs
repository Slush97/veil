use std::sync::Arc;

use tokio::sync::mpsc;
use veil_core::{ChannelId, GroupId, MessageId};
use veil_crypto::{DeviceIdentity, GroupKey, GroupKeyRing, MasterIdentity};
use veil_store::LocalStore;

use super::app::App;
use super::message::NetCommand;
use super::types::*;

/// Create a fully wired test App with identity, group, store, and channel.
fn test_app() -> (App, mpsc::Receiver<NetCommand>) {
    let (master, _phrase) = MasterIdentity::generate();
    let device = DeviceIdentity::new(&master, "test-device".into());
    let group_key = GroupKey::generate();
    let master_vk = master.peer_id().verifying_key.clone();
    let group_id_bytes = [42u8; 32];
    let keyring = GroupKeyRing::new(group_key, master_vk);

    let group_state = GroupState {
        name: "Test Group".into(),
        id: GroupId(group_id_bytes),
        key_ring: Arc::new(std::sync::Mutex::new(keyring)),
        device_certs: Vec::new(),
        members: Vec::new(),
    };

    let tmp = tempfile::NamedTempFile::new().expect("temp file");
    let storage_key = LocalStore::derive_storage_key(&master.to_bytes());
    let store = LocalStore::open(tmp.path(), storage_key).expect("open store");

    let (tx, rx) = mpsc::channel(64);

    let mut app = App::default();
    app.master = Some(master);
    app.device = Some(device);
    app.groups = vec![group_state.clone()];
    app.current_group = Some(group_state);
    app.store = Some(Arc::new(store));
    app.net_cmd_tx = Some(tx);
    app.channels = vec!["general".into()];
    app.current_channel = Some("general".into());

    (app, rx)
}

// ─── Helper tests ───

#[test]
fn seal_send_persist_returns_sealed() {
    let (mut app, _rx) = test_app();
    let content = veil_core::MessageContent {
        kind: veil_core::MessageKind::Text("hello".into()),
        timestamp: chrono::Utc::now(),
        channel_id: ChannelId::new(),
    };
    let result = app.seal_send_persist(&content);
    assert!(result.is_some());
}

#[test]
fn seal_send_persist_none_without_device() {
    let (mut app, _rx) = test_app();
    app.device = None;
    let content = veil_core::MessageContent {
        kind: veil_core::MessageKind::Text("hello".into()),
        timestamp: chrono::Utc::now(),
        channel_id: ChannelId::new(),
    };
    assert!(app.seal_send_persist(&content).is_none());
}

#[test]
fn seal_send_persist_none_without_group() {
    let (mut app, _rx) = test_app();
    app.current_group = None;
    let content = veil_core::MessageContent {
        kind: veil_core::MessageKind::Text("hello".into()),
        timestamp: chrono::Utc::now(),
        channel_id: ChannelId::new(),
    };
    assert!(app.seal_send_persist(&content).is_none());
}

// ─── Settings tests ───

#[test]
fn toggle_theme_switches_dark_to_light() {
    let (mut app, _rx) = test_app();
    assert_eq!(app.theme_choice, ThemeChoice::Dark);
    app.update_toggle_theme();
    assert_eq!(app.theme_choice, ThemeChoice::Light);
    app.update_toggle_theme();
    assert_eq!(app.theme_choice, ThemeChoice::Dark);
}

#[test]
fn toggle_theme_persists_to_store() {
    let (mut app, _rx) = test_app();
    app.update_toggle_theme();
    assert_eq!(app.theme_choice, ThemeChoice::Light);

    let store = app.store.as_ref().unwrap();
    let val = store.get_setting("theme").unwrap();
    assert_eq!(val.as_deref(), Some("light"));
}

#[test]
fn toggle_notifications() {
    let (mut app, _rx) = test_app();
    assert!(app.notifications_enabled);
    app.update_toggle_notifications();
    assert!(!app.notifications_enabled);
    app.update_toggle_notifications();
    assert!(app.notifications_enabled);
}

#[test]
fn escape_clears_editing() {
    let (mut app, _rx) = test_app();
    app.editing_message = Some(0);
    app.message_input = "draft".into();
    app.update_escape_pressed();
    assert!(app.editing_message.is_none());
    assert!(app.message_input.is_empty());
}

#[test]
fn escape_clears_reply_when_not_editing() {
    let (mut app, _rx) = test_app();
    app.replying_to = Some(0);
    app.update_escape_pressed();
    assert!(app.replying_to.is_none());
}

#[test]
fn escape_clears_search_when_not_replying() {
    let (mut app, _rx) = test_app();
    app.search_active = true;
    app.search_query = "test".into();
    app.search_results = vec![0, 1];
    app.update_escape_pressed();
    assert!(!app.search_active);
    assert!(app.search_query.is_empty());
    assert!(app.search_results.is_empty());
}

// ─── Chat tests ───

#[test]
fn send_message_seals_and_sends() {
    let (mut app, mut rx) = test_app();
    app.message_input = "hello world".into();
    app.update_send();

    assert!(app.message_input.is_empty());
    assert_eq!(app.messages.len(), 1);
    assert_eq!(app.messages[0].content, "hello world");
    assert_eq!(app.messages[0].status, Some(MessageStatus::Sent));

    // NetCommand should have been sent
    assert!(rx.try_recv().is_ok());
}

#[test]
fn send_empty_message_does_nothing() {
    let (mut app, mut rx) = test_app();
    app.message_input = "   ".into();
    app.update_send();

    assert!(app.messages.is_empty());
    assert!(rx.try_recv().is_err()); // nothing sent
}

#[test]
fn send_without_group_does_nothing() {
    let (mut app, mut rx) = test_app();
    app.current_group = None;
    app.message_input = "hello".into();
    app.update_send();

    assert!(app.messages.is_empty());
    assert!(rx.try_recv().is_err());
}

#[test]
fn send_queues_when_no_channel() {
    let (mut app, _rx) = test_app();
    app.net_cmd_tx = None;
    app.message_input = "offline msg".into();
    app.update_send();

    assert_eq!(app.messages.len(), 1);
    assert_eq!(app.messages[0].status, Some(MessageStatus::Sending));
    assert_eq!(app.pending_messages.len(), 1);
}

#[test]
fn edit_message_sets_editing_state() {
    let (mut app, _rx) = test_app();
    let our_id = app.master_peer_id();
    app.messages.push(ChatMessage {
        id: Some(MessageId::from_content(b"test")),
        sender: "me".into(),
        sender_id: Some(our_id),
        content: "original".into(),
        timestamp: "12:00".into(),
        datetime: None,
        edited: false,
        deleted: false,
        status: None,
        reply_to_content: None,
        reply_to_sender: None,
        channel_id: None,
        file_info: None,
        pinned: false,
    });

    app.update_edit_message(0);
    assert_eq!(app.editing_message, Some(0));
    assert_eq!(app.message_input, "original");
}

#[test]
fn edit_others_message_denied() {
    let (mut app, _rx) = test_app();
    let (other_master, _) = MasterIdentity::generate();
    app.messages.push(ChatMessage {
        id: Some(MessageId::from_content(b"test")),
        sender: "other".into(),
        sender_id: Some(other_master.peer_id()),
        content: "their msg".into(),
        timestamp: "12:00".into(),
        datetime: None,
        edited: false,
        deleted: false,
        status: None,
        reply_to_content: None,
        reply_to_sender: None,
        channel_id: None,
        file_info: None,
        pinned: false,
    });

    app.update_edit_message(0);
    assert!(app.editing_message.is_none());
}

#[test]
fn confirm_edit_sends_and_updates() {
    let (mut app, mut rx) = test_app();
    let our_id = app.master_peer_id();
    let msg_id = MessageId::from_content(b"test");
    app.messages.push(ChatMessage {
        id: Some(msg_id),
        sender: "me".into(),
        sender_id: Some(our_id),
        content: "original".into(),
        timestamp: "12:00".into(),
        datetime: None,
        edited: false,
        deleted: false,
        status: None,
        reply_to_content: None,
        reply_to_sender: None,
        channel_id: None,
        file_info: None,
        pinned: false,
    });

    app.editing_message = Some(0);
    app.message_input = "edited text".into();
    app.update_confirm_edit();

    assert!(app.editing_message.is_none());
    assert!(app.message_input.is_empty());
    assert_eq!(app.messages[0].content, "edited text");
    assert!(app.messages[0].edited);

    // NetCommand should have been sent (seal_send_persist sends it)
    assert!(rx.try_recv().is_ok());
}

#[test]
fn delete_message_marks_deleted() {
    let (mut app, mut rx) = test_app();
    let our_id = app.master_peer_id();
    let msg_id = MessageId::from_content(b"test");
    app.messages.push(ChatMessage {
        id: Some(msg_id),
        sender: "me".into(),
        sender_id: Some(our_id),
        content: "to delete".into(),
        timestamp: "12:00".into(),
        datetime: None,
        edited: false,
        deleted: false,
        status: None,
        reply_to_content: None,
        reply_to_sender: None,
        channel_id: None,
        file_info: None,
        pinned: false,
    });

    app.update_delete_message(0);

    assert!(app.messages[0].deleted);
    assert_eq!(app.messages[0].content, "[deleted]");

    assert!(rx.try_recv().is_ok());
}

// ─── Social tests ───

#[test]
fn toggle_search() {
    let (mut app, _rx) = test_app();
    assert!(!app.search_active);
    app.update_toggle_search();
    assert!(app.search_active);
    app.update_toggle_search();
    assert!(!app.search_active);
}

#[test]
fn search_query_filters_messages() {
    let (mut app, _rx) = test_app();
    let our_id = app.master_peer_id();
    app.messages.push(ChatMessage {
        id: None,
        sender: "user".into(),
        sender_id: Some(our_id.clone()),
        content: "hello world".into(),
        timestamp: "12:00".into(),
        datetime: None,
        edited: false,
        deleted: false,
        status: None,
        reply_to_content: None,
        reply_to_sender: None,
        channel_id: None,
        file_info: None,
        pinned: false,
    });
    app.messages.push(ChatMessage {
        id: None,
        sender: "user".into(),
        sender_id: Some(our_id),
        content: "goodbye".into(),
        timestamp: "12:01".into(),
        datetime: None,
        edited: false,
        deleted: false,
        status: None,
        reply_to_content: None,
        reply_to_sender: None,
        channel_id: None,
        file_info: None,
        pinned: false,
    });

    app.update_search_query("hello".into());
    assert_eq!(app.search_results, vec![0]);
}

#[test]
fn set_display_name_persists() {
    let (mut app, _rx) = test_app();
    let fp = app.master_peer_id().fingerprint();
    app.display_name_input = "Alice".into();
    app.update_set_display_name();

    assert_eq!(
        app.display_names.get(&fp).map(String::as_str),
        Some("Alice")
    );
    assert!(app.display_name_input.is_empty());

    let store = app.store.as_ref().unwrap();
    let names = store.list_display_names().unwrap();
    assert!(names.iter().any(|(f, n)| f == &fp && n == "Alice"));
}

#[test]
fn react_sends_and_tracks() {
    let (mut app, mut rx) = test_app();
    let msg_id = MessageId::from_content(b"test");
    let our_id = app.master_peer_id();
    app.messages.push(ChatMessage {
        id: Some(msg_id.clone()),
        sender: "other".into(),
        sender_id: None,
        content: "nice".into(),
        timestamp: "12:00".into(),
        datetime: None,
        edited: false,
        deleted: false,
        status: None,
        reply_to_content: None,
        reply_to_sender: None,
        channel_id: None,
        file_info: None,
        pinned: false,
    });

    app.update_react(0, "\u{1F44D}".into());

    let reactions = app.reactions.get(&msg_id.0).unwrap();
    assert_eq!(reactions.len(), 1);
    assert_eq!(reactions[0].0, our_id);
    assert_eq!(reactions[0].1, "\u{1F44D}");

    assert!(rx.try_recv().is_ok());
}

#[test]
fn reply_to_sets_state() {
    let (mut app, _rx) = test_app();
    app.messages.push(ChatMessage {
        id: Some(MessageId::from_content(b"test")),
        sender: "other".into(),
        sender_id: None,
        content: "hey".into(),
        timestamp: "12:00".into(),
        datetime: None,
        edited: false,
        deleted: false,
        status: None,
        reply_to_content: None,
        reply_to_sender: None,
        channel_id: None,
        file_info: None,
        pinned: false,
    });

    app.update_reply_to(0);
    assert_eq!(app.replying_to, Some(0));
}

// ─── Relay tests ───

#[test]
fn relay_connected_flushes_pending() {
    let (mut app, mut rx) = test_app();

    // Create a pending sealed message
    let content = veil_core::MessageContent {
        kind: veil_core::MessageKind::Text("pending".into()),
        timestamp: chrono::Utc::now(),
        channel_id: ChannelId::new(),
    };
    let sealed = app.seal_send_persist(&content).unwrap();
    // Drain what seal_send_persist sent
    let _ = rx.try_recv();

    app.pending_messages.push(sealed);
    app.messages.push(ChatMessage {
        id: None,
        sender: "me".into(),
        sender_id: None,
        content: "pending".into(),
        timestamp: "12:00".into(),
        datetime: None,
        edited: false,
        deleted: false,
        status: Some(MessageStatus::Sending),
        reply_to_content: None,
        reply_to_sender: None,
        channel_id: None,
        file_info: None,
        pinned: false,
    });

    app.update_relay_connected();

    assert!(app.pending_messages.is_empty());
    assert!(app.relay_connected);
    assert_eq!(app.messages[0].status, Some(MessageStatus::Sent));

    // Should have flushed via channel
    assert!(rx.try_recv().is_ok());
}

#[test]
fn relay_disconnected_sets_reconnecting() {
    let (mut app, _rx) = test_app();
    app.relay_connected = true;
    app.update_relay_disconnected();

    assert!(!app.relay_connected);
    assert_eq!(app.connection_state, ConnectionState::Reconnecting);
}

// ─── Invite tests ───

#[test]
fn create_invite_sends_command() {
    let (mut app, mut rx) = test_app();
    app.update_create_invite();

    assert!(rx.try_recv().is_ok());
}

#[test]
fn accept_invite_zeroizes_passphrase() {
    let (mut app, _rx) = test_app();
    app.invite_input = "veil://invite/test".into();
    app.invite_passphrase = "secret123".into();
    app.update_accept_invite();

    // Passphrase should be zeroized (all null bytes)
    assert!(app.invite_passphrase.bytes().all(|b| b == 0));
}
