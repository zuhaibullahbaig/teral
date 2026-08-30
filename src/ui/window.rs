//! Window assembly, application actions and keyboard behaviour.

use super::{
    App, AppInner, State, ViewMode, details, dialogs, fileview, header, sidebar, statusbar,
};
use crate::files::ops::{self, CancelFlag, Clipboard, TransferKind};
use crate::files::{FileEntry, Sorting};
use crate::icons;
use crate::places;
use crate::theme::{ThemeConfig, home_dir};
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Every widget Teral needs to reach again after construction.
pub struct Widgets {
    pub window: gtk::ApplicationWindow,

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
    pub hidden_check: RefCell<Option<gtk::CheckButton>>,

    pub search_bar: gtk::SearchBar,
    pub search_entry: gtk::SearchEntry,

    pub folder_title: gtk::Label,
    pub folder_subtitle: gtk::Label,
    pub new_folder: gtk::Button,

    pub content: gtk::Box,
    pub view_stack: gtk::Stack,
    pub grid: gtk::GridView,
    pub list: gtk::ColumnView,
    pub grid_scroller: gtk::ScrolledWindow,
    pub list_scroller: gtk::ScrolledWindow,
    pub context_menu: gtk::Popover,

    pub places_box: gtk::Box,
    pub devices_box: gtk::Box,
    pub pinned_box: gtk::Box,
    pub pinned_section: gtk::Box,

    pub details: details::Details,

    pub console: statusbar::Console,
    pub command_entry: gtk::Entry,
    pub status_selection: gtk::Label,
    pub status_size: gtk::Label,
    pub status_free: gtk::Label,
    pub status_message: gtk::Label,
    pub zoom: gtk::Scale,
}

