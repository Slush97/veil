use iced::widget::{Column, button, column, container, row, text, text_input};
use iced::{Element, Length};

use crate::ui::app::App;
use crate::ui::message::Message;

impl App {
    pub(crate) fn view_setup(&self) -> Element<'_, Message> {
        let mut form = column![
            text("Veil").size(48),
            text("Encrypted. Decentralized. Yours.").size(16),
        ]
        .spacing(20)
        .align_x(iced::Alignment::Center);

        // Username input
        form = form.push(
            text_input("Choose a username", &self.username_input)
                .on_input(Message::UsernameInputChanged)
                .padding(8)
                .width(300),
        );

        // Password input
        form = form.push(
            text_input("Password (optional)", &self.passphrase_input)
                .on_input(Message::PassphraseChanged)
                .secure(true)
                .padding(8)
                .width(300),
        );

        // Create / Sign In buttons
        form = form.push(
            row![
                button("Create Account")
                    .on_press(Message::CreateIdentity)
                    .padding(12),
                button("Sign In")
                    .on_press(Message::LoadIdentity)
                    .padding(12),
            ]
            .spacing(12),
        );

        // Status/error feedback
        if let Some(ref status) = self.registration_status {
            form = form.push(text(status.as_str()).size(13));
        }

        // Connection state
        let state_str = self.connection_state.to_string();
        if state_str != "Disconnected" {
            form = form.push(text(state_str).size(12));
        }

        // Advanced: relay address (collapsed by default, but show the input)
        form = form.push(
            column![
                text("Relay server").size(11),
                text_input("relay host:port", &self.relay_addr_input)
                    .on_input(Message::RelayAddrChanged)
                    .padding(6)
                    .width(300),
            ]
            .spacing(4),
        );

        container(form)
            .center(Length::Fill)
            .into()
    }

    pub(crate) fn view_recovery_phrase(&self, phrase: &str) -> Element<'_, Message> {
        let words: Vec<&str> = phrase.split_whitespace().collect();
        let mut word_rows = Column::new().spacing(8);
        for (i, word) in words.iter().enumerate() {
            word_rows = word_rows.push(text(format!("{}. {}", i + 1, word)).size(18));
        }

        container(
            column![
                text("Your Recovery Phrase").size(32),
                text("Write these 12 words down and store them safely.").size(14),
                text("You will need them to recover your identity.").size(14),
                container(word_rows.padding(16)).padding(16),
                button("I have saved my recovery phrase")
                    .on_press(Message::ConfirmRecoveryPhrase)
                    .padding(12),
            ]
            .spacing(20)
            .align_x(iced::Alignment::Center),
        )
        .center(Length::Fill)
        .into()
    }
}
