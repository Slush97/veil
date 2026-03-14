use iced::Element;
use iced::widget::row;

use crate::ui::app::App;
use crate::ui::message::Message;

impl App {
    pub(crate) fn view_chat(&self) -> Element<'_, Message> {
        let sidebar = self.view_sidebar();
        let chat = self.view_messages();
        row![sidebar, chat].into()
    }
}
