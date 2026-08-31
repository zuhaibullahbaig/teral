//! Window assembly, application actions and keyboard behaviour.

use super::{
    App, AppInner, State, Tab, ViewMode, details, dialogs, fileview, header, help, search,
    settings, sidebar, statusbar, tabs,
};
use crate::config::{Config, ViewPreference};
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
use std::sync::mpsc;
use std::time::Duration;

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

    pub tabs: tabs::Tabs,

    pub folder_title: gtk::Label,
    pub folder_subtitle: gtk::Label,
    pub new_folder: gtk::Button,

    pub content: gtk::Box,
    pub file_paned: gtk::Paned,
    pub search: search::Search,
    pub view_stack: gtk::Stack,
    pub grid: gtk::GridView,
    pub list: gtk::ColumnView,
    pub grid_scroller: gtk::ScrolledWindow,
    pub list_scroller: gtk::ScrolledWindow,
    pub context_menu: gtk::Popover,

    pub places_box: gtk::Box,
    pub devices_box: gtk::Box,
    pub pinned_box: gtk::Box,
    pub pin_drop: gtk::Label,
    pub tags: gtk::Box,
    pub add_tag: gtk::Button,

    pub details: details::Details,

    pub console: statusbar::Console,
    pub command_entry: gtk::Entry,
    pub status_selection: gtk::Label,
    pub status_size: gtk::Label,
    pub status_message: gtk::Label,
    pub zoom: gtk::Scale,
    pub zoom_value: gtk::Label,
    pub zoom_out: gtk::Button,
    pub zoom_in: gtk::Button,
    pub settings: gtk::Button,
    pub details_toggle: gtk::ToggleButton,
    pub settings_window: RefCell<Option<gtk::Window>>,
}

/// Build the main window and start browsing the user's home directory.
pub fn build_window(
    application: &gtk::Application,
    config: Config,
    theme: ThemeConfig,
) -> gtk::ApplicationWindow {
    build_window_at(application, config, theme, None)
}

/// Build a window, optionally starting somewhere other than the home directory.
pub fn build_window_at(
    application: &gtk::Application,
    config: Config,
    theme: ThemeConfig,
    start_at: Option<PathBuf>,
) -> gtk::ApplicationWindow {
    let store = gio::ListStore::new::<FileEntry>();
    let selection = gtk::MultiSelection::new(Some(store.clone()));

    let search_field = search::build();
    let head = header::build(&search_field.root);
    let side = sidebar::build(theme.sidebar_width());
    let detail = details::build(theme.details_width());
    let status = statusbar::build(
        theme.grid_icon_size(),
        theme.sidebar_width(),
        theme.details_width(),
    );
    let console = statusbar::build_console();
    let tab_bar = tabs::build();

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

    let content_header = gtk::Box::new(gtk::Orientation::Horizontal, theme.spacing());
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

    // The console shares the file column through a paned split, so its top edge can be
    // dragged to make an interactive command taller or shorter.
    console.root.set_visible(false);

    let file_paned = gtk::Paned::new(gtk::Orientation::Vertical);
    file_paned.add_css_class("teral-file-paned");
    file_paned.set_vexpand(true);
    file_paned.set_resize_start_child(true);
    file_paned.set_resize_end_child(true);
    file_paned.set_shrink_start_child(false);
    file_paned.set_shrink_end_child(false);
    file_paned.set_start_child(Some(&view_stack));
    file_paned.set_end_child(Some(&console.root));

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("teral-content");
    content.set_hexpand(true);
    content.append(&tab_bar.root);
    content.append(&content_header);
    content.append(&file_paned);

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
        tabs: tab_bar,
        folder_title,
        folder_subtitle,
        new_folder,
        content,
        file_paned,
        search: search_field,
        view_stack,
        grid,
        list,
        grid_scroller,
        list_scroller,
        context_menu,
        places_box: side.places,
        devices_box: side.devices,
        pinned_box: side.pinned,
        pin_drop: side.pin_drop,
        tags: side.tags,
        add_tag: side.add_tag,
        details: detail,
        console,
        command_entry: status.command_entry,
        status_selection: status.selection,
        status_size: status.size,
        status_message: status.message,
        zoom: status.zoom,
        zoom_value: status.zoom_value,
        zoom_out: status.zoom_out,
        zoom_in: status.zoom_in,
        settings: status.settings,
        details_toggle: status.details_toggle,
        settings_window: RefCell::new(None),
    };

    let start = start_at.unwrap_or_else(|| home_dir().unwrap_or_else(|| PathBuf::from("/")));

    let state = State {
        current: RefCell::new(PathBuf::from("/")),
        back: RefCell::new(Vec::new()),
        forward: RefCell::new(Vec::new()),
        all: RefCell::new(Vec::new()),
        store,
        selection,
        sorting: Cell::new(Sorting {
            key: config.sort,
            descending: config.descending,
            folders_first: config.folders_first,
        }),
        show_hidden: Cell::new(config.show_hidden),
        query: RefCell::new(String::new()),
        generation: Cell::new(0),
        pinned: RefCell::new(places::load_pinned()),
        clipboard: RefCell::new(None),
        icon_size: Cell::new(theme.grid_icon_size()),
        view_mode: Cell::new(match config.view {
            ViewPreference::Grid => ViewMode::Grid,
            ViewPreference::List => ViewMode::List,
        }),
        running_command: Cell::new(false),
        running_transfer: RefCell::new(None),
        updating: Cell::new(false),
        tabs: RefCell::new(vec![Tab::new(start.clone())]),
        active_tab: Cell::new(0),
        directory_monitor: RefCell::new(None),
        refresh_queued: Cell::new(false),
        config_monitors: RefCell::new(Vec::new()),
        console_height: Cell::new(statusbar::CONSOLE_HEIGHT),
        tag_view: RefCell::new(None),
        icon_size_save: Cell::new(None),
    };

    let app: App = Rc::new(AppInner {
        state,
        widgets,
        config: RefCell::new(config),
        theme: RefCell::new(theme),
    });

    header::connect(&app);
    search::connect(&app);
    sidebar::connect(&app);
    fileview::connect(&app);
    details::connect(&app);
    statusbar::connect(&app);
    tabs::connect(&app);
    connect_window(&app);
    watch_configuration(&app);

    if app.state.view_mode.get() == ViewMode::List {
        app.widgets.list_toggle.set_active(true);
    }
    app.load(&start, None);

    window
}

