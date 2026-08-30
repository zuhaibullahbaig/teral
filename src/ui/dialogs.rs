//! Small Teral-styled dialogs and popovers.

use super::App;
use crate::files::{FileEntry, ops};
use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;
use gtk::prelude::*;
use std::rc::Rc;

/// A Teral-styled window that the compositor will place over Teral's own.
///
/// GTK4 has no window-positioning API; a modal, transient window is what tells the
/// desktop to centre it on its parent instead of dropping it in a corner.
pub fn window(
    app: &App,
    title: &str,
    width: i32,
    height: i32,
    child: &impl IsA<gtk::Widget>,
) -> gtk::Window {
    let window = gtk::Window::builder()
        .transient_for(&app.widgets.window)
        .modal(true)
        .destroy_with_parent(true)
        .title(title)
        .default_width(width)
        .default_height(height)
        .child(child)
        .build();
    window.add_css_class("teral-dialog");

    let escape = gtk::EventControllerKey::new();
    escape.connect_key_pressed({
        let window = window.clone();
        move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                window.close();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    });
    window.add_controller(escape);

    window
}

/// Ask for a single line of text. `confirm` runs only when the user accepts.
pub fn prompt(
    app: &App,
    title: &str,
    accept_label: &str,
    initial: &str,
    select_range: Option<(i32, i32)>,
    confirm: impl Fn(&App, String) + 'static,
) {
    let heading = gtk::Label::new(Some(title));
    heading.set_xalign(0.0);
    heading.add_css_class("teral-dialog-title");

    let entry = gtk::Entry::new();
    entry.add_css_class("teral-input");
    entry.set_text(initial);
    entry.set_activates_default(true);
    match select_range {
        Some((start, end)) => entry.select_region(start, end),
        None => entry.select_region(0, -1),
    }

    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("teral-secondary");
    let accept = gtk::Button::with_label(accept_label);
    accept.add_css_class("teral-primary");

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(6);
    buttons.append(&cancel);
    buttons.append(&accept);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(18);
    content.set_margin_bottom(18);
    content.set_margin_start(18);
    content.set_margin_end(18);
    content.append(&heading);
    content.append(&entry);
    content.append(&buttons);

    let window = window(app, title, 400, -1, &content);
    window.set_resizable(false);

    let confirm = Rc::new(confirm);

    let submit = {
        let app = Rc::clone(app);
        let entry = entry.clone();
        let window = window.clone();
        let confirm = Rc::clone(&confirm);
        move || {
            let value = entry.text().trim().to_owned();
            window.close();
            if !value.is_empty() {
                confirm(&app, value);
            }
        }
    };

    accept.connect_clicked({
        let submit = submit.clone();
        move |_| submit()
    });
    entry.connect_activate(move |_| submit());
    cancel.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });

    window.present();
    entry.grab_focus();
}

/// Confirm a destructive action.
pub fn confirm(
    app: &App,
    title: &str,
    body: &str,
    accept_label: &str,
    action: impl Fn() + 'static,
) {
    let heading = gtk::Label::new(Some(title));
    heading.set_xalign(0.0);
    heading.add_css_class("teral-dialog-title");

    let text = gtk::Label::new(Some(body));
    text.set_xalign(0.0);
    text.set_wrap(true);
    text.add_css_class("teral-status-item");

    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("teral-secondary");
    let accept = gtk::Button::with_label(accept_label);
    accept.add_css_class("teral-primary");

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(6);
    buttons.append(&cancel);
    buttons.append(&accept);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(18);
    content.set_margin_bottom(18);
    content.set_margin_start(18);
    content.set_margin_end(18);
    content.append(&heading);
    content.append(&text);
    content.append(&buttons);

    let window = window(app, title, 420, -1, &content);
    window.set_resizable(false);

    accept.connect_clicked({
        let window = window.clone();
        move |_| {
            window.close();
            action();
        }
    });
    cancel.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });

    window.present();
}

/// A popover listing the applications the desktop recommends for an entry.
pub fn open_with_popover(
    app: &App,
    entry: &FileEntry,
    applications: Vec<gio::AppInfo>,
) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.add_css_class("teral-popover");

    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.set_width_request(220);

    for application in applications.into_iter().take(12) {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 9);
        let icon = match application.icon() {
            Some(icon) => gtk::Image::from_gicon(&icon),
            None => gtk::Image::from_icon_name("application-x-executable-symbolic"),
        };
        icon.set_pixel_size(15);
        let label = gtk::Label::new(Some(&application.display_name()));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        row.append(&icon);
        row.append(&label);

        let button = gtk::Button::new();
        button.set_child(Some(&row));
        button.add_css_class("teral-menu-item");
        button.set_has_frame(false);

        let app = Rc::clone(app);
        let popover = popover.clone();
        let path = entry.path().to_path_buf();
        button.connect_clicked(move |_| {
            popover.popdown();
            if let Err(error) = ops::open_with(&application, &path) {
                app.show_error(&format!(
                    "Could not open with {}: {}",
                    application.display_name(),
                    error.message().trim()
                ));
            }
        });

        content.append(&button);
    }

    popover.set_child(Some(&content));
    popover
}
