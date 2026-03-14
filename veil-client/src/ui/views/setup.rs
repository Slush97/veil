use iced::widget::{Column, button, column, container, text, text_input};
use iced::{Element, Length};

use crate::ui::app::App;
use crate::ui::message::Message;

impl App {
    pub(crate) fn view_setup(&self) -> Element<'_, Message> {
        container(
            column![
                text("Veil").size(48),
                text("Encrypted. Decentralized. Yours.").size(16),
                text_input("Passphrase (optional)", &self.passphrase_input)
                    .on_input(Message::PassphraseChanged)
                    .secure(true)
                    .padding(8)
                    .width(300),
                iced::widget::row![
                    button("Create New")
                        .on_press(Message::CreateIdentity)
                        .padding(12),
                    button("Load Existing")
                        .on_press(Message::LoadIdentity)
                        .padding(12),
                ]
                .spacing(12),
                text(self.connection_state.to_string()).size(12),
            ]
            .spacing(20)
            .align_x(iced::Alignment::Center),
        )
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
