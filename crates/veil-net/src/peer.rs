use std::net::SocketAddr;
use std::sync::Arc;

use quinn::{Endpoint, ServerConfig};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use veil_crypto::PeerId;

use crate::framing;
use crate::protocol::{WireMessage, MAX_WIRE_MESSAGE_SIZE};
use crate::NetError;

/// A connection to a single peer.
pub struct PeerConnection {
    pub peer_id: Option<PeerId>,
    pub addr: SocketAddr,
    connection: quinn::Connection,
}

impl PeerConnection {
    pub fn new(connection: quinn::Connection, addr: SocketAddr) -> Self {
        Self {
            peer_id: None,
            addr,
            connection,
        }
    }

    /// Send a wire message to this peer.
    pub async fn send(&self, msg: &WireMessage) -> Result<(), NetError> {
        let data = msg.encode().map_err(|e| NetError::Serialization(e.to_string()))?;
        framing::send_framed(&self.connection, &data).await
    }

    /// Receive a wire message from this peer.
    pub async fn recv(&self) -> Result<WireMessage, NetError> {
        let data = framing::recv_framed(&self.connection, MAX_WIRE_MESSAGE_SIZE).await?;
        WireMessage::decode(&data).map_err(|e| NetError::Serialization(e.to_string()))
    }
}

/// Create a QUIC endpoint for peer-to-peer communication.
/// Uses a self-signed certificate — authentication is done at the application layer
/// via Ed25519 identities, not TLS certificates.
pub fn create_endpoint(bind_addr: SocketAddr) -> Result<Endpoint, NetError> {
    let (server_config, _cert) =
        self_signed_config().map_err(|e| NetError::Connection(e.to_string()))?;

    let mut endpoint = Endpoint::server(server_config, bind_addr)
        .map_err(|e| NetError::Connection(e.to_string()))?;

    // Set up client TLS config that accepts all certs.
    // Veil authenticates via Ed25519 identities, not TLS certificates.
    let client_crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerifier))
        .with_no_client_auth();

    let client_config = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto)
            .map_err(|e| NetError::Connection(e.to_string()))?,
    ));

    endpoint.set_default_client_config(client_config);

    Ok(endpoint)
}

fn self_signed_config() -> Result<(ServerConfig, Vec<u8>), Box<dyn std::error::Error>> {
    // Randomize the CN to avoid fingerprinting based on a static string
    let mut cn_bytes = [0u8; 8];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut cn_bytes);
    let cn = hex::encode(cn_bytes);

    let cert = rcgen::generate_simple_self_signed(vec![cn])?;
    let cert_der = cert.cert.der().clone();
    let key_der = cert.key_pair.serialize_der();

    let cert_chain = vec![rustls::pki_types::CertificateDer::from(cert_der.to_vec())];
    let key = rustls::pki_types::PrivateKeyDer::try_from(key_der)
        .map_err(|e| format!("invalid key: {e}"))?;

    let server_crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)?;

    let server_config = ServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)?,
    ));

    Ok((server_config, cert.cert.der().to_vec()))
}

/// TLS certificate verifier that accepts all certificates.
/// Veil authenticates peers via Ed25519 signatures, not TLS.
#[derive(Debug)]
struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
