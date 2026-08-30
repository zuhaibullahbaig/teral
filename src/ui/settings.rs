//! The Settings window.
//!
//! Every control here edits the same `~/.config/teral/teral.toml` that a user can write
//! by hand, and every change applies immediately, so the window never becomes a second
//! source of truth that has to be reconciled later.

use super::App;
use crate::config::{Config, ThemeMode, ViewPreference};
use crate::files::SortKey;
use crate::theme::{MAX_ICON_SIZE, MIN_ICON_SIZE};
use gtk::gdk;
use gtk::prelude::*;
use std::rc::Rc;

/// Open the Settings window, or focus it when it is already open.
pub fn present(app: &App) {
    if let Some(window) = app.widgets.settings_window.borrow().as_ref() {
        window.present();
        return;
    }

    let window = build(app);
    window.present();

    let app_for_close = Rc::clone(app);
    window.connect_close_request(move |_| {
        app_for_close.widgets.settings_window.borrow_mut().take();
        gtk::glib::Propagation::Proceed
    });

    *app.widgets.settings_window.borrow_mut() = Some(window);
}

fn build(app: &App) -> gtk::Window {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("teral-settings");

    content.append(&appearance_section(app));
    content.append(&files_section(app));
    content.append(&commands_section(app));

    let scroller = gtk::ScrolledWindow::builder()
        .child(&content)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();

    let reset = gtk::Button::with_label("Reset to defaults");
    reset.add_css_class("teral-secondary");

    let path = gtk::Label::new(Some(&crate::config::config_path().to_string_lossy()));
    path.set_xalign(0.0);
    path.set_hexpand(true);
    path.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    path.set_tooltip_text(Some(
        "Settings are stored here and can also be edited by hand",
    ));
    path.add_css_class("teral-status-item");

    let close = gtk::Button::with_label("Done");
    close.add_css_class("teral-primary");

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    footer.add_css_class("teral-settings-footer");
    footer.append(&path);
    footer.append(&reset);
    footer.append(&close);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&scroller);
    root.append(&footer);

    let window = super::dialogs::window(app, "Teral Settings", 540, 680, &root);

    close.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });

    reset.connect_clicked({
        let app = Rc::clone(app);
        let window = window.clone();
        move |_| {
            app.apply_config(Config::default(), true);
            app.apply_preferences();
            // Rebuild so every control shows the restored value.
            window.close();
            present(&app);
        }
    });

    window
}

// ------------------------------------------------------------------ sections ----

fn appearance_section(app: &App) -> gtk::Box {
    let section = section("APPEARANCE");
    let config = app.config.borrow().clone();

    let mut group: Option<gtk::CheckButton> = None;
    for mode in ThemeMode::ALL {
        let button = gtk::CheckButton::new();
        button.add_css_class("teral-menu-check");
        button.set_active(config.mode == mode);
        match group.as_ref() {
            Some(first) => button.set_group(Some(first)),
            None => group = Some(button.clone()),
        }

        let title = gtk::Label::new(Some(mode.label()));
        title.set_xalign(0.0);
        let description = gtk::Label::new(Some(mode.description()));
        description.set_xalign(0.0);
        description.set_wrap(true);
        description.add_css_class("teral-setting-hint");

        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.append(&title);
        text.append(&description);
        button.set_child(Some(&text));

        button.connect_toggled({
            let app = Rc::clone(app);
            move |button| {
                if !button.is_active() {
                    return;
                }
                let mut config = app.config.borrow().clone();
                config.mode = mode;
                app.apply_config(config, true);
            }
        });

        section.append(&button);
    }

    section.append(&accent_row(app));
    section.append(&icon_size_row(app));
    section.append(&row_height_row(app));
    section
}

