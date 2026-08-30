mod app;
mod theme;
mod ui;

use gtk::glib;
use gtk::prelude::*;
use gtk::Application;

const APP_ID: &str = "dev.zuhaibullahbaig.Teral";

fn main() -> glib::ExitCode {
    let application = Application::builder().application_id(APP_ID).build();
    application.connect_activate(app::activate);
    application.run()
}
