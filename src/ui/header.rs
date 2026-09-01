//! Top navigation: branding, history, breadcrumbs, search and view controls.

use super::{App, ViewMode, icon_button, icon_toggle};
use crate::files::SortKey;
use crate::icons;
use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;
use gtk::prelude::*;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Widgets that make up the top bar.
pub struct Header {
    pub bar: gtk::HeaderBar,
    pub back: gtk::Button,
    pub forward: gtk::Button,
    pub up: gtk::Button,
    pub crumbs: gtk::Box,
    pub path_stack: gtk::Stack,
    pub location: gtk::Entry,
    pub search_button: gtk::ToggleButton,
    pub grid_toggle: gtk::ToggleButton,
    pub list_toggle: gtk::ToggleButton,
    pub sort_button: gtk::MenuButton,
    pub menu_button: gtk::MenuButton,
}

pub fn build(search: &gtk::Revealer) -> Header {
    let bar = gtk::HeaderBar::new();
    bar.add_css_class("teral-toolbar");
    bar.set_show_title_buttons(true);

    let brand = super::brand::build();
    brand.set_margin_start(4);
    brand.set_margin_end(14);

    let back = icon_button(icons::ui(icons::names::BACK), "Back (Alt+Left)");
    let forward = icon_button(icons::ui(icons::names::FORWARD), "Forward (Alt+Right)");
    let up = icon_button(icons::ui(icons::names::UP), "Parent folder (Backspace)");

    let crumbs = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    crumbs.set_valign(gtk::Align::Center);

    let crumb_scroller = gtk::ScrolledWindow::builder()
        .child(&crumbs)
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_width(true)
        .build();
    crumb_scroller.add_css_class("teral-breadcrumb");

    let location = gtk::Entry::new();
    location.add_css_class("teral-location");
    location.set_width_request(360);

    let path_stack = gtk::Stack::new();
    path_stack.set_valign(gtk::Align::Center);
    path_stack.add_named(&crumb_scroller, Some("crumbs"));
    path_stack.add_named(&location, Some("location"));
    path_stack.set_visible_child_name("crumbs");

    let search_button = icon_toggle(
        icons::ui(icons::names::SEARCH),
        "Search this folder (Ctrl+F)",
    );

    let grid_toggle = icon_toggle(icons::ui(icons::names::GRID), "Grid view");
    let list_toggle = icon_toggle(icons::ui(icons::names::LIST), "List view");
    grid_toggle.set_active(true);
    list_toggle.set_group(Some(&grid_toggle));

    let view_group = gtk::Box::new(gtk::Orientation::Horizontal, 1);
    view_group.add_css_class("teral-button-group");
    view_group.set_valign(gtk::Align::Center);
    view_group.append(&grid_toggle);
    view_group.append(&list_toggle);

    let sort_button = icon_menu_button(icons::ui(icons::names::SORT), "Sorting and visibility");
    let menu_button = icon_menu_button(icons::ui(icons::names::MENU), "Folder actions");

    bar.pack_start(&brand);
    bar.pack_start(&back);
    bar.pack_start(&forward);
    bar.pack_start(&up);
    bar.pack_start(&path_stack);

    bar.pack_end(&menu_button);
    bar.pack_end(&sort_button);
    bar.pack_end(&view_group);
    bar.pack_end(&search_button);
    bar.pack_end(search);

    // The window title is carried by the breadcrumbs, not by a second heading.
    bar.set_title_widget(Some(&gtk::Label::new(None)));

    Header {
        bar,
        back,
        forward,
        up,
        crumbs,
        path_stack,
        location,
        search_button,
        grid_toggle,
        list_toggle,
        sort_button,
        menu_button,
    }
}