fn accent_row(app: &App) -> gtk::Box {
    let accent = app
        .theme
        .borrow()
        .color(crate::theme::ColorRole::Accent)
        .to_owned();
    let rgba = accent.parse::<gdk::RGBA>().unwrap_or(gdk::RGBA::WHITE);

    let button = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
    button.set_rgba(&rgba);
    button.set_valign(gtk::Align::Center);

    button.connect_rgba_notify({
        let app = Rc::clone(app);
        move |button| {
            let color = button.rgba();
            let mut config = app.config.borrow().clone();
            config.accent = Some(format!(
                "#{:02x}{:02x}{:02x}",
                channel(color.red()),
                channel(color.green()),
                channel(color.blue())
            ));
            app.apply_config(config, true);
        }
    });

    let clear = gtk::Button::with_label("Use theme accent");
    clear.add_css_class("teral-secondary");
    clear.set_valign(gtk::Align::Center);
    clear.connect_clicked({
        let app = Rc::clone(app);
        move |_| {
            let mut config = app.config.borrow().clone();
            config.accent = None;
            app.apply_config(config, true);
        }
    });

    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    controls.append(&button);
    controls.append(&clear);

    labelled_row(
        "Accent colour",
        "Used for selection, highlights and meters.",
        &controls,
    )
}

fn icon_size_row(app: &App) -> gtk::Box {
    let scale = gtk::Scale::with_range(
        gtk::Orientation::Horizontal,
        f64::from(MIN_ICON_SIZE),
        f64::from(MAX_ICON_SIZE),
        8.0,
    );
    scale.add_css_class("teral-zoom");
    scale.set_draw_value(true);
    scale.set_value_pos(gtk::PositionType::Right);
    scale.set_digits(0);
    scale.set_size_request(200, -1);
    scale.set_value(f64::from(app.theme.borrow().grid_icon_size()));

    scale.connect_value_changed({
        let app = Rc::clone(app);
        move |scale| {
            let mut config = app.config.borrow().clone();
            let value = scale.value().round() as i32;
            if config.layout.grid_icon_size == Some(value) {
                return;
            }
            config.layout.grid_icon_size = Some(value);
            app.apply_config(config, true);
        }
    });

    labelled_row(
        "Grid icon size",
        "Also changes with the zoom slider.",
        &scale,
    )
}

fn row_height_row(app: &App) -> gtk::Box {
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 22.0, 64.0, 2.0);
    scale.add_css_class("teral-zoom");
    scale.set_draw_value(true);
    scale.set_value_pos(gtk::PositionType::Right);
    scale.set_digits(0);
    scale.set_size_request(200, -1);
    scale.set_value(f64::from(app.theme.borrow().row_height()));

    scale.connect_value_changed({
        let app = Rc::clone(app);
        move |scale| {
            let mut config = app.config.borrow().clone();
            let value = scale.value().round() as i32;
            if config.layout.row_height == Some(value) {
                return;
            }
            config.layout.row_height = Some(value);
            app.apply_config(config, true);
        }
    });

    labelled_row("List row height", "Density of the list view.", &scale)
}

