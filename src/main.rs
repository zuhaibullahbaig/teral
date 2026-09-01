mod app;
mod command;
mod config;
mod files;
mod icons;
mod persistence;
mod places;
mod session;
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

    // Registering claims Teral's name on the session bus, which is how a second launch
    // finds the copy already running. GLib's own failure message is one line with no
    // indication of what to do about it, and the usual cause — an earlier Teral that is
    // still holding the name but no longer answering — is fixable in one command.
    if let Err(error) = application.register(gtk::gio::Cancellable::NONE) {
        eprintln!(
            "teral: could not register with the desktop session bus: {}",
            error.message().trim()
        );
        eprintln!(
            "       An earlier Teral may still be running and unresponsive. \
             Try `pkill -f teral`, then start Teral again."
        );
        return glib::ExitCode::FAILURE;
    }

    application.run()
}