pub fn connect(app: &App) {
    let widgets = &app.widgets;

    widgets.back.connect_clicked({
        let app = Rc::clone(app);
        move |_| app.go_back()
    });
    widgets.forward.connect_clicked({
        let app = Rc::clone(app);
        move |_| app.go_forward()
    });
    widgets.up.connect_clicked({
        let app = Rc::clone(app);
        move |_| app.go_up()
    });

    widgets.location.connect_activate({
        let app = Rc::clone(app);
        move |entry| {
            let text = entry.text();
            let expanded = expand_path(text.as_str());
            let file = gio::File::for_path(&expanded);
            let weak = Rc::downgrade(&app);
            glib::spawn_future_local(async move {
                let result = file
                    .query_info_future(
                        gio::FILE_ATTRIBUTE_STANDARD_TYPE,
                        gio::FileQueryInfoFlags::NONE,
                        glib::Priority::DEFAULT,
                    )
                    .await;
                let Some(app) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(info) if info.file_type() == gio::FileType::Directory => {
                        app.navigate(&expanded);
                        hide_location(&app);
                    }
                    Ok(_) => app.show_error(&format!("{} is not a folder", expanded.display())),
                    Err(error) => app.show_error(&format!(
                        "Could not open {}: {}",
                        expanded.display(),
                        error.message().trim()
                    )),
                }
            });
        }
    });

    let escape = gtk::EventControllerKey::new();
    escape.connect_key_pressed({
        let app = Rc::clone(app);
        move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                hide_location(&app);
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    });
    widgets.location.add_controller(escape);

    widgets.search_button.connect_toggled({
        let app = Rc::clone(app);
        move |button| {
            if app.state.updating.get() {
                return;
            }
            if button.is_active() {
                super::search::open(&app);
            } else {
                super::search::close(&app);
            }
        }
    });

    widgets.grid_toggle.connect_toggled({
        let app = Rc::clone(app);
        move |button| {
            if button.is_active() {
                app.set_view_mode(ViewMode::Grid);
                if !app.state.updating.get() {
                    app.persist_file_preferences();
                }
            }
        }
    });
    widgets.list_toggle.connect_toggled({
        let app = Rc::clone(app);
        move |button| {
            if button.is_active() {
                app.set_view_mode(ViewMode::List);
                if !app.state.updating.get() {
                    app.persist_file_preferences();
                }
            }
        }
    });

    widgets.sort_button.set_popover(Some(&sort_popover(app)));
    widgets.menu_button.set_popover(Some(&folder_popover(app)));
}

/// Swap the breadcrumbs for an editable path entry (Ctrl+L).
pub fn show_location(app: &App) {
    let current = app.current_dir();
    app.widgets.location.set_text(&current.to_string_lossy());
    app.widgets.location.select_region(0, -1);
    app.widgets.path_stack.set_visible_child_name("location");
    app.widgets.location.grab_focus();
}

pub fn hide_location(app: &App) {
    app.widgets.path_stack.set_visible_child_name("crumbs");
    super::window::focus_file_view(app);
}

/// Replace the breadcrumbs with a single crumb naming the tag being shown.
pub fn show_tag_crumb(app: &App, tag: &str) {
    let crumbs = &app.widgets.crumbs;
    while let Some(child) = crumbs.first_child() {
        crumbs.remove(&child);
    }

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let dot = gtk::Label::new(Some("●"));
    dot.add_css_class("teral-tag-dot");
    if let Some(color) = crate::tags::current().get(tag).map(|tag| tag.color.clone()) {
        super::apply_color(&dot, &color);
    }
    content.append(&dot);
    content.append(&gtk::Label::new(Some(tag)));

    let button = gtk::Button::new();
    button.add_css_class("teral-crumb");
    button.add_css_class("current");
    button.set_has_frame(false);
    button.set_child(Some(&content));
    crumbs.append(&button);
}