fn files_section(app: &App) -> gtk::Box {
    let section = section("FILES");
    let config = app.config.borrow().clone();

    section.append(&switch_row(
        app,
        "Show hidden files",
        "Dotfiles and backup files. Ctrl+H toggles this too.",
        config.show_hidden,
        |config, active| config.show_hidden = active,
    ));

    section.append(&switch_row(
        app,
        "Folders first",
        "Keep folders above files whatever the sort order.",
        config.folders_first,
        |config, active| config.folders_first = active,
    ));

    section.append(&switch_row(
        app,
        "Descending order",
        "Reverse the sort direction.",
        config.descending,
        |config, active| config.descending = active,
    ));

    let sort = gtk::DropDown::from_strings(&["Name", "Size", "Type", "Modified"]);
    sort.set_valign(gtk::Align::Center);
    sort.set_selected(match config.sort {
        SortKey::Name => 0,
        SortKey::Size => 1,
        SortKey::Kind => 2,
        SortKey::Modified => 3,
    });
    sort.connect_selected_notify({
        let app = Rc::clone(app);
        move |dropdown| {
            let key = match dropdown.selected() {
                1 => SortKey::Size,
                2 => SortKey::Kind,
                3 => SortKey::Modified,
                _ => SortKey::Name,
            };
            let mut config = app.config.borrow().clone();
            config.sort = key;
            app.apply_config(config, true);
            app.apply_preferences();
        }
    });
    section.append(&labelled_row(
        "Sort files by",
        "The order new folders open in.",
        &sort,
    ));

    let view = gtk::DropDown::from_strings(&["Grid", "List"]);
    view.set_valign(gtk::Align::Center);
    view.set_selected(u32::from(config.view == ViewPreference::List));
    view.connect_selected_notify({
        let app = Rc::clone(app);
        move |dropdown| {
            let mut config = app.config.borrow().clone();
            config.view = if dropdown.selected() == 1 {
                ViewPreference::List
            } else {
                ViewPreference::Grid
            };
            app.apply_config(config, true);
            app.apply_preferences();
        }
    });
    section.append(&labelled_row(
        "Default view",
        "Which view Teral opens with.",
        &view,
    ));

    section
}

fn commands_section(app: &App) -> gtk::Box {
    let section = section("COMMANDS");
    let config = app.config.borrow().clone();

    let shell = gtk::Entry::new();
    shell.add_css_class("teral-input");
    shell.set_text(&config.shell);
    shell.set_placeholder_text(Some("$SHELL, then /bin/sh"));
    shell.set_width_request(200);
    shell.set_valign(gtk::Align::Center);
    shell.connect_changed({
        let app = Rc::clone(app);
        move |entry| {
            let mut config = app.config.borrow().clone();
            config.shell = entry.text().to_string();
            app.apply_config(config, true);
        }
    });
    section.append(&labelled_row(
        "Quick Command shell",
        "Runs your typed command with -c.",
        &shell,
    ));

    let terminal = gtk::Entry::new();
    terminal.add_css_class("teral-input");
    terminal.set_text(&config.terminal);
    terminal.set_placeholder_text(Some("Detected from PATH"));
    terminal.set_width_request(200);
    terminal.set_valign(gtk::Align::Center);
    terminal.connect_changed({
        let app = Rc::clone(app);
        move |entry| {
            let mut config = app.config.borrow().clone();
            config.terminal = entry.text().to_string();
            app.apply_config(config, true);
        }
    });
    section.append(&labelled_row(
        "Terminal",
        "Opened by Open Terminal Here (Ctrl+Shift+T).",
        &terminal,
    ));

    section
}

// ------------------------------------------------------------------- helpers ----

fn section(title: &str) -> gtk::Box {
    let heading = super::tracked_label(title, 1);
    heading.add_css_class("teral-section-title");

    let section = gtk::Box::new(gtk::Orientation::Vertical, 10);
    section.add_css_class("teral-settings-section");
    section.append(&heading);
    section
}

fn labelled_row(title: &str, hint: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let name = gtk::Label::new(Some(title));
    name.set_xalign(0.0);

    let description = gtk::Label::new(Some(hint));
    description.set_xalign(0.0);
    description.set_wrap(true);
    description.add_css_class("teral-setting-hint");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    text.append(&name);
    text.append(&description);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    row.add_css_class("teral-setting-row");
    row.append(&text);
    row.append(control);
    row
}

fn switch_row(
    app: &App,
    title: &str,
    hint: &str,
    active: bool,
    update: fn(&mut Config, bool),
) -> gtk::Box {
    let switch = gtk::Switch::new();
    switch.set_active(active);
    switch.set_valign(gtk::Align::Center);

    switch.connect_state_set({
        let app = Rc::clone(app);
        move |_, active| {
            let mut config = app.config.borrow().clone();
            update(&mut config, active);
            app.apply_config(config, true);
            app.apply_preferences();
            gtk::glib::Propagation::Proceed
        }
    });

    labelled_row(title, hint, &switch)
}

fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}
