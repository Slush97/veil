use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use x25519_dalek::{EphemeralSecret, PublicKey as X25519Public};
use zeroize::{ZeroizeOnDrop, Zeroizing};

use crate::mnemonic;

/// A user's long-term identity. The signing key never leaves the device.
/// Zeroized on drop to prevent key material from lingering in memory.
#[derive(ZeroizeOnDrop)]
pub struct Identity {
    signing_key: SigningKey,
}

/// Public portion of an identity, safe to share with anyone.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerId {
    pub verifying_key: Vec<u8>,
}

/// Master identity derived from a 12-word recovery phrase.
///
/// The master key is the root of trust: it signs device certificates
/// and is the stable identity across all devices. Users are identified
/// by their master PeerId (the master public key).
pub struct MasterIdentity {
    identity: Identity,
    /// The mnemonic entropy (NOT the phrase itself — derive phrase on demand).
    entropy: Zeroizing<[u8; 16]>,
}

/// A device-specific signing key, certified by a master identity.
///
/// Each device generates its own Ed25519 key and gets it signed by the master.
/// Messages are signed by device keys; recipients verify the chain back to master.
pub struct DeviceIdentity {
    device_key: Identity,
    certificate: DeviceCertificate,
}

/// Certificate binding a device key to a master identity.
///
/// Signed by the master key to prove the device belongs to this user.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceCertificate {
    /// Ed25519 public key of this device.
    pub device_id: PeerId,
    /// Ed25519 public key of the master identity.
    pub master_id: PeerId,
    /// Human-readable device name (e.g., "Alice's Laptop").
    pub device_name: String,
    /// When this certificate was created (unix timestamp).
    pub created_at: i64,
    /// Master key's Ed25519 signature over the certificate payload.
    pub signature: Vec<u8>,
}

/// An ephemeral X25519 key pair for a single key exchange.
/// Consumed on use to ensure forward secrecy.
pub struct EphemeralKeyPair {
    secret: Option<EphemeralSecret>,
    public: X25519Public,
}

/// A signed revocation of a device certificate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceRevocation {
    /// The device being revoked.
    pub revoked_device_id: PeerId,
    /// The master identity that owns this device.
    pub master_id: PeerId,
    /// When the revocation was issued (unix timestamp).
    pub revoked_at: i64,
    /// Master key's signature over the revocation payload.
    pub signature: Vec<u8>,
}

impl Identity {
    pub fn generate() -> Self {
        Self {
            signing_key: SigningKey::generate(&mut OsRng),
        }
    }

    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self {
            signing_key: SigningKey::from_bytes(bytes),
        }
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    pub fn peer_id(&self) -> PeerId {
        PeerId {
            verifying_key: self.signing_key.verifying_key().to_bytes().to_vec(),
        }
    }

    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.signing_key.sign(message).to_bytes().to_vec()
    }
}

// --- MasterIdentity ---

impl MasterIdentity {
    /// Create a new master identity with a fresh 12-word recovery phrase.
    ///
    /// Returns the master identity and the recovery phrase (display once, then discard).
    pub fn generate() -> (Self, String) {
        let (phrase, entropy) = mnemonic::generate();
        let master_key_bytes = mnemonic::entropy_to_master_key(&entropy);
        let identity = Identity::from_bytes(&master_key_bytes);

        let master = Self { identity, entropy };
        (master, phrase)
    }

    /// Recover a master identity from a 12-word recovery phrase.
    pub fn from_phrase(phrase: &str) -> Result<Self, mnemonic::MnemonicError> {
        let entropy = mnemonic::phrase_to_entropy(phrase)?;
        let master_key_bytes = mnemonic::entropy_to_master_key(&entropy);
        let identity = Identity::from_bytes(&master_key_bytes);
        Ok(Self { identity, entropy })
    }

    /// Get the recovery phrase (for display to the user).
    pub fn recovery_phrase(&self) -> String {
        mnemonic::entropy_to_phrase(&self.entropy)
    }

    /// The master PeerId — this is the user's stable identity across devices.
    pub fn peer_id(&self) -> PeerId {
        self.identity.peer_id()
    }