/// Build the main window and start browsing the user's home directory.
pub fn build_window(application: &gtk::Application, theme: ThemeConfig) -> gtk::ApplicationWindow {
    let store = gio::ListStore::new::<FileEntry>();
    let selection = gtk::MultiSelection::new(Some(store.clone()));

    let head = header::build();
    let side = sidebar::build(theme.sidebar_width());
    let detail = details::build(theme.details_width());
    let status = statusbar::build(theme.grid_icon_size(), theme.spacing());
    let console = statusbar::build_console();

    let search_entry = gtk::SearchEntry::new();
    search_entry.add_css_class("teral-search");
    search_entry.set_hexpand(true);
    search_entry.set_placeholder_text(Some("Filter this folder by name"));

    let search_bar = gtk::SearchBar::new();
    search_bar.add_css_class("teral-searchbar");
    search_bar.set_child(Some(&search_entry));
    search_bar.set_show_close_button(false);

    let folder_title = gtk::Label::new(None);
    folder_title.set_xalign(0.0);
    folder_title.add_css_class("teral-folder-title");
    folder_title.set_ellipsize(gtk::pango::EllipsizeMode::Middle);

    let folder_subtitle = gtk::Label::new(None);
    folder_subtitle.set_xalign(0.0);
    folder_subtitle.add_css_class("teral-folder-subtitle");

    let titles = gtk::Box::new(gtk::Orientation::Vertical, 2);
    titles.set_hexpand(true);
    titles.append(&folder_title);
    titles.append(&folder_subtitle);

    let new_folder = super::icon_button(
        crate::icons::ui(crate::icons::names::ADD),
        "New folder (Ctrl+Shift+N)",
    );

    let content_header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content_header.add_css_class("teral-content-header");
    content_header.append(&titles);
    content_header.append(&new_folder);

    let grid = gtk::GridView::new(Some(selection.clone()), None::<gtk::SignalListItemFactory>);
    grid.add_css_class("teral-grid");
    grid.set_min_columns(1);
    grid.set_max_columns(24);
    grid.set_enable_rubberband(true);
    grid.set_vexpand(true);

    let grid_scroller = gtk::ScrolledWindow::builder()
        .child(&grid)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    grid_scroller.add_css_class("teral-file-area");

    let list = gtk::ColumnView::new(Some(selection.clone()));
    list.add_css_class("teral-list");
    list.set_show_row_separators(false);
    list.set_show_column_separators(false);
    list.set_reorderable(false);
    list.set_vexpand(true);

    let list_scroller = gtk::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .build();
    list_scroller.add_css_class("teral-file-area");

    let view_stack = gtk::Stack::new();
    view_stack.add_named(&grid_scroller, Some("grid"));
    view_stack.add_named(&list_scroller, Some("list"));
    view_stack.set_visible_child_name("grid");
    view_stack.set_vexpand(true);

    let context_menu = gtk::Popover::new();
    context_menu.add_css_class("teral-popover");
    context_menu.set_has_arrow(false);
    context_menu.set_halign(gtk::Align::Start);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("teral-content");
    content.set_hexpand(true);
    content.append(&content_header);
    content.append(&search_bar);
    content.append(&view_stack);
    content.append(&console.root);
    context_menu.set_parent(&content);

    let panes = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    panes.set_vexpand(true);
    panes.append(&side.root);
    panes.append(&content);
    panes.append(&detail.root);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("teral-root");
    root.append(&panes);
    root.append(&status.root);

    let window = gtk::ApplicationWindow::builder()
        .application(application)
        .title("Teral")
        .default_width(theme.window_width())
        .default_height(theme.window_height())
        .child(&root)
        .build();
    window.add_css_class("teral-window");
    window.set_titlebar(Some(&head.bar));

    search_bar.set_key_capture_widget(Some(&window));

    let widgets = Widgets {
        window: window.clone(),
        back: head.back,
        forward: head.forward,
        up: head.up,
        crumbs: head.crumbs,
        path_stack: head.path_stack,
        location: head.location,
        search_button: head.search_button,
        grid_toggle: head.grid_toggle,
        list_toggle: head.list_toggle,
        sort_button: head.sort_button,
        menu_button: head.menu_button,
        hidden_check: RefCell::new(None),
        search_bar,
        search_entry,
        folder_title,
        folder_subtitle,
        new_folder,
        content,
        view_stack,
        grid,
        list,
        grid_scroller,
        list_scroller,
        context_menu,
        places_box: side.places,
        devices_box: side.devices,
        pinned_box: side.pinned,
        pinned_section: side.pinned_section,
        details: detail,
        console,
        command_entry: status.command_entry,
        status_selection: status.selection,
        status_size: status.size,
        status_free: status.free,
        status_message: status.message,
        zoom: status.zoom,
    };

    let state = State {
        current: RefCell::new(PathBuf::from("/")),
        back: RefCell::new(Vec::new()),
        forward: RefCell::new(Vec::new()),
        all: RefCell::new(Vec::new()),
        store,
        selection,
        sorting: Cell::new(Sorting::default()),
        show_hidden: Cell::new(false),
        query: RefCell::new(String::new()),
        generation: Cell::new(0),
        pinned: RefCell::new(places::load_pinned()),
        clipboard: RefCell::new(None),
        icon_size: Cell::new(theme.grid_icon_size()),
        view_mode: Cell::new(ViewMode::Grid),
        running_command: RefCell::new(None),
        running_transfer: RefCell::new(None),
        updating: Cell::new(false),
    };

    let app: App = Rc::new(AppInner {
        state,
        widgets,
        theme,
    });

    header::connect(&app);
    sidebar::connect(&app);
    fileview::connect(&app);
    details::connect(&app);
    statusbar::connect(&app);
    connect_window(&app);

    let start = home_dir().unwrap_or_else(|| PathBuf::from("/"));
    app.load(&start, None);

    window
}

fn connect_window(app: &App) {
    app.widgets.new_folder.connect_clicked({
        let app = Rc::clone(app);
        move |_| new_folder(&app)
    });

    app.widgets.search_entry.connect_search_changed({
        let app = Rc::clone(app);
        move |entry| {
            *app.state.query.borrow_mut() = entry.text().to_string();
            app.apply_filter();
        }
    });

    app.widgets.search_bar.connect_search_mode_enabled_notify({
        let app = Rc::clone(app);
        move |bar| {
            app.state.updating.set(true);
            app.widgets.search_button.set_active(bar.is_search_mode());
            app.state.updating.set(false);
            if !bar.is_search_mode() && !app.state.query.borrow().is_empty() {
                app.state.query.borrow_mut().clear();
                app.widgets.search_entry.set_text("");
                app.apply_filter();
            }
        }
    });

    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed({
        let app = Rc::clone(app);
        move |_, key, _, modifiers| on_key(&app, key, modifiers)
    });
    app.widgets.window.add_controller(keys);

    // Right-clicking empty space acts on the current folder.
    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_SECONDARY);
    gesture.connect_pressed({
        let app = Rc::clone(app);
        move |_, _, x, y| {
            app.state.selection.unselect_all();
            let widget = app.widgets.view_stack.clone();
            show_context_menu(&app, widget.upcast_ref::<gtk::Widget>(), x, y);
        }
    });
    app.widgets.view_stack.add_controller(gesture);
}

