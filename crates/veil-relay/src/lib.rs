//! Veil Relay — a zero-knowledge encrypted message relay.
//!
//! The relay never sees plaintext. It routes opaque ciphertext between clients
//! based on routing tags (which are themselves derived from encrypted group keys,
//! so the relay cannot determine group membership).
//!
//! Designed to run on anything: Raspberry Pi, phone (Termux), cloud VM, laptop.

pub mod directory;
pub mod mailbox;
pub mod protocol;
pub mod server;

pub use directory::DirectoryStore;
pub use mailbox::MailboxStore;
pub use protocol::RelayMessage;
pub use server::{RelayConfig, RelayServer};
