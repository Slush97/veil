pub mod group_key;
pub mod identity;
pub mod keystore;
pub mod mnemonic;

pub use group_key::{
    EncryptError, EpochReason, GroupKey, GroupKeyRing, KeyEpoch, KeyPackage, KeyRingData,
    KeyRingEntry,
};
pub use identity::{
    DeviceCertificate, DeviceIdentity, DeviceRevocation, EphemeralKeyPair, Identity,
    MasterIdentity, PeerId,
};
pub use keystore::{
    KeystoreError, load_device_identity, load_identity, migrate_v1_to_v2, save_device_identity,
    save_identity,
};

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
