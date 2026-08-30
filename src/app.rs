use crate::theme::ThemeConfig;
use crate::ui;
use gtk::prelude::*;
use gtk::Application;

pub fn activate(application: &Application) {
    let theme = ThemeConfig::load();
    theme.apply_css();

    let window = ui::build_window(application, &theme);
    window.present();
}