fn on_key(app: &App, key: gdk::Key, modifiers: gdk::ModifierType) -> glib::Propagation {
    let control = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
    let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
    let alt = modifiers.contains(gdk::ModifierType::ALT_MASK);

    match key {
        gdk::Key::Left if alt => app.go_back(),
        gdk::Key::Right if alt => app.go_forward(),
        gdk::Key::BackSpace => app.go_up(),
        gdk::Key::l | gdk::Key::L if control => header::show_location(app),
        gdk::Key::f | gdk::Key::F if control => {
            app.widgets.search_bar.set_search_mode(true);
            app.widgets.search_entry.grab_focus();
        }
        gdk::Key::h | gdk::Key::H if control => toggle_hidden(app),
        gdk::Key::k | gdk::Key::K if control => {
            app.widgets.command_entry.grab_focus();
        }
        gdk::Key::c | gdk::Key::C if control => stage_transfer(app, TransferKind::Copy),
        gdk::Key::x | gdk::Key::X if control => stage_transfer(app, TransferKind::Move),
        gdk::Key::v | gdk::Key::V if control => paste(app),
        gdk::Key::n | gdk::Key::N if control && shift => new_folder(app),
        gdk::Key::t | gdk::Key::T if control && shift => {
            let directory = app.current_dir();
            open_terminal(app, &directory);
        }
        gdk::Key::r | gdk::Key::R if control => app.reload(),
        gdk::Key::F5 => app.reload(),
        gdk::Key::F2 => rename_selection(app),
        gdk::Key::Delete => trash_selection(app),
        gdk::Key::_0 if control => app.reset_zoom(),
        gdk::Key::Escape => {
            let cancelled = app
                .state
                .running_transfer
                .borrow()
                .as_ref()
                .map(|transfer| transfer.cancel())
                .is_some();
            if cancelled {
                app.set_message("Cancelling the transfer…", false);
            } else {
                app.widgets.console.root.set_reveal_child(false);
                app.clear_message();
            }
        }
        _ => return glib::Propagation::Proceed,
    }

    glib::Propagation::Stop
}

/// Move focus back to whichever file view is showing.
pub fn focus_file_view(app: &App) {
    match app.state.view_mode.get() {
        ViewMode::Grid => {
            app.widgets.grid.grab_focus();
        }
        ViewMode::List => {
            app.widgets.list.grab_focus();
        }
    };
}

fn toggle_hidden(app: &App) {
    let value = !app.state.show_hidden.get();
    app.state.show_hidden.set(value);
    app.apply_filter();

    if let Some(check) = app.widgets.hidden_check.borrow().as_ref() {
        app.state.updating.set(true);
        check.set_active(value);
        app.state.updating.set(false);
    }
}

/// Launch an entry with the desktop's default application.
pub fn open_entry(app: &App, entry: &FileEntry) {
    if let Err(error) = ops::open(entry.path()) {
        app.show_error(&format!(
            "Could not open {}: {}",
            entry.display_name(),
            error.message().trim()
        ));
    }
}

pub fn open_terminal(app: &App, directory: &Path) {
    match ops::open_terminal(directory) {
        Ok(()) => app.set_message(
            &format!("Terminal opened in {}", directory.display()),
            false,
        ),
        Err(error) => app.show_error(&format!("Could not open a terminal: {error}")),
    }
}

fn new_folder(app: &App) {
    let parent = app.current_dir();
    dialogs::prompt(
        app,
        "New folder",
        "Create",
        "Untitled folder",
        None,
        move |app, name| {
            let app = Rc::clone(app);
            let parent = parent.clone();
            glib::spawn_future_local(async move {
                match ops::create_directory(&parent, OsStr::new(&name)).await {
                    Ok(path) => {
                        app.set_message(&format!("Created {name}"), false);
                        app.reload();
                        let _ = path;
                    }
                    Err(error) => app.show_error(&format!(
                        "Could not create {name}: {}",
                        error.message().trim()
                    )),
                }
            });
        },
    );
}

/// Rename the single selected entry.
pub fn rename_selection(app: &App) {
    let Some(entry) = app.single_selection() else {
        app.set_message("Select exactly one item to rename", false);
        return;
    };

    let path = entry.path().to_path_buf();
    let current = entry.display_name().to_owned();
    let stem_length = Path::new(&current)
        .file_stem()
        .map(|stem| stem.to_string_lossy().chars().count())
        .unwrap_or(current.chars().count());

    dialogs::prompt(
        app,
        "Rename",
        "Rename",
        &current,
        Some((0, i32::try_from(stem_length).unwrap_or(-1))),
        {
            let current = current.clone();
            move |app, name| {
                if name == current {
                    return;
                }
                let app = Rc::clone(app);
                let path = path.clone();
                glib::spawn_future_local(async move {
                    match ops::rename(&path, &name).await {
                        Ok(_) => {
                            app.set_message(&format!("Renamed to {name}"), false);
                            app.reload();
                        }
                        Err(error) => {
                            app.show_error(&format!("Could not rename: {}", error.message().trim()))
                        }
                    }
                });
            }
        },
    );
}

