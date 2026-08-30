mod app;
mod command;
mod files;
mod icons;
mod places;
mod style;
mod theme;
mod ui;

use gtk::Application;
use gtk::glib;
use gtk::prelude::*;

const APP_ID: &str = "dev.zuhaibullahbaig.Teral";

fn main() -> glib::ExitCode {
    let application = Application::builder().application_id(APP_ID).build();
    application.connect_activate(app::activate);
    application.run()
}