/// Re-apply the theme when the configuration file or the active Omarchy theme changes,
/// so switching desktop themes restyles a running Teral.
pub fn watch_configuration(app: &App) {
    // Rebuilt from scratch each time: switching Omarchy themes changes which directory
    // holds the active theme, so the old monitors would be watching the theme that was
    // in use a moment ago.
    app.state.config_monitors.borrow_mut().clear();

    let mut watched = vec![crate::config::config_path()];
    watched.extend(crate::theme::omarchy_watch_paths());

    for path in watched {
        let file = gio::File::for_path(&path);
        let monitor = if path.is_dir() {
            file.monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
        } else {
            file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
        };

        let Ok(monitor) = monitor else { continue };
        monitor.connect_changed({
            let app = Rc::clone(app);
            move |_, _, _, _| {
                // The Settings window owns the file while it is open; do not fight it.
                if app.widgets.settings_window.borrow().is_some() {
                    return;
                }
                // The handler must not still be running when its own monitor is
                // dropped by the rebuild below, so both happen on the next idle.
                let app = Rc::clone(&app);
                super::defer(move || {
                    app.reload_theme();
                    watch_configuration(&app);
                });
            }
        });
        app.state.config_monitors.borrow_mut().push(monitor);
    }
}

fn connect_window(app: &App) {
    app.widgets.new_folder.connect_clicked({
        let app = Rc::clone(app);
        move |_| new_folder(&app)
    });

    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed({
        let app = Rc::clone(app);
        move |_, key, _, modifiers| on_key(&app, key, modifiers)
    });
    app.widgets.window.add_controller(keys);

    // The console runs a real terminal, which swallows almost every key so that
    // interactive programs work. One shortcut therefore has to be caught before the
    // terminal sees it, otherwise there is no keyboard way back out.
    let escape_hatch = gtk::EventControllerKey::new();
    escape_hatch.set_propagation_phase(gtk::PropagationPhase::Capture);
    escape_hatch.connect_key_pressed({
        let app = Rc::clone(app);
        move |_, key, _, modifiers| {
            if key == gdk::Key::grave && modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
                toggle_console(&app);
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    });
    app.widgets.window.add_controller(escape_hatch);

    // Clipboard ownership can change in another application. Clear only Teral's visual
    // cut marker; Paste reads the new desktop clipboard directly when invoked.
    app.widgets.window.clipboard().connect_changed({
        let app = Rc::clone(app);
        move |clipboard| {
            if !clipboard.is_local() {
                app.state.clipboard.borrow_mut().take();
                fileview::refresh_cut_state(&app);
                app.update_details();
            }
        }
    });

    // Clicking empty space clears the selection, the way every file manager behaves.
    let clear = gtk::GestureClick::new();
    clear.set_button(gdk::BUTTON_PRIMARY);
    clear.set_propagation_phase(gtk::PropagationPhase::Bubble);
    clear.connect_released({
        let app = Rc::clone(app);
        move |gesture, _, x, y| {
            let Some(view) = gesture.widget() else {
                return;
            };
            if !hit_an_item(&view, x, y) {
                app.state.selection.unselect_all();
            }
        }
    });
    app.widgets.view_stack.add_controller(clear);

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

    // Dropping on empty space puts files into the folder being browsed.
    let drop = gtk::DropTarget::new(
        gdk::FileList::static_type(),
        gdk::DragAction::COPY | gdk::DragAction::MOVE | gdk::DragAction::LINK,
    );
    drop.connect_enter({
        let app = Rc::clone(app);
        move |target, _, _| {
            let action = drop_action(target);
            show_drop_action(&app, action);
            action
        }
    });
    drop.connect_drop({
        let app = Rc::clone(app);
        move |target, value, _, _| {
            let Ok(files) = value.get::<gdk::FileList>() else {
                return false;
            };
            let destination = app.current_dir();
            let action = drop_action(target);
            drop_files(&app, &files, &destination, action)
        }
    });
    app.widgets.view_stack.add_controller(drop);
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
        gdk::Key::f | gdk::Key::F if control => search::open(app),
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
        gdk::Key::t | gdk::Key::T if control => {
            let current = app.current_dir();
            app.open_tab(current);
        }
        gdk::Key::w | gdk::Key::W if control => app.close_tab(app.state.active_tab.get()),
        gdk::Key::Tab | gdk::Key::ISO_Left_Tab if control => app.cycle_tab(!shift),
        gdk::Key::d | gdk::Key::D if control => duplicate_selection(app),
        gdk::Key::comma if control => settings::present(app),
        gdk::Key::F1 => help::present_shortcuts(app),
        gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add if control => app.step_zoom(8),
        gdk::Key::minus | gdk::Key::KP_Subtract if control => app.step_zoom(-8),
        gdk::Key::i | gdk::Key::I if control => {
            let toggle = &app.widgets.details_toggle;
            toggle.set_active(!toggle.is_active());
        }
        gdk::Key::r | gdk::Key::R if control => app.reload(),
        gdk::Key::F5 => app.reload(),
        gdk::Key::F2 => rename_selection(app),
        gdk::Key::Delete if shift => delete_permanently(app),
        gdk::Key::Delete => trash_selection(app),
        gdk::Key::_0 if control => app.reset_zoom(),
        gdk::Key::Escape if search::is_open(app) => search::close(app),
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
            } else if statusbar::console_visible(app) {
                statusbar::hide_console(app);
            } else {
                app.clear_message();
            }
        }
        // Typing a printable character with nothing else focused starts a search, the
        // way type-ahead has always worked in file managers.
        _ => {
            if control || alt {
                return glib::Propagation::Proceed;
            }
            let Some(character) = key.to_unicode().filter(|c| !c.is_control()) else {
                return glib::Propagation::Proceed;
            };
            // Once the entry has focus it handles its own typing.
            if app.widgets.search.entry.has_focus() {
                return glib::Propagation::Proceed;
            }
            search::type_ahead(app, character);
        }
    }

    glib::Propagation::Stop
}