/// Stage the current selection for a later paste.
pub fn stage_transfer(app: &App, kind: TransferKind) {
    let sources: Vec<PathBuf> = app
        .selected_entries()
        .iter()
        .map(|entry| entry.path().to_path_buf())
        .collect();

    if sources.is_empty() {
        app.set_message("Select something first", false);
        return;
    }

    let count = sources.len();
    *app.state.clipboard.borrow_mut() = Some(Clipboard { kind, sources });
    app.set_message(
        &format!(
            "{} ready to paste — {}",
            crate::files::item_count_label(count),
            match kind {
                TransferKind::Copy => "copy",
                TransferKind::Move => "move",
            }
        ),
        false,
    );
}

/// Paste whatever Copy or Move staged into the current directory.
pub fn paste(app: &App) {
    let Some(clipboard) = app.state.clipboard.borrow().clone() else {
        app.set_message("Nothing has been copied yet", false);
        return;
    };

    if app.state.running_transfer.borrow().is_some() {
        app.show_error("Another transfer is still running");
        return;
    }

    let destination = app.current_dir();
    let cancel = CancelFlag::new();
    *app.state.running_transfer.borrow_mut() = Some(cancel.clone());

    let count = clipboard.sources.len();
    app.set_message(
        &format!(
            "{} {}… (Esc to cancel)",
            clipboard.kind.verb(),
            crate::files::item_count_label(count)
        ),
        false,
    );

    let app = Rc::clone(app);
    glib::spawn_future_local(async move {
        let kind = clipboard.kind;
        let report = ops::transfer(kind, clipboard.sources, destination, cancel).await;
        app.state.running_transfer.borrow_mut().take();

        if kind == TransferKind::Move && report.failures.is_empty() {
            app.state.clipboard.borrow_mut().take();
        }

        if report.failures.is_empty() {
            app.set_message(
                &format!(
                    "{} {}",
                    kind.past_tense(),
                    crate::files::item_count_label(report.succeeded)
                ),
                false,
            );
        } else {
            app.show_error(&report.failures.join("; "));
        }
        app.reload();
    });
}

/// Move the selection to the trash after confirmation.
pub fn trash_selection(app: &App) {
    let entries = app.selected_entries();
    if entries.is_empty() {
        app.set_message("Select something first", false);
        return;
    }

    let paths: Vec<PathBuf> = entries
        .iter()
        .map(|entry| entry.path().to_path_buf())
        .collect();

    let summary = if paths.len() == 1 {
        entries[0].display_name().to_owned()
    } else {
        crate::files::item_count_label(paths.len())
    };

    let app_for_action = Rc::clone(app);
    dialogs::confirm(
        app,
        "Move to Trash",
        &format!("{summary} will be moved to the trash."),
        "Move to Trash",
        move || {
            let app = Rc::clone(&app_for_action);
            let paths = paths.clone();
            glib::spawn_future_local(async move {
                let report = ops::trash(paths).await;
                if report.failures.is_empty() {
                    app.set_message(
                        &format!(
                            "Moved {} to the trash",
                            crate::files::item_count_label(report.succeeded)
                        ),
                        false,
                    );
                } else {
                    app.show_error(&report.failures.join("; "));
                }
                app.reload();
            });
        },
    );
}

/// Run an entry from the folder menu.
pub fn run_menu_action(app: &App, action: header::MenuAction) {
    match action {
        header::MenuAction::NewFolder => new_folder(app),
        header::MenuAction::Paste => paste(app),
        header::MenuAction::Terminal => {
            let directory = app.current_dir();
            open_terminal(app, &directory);
        }
        header::MenuAction::TogglePin => {
            let current = app.current_dir();
            app.toggle_pin(&current);
        }
        header::MenuAction::Refresh => app.reload(),
    }
}