/// Rebuild the clickable path components.
pub fn rebuild_breadcrumbs(app: &App, path: &Path) {
    let crumbs = &app.widgets.crumbs;
    while let Some(child) = crumbs.first_child() {
        crumbs.remove(&child);
    }

    let ancestors: Vec<PathBuf> = path
        .ancestors()
        .map(Path::to_path_buf)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    let last = ancestors.len().saturating_sub(1);
    for (index, ancestor) in ancestors.iter().enumerate() {
        if index > 0 {
            let separator = gtk::Label::new(Some("›"));
            separator.add_css_class("teral-crumb-separator");
            crumbs.append(&separator);
        }

        let button = gtk::Button::new();
        button.add_css_class("teral-crumb");
        button.set_has_frame(false);

        if index == 0 {
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 5);
            let icon = gtk::Image::from_icon_name(icons::ui(icons::names::DRIVE));
            icon.set_pixel_size(13);
            content.append(&icon);
            content.append(&gtk::Label::new(Some("Filesystem")));
            button.set_child(Some(&content));
        } else {
            let name = crate::places::display_label(ancestor);
            button.set_child(Some(&gtk::Label::new(Some(&name))));
        }

        if index == last {
            button.add_css_class("current");
        }

        let app = Rc::clone(app);
        let target = ancestor.clone();
        button.connect_clicked(move |_| app.navigate(&target));
        crumbs.append(&button);
    }
}

fn sort_popover(app: &App) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.add_css_class("teral-popover");

    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.set_width_request(180);

    let heading = super::tracked_label("SORT BY", 1);
    heading.add_css_class("teral-section-title");
    heading.set_margin_start(6);
    heading.set_margin_bottom(2);
    content.append(&heading);

    let mut group: Option<gtk::CheckButton> = None;
    for key in SortKey::ALL {
        let button = gtk::CheckButton::with_label(key.label());
        button.add_css_class("teral-menu-check");
        button.set_active(app.state.sorting.get().key == key);
        match group.as_ref() {
            Some(first) => button.set_group(Some(first)),
            None => group = Some(button.clone()),
        }

        let app = Rc::clone(app);
        button.connect_toggled(move |button| {
            if !button.is_active() || app.state.updating.get() {
                return;
            }
            let mut sorting = app.state.sorting.get();
            sorting.key = key;
            app.state.sorting.set(sorting);
            app.apply_filter();
            app.persist_file_preferences();
        });
        content.append(&button);
    }

    content.append(&separator());

    let descending = gtk::CheckButton::with_label("Descending");
    descending.add_css_class("teral-menu-check");
    descending.set_active(app.state.sorting.get().descending);
    descending.connect_toggled({
        let app = Rc::clone(app);
        move |button| {
            let mut sorting = app.state.sorting.get();
            sorting.descending = button.is_active();
            app.state.sorting.set(sorting);
            app.apply_filter();
            app.persist_file_preferences();
        }
    });
    content.append(&descending);

    let folders_first = gtk::CheckButton::with_label("Folders first");
    folders_first.add_css_class("teral-menu-check");
    folders_first.set_active(app.state.sorting.get().folders_first);
    folders_first.connect_toggled({
        let app = Rc::clone(app);
        move |button| {
            let mut sorting = app.state.sorting.get();
            sorting.folders_first = button.is_active();
            app.state.sorting.set(sorting);
            app.apply_filter();
            app.persist_file_preferences();
        }
    });
    content.append(&folders_first);

    let hidden = gtk::CheckButton::with_label("Show hidden files");
    hidden.add_css_class("teral-menu-check");
    hidden.set_active(app.state.show_hidden.get());
    hidden.connect_toggled({
        let app = Rc::clone(app);
        move |button| {
            if app.state.updating.get() {
                return;
            }
            app.state.show_hidden.set(button.is_active());
            app.apply_filter();
            app.persist_file_preferences();
        }
    });
    content.append(&hidden);

    // Keep the checkbox in step with the Ctrl+H shortcut.
    app.widgets.hidden_check.replace(Some(hidden));

    popover.set_child(Some(&content));
    popover
}