/// Show or hide the Quick Command console, moving focus with it.
pub fn toggle_console(app: &App) {
    if statusbar::console_visible(app) {
        statusbar::hide_console(app);
        focus_file_view(app);
    } else {
        statusbar::show_console(app);
        app.widgets.console.terminal.grab_focus();
    }
}

/// Whether the point `x`, `y` inside `view` landed on a file rather than on the
/// background. Items carry a marker class, and GTK's own row widgets are recognised by
/// type, so a click on any part of a list row still counts as hitting that row.
fn hit_an_item(view: &gtk::Widget, x: f64, y: f64) -> bool {
    let mut widget = view.pick(x, y, gtk::PickFlags::DEFAULT);

    while let Some(current) = widget {
        if current.has_css_class("teral-item") {
            return true;
        }
        let type_name = current.type_().name().to_owned();
        if type_name.contains("ListItemWidget") || type_name.contains("ColumnViewRowWidget") {
            return true;
        }
        if &current == view {
            return false;
        }
        widget = current.parent();
    }

    false
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

    let hidden_check = app.widgets.hidden_check.borrow().clone();
    if let Some(check) = hidden_check {
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
                        Ok(renamed) => {
                            crate::tags::edit(|tags| tags.relocate(&path, &renamed));
                            sidebar::rebuild_tags(&app);
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
    let staged = Clipboard { kind, sources };
    if let Err(error) = ops::write_clipboard(&app.widgets.window.clipboard(), &staged) {
        app.show_error(&format!("Could not update the desktop clipboard: {error}"));
        return;
    }
    *app.state.clipboard.borrow_mut() = Some(staged);
    // Cut entries are dimmed until they are pasted, the way desktops normally show it.
    fileview::refresh_cut_state(app);
    app.set_message(
        &format!(
            "{} ready to paste — {}",
            crate::files::item_count_label(count),
            match kind {
                TransferKind::Copy => "copy",
                TransferKind::Move => "cut",
                TransferKind::Link => "link",
            }
        ),
        false,
    );
}

/// Paste whatever Copy or Move staged into the current directory.
pub fn paste(app: &App) {
    if app.state.running_transfer.borrow().is_some() {
        app.show_error("Another transfer is still running");
        return;
    }
    if !ops::clipboard_has_files(&app.widgets.window.clipboard()) {
        app.set_message("The clipboard does not contain files", false);
        return;
    }

    let system_clipboard = app.widgets.window.clipboard();
    let destination = app.current_dir();
    let app = Rc::clone(app);
    glib::spawn_future_local(async move {
        match ops::read_clipboard(&system_clipboard).await {
            Ok(clipboard) => {
                prepare_transfer(&app, clipboard.kind, clipboard.sources, destination, true)
            }
            Err(error) => app.show_error(&format!("Could not read the desktop clipboard: {error}")),
        }
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
                crate::tags::edit(|tags| {
                    for path in &report.completed_paths {
                        tags.forget(path);
                    }
                });
                sidebar::rebuild_tags(&app);
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

/// Handle files dropped onto a folder or onto the current folder's background.
///
/// The negotiated desktop drag action wins. If a backend supplies no action, Teral
/// uses the non-destructive Copy action; it never guesses that a drop should move data.
pub fn drop_files(
    app: &App,
    files: &gdk::FileList,
    destination: &Path,
    requested_action: gdk::DragAction,
) -> bool {
    let sources: Vec<PathBuf> = files
        .files()
        .iter()
        .filter_map(gio::File::path)
        .filter(|path| path.parent() != Some(destination))
        .collect();

    if sources.is_empty() {
        return false;
    }

    let kind = if requested_action.contains(gdk::DragAction::LINK) {
        TransferKind::Link
    } else if requested_action.contains(gdk::DragAction::MOVE) {
        TransferKind::Move
    } else {
        TransferKind::Copy
    };

    prepare_transfer(app, kind, sources, destination.to_path_buf(), false);
    true
}

/// Tell the user what a pending drop will do before they release the pointer.
pub fn show_drop_action(app: &App, action: gdk::DragAction) {
    let label = if action.contains(gdk::DragAction::LINK) {
        "link"
    } else if action.contains(gdk::DragAction::MOVE) {
        "move"
    } else {
        "copy"
    };
    app.set_message(&format!("Drop to {label}"), false);
}

/// Resolve the action chosen for the current drop. `selected_action` belongs to the
/// source-side `Drag`, not the destination-side `Drop`; external drops may not expose a
/// `Drag`, so a unique offered action is used and ambiguous offers safely prefer Copy.
pub fn drop_action(target: &gtk::DropTarget) -> gdk::DragAction {
    let Some(drop) = target.current_drop() else {
        return gdk::DragAction::COPY;
    };

    if let Some(action) = drop
        .drag()
        .map(|drag| drag.selected_action())
        .filter(|action| !action.is_empty())
    {
        return action;
    }

    let offered = drop.actions();
    if offered.is_unique() {
        offered
    } else if offered.contains(gdk::DragAction::COPY) {
        gdk::DragAction::COPY
    } else if offered.contains(gdk::DragAction::LINK) {
        gdk::DragAction::LINK
    } else if offered.contains(gdk::DragAction::MOVE) {
        gdk::DragAction::MOVE
    } else {
        gdk::DragAction::COPY
    }
}

/// Check conflicts away from GTK, then either start immediately or ask once for the
/// policy that applies to the whole batch.
fn prepare_transfer(
    app: &App,
    kind: TransferKind,
    sources: Vec<PathBuf>,
    destination: PathBuf,
    from_clipboard: bool,
) {
    if app.state.running_transfer.borrow().is_some() {
        app.show_error("Another transfer is still running");
        return;
    }

    app.set_message("Checking destination…", false);
    let app = Rc::clone(app);
    glib::spawn_future_local(async move {
        match ops::conflicts(sources.clone(), destination.clone()).await {
            Ok(conflicts) if conflicts.is_empty() => start_transfer(
                &app,
                kind,
                sources,
                destination,
                ops::ConflictPolicy::RenameIncoming,
                from_clipboard,
            ),
            Ok(conflicts) => {
                let app_for_choice = Rc::clone(&app);
                dialogs::resolve_transfer_conflicts(&app, conflicts.len(), move |policy| {
                    if policy == ops::ConflictPolicy::Cancel {
                        app_for_choice.set_message("Transfer cancelled", false);
                        return;
                    }
                    start_transfer(
                        &app_for_choice,
                        kind,
                        sources,
                        destination,
                        policy,
                        from_clipboard,
                    );
                });
            }
            Err(error) => app.show_error(&format!("Could not inspect the destination: {error}")),
        }
    });
}

/// Copy or move `sources` through the authoritative job runner.
fn start_transfer(
    app: &App,
    kind: TransferKind,
    sources: Vec<PathBuf>,
    destination: PathBuf,
    policy: ops::ConflictPolicy,
    from_clipboard: bool,
) {
    if app.state.running_transfer.borrow().is_some() {
        app.show_error("Another transfer is still running");
        return;
    }

    let cancel = CancelFlag::new();
    *app.state.running_transfer.borrow_mut() = Some(cancel.clone());

    let (progress, updates) = mpsc::sync_channel(1);
    watch_transfer_progress(app, kind, updates);

    let count = sources.len();
    app.set_message(
        &format!(
            "{} {}… (Esc to cancel)",
            kind.verb(),
            crate::files::item_count_label(count)
        ),
        false,
    );

    let app = Rc::clone(app);
    glib::spawn_future_local(async move {
        let report = ops::transfer(kind, sources, destination, policy, cancel, progress).await;
        app.state.running_transfer.borrow_mut().take();

        if kind == TransferKind::Move {
            crate::tags::edit(|tags| {
                for (from, to) in report.completed_moves() {
                    tags.relocate(from, to);
                }
            });
            sidebar::rebuild_tags(&app);
        }

        if from_clipboard && kind == TransferKind::Move {
            update_cut_clipboard(&app, &report);
        }

        if report.is_complete() {
            app.set_message(
                &format!(
                    "{} {}",
                    kind.past_tense(),
                    crate::files::item_count_label(report.succeeded())
                ),
                false,
            );
        } else if report.cancelled
            && report
                .problems()
                .iter()
                .all(|problem| problem.ends_with("cancelled"))
        {
            app.set_message(
                &format!(
                    "Transfer cancelled after {}",
                    crate::files::item_count_label(report.succeeded())
                ),
                false,
            );
        } else if report.problems().is_empty() {
            app.set_message(
                &format!(
                    "{} {}; skipped {} existing",
                    kind.past_tense(),
                    crate::files::item_count_label(report.succeeded()),
                    crate::files::item_count_label(report.skipped())
                ),
                false,
            );
        } else {
            let problems = report.problems();
            app.show_error(&format!(
                "{} completed; {}",
                crate::files::item_count_label(report.succeeded()),
                problems.join("; ")
            ));
        }
        app.reload();
    });
}

fn watch_transfer_progress(
    app: &App,
    kind: TransferKind,
    updates: mpsc::Receiver<ops::JobProgress>,
) {
    let app = Rc::clone(app);
    glib::timeout_add_local(Duration::from_millis(120), move || {
        let mut latest = None;
        let mut disconnected = false;
        loop {
            match updates.try_recv() {
                Ok(update) => latest = Some(update),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }

        if let Some(update) = latest {
            let bytes = if update.total_bytes > 0 {
                format!(
                    " · {} / {}",
                    crate::files::format_size(update.completed_bytes),
                    crate::files::format_size(update.total_bytes)
                )
            } else {
                String::new()
            };
            app.set_message(
                &format!(
                    "{} {} of {}{bytes} (Esc to cancel)",
                    kind.verb(),
                    update.processed_items,
                    update.total_items
                ),
                false,
            );
        }

        if disconnected {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

fn update_cut_clipboard(app: &App, report: &ops::JobReport) {
    let remaining = report.remaining_sources();
    if remaining.is_empty() {
        app.state.clipboard.borrow_mut().take();
        let _ = app
            .widgets
            .window
            .clipboard()
            .set_content(None::<&gdk::ContentProvider>);
    } else {
        let staged = Clipboard {
            kind: TransferKind::Move,
            sources: remaining,
        };
        if let Err(error) = ops::write_clipboard(&app.widgets.window.clipboard(), &staged) {
            app.show_error(&format!(
                "Could not preserve the remaining cut files: {error}"
            ));
        }
        *app.state.clipboard.borrow_mut() = Some(staged);
    }
    fileview::refresh_cut_state(app);
}

/// Open whatever is selected, following folders and launching files.
pub fn activate_selection(app: &App) {
    let Some(entry) = app.selected_entries().into_iter().next() else {
        return;
    };
    if entry.is_directory() {
        app.navigate(entry.path());
    } else {
        open_entry(app, &entry);
    }
}

/// Delete the selection without going through the trash.
pub fn delete_permanently(app: &App) {
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
        "Delete Permanently",
        &format!("{summary} will be deleted permanently. This cannot be undone."),
        "Delete Permanently",
        move || {
            let app = Rc::clone(&app_for_action);
            let paths = paths.clone();
            glib::spawn_future_local(async move {
                let report = ops::delete_permanently(paths).await;
                crate::tags::edit(|tags| {
                    for path in &report.completed_paths {
                        tags.forget(path);
                    }
                });
                sidebar::rebuild_tags(&app);
                if report.failures.is_empty() {
                    app.set_message(
                        &format!(
                            "Deleted {}",
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

/// Copy the selection beside itself.
pub fn duplicate_selection(app: &App) {
    let paths: Vec<PathBuf> = app
        .selected_entries()
        .iter()
        .map(|entry| entry.path().to_path_buf())
        .collect();

    if paths.is_empty() {
        app.set_message("Select something first", false);
        return;
    }

    if app.state.running_transfer.borrow().is_some() {
        app.show_error("Another transfer is still running");
        return;
    }

    let cancel = CancelFlag::new();
    *app.state.running_transfer.borrow_mut() = Some(cancel.clone());
    let (progress, updates) = mpsc::sync_channel(1);
    watch_transfer_progress(app, TransferKind::Copy, updates);
    app.set_message("Duplicating selection… (Esc to cancel)", false);

    let app = Rc::clone(app);
    glib::spawn_future_local(async move {
        let report = ops::duplicate(paths, cancel, progress).await;
        app.state.running_transfer.borrow_mut().take();
        if report.is_complete() {
            app.set_message(
                &format!(
                    "Duplicated {}",
                    crate::files::item_count_label(report.succeeded())
                ),
                false,
            );
        } else if report.cancelled {
            app.set_message(
                &format!(
                    "Duplication cancelled after {}",
                    crate::files::item_count_label(report.succeeded())
                ),
                false,
            );
        } else {
            app.show_error(&report.problems().join("; "));
        }
        app.reload();
    });
}

/// Put trashed entries back where they came from.
pub fn restore_selection(app: &App) {
    let paths: Vec<PathBuf> = app
        .selected_entries()
        .iter()
        .map(|entry| entry.path().to_path_buf())
        .collect();

    if paths.is_empty() {
        app.set_message("Select something to restore", false);
        return;
    }

    let app = Rc::clone(app);
    glib::spawn_future_local(async move {
        let report = ops::restore_from_trash(paths).await;
        if report.failures.is_empty() {
            app.set_message(
                &format!(
                    "Restored {}",
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

/// Permanently delete everything in the trash, after confirmation.
pub fn empty_trash(app: &App) {
    let app_for_action = Rc::clone(app);
    dialogs::confirm(
        app,
        "Empty Trash",
        "Everything in the trash will be deleted permanently. This cannot be undone.",
        "Delete Permanently",
        move || {
            let app = Rc::clone(&app_for_action);
            glib::spawn_future_local(async move {
                let report = ops::empty_trash().await;
                if report.failures.is_empty() {
                    app.set_message("Trash emptied", false);
                } else {
                    app.show_error(&report.failures.join("; "));
                }
                app.reload();
            });
        },
    );
}

/// A popover of tag checkboxes for a set of paths.
pub fn tag_popover(app: &App, paths: &[PathBuf]) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.add_css_class("teral-popover");

    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.set_width_request(190);

    let store = crate::tags::current();
    let paths: Vec<PathBuf> = paths.to_vec();

    for tag in &store.tags {
        let check = gtk::CheckButton::new();
        check.add_css_class("teral-menu-check");
        check.set_active(paths.iter().all(|path| store.is_tagged(&tag.name, path)));

        let dot = gtk::Label::new(Some("●"));
        dot.add_css_class("teral-tag-dot");
        super::apply_color(&dot, &tag.color);

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 7);
        row.append(&dot);
        row.append(&gtk::Label::new(Some(&tag.name)));
        check.set_child(Some(&row));

        check.connect_toggled({
            let app = Rc::clone(app);
            let name = tag.name.clone();
            let paths = paths.clone();
            move |check| {
                let tagged = check.is_active();
                crate::tags::edit(|tags| tags.set_tagged(&name, &paths, tagged));

                let app = Rc::clone(&app);
                super::defer(move || {
                    sidebar::rebuild_tags(&app);
                    app.update_details();
                    let in_tag_view = app.state.tag_view.borrow().is_some();
                    if in_tag_view {
                        app.reload();
                    }
                });
            }
        });

        content.append(&check);
    }

    let new_tag = header::menu_item(icons::ui(icons::names::ADD), "New tag…");
    new_tag.connect_clicked({
        let app = Rc::clone(app);
        let popover = popover.clone();
        move |_| {
            popover.popdown();
            dialogs::edit_tag(&app, None);
        }
    });
    content.append(&new_tag);

    popover.set_child(Some(&content));
    popover
}

/// Open `path` in a new tab.
pub fn open_in_new_tab(app: &App, path: &Path) {
    app.open_tab(path.to_path_buf());
}

/// Open `path` in a second Teral window.
pub fn open_in_new_window(app: &App, path: &Path) {
    let Some(application) = app.widgets.window.application() else {
        app.show_error("Teral is not attached to an application");
        return;
    };

    let config = app.config.borrow().clone();
    let theme = ThemeConfig::resolve(&config);
    let window = build_window_at(&application, config, theme, Some(path.to_path_buf()));
    window.present();
}

/// Unpack an archive, either into this folder or into a folder named after it.
fn extract_archive(app: &App, entry: &FileEntry, into_subfolder: bool) {
    let archive = entry.path().to_path_buf();
    let Some(parent) = archive.parent().map(Path::to_path_buf) else {
        app.show_error("That archive has no parent folder");
        return;
    };

    let destination = if into_subfolder {
        parent.join(ops::archive_stem(&archive))
    } else {
        parent
    };

    let content_type = entry
        .data()
        .content_type
        .clone()
        .unwrap_or_else(|| "application/zip".to_owned());

    app.set_message(&format!("Extracting {}…", entry.display_name()), false);

    let app = Rc::clone(app);
    glib::spawn_future_local(async move {
        match ops::extract(archive, destination, content_type).await {
            Ok(destination) => app.set_message(
                &format!("Extracted into {}", places::display_label(&destination)),
                false,
            ),
            Err(error) => app.show_error(&format!("Could not extract: {error}")),
        }
        app.reload();
    });
}

/// Pack the selection into a zip archive beside it.
pub fn compress_selection(app: &App) {
    let paths: Vec<PathBuf> = app
        .selected_entries()
        .iter()
        .map(|entry| entry.path().to_path_buf())
        .collect();

    if paths.is_empty() {
        return;
    }

    let directory = app.current_dir();
    app.set_message(
        &format!(
            "Compressing {}…",
            crate::files::item_count_label(paths.len())
        ),
        false,
    );

    let app = Rc::clone(app);
    glib::spawn_future_local(async move {
        match ops::compress(directory, paths).await {
            Ok(archive) => app.set_message(
                &format!("Created {}", places::display_label(&archive)),
                false,
            ),
            Err(error) => app.show_error(&format!("Could not compress: {error}")),
        }
        app.reload();
    });
}

/// Add or remove the execute bit on the selection.
fn set_executable(app: &App, executable: bool) {
    let paths: Vec<PathBuf> = app
        .selected_entries()
        .iter()
        .map(|entry| entry.path().to_path_buf())
        .collect();

    if paths.is_empty() {
        return;
    }

    let app = Rc::clone(app);
    glib::spawn_future_local(async move {
        let report = ops::set_executable(paths, executable).await;
        if report.failures.is_empty() {
            app.set_message(
                if executable {
                    "Marked as executable"
                } else {
                    "No longer executable"
                },
                false,
            );
        } else {
            app.show_error(&report.failures.join("; "));
        }
        app.reload();
    });
}

/// A group of entries the current listing can select in one go.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SelectionGroup {
    /// Every folder — folders have no extension, so they need a group of their own.
    Folders,
    /// Every file carrying this extension, lower-cased.
    Extension(String),
    /// Every file with no extension at all.
    NoExtension,
}

impl SelectionGroup {
    fn label(&self) -> String {
        match self {
            Self::Folders => "Folders".to_owned(),
            Self::Extension(extension) => format!(".{extension}"),
            Self::NoExtension => "No extension".to_owned(),
        }
    }

    fn matches(&self, entry: &FileEntry) -> bool {
        match self {
            Self::Folders => entry.is_directory(),
            Self::Extension(extension) => {
                !entry.is_directory()
                    && entry
                        .path()
                        .extension()
                        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
            }
            Self::NoExtension => !entry.is_directory() && entry.path().extension().is_none(),
        }
    }
}

/// Select every entry in a group, replacing the current selection.
fn select_group(app: &App, group: &SelectionGroup) {
    let selection = &app.state.selection;
    selection.unselect_all();

    for index in 0..selection.n_items() {
        let Some(entry) = selection.item(index).and_downcast::<FileEntry>() else {
            continue;
        };
        if group.matches(&entry) {
            selection.select_item(index, false);
        }
    }
}

/// The groups worth offering for the current listing, most numerous first.
fn selection_groups(app: &App) -> Vec<(SelectionGroup, usize)> {
    let mut folders = 0usize;
    let mut bare = 0usize;
    let mut extensions: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for index in 0..app.state.selection.n_items() {
        let Some(entry) = app.state.selection.item(index).and_downcast::<FileEntry>() else {
            continue;
        };
        if entry.is_directory() {
            folders += 1;
            continue;
        }
        match entry.path().extension().and_then(|value| value.to_str()) {
            Some(extension) => *extensions.entry(extension.to_lowercase()).or_default() += 1,
            None => bare += 1,
        }
    }

    let mut groups: Vec<(SelectionGroup, usize)> = extensions
        .into_iter()
        .map(|(extension, count)| (SelectionGroup::Extension(extension), count))
        .collect();
    groups.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| left.0.label().cmp(&right.0.label()))
    });
    groups.truncate(10);

    if bare > 0 {
        groups.push((SelectionGroup::NoExtension, bare));
    }
    // Folders lead: they are what people reach for first, and they are the one group
    // an extension list can never describe.
    if folders > 0 {
        groups.insert(0, (SelectionGroup::Folders, folders));
    }
    groups
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
        header::MenuAction::EmptyTrash => empty_trash(app),
        header::MenuAction::Shortcuts => help::present_shortcuts(app),
        header::MenuAction::About => help::present_about(app),
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

/// The menu for a selected file or folder: things you can do to it, nothing else.
fn context_menu_content(app: &App) -> gtk::Box {
    let selected = app.selected_entries();
    if selected.is_empty() {
        return background_menu_content(app);
    }

    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.set_width_request(214);

    let single = app.single_selection();
    let in_trash = ops::is_in_trash(&app.current_dir());
    let mut items: Vec<(gtk::Button, ContextAction)> = Vec::new();

    // Opening comes first, destructive actions come last: the order every desktop uses.
    if let Some(entry) = single.as_ref() {
        if entry.is_directory() {
            items.push((
                header::menu_item(icons::ui(icons::names::OPEN_FOLDER), "Open"),
                ContextAction::Open,
            ));
            items.push((
                header::menu_item(icons::ui(icons::names::ADD), "Open in New Tab"),
                ContextAction::OpenInNewTab,
            ));
            items.push((
                header::menu_item(icons::ui(icons::names::WINDOW), "Open in New Window"),
                ContextAction::OpenInNewWindow,
            ));
        } else {
            items.push((
                header::menu_item(icons::ui(icons::names::OPEN), "Open"),
                ContextAction::Open,
            ));
        }
    }

    for (button, action) in std::mem::take(&mut items) {
        wire_menu_item(app, &button, action);
        content.append(&button);
    }

    // Open With sits right under Open, where it belongs.
    if let Some(entry) = single.as_ref().filter(|entry| !entry.is_directory()) {
        let applications = ops::applications_for(entry.data().content_type.as_deref());
        if !applications.is_empty() {
            let open_with = submenu_item(icons::ui(icons::names::OPEN_WITH), "Open With");
            open_with.set_popover(Some(&dialogs::open_with_popover(app, entry, applications)));
            content.append(&open_with);
        }
    }

    if let Some(entry) = single.as_ref() {
        if entry.is_directory() {
            items.push((
                header::menu_item(icons::ui(icons::names::TERMINAL), "Open Terminal Here"),
                ContextAction::TerminalHere,
            ));
            items.push((
                header::menu_item(
                    icons::ui(icons::names::PIN),
                    if app.is_pinned(entry.path()) {
                        "Remove Bookmark"
                    } else {
                        "Bookmark"
                    },
                ),
                ContextAction::Pin,
            ));
        }

        if ops::is_archive(entry.data().content_type.as_deref()) {
            items.push((
                header::menu_item(icons::ui(icons::names::EXTRACT), "Extract Here"),
                ContextAction::ExtractHere,
            ));
            items.push((
                header::menu_item(icons::ui(icons::names::EXTRACT), "Extract to Folder"),
                ContextAction::ExtractToFolder,
            ));
        }
    }

    if !in_trash {
        items.push((
            header::menu_item(icons::ui(icons::names::COPY), "Copy"),
            ContextAction::Copy,
        ));
        items.push((
            header::menu_item(icons::ui(icons::names::CUT), "Cut"),
            ContextAction::Cut,
        ));
        items.push((
            header::menu_item(icons::ui(icons::names::COPY), "Duplicate"),
            ContextAction::Duplicate,
        ));
        if ops::can_compress() {
            items.push((
                header::menu_item(icons::ui(icons::names::COMPRESS), "Compress"),
                ContextAction::Compress,
            ));
        }
    }

    if let Some(entry) = single.as_ref() {
        items.push((
            header::menu_item(icons::ui(icons::names::RENAME), "Rename"),
            ContextAction::Rename,
        ));
        items.push((
            header::menu_item(icons::ui(icons::names::COPY_PATH), "Copy Path"),
            ContextAction::CopyPath,
        ));

        // Only offered for the kinds of file that are meant to be run.
        let data = entry.data();
        if ops::can_be_executable(entry.is_directory(), data.content_type.as_deref()) {
            let executable = data.mode.is_some_and(|mode| mode & 0o111 != 0);
            items.push((
                header::menu_item(
                    icons::ui(icons::names::EXECUTABLE),
                    if executable {
                        "Stop Allowing Execution"
                    } else {
                        "Allow Executing as a Program"
                    },
                ),
                if executable {
                    ContextAction::ClearExecutable
                } else {
                    ContextAction::SetExecutable
                },
            ));
        }
    }

    for (button, action) in std::mem::take(&mut items) {
        wire_menu_item(app, &button, action);
        content.append(&button);
    }

    let paths: Vec<PathBuf> = selected
        .iter()
        .map(|entry| entry.path().to_path_buf())
        .collect();
    let tags = submenu_item(icons::ui(icons::names::TAG), "Tags");
    tags.set_popover(Some(&tag_popover(app, &paths)));
    content.append(&tags);

    content.append(&menu_separator());

    if in_trash {
        items.push((
            header::menu_item(icons::ui(icons::names::RESTORE), "Restore"),
            ContextAction::Restore,
        ));
        items.push((
            header::menu_item(icons::ui(icons::names::DELETE), "Delete Permanently"),
            ContextAction::Delete,
        ));
    } else {
        items.push((
            header::menu_item(icons::ui(icons::names::TRASH), "Move to Trash"),
            ContextAction::Trash,
        ));
    }

    for (button, action) in items {
        button.add_css_class("destructive");
        wire_menu_item(app, &button, action);
        content.append(&button);
    }

    content
}

/// The menu for the folder itself, shown when the click missed every file.
fn background_menu_content(app: &App) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.set_width_request(214);

    let in_trash = ops::is_in_trash(&app.current_dir());
    let in_tag_view = app.state.tag_view.borrow().is_some();
    let mut items: Vec<(gtk::Button, ContextAction)> = Vec::new();

    if !in_trash && !in_tag_view {
        items.push((
            header::menu_item(icons::ui(icons::names::NEW_FOLDER), "New Folder"),
            ContextAction::NewFolder,
        ));

        let paste = header::menu_item(icons::ui(icons::names::PASTE), "Paste");
        paste.set_sensitive(ops::clipboard_has_files(&app.widgets.window.clipboard()));
        items.push((paste, ContextAction::Paste));
        items.push((
            header::menu_item(icons::ui(icons::names::TERMINAL), "Open Terminal Here"),
            ContextAction::TerminalHere,
        ));
    }

    items.push((
        header::menu_item(icons::ui(icons::names::SELECT_ALL), "Select All"),
        ContextAction::SelectAll,
    ));
    items.push((
        header::menu_item(icons::ui(icons::names::REFRESH), "Refresh"),
        ContextAction::Refresh,
    ));

    for (button, action) in std::mem::take(&mut items) {
        wire_menu_item(app, &button, action);
        content.append(&button);
    }

    let groups = selection_groups(app);
    if !groups.is_empty() {
        let by_type = submenu_item(icons::ui(icons::names::SELECT_ALL), "Select by Type");
        by_type.set_popover(Some(&selection_group_popover(app, &groups)));
        content.append(&by_type);
    }

    if in_trash {
        content.append(&menu_separator());
        let empty = header::menu_item(icons::ui(icons::names::TRASH), "Empty Trash");
        empty.add_css_class("destructive");
        wire_menu_item(app, &empty, ContextAction::EmptyTrash);
        content.append(&empty);
    }

    content
}

fn wire_menu_item(app: &App, button: &gtk::Button, action: ContextAction) {
    let app = Rc::clone(app);
    let menu = app.widgets.context_menu.clone();
    button.connect_clicked(move |_| {
        menu.popdown();
        run_context_action(&app, action);
    });
}

fn menu_separator() -> gtk::Separator {
    let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
    separator.add_css_class("teral-separator");
    separator.set_margin_top(4);
    separator.set_margin_bottom(4);
    separator
}

/// A menu row that opens a popover of its own.
fn submenu_item(icon_name: &str, label: &str) -> gtk::MenuButton {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_hexpand(true);
    let arrow = gtk::Image::from_icon_name(icons::ui(icons::names::FORWARD));
    arrow.set_pixel_size(12);
    content.append(&icon);
    content.append(&text);
    content.append(&arrow);

    let button = gtk::MenuButton::new();
    button.set_child(Some(&content));
    button.set_always_show_arrow(false);
    button.set_direction(gtk::ArrowType::Right);
    button.add_css_class("teral-menu-item");
    button.set_has_frame(false);
    button
}

/// A popover offering to select every folder, or every file of one type.
fn selection_group_popover(app: &App, groups: &[(SelectionGroup, usize)]) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.add_css_class("teral-popover");

    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.set_width_request(180);

    for (group, count) in groups {
        let icon = match group {
            SelectionGroup::Folders => icons::ui(icons::names::OPEN_FOLDER),
            _ => icons::ui(icons::names::SELECT_ALL),
        };
        let item = header::menu_item(icon, &format!("{}   ({count})", group.label()));
        item.connect_clicked({
            let app = Rc::clone(app);
            let group = group.clone();
            let popover = popover.clone();
            move |_| {
                popover.popdown();
                app.widgets.context_menu.popdown();
                select_group(&app, &group);
            }
        });
        content.append(&item);
    }

    popover.set_child(Some(&content));
    popover
}

#[derive(Debug, Clone, Copy)]
enum ContextAction {
    Open,
    OpenInNewTab,
    OpenInNewWindow,
    Duplicate,
    Restore,
    Delete,
    ExtractHere,
    ExtractToFolder,
    SetExecutable,
    ClearExecutable,
    SelectAll,
    EmptyTrash,
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
    Compress,
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
        // From the background menu nothing is selected, and the folder being browsed is
        // what "here" means; a selected folder is more specific, so it wins.
        ContextAction::TerminalHere => {
            let target = app
                .single_selection()
                .filter(FileEntry::is_directory)
                .map(|entry| entry.path().to_path_buf())
                .unwrap_or_else(|| app.current_dir());
            open_terminal(app, &target);
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
        ContextAction::OpenInNewTab => {
            if let Some(entry) = app.single_selection().filter(FileEntry::is_directory) {
                open_in_new_tab(app, entry.path());
            }
        }
        ContextAction::OpenInNewWindow => {
            if let Some(entry) = app.single_selection().filter(FileEntry::is_directory) {
                open_in_new_window(app, entry.path());
            }
        }
        ContextAction::ExtractHere => {
            if let Some(entry) = app.single_selection() {
                extract_archive(app, &entry, false);
            }
        }
        ContextAction::ExtractToFolder => {
            if let Some(entry) = app.single_selection() {
                extract_archive(app, &entry, true);
            }
        }
        ContextAction::SetExecutable => set_executable(app, true),
        ContextAction::ClearExecutable => set_executable(app, false),
        ContextAction::SelectAll => {
            app.state.selection.select_all();
        }
        ContextAction::EmptyTrash => empty_trash(app),
        ContextAction::Duplicate => duplicate_selection(app),
        ContextAction::Restore => restore_selection(app),
        ContextAction::Delete => delete_permanently(app),
        ContextAction::Copy => stage_transfer(app, TransferKind::Copy),
        ContextAction::Cut => stage_transfer(app, TransferKind::Move),
        ContextAction::Trash => trash_selection(app),
        ContextAction::NewFolder => new_folder(app),
        ContextAction::Paste => paste(app),
        ContextAction::Refresh => app.reload(),
        ContextAction::Compress => compress_selection(app),
    }
}
