//! The bottom bar: Quick Command, selection status, free space and grid zoom.

use super::App;
use crate::command::{self, RunningCommand};
use gtk::prelude::*;
use std::rc::Rc;

/// Widgets of the bottom bar and the Quick Command console above it.
pub struct StatusBar {
    pub root: gtk::Box,
    pub command_entry: gtk::Entry,
    pub selection: gtk::Label,
    pub size: gtk::Label,
    pub free: gtk::Label,
    pub message: gtk::Label,
    pub zoom: gtk::Scale,
    pub settings: gtk::Button,
    pub details_toggle: gtk::ToggleButton,
}

/// The collapsible Quick Command output area.
pub struct Console {
    pub root: gtk::Revealer,
    pub title: gtk::Label,
    pub output: gtk::TextView,
    pub cancel: gtk::Button,
    pub close: gtk::Button,
}

pub fn build_console() -> Console {
    let title = gtk::Label::new(None);
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("teral-status-item");
    title.add_css_class("strong");

    let cancel = super::icon_button(
        crate::icons::ui(crate::icons::names::STOP),
        "Stop the running command",
    );
    cancel.set_visible(false);
    let close = super::icon_button(
        crate::icons::ui(crate::icons::names::CLOSE),
        "Hide command output",
    );

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    header.add_css_class("teral-console-header");
    header.append(&title);
    header.append(&cancel);
    header.append(&close);

    let output = gtk::TextView::new();
    output.add_css_class("teral-console-output");
    output.set_editable(false);
    output.set_monospace(true);
    output.set_left_margin(12);
    output.set_right_margin(12);
    output.set_top_margin(8);
    output.set_bottom_margin(8);
    output.set_wrap_mode(gtk::WrapMode::WordChar);

    let scroller = gtk::ScrolledWindow::builder()
        .child(&output)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .build();
    scroller.set_size_request(-1, 168);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("teral-console");
    content.append(&header);
    content.append(&scroller);

    let root = gtk::Revealer::new();
    root.set_child(Some(&content));
    root.set_transition_type(gtk::RevealerTransitionType::SlideUp);
    root.set_reveal_child(false);

    Console {
        root,
        title,
        output,
        cancel,
        close,
    }
}

/// Build the footer.
///
/// The footer is split into the same three columns as the window above it: the
/// navigation column carries Teral's own controls, the file column carries Quick
/// Command at exactly the width of the file list, and the details column carries the
/// selection and storage readout.
pub fn build(icon_size: i32, sidebar_width: i32, details_width: i32) -> StatusBar {
    // ---- navigation column -------------------------------------------------
    let settings = super::icon_button(
        crate::icons::ui(crate::icons::names::SETTINGS),
        "Settings (Ctrl+,)",
    );

    let details_toggle = super::icon_toggle(
        crate::icons::ui(crate::icons::names::PANEL),
        "Show the details panel (Ctrl+I)",
    );
    details_toggle.set_active(true);

    let message = gtk::Label::new(None);
    message.set_xalign(0.0);
    message.set_hexpand(true);
    message.set_ellipsize(gtk::pango::EllipsizeMode::End);
    message.add_css_class("teral-status-item");

    let left = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    left.add_css_class("teral-footer-navigation");
    left.set_size_request(sidebar_width, -1);
    left.set_hexpand(false);
    left.append(&settings);
    left.append(&details_toggle);
    left.append(&message);

    // ---- file column -------------------------------------------------------
    let prompt = gtk::Label::new(Some(">_"));
    prompt.add_css_class("teral-command-prompt");

    let command_entry = gtk::Entry::new();
    command_entry.add_css_class("teral-command-entry");
    command_entry.set_hexpand(true);
    command_entry.set_has_frame(false);
    command_entry.set_placeholder_text(Some("Quick command in this folder (Ctrl+K)"));

    let command_bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    command_bar.add_css_class("teral-command-bar");
    command_bar.set_hexpand(true);
    command_bar.set_valign(gtk::Align::Center);
    command_bar.append(&prompt);
    command_bar.append(&command_entry);

    let middle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    middle.add_css_class("teral-footer-files");
    middle.set_hexpand(true);
    middle.append(&command_bar);

    // ---- details column ----------------------------------------------------
    let selection = status_label();
    selection.add_css_class("strong");
    let size = status_label();
    let free = status_label();

    let zoom = gtk::Scale::with_range(
        gtk::Orientation::Horizontal,
        f64::from(crate::theme::MIN_ICON_SIZE),
        f64::from(crate::theme::MAX_ICON_SIZE),
        8.0,
    );
    zoom.add_css_class("teral-zoom");
    zoom.set_draw_value(false);
    zoom.set_value(f64::from(icon_size));
    zoom.set_hexpand(true);
    zoom.set_valign(gtk::Align::Center);
    zoom.set_tooltip_text(Some("Icon size (Ctrl+0 resets)"));

    let right = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    right.add_css_class("teral-footer-details");
    right.set_size_request(details_width, -1);
    right.set_hexpand(false);
    right.append(&selection);
    right.append(&size);
    right.append(&free);
    right.append(&zoom);

    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.add_css_class("teral-status-bar");
    root.append(&left);
    root.append(&divider());
    root.append(&middle);
    root.append(&divider());
    root.append(&right);

    StatusBar {
        root,
        command_entry,
        selection,
        size,
        free,
        message,
        zoom,
        settings,
        details_toggle,
    }
}

