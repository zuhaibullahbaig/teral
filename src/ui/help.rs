//! The Shortcuts and About windows.

use super::App;
use gtk::prelude::*;

/// Every shortcut Teral implements, grouped the way people look for them.
const SHORTCUTS: [(&str, &[(&str, &str)]); 5] = [
    (
        "Navigation",
        &[
            ("Enter", "Open the selection"),
            ("Backspace", "Go to the parent folder"),
            ("Alt+Left / Alt+Right", "Back / Forward"),
            ("Ctrl+L", "Edit the location"),
            ("F5 / Ctrl+R", "Refresh"),
        ],
    ),
    (
        "Tabs and panels",
        &[
            ("Ctrl+T", "New tab"),
            ("Ctrl+W", "Close tab"),
            ("Ctrl+Tab / Ctrl+Shift+Tab", "Next / previous tab"),
            ("Ctrl+I", "Show or hide the details panel"),
            (
                "Ctrl+1 / Ctrl+2 / Ctrl+3",
                "Navigation drawer / Files / Details drawer",
            ),
            ("Ctrl+F", "Search this folder"),
            ("Ctrl+Shift+F", "Search Home and subfolders"),
        ],
    ),
    (
        "Files",
        &[
            ("Ctrl+C / Ctrl+X", "Copy / Cut"),
            ("Ctrl+V", "Paste"),
            ("Ctrl+D", "Duplicate"),
            ("F2", "Rename"),
            ("Delete", "Move to trash"),
            ("Shift+Delete", "Delete permanently"),
            ("Ctrl+Shift+N", "New folder"),
            ("Ctrl+H", "Show hidden files"),
            ("Ctrl+A", "Select all"),
        ],
    ),
    (
        "Commands",
        &[
            ("Ctrl+K", "Focus Quick Command"),
            ("Ctrl+`", "Show or hide the command console"),
            ("Ctrl+Shift+T", "Open a terminal here"),
            ("Escape", "Close search, or hide the console"),
        ],
    ),
    (
        "View",
        &[
            ("Ctrl+= / Ctrl+-", "Larger / smaller icons"),
            ("Ctrl+I", "Show or hide the details panel"),
            ("Ctrl+0", "Reset the icon size"),
            ("Ctrl+,", "Settings"),
            ("F1", "This shortcut list"),
        ],
    ),
];

/// Show the keyboard shortcut reference.
pub fn present_shortcuts(app: &App) {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("teral-settings");

    for (group, entries) in SHORTCUTS {
        let heading = super::tracked_label(&group.to_uppercase(), 1);
        heading.add_css_class("teral-section-title");

        let section = gtk::Box::new(gtk::Orientation::Vertical, 8);
        section.add_css_class("teral-settings-section");
        section.append(&heading);

        for (keys, description) in entries {
            let key = gtk::Label::new(Some(keys));
            key.set_xalign(0.0);
            key.set_width_request(180);
            key.add_css_class("teral-shortcut-key");

            let text = gtk::Label::new(Some(description));
            text.set_xalign(0.0);
            text.set_hexpand(true);
            text.set_wrap(true);

            let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
            row.add_css_class("teral-setting-row");
            row.append(&key);
            row.append(&text);
            section.append(&row);
        }

        content.append(&section);
    }

    present(app, "Keyboard Shortcuts", 560, 700, &content);
}

/// Show what Teral is, and which version this is.
pub fn present_about(app: &App) {
    let name = gtk::Label::new(Some("Teral"));
    name.add_css_class("teral-dialog-title");

    let version = gtk::Label::new(Some(&format!(
        "Version {}   ·   GTK {}.{}.{}",
        env!("CARGO_PKG_VERSION"),
        gtk::major_version(),
        gtk::minor_version(),
        gtk::micro_version()
    )));
    version.add_css_class("teral-status-item");

    let description = gtk::Label::new(Some(
        "A modern native Linux file manager, written in Rust with GTK4.\n\
         One application that belongs on every Linux desktop, and adopts the \
         appearance of the one it happens to be running on.",
    ));
    description.set_justify(gtk::Justification::Center);
    description.set_wrap(true);
    description.set_max_width_chars(46);

    let author = gtk::Label::new(Some("© 2026 Zuhaib Ullah Baig   ·   MIT License"));
    author.add_css_class("teral-status-item");

    let paths = gtk::Box::new(gtk::Orientation::Vertical, 4);
    paths.set_halign(gtk::Align::Center);
    for (label, path) in [
        ("Settings", crate::config::config_path()),
        (
            "Bookmarks",
            crate::theme::data_home().join("teral/places.toml"),
        ),
    ] {
        let row = gtk::Label::new(Some(&format!("{label}: {}", path.display())));
        row.add_css_class("teral-status-item");
        row.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        paths.append(&row);
    }

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_halign(gtk::Align::Center);
    content.set_valign(gtk::Align::Center);
    content.set_margin_top(28);
    content.set_margin_bottom(28);
    content.set_margin_start(28);
    content.set_margin_end(28);
    content.append(&name);
    content.append(&version);
    content.append(&description);
    content.append(&paths);
    content.append(&author);

    present(app, "About Teral", 420, 420, &content);
}

/// Show `content` in a modal window centred on Teral's own window.
fn present(app: &App, title: &str, width: i32, height: i32, content: &impl IsA<gtk::Widget>) {
    let scroller = gtk::ScrolledWindow::builder()
        .child(content)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();

    let close = gtk::Button::with_label("Close");
    close.add_css_class("teral-primary");
    close.set_hexpand(true);
    close.set_halign(gtk::Align::Center);

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    footer.add_css_class("teral-settings-footer");
    footer.append(&close);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&scroller);
    root.append(&footer);

    let window = super::dialogs::window(app, title, width, height, &root);

    close.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });

    window.present();
}
