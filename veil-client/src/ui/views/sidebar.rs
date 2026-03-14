use iced::widget::{Column, button, column, container, row, scrollable, text, text_input};
use iced::{Element, Length};

use crate::ui::app::App;
use crate::ui::message::Message;

impl App {
    pub(crate) fn view_sidebar(&self) -> Element<'_, Message> {
        let mut group_list = Column::new().spacing(4).padding(8);
        group_list = group_list.push(text("Groups").size(12));

        for group in &self.groups {
            let is_selected = self
                .current_group
                .as_ref()
                .is_some_and(|g| g.name == group.name);
            let unread = self.unread_counts.get(&group.id.0).copied().unwrap_or(0);
            let badge = if unread > 0 {
                format!(" ({})", unread)
            } else {
                String::new()
            };
            let label = if is_selected {
                text(format!("> {}{badge}", group.name)).size(14)
            } else {
                text(format!("  {}{badge}", group.name)).size(14)
            };
            group_list = group_list.push(
                button(label)
                    .on_press(Message::SelectGroup(group.name.clone()))
                    .width(Length::Fill)
                    .padding(4),
            );
        }

        let mut channel_list = Column::new().spacing(4).padding(8);
        channel_list = channel_list.push(text("Channels").size(12));

        for channel in &self.channels {
            let is_selected = self.current_channel.as_ref() == Some(channel);
            let label = if is_selected {
                text(format!("# {channel}")).size(14)
            } else {
                text(format!("  # {channel}")).size(14)
            };
            channel_list = channel_list.push(
                button(label)
                    .on_press(Message::SelectChannel(channel.clone()))
                    .width(Length::Fill)
                    .padding(2),
            );
        }

        // Members section
        let mut peers_section = Column::new().spacing(2).padding(8);
        peers_section = peers_section.push(text("Members").size(12));
        if let Some(ref master) = self.master {
            let our_name = self.resolve_display_name(&master.peer_id());
            peers_section = peers_section.push(text(format!("{our_name} (you)")).size(10));
        }
        for (_, pid) in &self.connected_peers {
            let name = self.resolve_display_name(pid);
            let is_typing = self
                .typing_peers
                .iter()
                .any(|(p, t)| p == pid && t.elapsed() < std::time::Duration::from_secs(5));
            let status = if is_typing { " (typing)" } else { " (online)" };
            peers_section = peers_section.push(text(format!("{name}{status}")).size(10));
        }

        // Set display name
        let name_section = column![
            text("Display Name").size(12),
            row![
                text_input("Your name", &self.display_name_input)
                    .on_input(Message::DisplayNameInputChanged)
                    .on_submit(Message::SetDisplayName)
                    .padding(4)
                    .width(Length::Fill),
                button("Set").on_press(Message::SetDisplayName).padding(4),
            ]
            .spacing(4),
        ]
        .spacing(4)
        .padding(8);

        // LAN Peers
        let mut lan_section = Column::new().spacing(2).padding(8);
        if !self.discovered_peers.is_empty() {
            lan_section = lan_section.push(text("LAN Peers").size(12));
            for (_, addr, fp) in &self.discovered_peers {
                let label = self
                    .display_names
                    .get(fp)
                    .cloned()
                    .unwrap_or_else(|| fp.clone());
                lan_section = lan_section.push(
                    button(text(label.to_string()).size(10))
                        .on_press(Message::ConnectDiscoveredPeer(*addr))
                        .padding(2)
                        .width(Length::Fill),
                );
            }
        }

        // Connect-to-peer input
        let connect_section = column![
            text("Connect").size(12),
            text_input("host:port", &self.connect_input)
                .on_input(Message::ConnectInputChanged)
                .on_submit(Message::ConnectToPeer)
                .padding(4)
                .width(Length::Fill),
            text(self.connection_state.to_string()).size(10),
        ]
        .spacing(4)
        .padding(8);

        // Relay section
        let relay_status_text = if self.relay_connected {
            "Relay: connected"
        } else {
            "Relay: disconnected"
        };
        let relay_section = column![
            text("Relay").size(12),
            text_input("relay host:port", &self.relay_addr_input)
                .on_input(Message::RelayAddrChanged)
                .on_submit(Message::ConnectToRelay)
                .padding(4)
                .width(Length::Fill),
            button("Connect Relay")
                .on_press(Message::ConnectToRelay)
                .padding(4),
            text(relay_status_text).size(10),
        ]
        .spacing(4)
        .padding(8);

        // Invite section
        let mut invite_section = column![
            text("Invite").size(12),
            text_input("Passphrase", &self.invite_passphrase)
                .on_input(Message::InvitePassphraseChanged)
                .secure(true)
                .padding(4)
                .width(Length::Fill),
            button("Create Invite")
                .on_press(Message::CreateInvite)
                .padding(4),
        ]
        .spacing(4)
        .padding(8);

        if let Some(ref url) = self.generated_invite_url {
            invite_section =
                invite_section.push(text_input("Invite URL", url).padding(4).width(Length::Fill));
        }

        invite_section = invite_section.push(
            text_input("Paste invite URL", &self.invite_input)
                .on_input(Message::InviteInputChanged)
                .on_submit(Message::AcceptInvite)
                .padding(4)
                .width(Length::Fill),
        );
        invite_section =
            invite_section.push(button("Join").on_press(Message::AcceptInvite).padding(4));

        // Contact search section
        let mut contact_section = column![
            text("Add Contact").size(12),
            text_input("Search @username", &self.contact_search_input)
                .on_input(Message::ContactSearchInputChanged)
                .on_submit(Message::LookupContact)
                .padding(4)
                .width(Length::Fill),
            button("Search")
                .on_press(Message::LookupContact)
                .padding(4),
        ]
        .spacing(4)
        .padding(8);

        if let Some(ref result) = self.contact_search_result {
            match result {
                crate::ui::types::ContactSearchResult::Found { username, public_key } => {
                    let un = username.clone();
                    let pk = *public_key;
                    contact_section = contact_section.push(
                        row![
                            text(format!("@{un} found")).size(11),
                            button("Add")
                                .on_press(Message::AddContact { username: un, public_key: pk })
                                .padding(4),
                        ]
                        .spacing(4),
                    );
                }
                crate::ui::types::ContactSearchResult::NotFound(username) => {
                    contact_section = contact_section.push(
                        text(format!("@{username} not found")).size(11),
                    );
                }
                crate::ui::types::ContactSearchResult::Searching => {
                    contact_section = contact_section.push(
                        text("Searching...").size(11),
                    );
                }
            }
        }

        // Show username if registered
        let mut user_section = Column::new().spacing(2).padding(8);
        if let Some(ref username) = self.username {
            user_section = user_section.push(text(format!("@{username}")).size(14));
        }

        // Settings button at bottom
        let settings_button = container(
            button("Settings")
                .on_press(Message::OpenSettings)
                .padding(6)
                .width(Length::Fill),
        )
        .padding(8);

        container(scrollable(
            column![
                user_section,
                group_list,
                channel_list,
                contact_section,
                peers_section,
                name_section,
                lan_section,
                connect_section,
                relay_section,
                invite_section,
                settings_button,
            ]
            .spacing(8)
            .width(220),
        ))
        .height(Length::Fill)
        .into()
    }
}
