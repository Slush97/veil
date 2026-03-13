pub mod group_key;
pub mod identity;
pub mod keystore;

pub use group_key::{EncryptError, GroupKey};
pub use identity::{EphemeralKeyPair, Identity, PeerId};
pub use keystore::{load_identity, save_identity, KeystoreError};

/// Hardened Argon2id parameters for key derivation.
/// 64 MiB memory, 3 iterations, 1 parallelism lane.
pub(crate) fn hardened_argon2() -> argon2::Argon2<'static> {
    let params = argon2::Params::new(
        65536, // 64 MiB memory cost
        3,     // 3 iterations
        1,     // 1 parallelism lane
        Some(32),
    )
    .expect("valid argon2 params");
    argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
}