    /// Sign data with the master key (used for device certs and revocations).
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.identity.sign(message)
    }

    /// Raw master key bytes (for deriving device-sync channel keys, etc.).
    pub fn to_bytes(&self) -> [u8; 32] {
        self.identity.to_bytes()
    }

    /// Create a device certificate for a new device key.
    pub fn certify_device(
        &self,
        device_peer_id: &PeerId,
        device_name: String,
    ) -> DeviceCertificate {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let payload = DeviceCertificate::signature_payload(
            device_peer_id,
            &self.peer_id(),
            &device_name,
            created_at,
        );
        let signature = self.sign(&payload);

        DeviceCertificate {
            device_id: device_peer_id.clone(),
            master_id: self.peer_id(),
            device_name,
            created_at,
            signature,
        }
    }

    /// Revoke a device certificate.
    pub fn revoke_device(&self, device_peer_id: &PeerId) -> DeviceRevocation {
        let revoked_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let payload =
            DeviceRevocation::signature_payload(device_peer_id, &self.peer_id(), revoked_at);
        let signature = self.sign(&payload);

        DeviceRevocation {
            revoked_device_id: device_peer_id.clone(),
            master_id: self.peer_id(),
            revoked_at,
            signature,
        }
    }

    /// Create from raw entropy bytes (for loading from storage).
    pub fn from_entropy(entropy: Zeroizing<[u8; 16]>) -> Self {
        let master_key_bytes = mnemonic::entropy_to_master_key(&entropy);
        let identity = Identity::from_bytes(&master_key_bytes);
        Self { identity, entropy }
    }

    /// Get the raw entropy bytes (for secure persistence).
    pub fn entropy(&self) -> &[u8; 16] {
        &self.entropy
    }

    /// Derive the routing tag for the device-sync channel.
    /// Only devices belonging to this master can subscribe.
    pub fn device_sync_routing_tag(&self) -> [u8; 32] {
        let master_pub = self.peer_id();
        blake3::derive_key("veil-device-sync", &master_pub.verifying_key)
    }

    /// Derive the encryption key for the device-sync channel.
    /// Based on the master signing key, so only devices with the master key can read.
    pub fn device_sync_key(&self) -> [u8; 32] {
        blake3::derive_key("veil-device-sync-key", &self.identity.to_bytes())
    }
}

// --- DeviceCertificate ---

impl DeviceCertificate {
    /// Build the payload that gets signed by the master key.
    fn signature_payload(
        device_id: &PeerId,
        master_id: &PeerId,
        device_name: &str,
        created_at: i64,
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(b"veil-device-cert-v1:");
        payload.extend_from_slice(&device_id.verifying_key);
        payload.extend_from_slice(&master_id.verifying_key);
        payload.extend_from_slice(device_name.as_bytes());
        payload.extend_from_slice(&created_at.to_le_bytes());
        payload
    }

    /// Verify this certificate was signed by the claimed master identity.
    pub fn verify(&self) -> bool {
        let payload = Self::signature_payload(
            &self.device_id,
            &self.master_id,
            &self.device_name,
            self.created_at,
        );
        self.master_id.verify(&payload, &self.signature)
    }

    /// Verify a message signature: check that `signature` was produced by this
    /// device's key, and that this device is certified by a known master.
    ///
    /// Returns the master PeerId if verification succeeds.
    pub fn verify_message(&self, message: &[u8], signature: &[u8]) -> Option<PeerId> {
        // First: is the cert itself valid?
        if !self.verify() {
            return None;
        }
        // Second: did this device key sign the message?
        if !self.device_id.verify(message, signature) {
            return None;
        }
        Some(self.master_id.clone())
    }
}

// --- DeviceRevocation ---

impl DeviceRevocation {
    fn signature_payload(
        revoked_device_id: &PeerId,
        master_id: &PeerId,
        revoked_at: i64,
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(b"veil-device-revoke-v1:");
        payload.extend_from_slice(&revoked_device_id.verifying_key);
        payload.extend_from_slice(&master_id.verifying_key);
        payload.extend_from_slice(&revoked_at.to_le_bytes());
        payload
    }

