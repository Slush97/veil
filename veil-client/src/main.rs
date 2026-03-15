#![allow(clippy::needless_pass_by_value)]

mod ui;

use esox_platform::config::{PlatformConfig, WindowConfig};
use tracing_subscriber::EnvFilter;

fn main() -> Result<(), esox_platform::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config = PlatformConfig {
        window: WindowConfig {
            title: "Veil".into(),
            width: Some(1200),
            height: Some(800),
            ..Default::default()
        },
        ..Default::default()
    };

    esox_platform::run(config, Box::new(ui::VeilApp::new()))
}
