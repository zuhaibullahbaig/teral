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

pub fn build(icon_size: i32, spacing: i32) -> StatusBar {
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
    command_bar.append(&prompt);
    command_bar.append(&command_entry);

    let message = gtk::Label::new(None);
    message.set_ellipsize(gtk::pango::EllipsizeMode::End);
    message.set_max_width_chars(48);
    message.add_css_class("teral-status-item");

    let selection = status_label();
    selection.add_css_class("strong");
    let size = status_label();
    let free = status_label();

    let zoom = gtk::Scale::with_range(gtk::Orientation::Horizontal, 32.0, 96.0, 8.0);
    zoom.add_css_class("teral-zoom");
    zoom.set_draw_value(false);
    zoom.set_value(f64::from(icon_size));
    zoom.set_size_request(96, -1);
    zoom.set_tooltip_text(Some("Grid icon size"));

    // The command bar and this spacer share the free space, keeping the field to
    // roughly half the width the way the Teral layout expects.
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);

    let root = gtk::Box::new(gtk::Orientation::Horizontal, spacing + 4);
    root.add_css_class("teral-status-bar");
    root.append(&command_bar);
    root.append(&spacer);
    root.append(&message);
    root.append(&selection);
    root.append(&size);
    root.append(&free);
    root.append(&zoom);

    StatusBar {
        root,
        command_entry,
        selection,
        size,
        free,
        message,
        zoom,
    }
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

    app.widgets.zoom.connect_value_changed({
        let app = Rc::clone(app);
        move |scale| {
            let size = scale.value().round() as i32;
            if size == app.state.icon_size.get() {
                return;
            }
            app.state.icon_size.set(size);
            // Rebuilding the factory recreates every cell at the new size.
            super::fileview::refresh_grid_factory(&app);
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
