//! The bottom bar and the Quick Command console.

use super::App;
use crate::command;
use gtk::prelude::*;
use std::path::Path;
use std::rc::Rc;
use vte::prelude::*;

/// Widgets of the bottom bar and the Quick Command console above it.
pub struct StatusBar {
    pub root: gtk::Box,
    pub command_entry: gtk::Entry,
    pub selection: gtk::Label,
    pub size: gtk::Label,
    pub message: gtk::Label,
    pub zoom: gtk::Scale,
    pub zoom_value: gtk::Label,
    pub zoom_out: gtk::Button,
    pub zoom_in: gtk::Button,
    pub settings: gtk::Button,
    pub details_toggle: gtk::ToggleButton,
}

/// The Quick Command console: a real terminal, resizable by dragging its top edge.
pub struct Console {
    pub root: gtk::Box,
    pub header: gtk::Box,
    pub title: gtk::Label,
    pub terminal: vte::Terminal,
    pub stop: gtk::Button,
    pub close: gtk::Button,
}

/// Height the console opens at, in pixels.
pub const CONSOLE_HEIGHT: i32 = 260;

pub fn build_console() -> Console {
    let title = gtk::Label::new(None);
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    title.add_css_class("teral-status-item");
    title.add_css_class("strong");

    let stop = super::icon_button(
        crate::icons::ui(crate::icons::names::STOP),
        "Stop the running command",
    );
    stop.set_visible(false);

    let close = super::icon_button(
        crate::icons::ui(crate::icons::names::CLOSE),
        "Hide the console (Ctrl+`)",
    );

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    header.add_css_class("teral-console-header");
    header.set_tooltip_text(Some("Drag up or down to resize the console"));
    header.set_cursor_from_name(Some("ns-resize"));
    header.append(&title);
    header.append(&stop);
    header.append(&close);

    let terminal = command::build_terminal();

    let scroller = gtk::ScrolledWindow::builder()
        .child(&terminal)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    scroller.add_css_class("teral-console-scroller");

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("teral-console");
    root.append(&header);
    root.append(&scroller);

    Console {
        root,
        header,
        title,
        terminal,
        stop,
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
    // Without a tiny character request the label asks for its full natural width, and a
    // long message would squeeze Quick Command out of alignment with the file list.
    message.set_width_chars(1);
    message.set_max_width_chars(1);
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

    // A bare slider says nothing about what it does, so it is bracketed by the
    // universally understood minus and plus and labelled with the size it produces.
    let zoom_out = super::icon_button(
        crate::icons::ui(crate::icons::names::ZOOM_OUT),
        "Smaller icons (Ctrl+-)",
    );
    let zoom_in = super::icon_button(
        crate::icons::ui(crate::icons::names::ZOOM_IN),
        "Larger icons (Ctrl+=)",
    );

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

    let zoom_value = gtk::Label::new(Some(&format!("{icon_size} px")));
    zoom_value.add_css_class("teral-status-item");
    zoom_value.set_width_chars(6);

    let zoom_group = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    zoom_group.add_css_class("teral-zoom-group");
    zoom_group.set_hexpand(true);
    zoom_group.set_valign(gtk::Align::Center);
    zoom_group.set_tooltip_text(Some("Icon size"));
    zoom_group.append(&zoom_out);
    zoom_group.append(&zoom);
    zoom_group.append(&zoom_in);
    zoom_group.append(&zoom_value);

    let right = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    right.add_css_class("teral-footer-details");
    right.set_size_request(details_width, -1);
    right.set_hexpand(false);
    right.append(&selection);
    right.append(&size);
    right.append(&zoom_group);

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
        message,
        zoom,
        zoom_value,
        zoom_out,
        zoom_in,
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

    app.widgets.settings.connect_clicked({
        let app = Rc::clone(app);
        move |_| super::settings::present(&app)
    });

    app.widgets.details_toggle.connect_toggled({
        let app = Rc::clone(app);
        move |button| app.widgets.details.root.set_visible(button.is_active())
    });

    app.widgets.console.stop.connect_clicked({
        let app = Rc::clone(app);
        move |_| stop_command(&app)
    });

    app.widgets.console.close.connect_clicked({
        let app = Rc::clone(app);
        move |_| hide_console(&app)
    });

    connect_console_resize(app);

    // A finished command leaves its output on screen; the folder is reloaded because
    // commands frequently change what is in it.
    app.widgets.console.terminal.connect_child_exited({
        let app = Rc::clone(app);
        move |_, status| {
            app.state.running_command.set(false);
            app.widgets.console.stop.set_visible(false);
            if status == 0 {
                app.set_message("Command finished", false);
            } else {
                app.show_error(&format!("Command exited with status {status}"));
            }
            app.reload();
        }
    });

    app.widgets.zoom.connect_value_changed({
        let app = Rc::clone(app);
        move |scale| {
            if app.state.updating.get() {
                return;
            }
            app.set_icon_size(scale.value().round() as i32);
        }
    });

    app.widgets.zoom_out.connect_clicked({
        let app = Rc::clone(app);
        move |_| app.step_zoom(-8)
    });
    app.widgets.zoom_in.connect_clicked({
        let app = Rc::clone(app);
        move |_| app.step_zoom(8)
    });
}

/// Dragging anywhere along the console's title bar resizes it.
///
/// The paned handle alone is a one-pixel target that nobody finds; the whole header is
/// a much larger one.
fn connect_console_resize(app: &App) {
    let drag = gtk::GestureDrag::new();
    let start = std::rc::Rc::new(std::cell::Cell::new(0));

    drag.connect_drag_begin({
        let app = Rc::clone(app);
        let start = std::rc::Rc::clone(&start);
        move |_, _, _| start.set(app.widgets.file_paned.position())
    });

    drag.connect_drag_update({
        let app = Rc::clone(app);
        let start = std::rc::Rc::clone(&start);
        move |_, _, offset| {
            let paned = &app.widgets.file_paned;
            let target = (start.get() + offset.round() as i32).clamp(80, paned.height() - 80);
            paned.set_position(target);
        }
    });

    drag.connect_drag_end({
        let app = Rc::clone(app);
        move |_, _, _| {
            let paned = &app.widgets.file_paned;
            let height = paned.height() - paned.position();
            if height > 80 {
                app.state.console_height.set(height);
            }
        }
    });

    app.widgets.console.header.add_controller(drag);
}

/// Show the console, restoring the height it had last time.
pub fn show_console(app: &App) {
    if app.widgets.console.root.get_visible() {
        return;
    }

    let paned = &app.widgets.file_paned;
    let height = app.state.console_height.get().max(120);
    paned.set_position((paned.height() - height).max(120));
    app.widgets.console.root.set_visible(true);
}

/// Hide the console, remembering how tall the user made it.
pub fn hide_console(app: &App) {
    let paned = &app.widgets.file_paned;
    if app.widgets.console.root.get_visible() {
        let height = paned.height() - paned.position();
        if height > 80 {
            app.state.console_height.set(height);
        }
    }
    app.widgets.console.root.set_visible(false);
}

/// Whether the console is currently on screen.
pub fn console_visible(app: &App) -> bool {
    app.widgets.console.root.get_visible()
}

/// Stop whatever Quick Command is running.
pub fn stop_command(app: &App) {
    // Ctrl+C in the child's own terminal is the least surprising way to interrupt it.
    app.widgets.console.terminal.feed_child(&[0x03]);
}

/// Run a Quick Command in the directory currently being browsed.
pub fn run_command(app: &App, text: &str) {
    if app.state.running_command.get() {
        app.show_error("A Quick Command is already running");
        return;
    }

    // A tag view gathers files from all over the filesystem and the trash holds files
    // that belong somewhere else, so neither has a folder a shell could sensibly run in.
    // Running in whichever directory happened to be visited last would be a surprise.
    let Some(directory) = app.location().working_directory().map(Path::to_path_buf) else {
        app.show_error("Quick Command needs a folder; open one to run a command in it");
        return;
    };
    let console = &app.widgets.console;

    console
        .title
        .set_text(&command::summarise(text, &directory));
    console.terminal.reset(true, true);
    console.stop.set_visible(true);
    show_console(app);
    app.clear_message();
    app.state.running_command.set(true);

    let spawn = command::run(&console.terminal, text, &directory);
    let app = Rc::clone(app);
    gtk::glib::spawn_future_local(async move {
        if let Err(error) = spawn.await {
            app.state.running_command.set(false);
            app.widgets.console.stop.set_visible(false);
            app.show_error(&format!("Could not run the command: {}", error.message()));
        } else {
            // Typing goes to the child, not to the file list.
            app.widgets.console.terminal.grab_focus();
        }
    });
}
