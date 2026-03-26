use esox_ui::Ui;

use crate::ui::app::VeilApp;

impl VeilApp {
    pub(crate) fn draw_chat(&mut self, ui: &mut Ui) {
        let sidebar_ratio = (260.0 / self.width.max(800) as f32).clamp(0.18, 0.28);
        ui.columns_spaced(0.0, &[sidebar_ratio, 1.0 - sidebar_ratio], |ui, col| match col {
            0 => self.draw_sidebar(ui),
            1 => self.draw_messages(ui),
            _ => {}
        });
    }
}
