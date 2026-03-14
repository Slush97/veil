use veil_crypto::{DeviceIdentity, MasterIdentity};
use zeroize::Zeroize;

use crate::ui::app::App;
use crate::ui::network::veil_data_dir;
use crate::ui::types::*;

impl App {
    pub(crate) fn update_create_identity(&mut self) {
        // Generate master identity + device
        let (master, phrase) = MasterIdentity::generate();
        let device_name = hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "Unknown Device".into());
        let device = DeviceIdentity::new(&master, device_name);

        self.master = Some(master);
        self.device = Some(device);

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

        // Zeroize passphrase after use
        self.passphrase_input.zeroize();

        self.screen = Screen::Chat;
        self.setup_after_identity();
    }

    pub(crate) fn update_load_identity(&mut self) {
        let data_dir = veil_data_dir();
        let keystore = data_dir.join("identity.veil");

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
