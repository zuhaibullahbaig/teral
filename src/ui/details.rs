//! The right-hand details and actions panel.
//!
//! Every value shown here comes from the filesystem. Actions that Teral has not
//! implemented are simply absent rather than present and inert.

use super::App;
use crate::files::{FileEntry, format_permissions, format_size, format_time};
use crate::icons::{self, names};
use gtk::prelude::*;
use std::rc::Rc;

/// One `key: value` row in the metadata table.
pub struct MetaRow {
    pub root: gtk::Box,
    pub value: gtk::Label,
}

/// The panel's widgets, held by `Widgets` so updates stay cheap.
pub struct Details {
    pub root: gtk::Box,
    pub title: gtk::Label,
    pub stack: gtk::Stack,
    pub icon: gtk::Image,
    pub picture: gtk::Picture,
    pub name: gtk::Label,
    pub kind: gtk::Label,
    pub size: gtk::Label,
    pub rows: Vec<(&'static str, MetaRow)>,
    pub actions: Actions,
}

/// Action buttons in the details panel.
pub struct Actions {
    pub root: gtk::Grid,
    pub open: gtk::Button,
    pub open_with: gtk::MenuButton,
    pub copy_path: gtk::Button,
    pub terminal: gtk::Button,
    pub rename: gtk::Button,
    pub cut: gtk::Button,
    pub copy: gtk::Button,
    pub trash: gtk::Button,
}

/// Metadata rows, in display order.
const ROW_KEYS: [&str; 7] = [
    "Modified",
    "Created",
    "Accessed",
    "Owner",
    "Permissions",
    "Links to",
    "Path",
];

pub fn build(width: i32) -> Details {
    let title = gtk::Label::new(Some("Details"));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    title.add_css_class("teral-details-title");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.add_css_class("teral-details-header");
    header.append(&title);

    let icon = gtk::Image::new();
    icon.set_pixel_size(64);
    icon.set_hexpand(true);
    icon.set_vexpand(true);

    let picture = gtk::Picture::new();
    picture.set_content_fit(gtk::ContentFit::Contain);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_visible(false);

    let preview = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    preview.add_css_class("teral-preview");
    preview.set_overflow(gtk::Overflow::Hidden);
    preview.append(&icon);
    preview.append(&picture);

    let name = gtk::Label::new(None);
    name.set_xalign(0.0);
    name.set_wrap(true);
    name.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    name.add_css_class("teral-details-name");

    let kind = gtk::Label::new(None);
    kind.set_xalign(0.0);
    kind.add_css_class("teral-details-kind");

    let size = gtk::Label::new(None);
    size.set_xalign(0.0);
    size.add_css_class("teral-details-size");

    let summary = gtk::Box::new(gtk::Orientation::Vertical, 3);
    summary.set_margin_top(12);
    summary.append(&name);
    summary.append(&kind);
    summary.append(&size);

    let table = gtk::Box::new(gtk::Orientation::Vertical, 7);
    table.set_margin_top(14);

    let mut rows = Vec::new();
    for key in ROW_KEYS {
        let row = meta_row(key);
        table.append(&row.root);
        rows.push((key, row));
    }

    let actions_heading = super::tracked_label("ACTIONS", 1);
    actions_heading.add_css_class("teral-section-title");
    actions_heading.set_margin_top(16);
    actions_heading.set_margin_bottom(8);

    let actions = build_actions();

    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.add_css_class("teral-details-body");
    body.append(&preview);
    body.append(&summary);
    body.append(&separator());
    body.append(&table);
    body.append(&actions_heading);
    body.append(&actions.root);

    let body_scroller = gtk::ScrolledWindow::builder()
        .child(&body)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();

    let empty = gtk::Label::new(Some("Select a file or folder"));
    empty.add_css_class("teral-empty");
    empty.set_vexpand(true);

    let stack = gtk::Stack::new();
    stack.add_named(&empty, Some("empty"));
    stack.add_named(&body_scroller, Some("details"));
    stack.set_visible_child_name("empty");

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("teral-details");
    // Child labels expand; the pane itself must not inherit that.
    root.set_hexpand(false);
    root.set_size_request(width, -1);
    root.append(&header);
    root.append(&stack);

    Details {
        root,
        title,
        stack,
        icon,
        picture,
        name,
        kind,
        size,
        rows,
        actions,
    }
}

fn build_actions() -> Actions {
    let grid = gtk::Grid::new();
    grid.set_row_spacing(7);
    grid.set_column_spacing(7);
    grid.set_column_homogeneous(true);

    let open = action_button(icons::ui(names::OPEN), "Open");
    let open_with = action_menu_button(icons::ui(names::OPEN_WITH), "Open With");
    let copy_path = action_button(icons::ui(names::COPY_PATH), "Copy Path");
    let terminal = action_button(icons::ui(names::TERMINAL), "Terminal");
    let rename = action_button(icons::ui(names::RENAME), "Rename");
    let cut = action_button(icons::ui(names::CUT), "Move");
    let copy = action_button(icons::ui(names::COPY), "Copy");
    let trash = action_button(icons::ui(names::TRASH), "Trash");
    trash.add_css_class("destructive");

    grid.attach(&open, 0, 0, 1, 1);
    grid.attach(&open_with, 1, 0, 1, 1);
    grid.attach(&copy_path, 2, 0, 1, 1);
    grid.attach(&terminal, 3, 0, 1, 1);
    grid.attach(&rename, 0, 1, 1, 1);
    grid.attach(&cut, 1, 1, 1, 1);
    grid.attach(&copy, 2, 1, 1, 1);
    grid.attach(&trash, 3, 1, 1, 1);

    Actions {
        root: grid,
        open,
        open_with,
        copy_path,
        terminal,
        rename,
        cut,
        copy,
        trash,
    }
}

fn action_content(icon_name: &str, label: &str) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 5);
    content.set_halign(gtk::Align::Center);

    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(16);

    let text = gtk::Label::new(Some(label));
    text.add_css_class("teral-action-label");
    text.set_ellipsize(gtk::pango::EllipsizeMode::End);

    content.append(&icon);
    content.append(&text);
    content
}