fn folder_popover(app: &App) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.add_css_class("teral-popover");

    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.set_width_request(210);

    let new_folder = menu_item(icons::ui(icons::names::NEW_FOLDER), "New Folder");
    let new_file = menu_item(icons::ui(icons::names::NEW_FILE), "New File");
    let paste = menu_item(icons::ui(icons::names::PASTE), "Paste");
    let terminal = menu_item(icons::ui(icons::names::TERMINAL), "Open Terminal Here");
    let pin = menu_item(icons::ui(icons::names::PIN), "Bookmark This Folder");
    let refresh = menu_item(icons::ui(icons::names::REFRESH), "Refresh");
    let empty_trash = menu_item(icons::ui(icons::names::TRASH), "Empty Trash");
    let shortcuts = menu_item(icons::ui(icons::names::HELP), "Keyboard Shortcuts");
    let about = menu_item(icons::ui(icons::names::ABOUT), "About Teral");

    for (item, action) in [
        (&new_folder, MenuAction::NewFolder),
        (&new_file, MenuAction::NewFile),
        (&paste, MenuAction::Paste),
        (&terminal, MenuAction::Terminal),
        (&pin, MenuAction::TogglePin),
        (&refresh, MenuAction::Refresh),
        (&empty_trash, MenuAction::EmptyTrash),
        (&shortcuts, MenuAction::Shortcuts),
        (&about, MenuAction::About),
    ] {
        let app = Rc::clone(app);
        let popover = popover.clone();
        item.connect_clicked(move |_| {
            popover.popdown();
            super::window::run_menu_action(&app, action);
        });
    }

    content.append(&new_folder);
    content.append(&new_file);
    content.append(&paste);
    content.append(&separator());
    content.append(&terminal);
    content.append(&pin);
    content.append(&refresh);
    content.append(&empty_trash);
    content.append(&separator());
    content.append(&shortcuts);
    content.append(&about);

    // Reflect the current clipboard and pin state whenever the menu opens.
    popover.connect_show({
        let app = Rc::clone(app);
        let new_folder = new_folder.clone();
        let new_file = new_file.clone();
        let paste = paste.clone();
        let terminal = terminal.clone();
        let pin = pin.clone();
        move |_| {
            let accepts_new_files = app.location().accepts_new_files();
            new_folder.set_visible(accepts_new_files);
            new_file.set_visible(accepts_new_files);
            paste.set_visible(accepts_new_files);
            terminal.set_visible(accepts_new_files);
            pin.set_visible(accepts_new_files);
            paste.set_sensitive(crate::files::ops::clipboard_has_files(
                &app.widgets.window.clipboard(),
            ));
            empty_trash.set_visible(crate::files::ops::is_in_trash(&app.current_dir()));
            let pinned = app.is_pinned(&app.current_dir());
            if let Some(row) = pin.child().and_downcast::<gtk::Box>()
                && let Some(label) = row.last_child().and_downcast::<gtk::Label>()
            {
                label.set_text(if pinned {
                    "Remove Bookmark"
                } else {
                    "Bookmark This Folder"
                });
            }
        }
    });

    popover.set_child(Some(&content));
    popover
}

/// Actions available from the folder menu and keyboard shortcuts.
#[derive(Debug, Clone, Copy)]
pub enum MenuAction {
    NewFolder,
    NewFile,
    Paste,
    Terminal,
    TogglePin,
    Refresh,
    EmptyTrash,
    Shortcuts,
    About,
}

pub fn menu_item(icon_name: &str, label: &str) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_hexpand(true);
    content.append(&icon);
    content.append(&text);

    let button = gtk::Button::new();
    button.set_child(Some(&content));
    button.add_css_class("teral-menu-item");
    button.set_has_frame(false);
    button
}

/// A flat icon-only menu button that matches Teral's other toolbar controls.
fn icon_menu_button(icon_name: &str, tooltip: &str) -> gtk::MenuButton {
    let button = gtk::MenuButton::new();
    button.set_child(Some(&gtk::Image::from_icon_name(icon_name)));
    button.set_always_show_arrow(false);
    button.add_css_class("teral-icon-button");
    button.set_has_frame(false);
    button.set_tooltip_text(Some(tooltip));
    button.set_valign(gtk::Align::Center);
    button
}

fn separator() -> gtk::Separator {
    let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
    separator.add_css_class("teral-separator");
    separator.set_margin_top(4);
    separator.set_margin_bottom(4);
    separator
}

/// Expand a leading `~` so typed paths behave the way they do in a shell.
fn expand_path(text: &str) -> PathBuf {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix('~')
        && let Some(home) = crate::theme::home_dir()
    {
        return home.join(rest.trim_start_matches('/'));
    }
    PathBuf::from(trimmed)
}