/// A hairline between footer columns, echoing the borders of the panes above.
fn divider() -> gtk::Separator {
    let divider = gtk::Separator::new(gtk::Orientation::Vertical);
    divider.add_css_class("teral-footer-divider");
    divider
}

fn status_label() -> gtk::Label {
    let label = gtk::Label::new(None);
    label.add_css_class("teral-status-item");
    label
}

pub fn connect(app: &App) {
    app.widgets.command_entry.connect_activate({
        let app = Rc::clone(app);
        move |entry| {
            let text = entry.text().trim().to_owned();
            if text.is_empty() {
                return;
            }
            entry.set_text("");
            run_command(&app, &text);
        }
    });

    app.widgets.console.cancel.connect_clicked({
        let app = Rc::clone(app);
        move |_| {
            if let Some(running) = app.state.running_command.borrow().as_ref() {
                running.cancel();
            }
        }
    });

    app.widgets.console.close.connect_clicked({
        let app = Rc::clone(app);
        move |_| app.widgets.console.root.set_reveal_child(false)
    });

    app.widgets.settings.connect_clicked({
        let app = Rc::clone(app);
        move |_| super::settings::present(&app)
    });

    app.widgets.details_toggle.connect_toggled({
        let app = Rc::clone(app);
        move |button| app.widgets.details.root.set_visible(button.is_active())
    });

    app.widgets.zoom.connect_value_changed({
        let app = Rc::clone(app);
        move |scale| {
            let size = scale.value().round() as i32;
            if size == app.state.icon_size.get() {
                return;
            }
            if app.state.updating.get() {
                return;
            }
            app.state.icon_size.set(size);
            // Rebuilding the factory recreates every cell at the new size.
            super::fileview::refresh_grid_factory(&app);

            // Remember the choice the same way the Settings window would.
            let mut config = app.config.borrow().clone();
            config.layout.grid_icon_size = Some(size);
            app.apply_config(config, true);
        }
    });
}

/// Run a Quick Command in the directory currently being browsed.
pub fn run_command(app: &App, text: &str) {
    if app.state.running_command.borrow().is_some() {
        app.show_error("A Quick Command is already running");
        return;
    }

    let directory = app.current_dir();
    let running = match RunningCommand::spawn(text, &directory) {
        Ok(running) => running,
        Err(error) => {
            app.show_error(&format!("Could not run the command: {}", error.message()));
            return;
        }
    };

    let console = &app.widgets.console;
    console
        .title
        .set_text(&format!("$ {text}   ·   {}", directory.display()));
    console.output.buffer().set_text("");
    console.cancel.set_visible(true);
    console.root.set_reveal_child(true);
    app.clear_message();

    *app.state.running_command.borrow_mut() = Some(running.clone());

    let app = Rc::clone(app);
    let text = text.to_owned();
    gtk::glib::spawn_future_local(async move {
        let result = running.wait().await;
        app.state.running_command.borrow_mut().take();
        app.widgets.console.cancel.set_visible(false);

        match result {
            Ok(output) => {
                let body = command::tidy_output(&output.text);
                let body = if body.is_empty() {
                    format!("(no output)\n\nexit status {}", output.exit_status)
                } else {
                    body
                };
                app.widgets.console.output.buffer().set_text(&body);
                if output.succeeded() {
                    app.set_message(&format!("{text} finished"), false);
                } else {
                    app.show_error(&format!("{text} exited with status {}", output.exit_status));
                }
                // Commands frequently change the folder they ran in.
                app.reload();
            }
            Err(error) => app.show_error(&format!("Command failed: {}", error.message())),
        }
    });
}
