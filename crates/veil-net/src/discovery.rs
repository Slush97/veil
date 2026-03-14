use std::net::SocketAddr;

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::sync::mpsc;

const SERVICE_TYPE: &str = "_veil._udp.local.";

/// An event from the mDNS discovery system.
#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    /// A new Veil peer was discovered.
    PeerFound {
        instance_name: String,
        addr: SocketAddr,
        fingerprint: String,
    },
    /// A Veil peer went away.
    PeerLost { instance_name: String },
}

/// Manages mDNS service registration and browsing for Veil peers.
pub struct Discovery {
    daemon: ServiceDaemon,
}

impl Discovery {
    pub fn new() -> Result<Self, crate::NetError> {
        let daemon =
            ServiceDaemon::new().map_err(|e| crate::NetError::Connection(e.to_string()))?;
        Ok(Self { daemon })
    }

    /// Register this peer on the local network.
    pub fn register(&self, port: u16, fingerprint: &str) -> Result<(), crate::NetError> {
        let instance_name = format!("veil-{fingerprint}");
        let host = format!("{instance_name}.local.");

        let service = ServiceInfo::new(
            SERVICE_TYPE,
            &instance_name,
            &host,
            "",
            port,
            [("fp", fingerprint)].as_slice(),
        )
        .map_err(|e| crate::NetError::Connection(e.to_string()))?;

        self.daemon
            .register(service)
            .map_err(|e| crate::NetError::Connection(e.to_string()))?;

        Ok(())
    }

    /// Browse for peers and send discovery events to the returned receiver.
    pub fn browse(&self) -> Result<mpsc::Receiver<DiscoveryEvent>, crate::NetError> {
        let receiver = self
            .daemon
            .browse(SERVICE_TYPE)
            .map_err(|e| crate::NetError::Connection(e.to_string()))?;

        let (tx, rx) = mpsc::channel(64);

        std::thread::spawn(move || {
            while let Ok(event) = receiver.recv() {
                let discovery_event = match event {
                    ServiceEvent::ServiceResolved(info) => {
                        let addr = info
                            .get_addresses()
                            .iter()
                            .next()
                            .map(|ip| SocketAddr::new(*ip, info.get_port()));
                        let fingerprint = info
                            .get_property_val_str("fp")
                            .unwrap_or_default()
                            .to_string();
                        addr.map(|addr| DiscoveryEvent::PeerFound {
                            instance_name: info.get_fullname().to_string(),
                            addr,
                            fingerprint,
                        })
                    }
                    ServiceEvent::ServiceRemoved(_, fullname) => Some(DiscoveryEvent::PeerLost {
                        instance_name: fullname,
                    }),
                    _ => None,
                };

                if let Some(evt) = discovery_event
                    && tx.blocking_send(evt).is_err()
                {
                    break;
                }
            }
        });

        Ok(rx)
    }

    /// Unregister and shut down the mDNS daemon.
    pub fn shutdown(self) -> Result<(), crate::NetError> {
        self.daemon
            .shutdown()
            .map_err(|e| crate::NetError::Connection(e.to_string()))?;
        Ok(())
    }
}