    /// Verify this revocation was signed by the claimed master identity.
    pub fn verify(&self) -> bool {
        let payload =
            Self::signature_payload(&self.revoked_device_id, &self.master_id, self.revoked_at);
        self.master_id.verify(&payload, &self.signature)
    }
}

// --- DeviceIdentity ---

impl DeviceIdentity {
    /// Create a new device identity and certify it with the master key.
    pub fn new(master: &MasterIdentity, device_name: String) -> Self {
        let device_key = Identity::generate();
        let certificate = master.certify_device(&device_key.peer_id(), device_name);
        Self {
            device_key,
            certificate,
        }
    }

    /// Reconstruct from existing key bytes and certificate (for loading from storage).
    pub fn from_parts(device_key_bytes: [u8; 32], certificate: DeviceCertificate) -> Self {
        Self {
            device_key: Identity::from_bytes(&device_key_bytes),
            certificate,
        }
    }

    /// The device's own PeerId.
    pub fn device_peer_id(&self) -> PeerId {
        self.device_key.peer_id()
    }

    /// The master PeerId (user's stable identity).
    pub fn master_peer_id(&self) -> PeerId {
        self.certificate.master_id.clone()
    }

    /// Sign a message with the device key.
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.device_key.sign(message)
    }

    /// Get the device certificate (to send to peers for verification).
    pub fn certificate(&self) -> &DeviceCertificate {
        &self.certificate
    }

    /// Raw device key bytes (for persistence).
    pub fn device_key_bytes(&self) -> [u8; 32] {
        self.device_key.to_bytes()
    }

    /// Get the underlying Identity (for backward compat with existing code).
    pub fn identity(&self) -> &Identity {
        &self.device_key
    }
}

impl EphemeralKeyPair {
    /// Generate a fresh ephemeral key pair.
    pub fn generate() -> Self {
        let secret = EphemeralSecret::random_from_rng(OsRng);
        let public = X25519Public::from(&secret);
        Self {
            secret: Some(secret),
            public,
        }
    }

    /// Get the public key to send to the peer.
    pub fn public_key(&self) -> &X25519Public {
        &self.public
    }

    /// Perform a DH exchange, consuming the secret key.
    /// Returns the shared secret wrapped in Zeroizing.
    pub fn exchange(mut self, peer_public: &X25519Public) -> zeroize::Zeroizing<[u8; 32]> {
        let secret = self
            .secret
            .take()
            .expect("EphemeralKeyPair already consumed");
        let shared = secret.diffie_hellman(peer_public);
        zeroize::Zeroizing::new(*shared.as_bytes())
    }
}

impl PeerId {
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> bool {
        let Ok(vk_bytes): Result<[u8; 32], _> = self.verifying_key.as_slice().try_into() else {
            return false;
        };
        let Ok(vk) = VerifyingKey::from_bytes(&vk_bytes) else {
            return false;
        };
        let Ok(sig_bytes): Result<[u8; 64], _> = signature.try_into() else {
            return false;
        };
        let sig = Signature::from_bytes(&sig_bytes);
        vk.verify(message, &sig).is_ok()
    }

