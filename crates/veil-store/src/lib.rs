pub mod blob;
pub mod db;

pub use blob::{encode_blob, decode_blob, BlobShard};
pub use db::LocalStore;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(String),
    #[error("erasure coding error: {0}")]
    ErasureCoding(String),
    #[error("crypto error: {0}")]
    Crypto(String),
}
