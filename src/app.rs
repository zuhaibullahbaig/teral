use crate::style;
use crate::theme::ThemeConfig;
use crate::ui;
use gtk::Application;
use gtk::prelude::*;

pub fn activate(application: &Application) {
    let theme = ThemeConfig::load();
    style::apply(&theme);

    let window = ui::build_window(application, theme);
    window.present();
}