fn action_button(icon_name: &str, label: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_child(Some(&action_content(icon_name, label)));
    button.add_css_class("teral-action");
    button.set_has_frame(false);
    button.set_tooltip_text(Some(label));
    button
}

fn action_menu_button(icon_name: &str, label: &str) -> gtk::MenuButton {
    let button = gtk::MenuButton::new();
    button.set_always_show_arrow(false);
    button.set_child(Some(&action_content(icon_name, label)));
    button.add_css_class("teral-action");
    button.set_has_frame(false);
    button.set_tooltip_text(Some(label));
    button
}

fn meta_row(key: &str) -> MetaRow {
    let key_label = gtk::Label::new(Some(key));
    key_label.set_xalign(0.0);
    key_label.set_width_request(78);
    key_label.set_valign(gtk::Align::Start);
    key_label.add_css_class("teral-meta-key");

    let value = gtk::Label::new(None);
    value.set_xalign(1.0);
    value.set_hexpand(true);
    value.set_wrap(true);
    value.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    value.set_selectable(true);
    value.add_css_class("teral-meta-value");
    if key == "Path" {
        value.add_css_class("path");
    }

    let root = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    root.append(&key_label);
    root.append(&value);

    MetaRow { root, value }
}

fn separator() -> gtk::Separator {
    let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
    separator.add_css_class("teral-separator");
    separator.set_margin_top(14);
    separator
}

