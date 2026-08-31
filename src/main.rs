mod app;
mod command;
mod config;
mod files;
mod icons;
mod places;
mod style;
mod tags;
mod theme;
mod ui;

use gtk::Application;
use gtk::glib;
use gtk::prelude::*;

/// Teral's D-Bus name, and the name its installed desktop entry and icon carry.
pub const APP_ID: &str = "dev.zuhaibullahbaig.Teral";

fn main() -> glib::ExitCode {
    // HANDLES_OPEN is what lets the desktop entry's `%U` and `teral ~/Documents` reach
    // Teral at all; without it GApplication refuses arguments outright, so opening a
    // folder from another application does nothing.
    let application = Application::builder()
        .application_id(APP_ID)
        .flags(gtk::gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    application.connect_activate(app::activate);
    application.connect_open(app::open);
    application.run()
}
