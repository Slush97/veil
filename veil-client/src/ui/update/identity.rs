use veil_crypto::{DeviceIdentity, MasterIdentity};
use zeroize::Zeroize;

use crate::ui::app::{App, DEFAULT_RELAY};
use crate::ui::network::veil_data_dir;
use crate::ui::types::*;

impl App {
    pub(crate) fn update_create_identity(&mut self) {
        // Validate username
        let username = self.username_input.trim().to_string();
        if !username.is_empty() {
            let valid = username.len() >= 3
                && username.len() <= 20
                && username
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !valid {
                self.registration_status = Some(
                    "Username must be 3-20 characters (letters, numbers, underscore)".into(),
                );
                return;
            }
        }

        // Generate master identity + device
        let (master, phrase) = MasterIdentity::generate();
        let device_name = hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "Unknown Device".into());
        let device = DeviceIdentity::new(&master, device_name);

        self.master = Some(master);
        self.device = Some(device);

        // Set default relay if not already set
        if self.relay_addr_input.is_empty() {
            self.relay_addr_input = DEFAULT_RELAY.to_string();
        }

        // Show recovery phrase — identity will be saved after confirmation
        self.screen = Screen::ShowRecoveryPhrase(phrase);
    }

    pub(crate) fn update_confirm_recovery_phrase(&mut self) {
        // User confirmed the recovery phrase — save identity and go to chat
        let data_dir = veil_data_dir();
        std::fs::create_dir_all(&data_dir).ok();
        let keystore = data_dir.join("identity.veil");

        let (Some(master), Some(device)) = (self.master.as_ref(), self.device.as_ref()) else {
            return;
        };

        if let Err(e) = veil_crypto::save_device_identity(
            master.entropy(),
            device,
            self.passphrase_input.as_bytes(),
            &keystore,
        ) {
            self.connection_state =
                ConnectionState::Failed(format!("Failed to save identity: {e}"));
        }

        // Don't set self.username here — it gets set when registration
        // succeeds on the relay (in update_register_result). Keeping it None
        // allows update_relay_connected to trigger the registration.

        // Zeroize passphrase after use
        self.passphrase_input.zeroize();

        self.screen = Screen::Chat;
        self.setup_after_identity();

        // After identity is set up and network is ready, register username
        // This will happen when the relay connects via the network worker
    }

    pub(crate) fn update_load_identity(&mut self) {
        let data_dir = veil_data_dir();
        let keystore = data_dir.join("identity.veil");

        // Set default relay if not already set
        if self.relay_addr_input.is_empty() {
            self.relay_addr_input = DEFAULT_RELAY.to_string();
        }

        // Try v2 format first
        match veil_crypto::load_device_identity(self.passphrase_input.as_bytes(), &keystore) {
            Ok((master, device)) => {
                self.master = Some(master);
                self.device = Some(device);
                self.passphrase_input.zeroize();
                self.screen = Screen::Chat;
                self.setup_after_identity();
            }
            Err(_) => {
                // Try v1 format and migrate
                let device_name = hostname::get()
                    .ok()
                    .and_then(|h| h.into_string().ok())
                    .unwrap_or_else(|| "Unknown Device".into());

                match veil_crypto::migrate_v1_to_v2(
                    self.passphrase_input.as_bytes(),
                    &keystore,
                    device_name,
                ) {
                    Ok((master, device, phrase)) => {
                        self.master = Some(master);
                        self.device = Some(device);
                        self.passphrase_input.zeroize();
                        // Show recovery phrase for migration
                        self.screen = Screen::ShowRecoveryPhrase(phrase);
                    }
                    Err(e) => {
                        self.passphrase_input.zeroize();
                        self.connection_state =
                            ConnectionState::Failed(format!("Failed to load: {e}"));
                    }
                }
            }
        }
    }
}
