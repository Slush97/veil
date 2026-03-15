use esox_ui::Ui;

use crate::ui::app::VeilApp;

impl VeilApp {
    pub(crate) fn draw_chat(&mut self, ui: &mut Ui) {
        ui.columns_spaced(0.0, &[0.2, 0.8], |ui, col| {
            match col {
                0 => self.draw_sidebar(ui),
                1 => self.draw_messages(ui),
                _ => {}
            }
        });
    }
}