/// Wire the action buttons once the application object exists.
pub fn connect(app: &App) {
    let actions = &app.widgets.details.actions;

    actions.open.connect_clicked({
        let app = Rc::clone(app);
        move |_| {
            if let Some(entry) = app.single_selection() {
                if entry.is_directory() {
                    app.navigate(entry.path());
                } else {
                    super::window::open_entry(&app, &entry);
                }
            }
        }
    });

    actions.copy_path.connect_clicked({
        let app = Rc::clone(app);
        move |_| {
            let Some(entry) = app.single_selection() else {
                return;
            };
            app.widgets
                .window
                .clipboard()
                .set_text(&entry.path().to_string_lossy());
            app.set_message("Path copied to the clipboard", false);
        }
    });

    actions.terminal.connect_clicked({
        let app = Rc::clone(app);
        move |_| {
            let target = app
                .single_selection()
                .filter(FileEntry::is_directory)
                .map(|entry| entry.path().to_path_buf())
                .unwrap_or_else(|| app.current_dir());
            super::window::open_terminal(&app, &target);
        }
    });

    actions.rename.connect_clicked({
        let app = Rc::clone(app);
        move |_| super::window::rename_selection(&app)
    });

    actions.copy.connect_clicked({
        let app = Rc::clone(app);
        move |_| super::window::stage_transfer(&app, crate::files::ops::TransferKind::Copy)
    });

    actions.cut.connect_clicked({
        let app = Rc::clone(app);
        move |_| super::window::stage_transfer(&app, crate::files::ops::TransferKind::Move)
    });

    actions.trash.connect_clicked({
        let app = Rc::clone(app);
        move |_| super::window::trash_selection(&app)
    });
}

/// Refresh the panel for the current selection.
pub fn update(app: &App) {
    let details = &app.widgets.details;
    let selected = app.selected_entries();

    if selected.len() != 1 {
        details.stack.set_visible_child_name("empty");
        details.title.set_text(&match selected.len() {
            0 => "Details".to_owned(),
            count => format!("{count} items selected"),
        });
        return;
    }

    let entry = &selected[0];
    let data = entry.data();

    details.stack.set_visible_child_name("details");
    details.title.set_text(entry.display_name());
    details.name.set_text(entry.display_name());
    details.kind.set_text(&data.kind);

    details.size.set_text(&if entry.is_directory() {
        let count = entry.child_count();
        if count < 0 {
            String::new()
        } else {
            crate::files::item_count_label(usize::try_from(count).unwrap_or(0))
        }
    } else {
        format_size(data.size)
    });

    icons::set_entry_icon(&details.icon, entry);
    match entry.thumbnail() {
        Some(texture) => {
            details.picture.set_paintable(Some(&texture));
            details.picture.set_visible(true);
            details.icon.set_visible(false);
        }
        None => {
            details.picture.set_visible(false);
            details.icon.set_visible(true);
            icons::request_thumbnail(entry);
        }
    }

    let owner = match (data.owner.as_deref(), data.group.as_deref()) {
        (Some(user), Some(group)) => format!("{user}:{group}"),
        (Some(user), None) => user.to_owned(),
        (None, Some(group)) => group.to_owned(),
        (None, None) => String::new(),
    };

    for (key, row) in &details.rows {
        let value = match *key {
            "Modified" => data.modified.as_ref().map(format_time).unwrap_or_default(),
            "Created" => data.created.as_ref().map(format_time).unwrap_or_default(),
            "Accessed" => data.accessed.as_ref().map(format_time).unwrap_or_default(),
            "Owner" => owner.clone(),
            "Permissions" => data.mode.map(format_permissions).unwrap_or_default(),
            "Links to" => data
                .symlink_target
                .as_ref()
                .map(|target| target.to_string_lossy().into_owned())
                .unwrap_or_default(),
            "Path" => data.path.to_string_lossy().into_owned(),
            _ => String::new(),
        };

        row.root.set_visible(!value.is_empty());
        row.value.set_text(&value);
    }

    let actions = &details.actions;
    let applications = crate::files::ops::applications_for(data.content_type.as_deref());
    let can_open_with = !entry.is_directory() && !applications.is_empty();
    actions.open_with.set_sensitive(can_open_with);
    if can_open_with {
        let popover = super::dialogs::open_with_popover(app, entry, applications);
        actions.open_with.set_popover(Some(&popover));
    } else {
        actions.open_with.set_popover(None::<&gtk::Popover>);
    }
}
