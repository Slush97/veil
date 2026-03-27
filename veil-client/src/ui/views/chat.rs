use esox_ui::{Ui, id};

use crate::ui::app::VeilApp;

impl VeilApp {
    pub(crate) fn draw_chat(&mut self, ui: &mut Ui) {
        let sidebar_ratio = (260.0 / self.width.max(800) as f32).clamp(0.18, 0.28);
        ui.split_pane_h_mut(id!("chat_split"), sidebar_ratio, |ui, panel| match panel {
            0 => self.draw_sidebar(ui),
            1 => self.draw_messages(ui),
            _ => {}
        });
    }
}