/// Show the file context menu at `x`, `y` inside `origin`.
pub fn show_context_menu(app: &App, origin: &impl IsA<gtk::Widget>, x: f64, y: f64) {
    let menu = &app.widgets.context_menu;
    menu.set_child(Some(&context_menu_content(app)));

    let point = origin
        .as_ref()
        .compute_point(
            &app.widgets.content,
            &gtk::graphene::Point::new(x as f32, y as f32),
        )
        .unwrap_or_else(|| gtk::graphene::Point::new(x as f32, y as f32));

    menu.set_pointing_to(Some(&gdk::Rectangle::new(
        point.x() as i32,
        point.y() as i32,
        1,
        1,
    )));
    menu.popup();
}

fn context_menu_content(app: &App) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.set_width_request(206);

    let selected = app.selected_entries();
    let single = app.single_selection();

    let mut items: Vec<(gtk::Button, ContextAction)> = Vec::new();

    if let Some(entry) = single.as_ref() {
        if entry.is_directory() {
            items.push((
                header::menu_item(icons::ui(icons::names::OPEN_FOLDER), "Open"),
                ContextAction::Open,
            ));
            items.push((
                header::menu_item(icons::ui(icons::names::TERMINAL), "Open Terminal Here"),
                ContextAction::TerminalHere,
            ));
            items.push((
                header::menu_item(
                    icons::ui(icons::names::PIN),
                    if app.is_pinned(entry.path()) {
                        "Unpin"
                    } else {
                        "Pin"
                    },
                ),
                ContextAction::Pin,
            ));
        } else {
            items.push((
                header::menu_item(icons::ui(icons::names::OPEN), "Open"),
                ContextAction::Open,
            ));
        }
        items.push((
            header::menu_item(icons::ui(icons::names::RENAME), "Rename"),
            ContextAction::Rename,
        ));
        items.push((
            header::menu_item(icons::ui(icons::names::COPY_PATH), "Copy Path"),
            ContextAction::CopyPath,
        ));
    }

    if !selected.is_empty() {
        items.push((
            header::menu_item(icons::ui(icons::names::COPY), "Copy"),
            ContextAction::Copy,
        ));
        items.push((
            header::menu_item(icons::ui(icons::names::CUT), "Move"),
            ContextAction::Cut,
        ));
        items.push((
            header::menu_item(icons::ui(icons::names::TRASH), "Move to Trash"),
            ContextAction::Trash,
        ));
    }

    items.push((
        header::menu_item(icons::ui(icons::names::NEW_FOLDER), "New Folder"),
        ContextAction::NewFolder,
    ));

    let paste_item = header::menu_item(icons::ui(icons::names::PASTE), "Paste");
    paste_item.set_sensitive(app.state.clipboard.borrow().is_some());
    items.push((paste_item, ContextAction::Paste));

    items.push((
        header::menu_item(icons::ui(icons::names::REFRESH), "Refresh"),
        ContextAction::Refresh,
    ));

    for (button, action) in items {
        let app = Rc::clone(app);
        button.connect_clicked(move |_| {
            app.widgets.context_menu.popdown();
            run_context_action(&app, action);
        });
        content.append(&button);
    }

    content
}

#[derive(Debug, Clone, Copy)]
enum ContextAction {
    Open,
    TerminalHere,
    Pin,
    Rename,
    CopyPath,
    Copy,
    Cut,
    Trash,
    NewFolder,
    Paste,
    Refresh,
}

fn run_context_action(app: &App, action: ContextAction) {
    match action {
        ContextAction::Open => {
            if let Some(entry) = app.single_selection() {
                if entry.is_directory() {
                    app.navigate(entry.path());
                } else {
                    open_entry(app, &entry);
                }
            }
        }
        ContextAction::TerminalHere => {
            if let Some(entry) = app.single_selection() {
                let path = entry.path().to_path_buf();
                open_terminal(app, &path);
            }
        }
        ContextAction::Pin => {
            if let Some(entry) = app.single_selection() {
                let path = entry.path().to_path_buf();
                app.toggle_pin(&path);
            }
        }
        ContextAction::Rename => rename_selection(app),
        ContextAction::CopyPath => {
            if let Some(entry) = app.single_selection() {
                app.widgets
                    .window
                    .clipboard()
                    .set_text(&entry.path().to_string_lossy());
                app.set_message("Path copied to the clipboard", false);
            }
        }
        ContextAction::Copy => stage_transfer(app, TransferKind::Copy),
        ContextAction::Cut => stage_transfer(app, TransferKind::Move),
        ContextAction::Trash => trash_selection(app),
        ContextAction::NewFolder => new_folder(app),
        ContextAction::Paste => paste(app),
        ContextAction::Refresh => app.reload(),
    }
}
