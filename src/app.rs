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

    let window = ui::build_window(application, config, theme);
    window.present();
}
