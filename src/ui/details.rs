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
    pub text_preview: gtk::TextView,
    pub text_scroller: gtk::ScrolledWindow,
    pub preview_message: gtk::Label,
    pub name: gtk::Label,
    pub kind: gtk::Label,
    pub size: gtk::Label,
    pub rows: Vec<(&'static str, MetaRow)>,
    pub tags: gtk::FlowBox,
    pub actions: Actions,
    pub folder_actions: FolderActions,
    pub multi: MultiActions,
}

/// Actions offered for several selected entries at once.
pub struct MultiActions {
    pub root: gtk::Box,
    pub summary: gtk::Label,
    pub copy: gtk::Button,
    pub cut: gtk::Button,
    pub compress: gtk::Button,
    pub tags: gtk::MenuButton,
    pub copy_paths: gtk::Button,
    pub trash: gtk::Button,
}

/// Actions offered for the folder being browsed, with nothing selected.
pub struct FolderActions {
    pub root: gtk::Grid,
    pub terminal: gtk::Button,
    pub new_folder: gtk::Button,
    pub paste: gtk::Button,
    pub bookmark: gtk::Button,
    pub new_tab: gtk::Button,
    pub new_window: gtk::Button,
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
    title.set_max_width_chars(1);
    title.add_css_class("teral-details-title");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.add_css_class("teral-details-header");
    header.append(&title);

    let icon = gtk::Image::new();
    icon.set_pixel_size(52);
    icon.set_hexpand(true);
    icon.set_vexpand(true);

    let picture = gtk::Picture::new();
    picture.set_content_fit(gtk::ContentFit::Contain);
    picture.set_can_shrink(true);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_visible(false);

    let text_preview = gtk::TextView::new();
    text_preview.set_editable(false);
    text_preview.set_cursor_visible(false);
    text_preview.set_monospace(true);
    text_preview.set_wrap_mode(gtk::WrapMode::WordChar);
    text_preview.add_css_class("teral-text-preview");

    let text_scroller = gtk::ScrolledWindow::builder()
        .child(&text_preview)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .hexpand(true)
        .vexpand(true)
        .build();
    text_scroller.set_visible(false);

    let preview_message = gtk::Label::new(None);
    preview_message.set_wrap(true);
    preview_message.set_max_width_chars(1);
    preview_message.add_css_class("teral-muted");
    preview_message.set_visible(false);

    let preview = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    preview.add_css_class("teral-preview");
    preview.set_overflow(gtk::Overflow::Hidden);
    // A fixed, modest height: the metadata and actions below matter more than a
    // large picture, so the preview never takes over the panel.
    preview.set_size_request(-1, 116);
    preview.set_vexpand(false);
    preview.append(&icon);
    preview.append(&picture);
    preview.append(&text_scroller);
    preview.append(&preview_message);

    let name = gtk::Label::new(None);
    name.set_xalign(0.0);
    // A very long file name must never be allowed to widen the panel; it wraps to at
    // most three lines and then ellipsises.
    name.set_wrap(true);
    name.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    name.set_lines(3);
    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    name.set_max_width_chars(1);
    name.add_css_class("teral-details-name");

    let kind = gtk::Label::new(None);
    kind.set_xalign(0.0);
    kind.set_ellipsize(gtk::pango::EllipsizeMode::End);
    kind.set_max_width_chars(1);
    kind.add_css_class("teral-details-kind");

    let size = gtk::Label::new(None);
    size.set_xalign(0.0);
    size.set_ellipsize(gtk::pango::EllipsizeMode::End);
    size.set_max_width_chars(1);
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

    let tags_heading = super::tracked_label("TAGS", 1);
    tags_heading.add_css_class("teral-section-title");
    tags_heading.set_margin_top(16);
    tags_heading.set_margin_bottom(8);

