#![allow(clippy::needless_pass_by_value)]

mod ui;

use iced::application;
use tracing_subscriber::EnvFilter;

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    application("Veil", ui::App::update, ui::App::view)
        .theme(ui::App::theme)
        .subscription(ui::App::subscription)
        .run()
}
