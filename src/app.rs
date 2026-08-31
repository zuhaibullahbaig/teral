use crate::config::{self, Config};
use crate::style;
use crate::theme::ThemeConfig;
use crate::ui;
use gtk::Application;
use gtk::prelude::*;

pub fn activate(application: &Application) {
    crate::tags::init();
    let config = Config::load();
    let theme = ThemeConfig::resolve(&config);
    config::set_current(config.clone());
    style::apply(&theme);

    // Wear whatever icon this desktop already uses for its file manager, instead of the
    // blank placeholder an application with no installed icon gets.
    if let Some(icon) = crate::icons::file_manager_icon_name() {
        gtk::Window::set_default_icon_name(&icon);
    }

    let window = ui::build_window(application, config, theme);
    window.present();
}