    let tags = gtk::FlowBox::new();
    tags.add_css_class("teral-tag-chips");
    tags.set_selection_mode(gtk::SelectionMode::None);
    tags.set_max_children_per_line(4);
    tags.set_row_spacing(6);
    tags.set_column_spacing(6);

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
    body.append(&tags_heading);
    body.append(&tags);
    body.append(&actions_heading);
    body.append(&actions.root);

    let body_scroller = gtk::ScrolledWindow::builder()
        .child(&body)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();

    let empty_hint = gtk::Label::new(Some("Nothing selected"));
    empty_hint.set_xalign(0.0);
    empty_hint.add_css_class("teral-empty");

    let folder_heading = super::tracked_label("THIS FOLDER", 1);
    folder_heading.add_css_class("teral-section-title");
    folder_heading.set_margin_top(14);
    folder_heading.set_margin_bottom(8);

    let folder_actions = build_folder_actions();

    // With nothing selected the panel is still worth having: it offers the actions
    // that apply to the folder being browsed.
    let empty = gtk::Box::new(gtk::Orientation::Vertical, 0);
    empty.add_css_class("teral-details-body");
    empty.append(&empty_hint);
    empty.append(&folder_heading);
    empty.append(&folder_actions.root);

    // Several selected files can still be acted on, so the panel offers the actions
    // that work on a whole selection rather than going blank.
    let multi = build_multi_actions();

    let stack = gtk::Stack::new();
    stack.add_named(&empty, Some("empty"));
    stack.add_named(&body_scroller, Some("details"));
    stack.add_named(&multi.root, Some("multi"));
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
        text_preview,
        text_scroller,
        preview_message,
        name,
        kind,
        size,
        rows,
        tags,
        actions,
        folder_actions,
        multi,
    }
}

fn build_multi_actions() -> MultiActions {
    let summary = gtk::Label::new(None);
    summary.set_xalign(0.0);
    summary.set_wrap(true);
    summary.set_max_width_chars(1);
    summary.add_css_class("teral-empty");

    let heading = super::tracked_label("ACTIONS", 1);
    heading.add_css_class("teral-section-title");
    heading.set_margin_top(14);
    heading.set_margin_bottom(8);

    let grid = gtk::Grid::new();
    grid.set_row_spacing(7);
    grid.set_column_spacing(7);
    grid.set_column_homogeneous(true);

    let copy = action_button(icons::ui(names::COPY), "Copy");
    let cut = action_button(icons::ui(names::CUT), "Cut");
    let compress = action_button(icons::ui(names::COMPRESS), "Compress");
    let tags = action_menu_button(icons::ui(names::TAG), "Tags");
    let copy_paths = action_button(icons::ui(names::COPY_PATH), "Copy Paths");
    let trash = action_button(icons::ui(names::TRASH), "Trash");
    trash.add_css_class("destructive");

    grid.attach(&copy, 0, 0, 1, 1);
    grid.attach(&cut, 1, 0, 1, 1);
    grid.attach(&compress, 2, 0, 1, 1);
    grid.attach(&tags, 0, 1, 1, 1);
    grid.attach(&copy_paths, 1, 1, 1, 1);
    grid.attach(&trash, 2, 1, 1, 1);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("teral-details-body");
    root.append(&summary);
    root.append(&heading);
    root.append(&grid);

    MultiActions {
        root,
        summary,
        copy,
        cut,
        compress,
        tags,
        copy_paths,
        trash,
    }
}