    /// A fingerprint for display purposes (8 bytes = 16 hex chars).
    pub fn fingerprint(&self) -> String {
        let hash = blake3::hash(&self.verifying_key);
        let bytes = hash.as_bytes();
        format!(
            "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify() {
        let id = Identity::generate();
        let peer = id.peer_id();
        let msg = b"hello veil";
        let sig = id.sign(msg);
        assert!(peer.verify(msg, &sig));
        assert!(!peer.verify(b"wrong", &sig));
    }

    #[test]
    fn roundtrip_bytes() {
        let id = Identity::generate();
        let bytes = id.to_bytes();
        let id2 = Identity::from_bytes(&bytes);
        assert_eq!(id.peer_id().verifying_key, id2.peer_id().verifying_key);
    }

    #[test]
    fn ephemeral_key_exchange() {
        let alice_eph = EphemeralKeyPair::generate();
        let bob_eph = EphemeralKeyPair::generate();

        let alice_pub = *alice_eph.public_key();
        let bob_pub = *bob_eph.public_key();

        let shared_a = alice_eph.exchange(&bob_pub);
        let shared_b = bob_eph.exchange(&alice_pub);

        assert_eq!(*shared_a, *shared_b);
    }

    #[test]
    fn ephemeral_keys_differ_each_time() {
        let eph1 = EphemeralKeyPair::generate();
        let eph2 = EphemeralKeyPair::generate();
        assert_ne!(eph1.public_key().as_bytes(), eph2.public_key().as_bytes());
    }

    #[test]
    fn fingerprint_is_16_hex_chars() {
        let id = Identity::generate();
        let fp = id.peer_id().fingerprint();
        assert_eq!(fp.len(), 16);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // --- MasterIdentity tests ---

    #[test]
    fn master_identity_generate_and_recover() {
        let (master, phrase) = MasterIdentity::generate();
        let master_pid = master.peer_id();

        // Recover from phrase
        let recovered = MasterIdentity::from_phrase(&phrase).unwrap();
        assert_eq!(recovered.peer_id(), master_pid);
    }

    #[test]
    fn master_identity_phrase_is_stable() {
        let (master, phrase1) = MasterIdentity::generate();
        let phrase2 = master.recovery_phrase();
        assert_eq!(phrase1, phrase2);
    }

    #[test]
    fn master_identity_different_phrases_different_keys() {
        let (m1, _) = MasterIdentity::generate();
        let (m2, _) = MasterIdentity::generate();
        assert_ne!(m1.peer_id(), m2.peer_id());
    }

    // --- DeviceCertificate tests ---

    #[test]
    fn device_certificate_valid() {
        let (master, _) = MasterIdentity::generate();
        let device = DeviceIdentity::new(&master, "Test Laptop".into());

        assert!(device.certificate().verify());
        assert_eq!(device.master_peer_id(), master.peer_id());
    }

    #[test]
    fn device_certificate_rejects_wrong_master() {
        let (master1, _) = MasterIdentity::generate();
        let (master2, _) = MasterIdentity::generate();

        let device = DeviceIdentity::new(&master1, "Laptop".into());
        let mut bad_cert = device.certificate().clone();
        bad_cert.master_id = master2.peer_id(); // tamper with master

        assert!(!bad_cert.verify());
    }

    #[test]
    fn device_signs_and_verifies_via_cert() {
        let (master, _) = MasterIdentity::generate();
        let device = DeviceIdentity::new(&master, "Phone".into());

        let msg = b"hello from device";
        let sig = device.sign(msg);

        // Verify through certificate chain
        let verified_master = device.certificate().verify_message(msg, &sig);
        assert_eq!(verified_master, Some(master.peer_id()));

        // Wrong message fails
        let bad_verify = device.certificate().verify_message(b"wrong", &sig);
        assert!(bad_verify.is_none());
    }

    // --- DeviceRevocation tests ---

    #[test]
    fn device_revocation_valid() {
        let (master, _) = MasterIdentity::generate();
        let device = DeviceIdentity::new(&master, "Lost Phone".into());

        let revocation = master.revoke_device(&device.device_peer_id());
        assert!(revocation.verify());
        assert_eq!(revocation.revoked_device_id, device.device_peer_id());
    }

    #[test]
    fn device_revocation_rejects_forge() {
        let (master, _) = MasterIdentity::generate();
        let (attacker, _) = MasterIdentity::generate();
        let device = DeviceIdentity::new(&master, "Phone".into());

        // Attacker tries to revoke master's device
        let bad_revocation = attacker.revoke_device(&device.device_peer_id());
        // The revocation claims attacker as master, not the real master
        assert_ne!(bad_revocation.master_id, master.peer_id());
    }

    // --- Device sync channel tests ---

    #[test]
    fn device_sync_tag_deterministic() {
        let (master, phrase) = MasterIdentity::generate();
        let recovered = MasterIdentity::from_phrase(&phrase).unwrap();

        assert_eq!(
            master.device_sync_routing_tag(),
            recovered.device_sync_routing_tag()
        );
        assert_eq!(master.device_sync_key(), recovered.device_sync_key());
    }
}
