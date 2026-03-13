use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use x25519_dalek::{EphemeralSecret, PublicKey as X25519Public};
use zeroize::ZeroizeOnDrop;

/// A user's long-term identity. The signing key never leaves the device.
/// Zeroized on drop to prevent key material from lingering in memory.
#[derive(ZeroizeOnDrop)]
pub struct Identity {
    signing_key: SigningKey,
}

/// Public portion of an identity, safe to share with anyone.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerId {
    pub verifying_key: Vec<u8>,
}

/// An ephemeral X25519 key pair for a single key exchange.
/// Consumed on use to ensure forward secrecy.
pub struct EphemeralKeyPair {
    secret: Option<EphemeralSecret>,
    public: X25519Public,
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
}