fn build_folder_actions() -> FolderActions {
    let grid = gtk::Grid::new();
    grid.set_row_spacing(7);
    grid.set_column_spacing(7);
    grid.set_column_homogeneous(true);

    let terminal = action_button(icons::ui(names::TERMINAL), "Terminal");
    let new_folder = action_button(icons::ui(names::NEW_FOLDER), "New Folder");
    let paste = action_button(icons::ui(names::PASTE), "Paste");
    let bookmark = action_button(icons::ui(names::PIN), "Bookmark");
    let new_tab = action_button(icons::ui(names::ADD), "New Tab");
    let new_window = action_button(icons::ui(names::WINDOW), "New Window");

    grid.attach(&terminal, 0, 0, 1, 1);
    grid.attach(&new_folder, 1, 0, 1, 1);
    grid.attach(&paste, 2, 0, 1, 1);
    grid.attach(&bookmark, 0, 1, 1, 1);
    grid.attach(&new_tab, 1, 1, 1, 1);
    grid.attach(&new_window, 2, 1, 1, 1);

    FolderActions {
        root: grid,
        terminal,
        new_folder,
        paste,
        bookmark,
        new_tab,
        new_window,
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
    let cut = action_button(icons::ui(names::CUT), "Cut");
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
    text.set_max_width_chars(1);

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
    value.set_lines(if key == "Path" { 4 } else { 2 });
    value.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    value.set_max_width_chars(1);
    value.set_selectable(true);
    // Selectable labels take keyboard focus by default, which paints a focus block
    // over the first row as soon as the panel appears.
    value.set_can_focus(false);
    value.add_css_class("teral-meta-value");
    if key == "Path" {
        value.add_css_class("path");
    }

    let root = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    root.append(&key_label);
    root.append(&value);

    MetaRow { root, value }
}

/// Show the tags on the selected entry, with a control to attach more.
fn rebuild_tag_chips(app: &App, entry: &FileEntry) {
    let container = &app.widgets.details.tags;
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    let path = entry.path().to_path_buf();
    let store = crate::tags::current();

    for tag in store.for_path(&path) {
        let dot = gtk::Label::new(Some("●"));
        dot.add_css_class("teral-tag-dot");
        super::apply_color(&dot, &tag.color);

        let label = gtk::Label::new(Some(&tag.name));
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(14);

        let remove = gtk::Button::from_icon_name(icons::ui(names::CLOSE));
        remove.add_css_class("teral-tag-remove");
        remove.set_has_frame(false);
        remove.set_valign(gtk::Align::Center);
        remove.set_tooltip_text(Some("Remove this tag"));

        let chip = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        chip.add_css_class("teral-tag-chip");
        chip.append(&dot);
        chip.append(&label);
        chip.append(&remove);

        remove.connect_clicked({
            let app = Rc::clone(app);
            let name = tag.name.clone();
            let path = path.clone();
            move |_| {
                let app = Rc::clone(&app);
                let name = name.clone();
                let path = path.clone();
                gtk::glib::spawn_future_local(async move {
                    if let Err(error) = crate::tags::edit(|tags| {
                        tags.set_tagged(&name, std::slice::from_ref(&path), false)
                    })
                    .await
                    {
                        app.show_error(&format!("Could not save tags: {error}"));
                        return;
                    }
                    super::sidebar::rebuild_tags(&app);
                    app.update_details();
                });
            }
        });

        container.append(&chip);
    }

    let add = gtk::MenuButton::new();
    add.add_css_class("teral-tag-chip");
    add.add_css_class("add");
    add.set_always_show_arrow(false);
    add.set_child(Some(&gtk::Image::from_icon_name(icons::ui(names::ADD))));
    add.set_tooltip_text(Some("Add a tag"));
    add.set_popover(Some(&super::window::tag_popover(
        app,
        std::slice::from_ref(&path),
    )));
    container.append(&add);
}

/// Replace the caption under an action button.
fn relabel(button: &gtk::Button, text: &str) {
    if let Some(content) = button.child().and_downcast::<gtk::Box>()
        && let Some(label) = content.last_child().and_downcast::<gtk::Label>()
    {
        label.set_text(text);
    }
    button.set_tooltip_text(Some(text));
}

/// Replace the icon above an action button's caption.
fn set_action_icon(button: &gtk::Button, icon_name: &str) {
    if let Some(content) = button.child().and_downcast::<gtk::Box>()
        && let Some(image) = content.first_child().and_downcast::<gtk::Image>()
    {
        image.set_icon_name(Some(icon_name));
    }
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

    // Built the moment the button is pressed rather than on every selection: building
    // it eagerly made each click pay for icon lookups it usually never showed. GTK calls
    // this just before the popover is shown, which is the only hook a plain click has —
    // `activate` never fires for a mouse press, which is why this button used to do
    // nothing at all.
    actions.open_with.set_create_popup_func({
        let app = Rc::clone(app);
        move |button| {
            let Some(entry) = app.single_selection() else {
                return;
            };
            let applications =
                crate::files::ops::applications_for(entry.data().content_type.as_deref());
            if applications.is_empty() {
                return;
            }
            button.set_popover(Some(&super::dialogs::open_with_popover(
                &app,
                &entry,
                applications,
            )));
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

    // In the trash, Move becomes Restore and Trash becomes a permanent delete: moving
    // something that is already deleted, or trashing it twice, means nothing.
    actions.cut.connect_clicked({
        let app = Rc::clone(app);
        move |_| {
            if app.location().is_trash() {
                super::window::restore_selection(&app);
            } else {
                super::window::stage_transfer(&app, crate::files::ops::TransferKind::Move);
            }
        }
    });

    let folder = &app.widgets.details.folder_actions;

    folder.terminal.connect_clicked({
        let app = Rc::clone(app);
        move |_| {
            let current = app.current_dir();
            super::window::open_terminal(&app, &current);
        }
    });
    folder.new_folder.connect_clicked({
        let app = Rc::clone(app);
        move |_| super::window::run_menu_action(&app, super::header::MenuAction::NewFolder)
    });
    folder.paste.connect_clicked({
        let app = Rc::clone(app);
        move |_| super::window::paste(&app)
    });
    folder.bookmark.connect_clicked({
        let app = Rc::clone(app);
        move |_| {
            let current = app.current_dir();
            app.toggle_pin(&current);
            app.update_details();
        }
    });
    folder.new_tab.connect_clicked({
        let app = Rc::clone(app);
        move |_| {
            let current = app.current_dir();
            app.open_tab(current);
        }
    });
    folder.new_window.connect_clicked({
        let app = Rc::clone(app);
        move |_| {
            let current = app.current_dir();
            super::window::open_in_new_window(&app, &current);
        }
    });

    actions.trash.connect_clicked({
        let app = Rc::clone(app);
        move |_| {
            if app.location().is_trash() {
                super::window::delete_permanently(&app);
            } else {
                super::window::trash_selection(&app);
            }
        }
    });

    connect_multi(app);
}

/// Wire the actions offered for a selection of several entries.
fn connect_multi(app: &App) {
    let multi = &app.widgets.details.multi;

    multi.copy.connect_clicked({
        let app = Rc::clone(app);
        move |_| super::window::stage_transfer(&app, crate::files::ops::TransferKind::Copy)
    });
    multi.cut.connect_clicked({
        let app = Rc::clone(app);
        move |_| {
            if app.location().is_trash() {
                super::window::restore_selection(&app);
            } else {
                super::window::stage_transfer(&app, crate::files::ops::TransferKind::Move);
            }
        }
    });
    multi.compress.connect_clicked({
        let app = Rc::clone(app);
        move |_| super::window::compress_selection(&app)
    });
    multi.copy_paths.connect_clicked({
        let app = Rc::clone(app);
        move |_| {
            let paths: Vec<String> = app
                .selected_entries()
                .iter()
                .map(|entry| entry.path().to_string_lossy().into_owned())
                .collect();
            if paths.is_empty() {
                return;
            }
            app.widgets.window.clipboard().set_text(&paths.join("\n"));
            app.set_message(
                &format!("{} copied to the clipboard", paths_label(paths.len())),
                false,
            );
        }
    });
    multi.trash.connect_clicked({
        let app = Rc::clone(app);
        move |_| {
            if app.location().is_trash() {
                super::window::delete_permanently(&app);
            } else {
                super::window::trash_selection(&app);
            }
        }
    });

    // The popover is built against whatever is selected at the moment it opens, so it
    // never goes stale and costs nothing while the selection is only being changed.
    multi.tags.set_create_popup_func({
        let app = Rc::clone(app);
        move |button| {
            let paths: Vec<std::path::PathBuf> = app
                .selected_entries()
                .iter()
                .map(|entry| entry.path().to_path_buf())
                .collect();
            if paths.is_empty() {
                return;
            }
            button.set_popover(Some(&super::window::tag_popover(&app, &paths)));
        }
    });
}

fn paths_label(count: usize) -> String {
    if count == 1 {
        "1 path".to_owned()
    } else {
        format!("{count} paths")
    }
}

/// Describe and offer actions for a selection of several entries.
fn update_multi(app: &App, selected: &[FileEntry]) {
    let details = &app.widgets.details;
    let multi = &details.multi;
    let in_trash = crate::files::ops::is_in_trash(&app.current_dir());

    details.stack.set_visible_child_name("multi");
    details
        .title
        .set_text(&format!("{} items selected", selected.len()));

    let folders = selected.iter().filter(|entry| entry.is_directory()).count();
    let files = selected.len() - folders;
    // Only files carry a size worth adding up: a folder's own entry says nothing about
    // what is inside it, and Teral does not walk the tree to find out.
    let bytes: u64 = selected
        .iter()
        .filter(|entry| !entry.is_directory())
        .map(|entry| entry.data().size)
        .sum();

    let mut parts = Vec::new();
    if folders > 0 {
        parts.push(count_label(folders, "folder"));
    }
    if files > 0 {
        parts.push(count_label(files, "file"));
    }
    if bytes > 0 {
        parts.push(format_size(bytes));
    }
    multi.summary.set_text(&parts.join("   ·   "));

    relabel(&multi.cut, if in_trash { "Restore" } else { "Cut" });
    relabel(&multi.trash, if in_trash { "Delete" } else { "Trash" });
    set_action_icon(
        &multi.cut,
        if in_trash {
            icons::ui(names::RESTORE)
        } else {
            icons::ui(names::CUT)
        },
    );
    set_action_icon(
        &multi.trash,
        if in_trash {
            icons::ui(names::DELETE)
        } else {
            icons::ui(names::TRASH)
        },
    );

    multi.copy.set_sensitive(!in_trash);
    // Compress needs both an archiving tool and somewhere sensible to write; the trash
    // is neither.
    multi
        .compress
        .set_visible(!in_trash && crate::files::ops::can_compress());
    multi.tags.set_visible(!in_trash);
}

fn count_label(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// Refresh the panel for the current selection.
pub fn update(app: &App) {
    let preview_generation = app.state.preview_generation.get().wrapping_add(1);
    app.state.preview_generation.set(preview_generation);
    let details = &app.widgets.details;
    let selected = app.selected_entries();

    if selected.is_empty() {
        details.stack.set_visible_child_name("empty");
        details
            .title
            .set_text(&crate::places::display_label(&app.current_dir()));

        let current = app.current_dir();
        let folder = &details.folder_actions;
        let accepts_new_files = app.location().accepts_new_files();
        folder.terminal.set_visible(accepts_new_files);
        folder.new_folder.set_visible(accepts_new_files);
        folder.paste.set_visible(accepts_new_files);
        folder.bookmark.set_visible(accepts_new_files);
        folder
            .paste
            .set_sensitive(crate::files::ops::clipboard_has_files(
                &app.widgets.window.clipboard(),
            ));
        relabel(
            &folder.bookmark,
            if app.is_pinned(&current) {
                "Bookmarked"
            } else {
                "Bookmark"
            },
        );
        return;
    }

    if selected.len() > 1 {
        update_multi(app, &selected);
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
    details.text_scroller.set_visible(false);
    details.preview_message.set_visible(false);
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

    if is_text_preview(data.content_type.as_deref()) {
        details.picture.set_visible(false);
        details.icon.set_visible(false);
        if data.size > crate::files::preview::MAX_TEXT_BYTES {
            details
                .preview_message
                .set_text("Text preview is limited to 2 MiB");
            details.preview_message.set_visible(true);
        } else {
            details.preview_message.set_text("Loading preview…");
            details.preview_message.set_visible(true);
            let app = Rc::clone(app);
            let path = entry.path().to_path_buf();
            gtk::glib::spawn_future_local(async move {
                let result = crate::files::preview::load_text(&path).await;
                if app.state.preview_generation.get() != preview_generation
                    || app
                        .single_selection()
                        .map(|entry| entry.path().to_path_buf())
                        .as_ref()
                        != Some(&path)
                {
                    return;
                }
                let details = &app.widgets.details;
                match result {
                    Ok(crate::files::preview::TextPreview::Text(text)) => {
                        details.text_preview.buffer().set_text(&text);
                        details.preview_message.set_visible(false);
                        details.text_scroller.set_visible(true);
                    }
                    Ok(crate::files::preview::TextPreview::Oversized) => {
                        details.preview_message.set_text("Text preview is limited to 2 MiB");
                    }
                    Ok(crate::files::preview::TextPreview::Binary) => {
                        details.preview_message.set_text("This file contains binary data");
                    }
                    Ok(crate::files::preview::TextPreview::UnsupportedEncoding) => {
                        details.preview_message.set_text("This text encoding is not supported");
                    }
                    Err(error) => details
                        .preview_message
                        .set_text(&format!("Preview unavailable: {error}")),
                }
            });
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
            // A link whose target is gone still names what it pointed at, and says so.
            "Links to" => data.link_summary().unwrap_or_default(),
            "Path" => data.path.to_string_lossy().into_owned(),
            _ => String::new(),
        };

        row.root.set_visible(!value.is_empty());
        row.value.set_text(&value);
    }

    rebuild_tag_chips(app, entry);

    let actions = &details.actions;
    let in_trash = app.location().is_trash();
    actions
        .terminal
        .set_visible(entry.is_directory() && app.location().accepts_new_files());

    relabel(&actions.cut, if in_trash { "Restore" } else { "Cut" });
    relabel(&actions.trash, if in_trash { "Delete" } else { "Trash" });
    set_action_icon(
        &actions.cut,
        if in_trash {
            icons::ui(names::RESTORE)
        } else {
            icons::ui(names::CUT)
        },
    );
    set_action_icon(
        &actions.trash,
        if in_trash {
            icons::ui(names::DELETE)
        } else {
            icons::ui(names::TRASH)
        },
    );
    actions.copy.set_sensitive(!in_trash);

    // The popover is built when it is opened, not on every selection: building it
    // eagerly meant every click paid for icon lookups it usually never showed.
    let content_type = data.content_type.clone();
    let can_query = !entry.is_directory() && entry.is_openable() && content_type.is_some();
    actions.open_with.set_sensitive(false);
    actions.open_with.set_popover(None::<&gtk::Popover>);
    if can_query {
        let app = Rc::clone(app);
        let path = entry.path().to_path_buf();
        let content_type = content_type.expect("checked above");
        gtk::glib::spawn_future_local(async move {
            let applications = crate::files::ops::load_applications(content_type).await;
            let still_selected = app
                .single_selection()
                .is_some_and(|entry| entry.path() == path);
            if still_selected {
                app.widgets
                    .details
                    .actions
                    .open_with
                    .set_sensitive(!applications.is_empty());
            }
        });
    }

    // A broken link has nothing behind it, and a FIFO or device would hang whatever
    // opened it, so the action is refused up front rather than failing on the click.
    actions.open.set_sensitive(entry.is_openable());
}

fn is_text_preview(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|content_type| {
        content_type.starts_with("text/")
            || matches!(
                content_type,
                "application/json"
                    | "application/toml"
                    | "application/xml"
                    | "application/x-yaml"
            )
    })
}
