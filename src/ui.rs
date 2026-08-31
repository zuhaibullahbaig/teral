//! Presentation layer.
//!
//! `ui` owns the shared application object: browsing state, the list model every view
//! shares, and the operations the widgets in the submodules call into. The submodules
//! only build and update widgets.

pub mod brand;
pub mod details;
pub mod dialogs;
pub mod fileview;
pub mod header;
pub mod help;
pub mod search;
pub mod settings;
pub mod sidebar;
pub mod statusbar;
pub mod tabs;
pub mod window;

use crate::config::Config;
use crate::files::ops::{CancelFlag, Clipboard};
use crate::files::scan::{self, Sorting};
use crate::files::{EntryData, FileEntry};
use crate::places;
use crate::theme::ThemeConfig;
use gtk::gio;
use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub use window::build_window_at;

/// Which file view is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Grid,
    List,
}

impl ViewMode {
    const fn stack_name(self) -> &'static str {
        match self {
            Self::Grid => "grid",
            Self::List => "list",
        }
    }
}

/// What the file view is showing.
///
/// Not every view is a directory. The trash needs restore and permanent deletion where
/// an ordinary folder offers Cut and Trash, and a tag view gathers files from all over
/// the filesystem, so it has no single folder for a new file, a paste or a shell to act
/// in. Naming the difference here keeps every one of those decisions in one place
/// instead of scattered `is_in_trash` checks that a future virtual view would miss.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// An ordinary directory on disk.
    Directory(PathBuf),
    /// A trash directory. A real path, but its items belong somewhere else.
    Trash(PathBuf),
    /// Every file carrying one tag, gathered from across the filesystem.
    Tag(String),
}

impl Location {
    /// The directory an action should act in, for the views that have one.
    ///
    /// `None` means the view is not one place on disk, so creating a file, pasting into
    /// it, or opening a shell there has no defined meaning.
    pub fn working_directory(&self) -> Option<&Path> {
        match self {
            Self::Directory(path) => Some(path),
            Self::Trash(_) | Self::Tag(_) => None,
        }
    }

    /// True when the view gathers entries rather than showing one folder's contents.
    pub const fn is_virtual(&self) -> bool {
        matches!(self, Self::Tag(_))
    }

    /// True when the view's items are deleted files awaiting restore or removal.
    pub const fn is_trash(&self) -> bool {
        matches!(self, Self::Trash(_))
    }

    /// True when new folders, pastes, drops and a shell make sense here.
    pub const fn accepts_new_files(&self) -> bool {
        matches!(self, Self::Directory(_))
    }
}

/// Browsing state shared by every widget.
pub struct State {
    pub current: RefCell<PathBuf>,
    pub back: RefCell<Vec<PathBuf>>,
    pub forward: RefCell<Vec<PathBuf>>,
    /// Every entry in the current directory, before filtering.
    pub all: RefCell<Vec<EntryData>>,
    pub store: gio::ListStore,
    pub selection: gtk::MultiSelection,
    pub sorting: Cell<Sorting>,
    pub show_hidden: Cell<bool>,
    pub query: RefCell<String>,
    /// Increments on every navigation so stale async results are discarded.
    pub generation: Cell<u64>,
    pub pinned: RefCell<Vec<PathBuf>>,
    pub clipboard: RefCell<Option<Clipboard>>,
    pub icon_size: Cell<i32>,
    pub view_mode: Cell<ViewMode>,
    /// True while a Quick Command is attached to the console terminal.
    pub running_command: Cell<bool>,
    pub running_transfer: RefCell<Option<CancelFlag>>,
    /// Guards against feedback loops while widgets are being synchronised.
    pub updating: Cell<bool>,
    /// Open tabs, and which one is showing.
    pub tabs: RefCell<Vec<Tab>>,
    pub active_tab: Cell<usize>,
    /// Watches the directory on screen so external changes appear on their own.
    pub directory_monitor: RefCell<Option<gio::FileMonitor>>,
    pub refresh_queued: Cell<bool>,
    /// Kept alive so configuration and theme changes keep being delivered.
    pub config_monitors: RefCell<Vec<gio::FileMonitor>>,
    /// Height the console had when it was last visible.
    pub console_height: Cell<i32>,
    /// Set while the file view is showing everything carrying one tag.
    pub tag_view: RefCell<Option<String>>,
    /// True while a directory read is in flight, so a filesystem event arriving in the
    /// meantime cannot restart the view and cancel the navigation already under way.
    pub loading: Cell<bool>,
    /// An entry to select once the view it lives in has finished loading, used when
    /// another application asks Teral to open a file rather than a folder.
    pub pending_selection: RefCell<Option<PathBuf>>,
    /// Pending write of a new icon size, so dragging the slider writes once.
    pub icon_size_save: Cell<Option<glib::SourceId>>,
}

/// One browsing tab: a location and its own history.
///
/// `tag` is part of the tab, not the window. A tag view opened in one tab used to
/// disappear the moment another tab was shown, because switching tabs always loaded a
/// directory; keeping it here means each tab restores exactly the view it had.
#[derive(Debug, Clone)]
pub struct Tab {
    pub path: PathBuf,
    pub back: Vec<PathBuf>,
    pub forward: Vec<PathBuf>,
    pub tag: Option<String>,
}

impl Tab {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            back: Vec::new(),
            forward: Vec::new(),
            tag: None,
        }
    }
}

/// The application object. Widgets hold an `App` and call back into it.
pub struct AppInner {
    pub state: State,
    pub widgets: window::Widgets,
    /// The configuration and resolved theme currently applied, both replaceable at
    /// runtime by the Settings window or by an edit to the configuration file.
    pub config: RefCell<Config>,
    pub theme: RefCell<ThemeConfig>,
}

pub type App = Rc<AppInner>;

impl AppInner {
    pub fn current_dir(&self) -> PathBuf {
        self.state.current.borrow().clone()
    }

    /// What the view is currently showing.
    pub fn location(&self) -> Location {
        if let Some(tag) = self.state.tag_view.borrow().clone() {
            return Location::Tag(tag);
        }
        let path = self.current_dir();
        if crate::files::ops::is_in_trash(&path) {
            Location::Trash(path)
        } else {
            Location::Directory(path)
        }
    }

    /// Entries currently selected, in view order.
    pub fn selected_entries(&self) -> Vec<FileEntry> {
        let selection = &self.state.selection;
        let bitset = selection.selection();
        let mut entries = Vec::with_capacity(bitset.size() as usize);

        for index in 0..selection.n_items() {
            if bitset.contains(index)
                && let Some(entry) = selection.item(index).and_downcast::<FileEntry>()
            {
                entries.push(entry);
            }
        }
        entries
    }

    /// The single selected entry, if exactly one is selected.
    pub fn single_selection(&self) -> Option<FileEntry> {
        let entries = self.selected_entries();
        (entries.len() == 1).then(|| entries.into_iter().next().expect("length checked"))
    }

    /// Navigate to `path`, recording the current directory in the back history.
    pub fn navigate(self: &App, path: &Path) {
        if *self.state.current.borrow() == path {
            return;
        }
        let previous = self.current_dir();
        self.load(path, Some(HistoryStep::Push(previous)));
    }

    pub fn go_back(self: &App) {
        let target = self.state.back.borrow().last().cloned();
        if let Some(target) = target {
            self.load(&target.clone(), Some(HistoryStep::Back));
        }
    }

    pub fn go_forward(self: &App) {
        let target = self.state.forward.borrow().last().cloned();
        if let Some(target) = target {
            self.load(&target.clone(), Some(HistoryStep::Forward));
        }
    }

    pub fn go_up(self: &App) {
        let parent = self.state.current.borrow().parent().map(Path::to_path_buf);
        if let Some(parent) = parent {
            self.navigate(&parent);
        }
    }

    /// Re-read whatever the view is showing, keeping history and selection intact.
    pub fn reload(self: &App) {
        // The borrow is released before anything is called: `show_tag` takes the same
        // cell mutably, and a live shared borrow here would abort the process.
        let tag = self.state.tag_view.borrow().clone();
        if let Some(tag) = tag {
            self.show_tag(&tag);
            return;
        }
        let current = self.current_dir();
        self.load(&current, None);
    }

    /// Show every file carrying `tag` instead of a directory.
    pub fn show_tag(self: &App, tag: &str) {
        *self.state.tag_view.borrow_mut() = Some(tag.to_owned());

        let app = Rc::clone(self);
        let tag = tag.to_owned();
        let generation = self.state.generation.get().wrapping_add(1);
        self.state.generation.set(generation);
        self.state.loading.set(true);

        glib::spawn_future_local(async move {
            let paths = crate::tags::current()
                .get(&tag)
                .map(|tag| tag.paths.clone())
                .unwrap_or_default();
            let entries = scan::scan_paths(&paths).await;
            if app.state.generation.get() != generation {
                return;
            }
            app.state.loading.set(false);

            if let Some(tab) = app
                .state
                .tabs
                .borrow_mut()
                .get_mut(app.state.active_tab.get())
            {
                tab.tag = Some(tag.clone());
            }
            *app.state.all.borrow_mut() = entries;
            app.state.directory_monitor.borrow_mut().take();
            app.apply_filter();
            app.refresh_chrome();
            app.clear_message();
        });
    }

    /// Read a directory asynchronously and swap it in when it arrives.
    pub fn load(self: &App, path: &Path, history: Option<HistoryStep>) {
        *self.state.tag_view.borrow_mut() = None;
        if let Some(tab) = self
            .state
            .tabs
            .borrow_mut()
            .get_mut(self.state.active_tab.get())
        {
            tab.tag = None;
        }

        let app = Rc::clone(self);
        let path = path.to_path_buf();
        let generation = self.state.generation.get().wrapping_add(1);
        self.state.generation.set(generation);
        self.state.loading.set(true);

        glib::spawn_future_local(async move {
            let result = scan::scan_directory(&path).await;
            if app.state.generation.get() != generation {
                // A newer navigation has already taken over; it owns the flag now.
                return;
            }
            app.state.loading.set(false);

            match result {
                Ok(entries) => {
                    app.commit_directory(path, entries, history);
                    app.load_child_counts(generation);
                }
                Err(error) => app.show_error(&format!(
                    "Cannot open {}: {}",
                    places::display_label(&path),
                    error.message().trim()
                )),
            }
        });
    }

    fn commit_directory(
        self: &App,
        path: PathBuf,
        entries: Vec<EntryData>,
        history: Option<HistoryStep>,
    ) {
        let previous = self.current_dir();

        match history {
            Some(HistoryStep::Push(from)) if from != path => {
                self.state.back.borrow_mut().push(from);
                self.state.forward.borrow_mut().clear();
            }
            Some(HistoryStep::Push(_)) => {}
            Some(HistoryStep::Back) => {
                self.state.back.borrow_mut().pop();
                self.state.forward.borrow_mut().push(previous);
            }
            Some(HistoryStep::Forward) => {
                self.state.forward.borrow_mut().pop();
                self.state.back.borrow_mut().push(previous);
            }
            None => {}
        }

        let changed_directory = *self.state.current.borrow() != path;
        *self.state.current.borrow_mut() = path.clone();
        *self.state.all.borrow_mut() = entries;

        if let Some(tab) = self
            .state
            .tabs
            .borrow_mut()
            .get_mut(self.state.active_tab.get())
        {
            tab.path = path.clone();
        }
        self.watch_directory(&path);

        if changed_directory {
            self.state.query.borrow_mut().clear();
            self.widgets.search.entry.set_text("");
            search::close(self);
            self.state.selection.unselect_all();
            for scroller in [&self.widgets.grid_scroller, &self.widgets.list_scroller] {
                scroller.vadjustment().set_value(0.0);
            }
        }

        self.apply_filter();
        self.refresh_chrome();
        self.clear_message();
    }

    /// Rebuild the visible model from the current filter, sort and search settings.
    pub fn apply_filter(self: &App) {
        let previously_selected: Vec<PathBuf> = self
            .selected_entries()
            .iter()
            .map(|entry| entry.path().to_path_buf())
            .collect();

        let show_hidden = self.state.show_hidden.get();
        let query = self.state.query.borrow().to_lowercase();
        let sorting = self.state.sorting.get();

        let mut visible: Vec<EntryData> = self
            .state
            .all
            .borrow()
            .iter()
            .filter(|entry| scan::matches(entry, show_hidden, &query))
            .cloned()
            .collect();
        scan::sort(&mut visible, sorting);

        let objects: Vec<FileEntry> = visible.into_iter().map(FileEntry::new).collect();

        self.state.updating.set(true);
        self.state.store.splice(
            0,
            self.state.store.n_items(),
            &objects
                .iter()
                .map(|entry| entry.clone().upcast::<glib::Object>())
                .collect::<Vec<_>>(),
        );

        // Another application asked Teral to show one particular file. It is selected
        // once, when the folder holding it first appears, and then behaves like any
        // other selection.
        let requested = self.state.pending_selection.borrow_mut().take();
        let wanted: Vec<&Path> = requested
            .as_deref()
            .into_iter()
            .chain(previously_selected.iter().map(PathBuf::as_path))
            .collect();

        if !wanted.is_empty() {
            let mut found = false;
            for (index, entry) in objects.iter().enumerate() {
                if wanted.contains(&entry.path()) {
                    let index = u32::try_from(index).unwrap_or(u32::MAX);
                    self.state.selection.select_item(index, false);
                    found = true;
                }
            }
            // A file that is hidden, or filtered out, would otherwise be requested and
            // then silently not shown.
            if !found && requested.is_some() {
                self.state.updating.set(false);
                self.set_message("That file is not visible in this folder", false);
                self.update_counts();
                self.update_details();
                self.update_status();
                return;
            }
        }
        self.state.updating.set(false);

        self.update_counts();
        self.update_details();
        self.update_status();
    }

    /// Fill in folder item counts in the background, newest navigation wins.
    fn load_child_counts(self: &App, generation: u64) {
        const MAX_COUNTED_DIRECTORIES: usize = 600;

        let app = Rc::clone(self);
        glib::spawn_future_local(async move {
            let mut counted = 0usize;
            let mut index = 0u32;

            while index < app.state.store.n_items() {
                if app.state.generation.get() != generation || counted >= MAX_COUNTED_DIRECTORIES {
                    return;
                }

                let entry = app.state.store.item(index).and_downcast::<FileEntry>();
                index += 1;

                let Some(entry) = entry else { continue };
                if !entry.is_directory() || entry.child_count() >= 0 {
                    continue;
                }

                counted += 1;
                let count = scan::count_children(entry.path()).await.unwrap_or(0);
                if app.state.generation.get() != generation {
                    return;
                }
                entry.set_child_count(i64::try_from(count).unwrap_or(i64::MAX));
            }
        });
    }

    /// Refresh everything that depends on the current directory.
    pub fn refresh_chrome(self: &App) {
        let current = self.current_dir();

        let tag_view = self.state.tag_view.borrow().clone();
        if let Some(tag) = tag_view {
            self.widgets.folder_title.set_text(&tag);
            header::show_tag_crumb(self, &tag);
            sidebar::mark_active(self, Path::new(""));
            sidebar::mark_active_tag(self, Some(&tag));
            tabs::rebuild(self);
            return;
        }
        sidebar::mark_active_tag(self, None);

        self.widgets
            .folder_title
            .set_text(&places::display_label(&current));
        header::rebuild_breadcrumbs(self, &current);
        self.widgets
            .back
            .set_sensitive(!self.state.back.borrow().is_empty());
        self.widgets
            .forward
            .set_sensitive(!self.state.forward.borrow().is_empty());
        self.widgets.up.set_sensitive(current.parent().is_some());
        sidebar::mark_active(self, &current);
        if let Some(tab) = self
            .state
            .tabs
            .borrow_mut()
            .get_mut(self.state.active_tab.get())
        {
            tab.path = current.clone();
        }
        tabs::rebuild(self);
    }

    pub fn update_counts(self: &App) {
        let total = self.state.all.borrow().len();
        let visible = self.state.store.n_items() as usize;

        let text = if visible == total {
            crate::files::item_count_label(total)
        } else {
            format!("{visible} of {}", crate::files::item_count_label(total))
        };
        self.widgets.folder_subtitle.set_text(&text);
        search::update_matches(self, visible, total);
    }

    pub fn update_status(&self) {
        let selected = self.selected_entries();
        let selection_text = match selected.len() {
            0 => String::new(),
            count => format!("{count} selected"),
        };
        self.widgets.status_selection.set_text(&selection_text);

        let bytes: u64 = selected
            .iter()
            .filter(|entry| !entry.is_directory())
            .map(|entry| entry.data().size)
            .sum();
        let size_text = if bytes > 0 {
            crate::files::format_size(bytes)
        } else {
            String::new()
        };
        self.widgets.status_size.set_text(&size_text);
    }

    pub fn update_details(self: &App) {
        details::update(self);
    }

    pub fn set_message(&self, message: &str, is_error: bool) {
        self.widgets.status_message.set_text(message);
        if is_error {
            self.widgets.status_message.add_css_class("error");
        } else {
            self.widgets.status_message.remove_css_class("error");
        }
    }

    pub fn show_error(&self, message: &str) {
        eprintln!("Teral: {message}");
        self.set_message(message, true);
    }

    pub fn clear_message(&self) {
        self.set_message("", false);
    }

    /// Toggle a directory's presence in the pinned sidebar section.
    pub fn toggle_pin(self: &App, path: &Path) {
        {
            let mut pinned = self.state.pinned.borrow_mut();
            if let Some(index) = pinned.iter().position(|pin| pin == path) {
                pinned.remove(index);
            } else {
                pinned.push(path.to_path_buf());
            }
            places::save_pinned(&pinned);
        }
        sidebar::rebuild_pinned(self);
        sidebar::mark_active(self, &self.current_dir());
    }

    pub fn is_pinned(&self, path: &Path) -> bool {
        self.state.pinned.borrow().iter().any(|pin| pin == path)
    }

    /// Restore the grid icon size configured by the active theme.
    pub fn reset_zoom(self: &App) {
        let size = self.theme.borrow().grid_icon_size();
        self.widgets.zoom.set_value(f64::from(size));
    }

    /// Watch the directory on screen so changes made elsewhere show up by themselves.
    fn watch_directory(self: &App, path: &Path) {
        let monitor = gio::File::for_path(path)
            .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE);

        let monitor = match monitor {
            Ok(monitor) => monitor,
            Err(_) => {
                // Some filesystems cannot be watched; browsing still works without it.
                self.state.directory_monitor.borrow_mut().take();
                return;
            }
        };

        let app = Rc::clone(self);
        let watched = path.to_path_buf();
        monitor.connect_changed(move |_, _, _, _| app.queue_refresh(&watched));
        *self.state.directory_monitor.borrow_mut() = Some(monitor);
    }

    /// Coalesce a burst of filesystem events into a single reload.
    ///
    /// `watched` is the directory the event came from. Events are delivered on a delay,
    /// and a monitor is only replaced once its successor's directory has been read, so
    /// without this check a change to the folder being left could fire after the user
    /// had already clicked into the next one — reloading the old location and cancelling
    /// the navigation in flight.
    fn queue_refresh(self: &App, watched: &Path) {
        if *self.state.current.borrow() != watched {
            return;
        }
        if self.state.refresh_queued.replace(true) {
            return;
        }

        let app = Rc::clone(self);
        let watched = watched.to_path_buf();
        glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
            app.state.refresh_queued.set(false);
            // Still the same place, nothing already on its way in, and no transfer that
            // will reload on its own when it finishes.
            if *app.state.current.borrow() == watched
                && !app.state.loading.get()
                && app.state.running_transfer.borrow().is_none()
            {
                app.reload();
            }
        });
    }

    // ------------------------------------------------------------------ tabs ----

    /// Store the live history back into the active tab before leaving it.
    fn sync_active_tab(&self) {
        let index = self.state.active_tab.get();
        if let Some(tab) = self.state.tabs.borrow_mut().get_mut(index) {
            tab.path = self.current_dir();
            tab.back = self.state.back.borrow().clone();
            tab.forward = self.state.forward.borrow().clone();
            tab.tag = self.state.tag_view.borrow().clone();
        }
    }

    /// Open a new tab showing `path` and switch to it.
    pub fn open_tab(self: &App, path: PathBuf) {
        self.sync_active_tab();
        self.state.tabs.borrow_mut().push(Tab::new(path.clone()));
        let index = self.state.tabs.borrow().len() - 1;
        self.activate_tab(index);
    }

    /// Close a tab, keeping at least one open.
    pub fn close_tab(self: &App, index: usize) {
        if self.state.tabs.borrow().len() <= 1 {
            return;
        }

        self.sync_active_tab();
        self.state.tabs.borrow_mut().remove(index);

        let active = self.state.active_tab.get();
        let next = if active > index || active >= self.state.tabs.borrow().len() {
            active.saturating_sub(1)
        } else {
            active
        };
        self.activate_tab(next);
    }

    /// Show the tab at `index`.
    pub fn activate_tab(self: &App, index: usize) {
        let Some(tab) = self.state.tabs.borrow().get(index).cloned() else {
            return;
        };

        if index != self.state.active_tab.get() {
            self.sync_active_tab();
        }
        self.state.active_tab.set(index);
        *self.state.back.borrow_mut() = tab.back;
        *self.state.forward.borrow_mut() = tab.forward;
        match &tab.tag {
            Some(tag) => {
                *self.state.current.borrow_mut() = tab.path.clone();
                self.show_tag(tag);
            }
            None => self.load(&tab.path, None),
        }
        tabs::rebuild(self);
    }

    /// Move to the next tab, wrapping around.
    pub fn cycle_tab(self: &App, forward: bool) {
        let count = self.state.tabs.borrow().len();
        if count < 2 {
            return;
        }
        let active = self.state.active_tab.get();
        let next = if forward {
            (active + 1) % count
        } else {
            (active + count - 1) % count
        };
        self.activate_tab(next);
    }

    // ----------------------------------------------------------------- theme ----

    /// Apply a new configuration: persist it, re-resolve the theme and restyle.
    pub fn apply_config(self: &App, config: Config, persist: bool) {
        if persist && let Err(error) = config.save() {
            self.show_error(&format!("Could not save settings: {error}"));
        }

        crate::config::set_current(config.clone());
        let theme = ThemeConfig::resolve(&config);
        crate::style::apply(&theme);

        let icon_size = theme.grid_icon_size();
        crate::command::style_terminal(&self.widgets.console.terminal, &theme);
        *self.theme.borrow_mut() = theme;
        *self.config.borrow_mut() = config;

        self.schedule_theme_settle();

        if icon_size != self.state.icon_size.get() {
            self.state.icon_size.set(icon_size);
            self.state.updating.set(true);
            self.widgets.zoom.set_value(f64::from(icon_size));
            self.state.updating.set(false);
            self.widgets.zoom_value.set_text(&format!("{icon_size} px"));
            fileview::refresh_grid_factory(self);
        }
    }

    /// Re-resolve the palette after GTK has finished switching its own theme.
    ///
    /// Turning "Follow the system" on flips GTK's light/dark preference, and GTK only
    /// swaps its named colours once that has settled; resolving twice means Teral picks
    /// up the desktop's real colours on the first switch instead of the second.
    pub fn schedule_theme_settle(self: &App) {
        if self.config.borrow().mode != crate::config::ThemeMode::System {
            return;
        }

        let app = Rc::clone(self);
        glib::idle_add_local_once(move || {
            let config = app.config.borrow().clone();
            let theme = ThemeConfig::resolve(&config);
            crate::style::apply(&theme);
            crate::command::style_terminal(&app.widgets.console.terminal, &theme);
            *app.theme.borrow_mut() = theme;
        });
    }

    /// Re-read the configuration file and any environment theme, then restyle.
    pub fn reload_theme(self: &App) {
        let config = Config::load();
        self.apply_config(config, false);
        self.apply_preferences();
    }

    /// Push the configuration's file preferences into the live browsing state.
    pub fn apply_preferences(self: &App) {
        let config = self.config.borrow().clone();

        self.state.show_hidden.set(config.show_hidden);
        self.state.sorting.set(Sorting {
            key: config.sort,
            descending: config.descending,
            folders_first: config.folders_first,
        });

        self.state.updating.set(true);
        let hidden_check = self.widgets.hidden_check.borrow().clone();
        if let Some(check) = hidden_check {
            check.set_active(config.show_hidden);
        }
        match config.view {
            crate::config::ViewPreference::Grid => self.widgets.grid_toggle.set_active(true),
            crate::config::ViewPreference::List => self.widgets.list_toggle.set_active(true),
        }
        self.state.updating.set(false);

        self.set_view_mode(match config.view {
            crate::config::ViewPreference::Grid => ViewMode::Grid,
            crate::config::ViewPreference::List => ViewMode::List,
        });
        self.apply_filter();
    }

    /// Step the grid icon size, keeping it inside the range Teral will draw.
    pub fn step_zoom(self: &App, delta: i32) {
        let size = (self.state.icon_size.get() + delta)
            .clamp(crate::theme::MIN_ICON_SIZE, crate::theme::MAX_ICON_SIZE);
        self.widgets.zoom.set_value(f64::from(size));
    }

    /// Resize the grid icons.
    ///
    /// Re-resolving the theme and rewriting the configuration on every step of the
    /// slider made dragging it feel sticky, so the size is applied immediately and the
    /// configuration is written once the slider settles.
    pub fn set_icon_size(self: &App, size: i32) {
        if size == self.state.icon_size.get() {
            return;
        }
        self.state.icon_size.set(size);
        self.widgets.zoom_value.set_text(&format!("{size} px"));
        fileview::refresh_grid_factory(self);

        let pending = self.state.icon_size_save.replace(None);
        if let Some(source) = pending {
            source.remove();
        }

        let app = Rc::clone(self);
        let source =
            glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                app.state.icon_size_save.set(None);
                let mut config = app.config.borrow().clone();
                config.layout.grid_icon_size = Some(app.state.icon_size.get());
                if let Err(error) = config.save() {
                    app.show_error(&format!("Could not save settings: {error}"));
                }
                crate::config::set_current(config.clone());
                *app.config.borrow_mut() = config;
            });
        self.state.icon_size_save.set(Some(source));
    }

    pub fn set_view_mode(self: &App, mode: ViewMode) {
        self.state.view_mode.set(mode);
        self.widgets
            .view_stack
            .set_visible_child_name(mode.stack_name());
    }
}

/// How a load affects the navigation history.
#[derive(Debug, Clone)]
pub enum HistoryStep {
    Push(PathBuf),
    Back,
    Forward,
}

// ------------------------------------------------------------------ helpers ----

/// A label with the wide letter spacing used for Teral's section headings.
pub fn tracked_label(text: &str, tracking: i32) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    let attributes = pango::AttrList::new();
    attributes.insert(pango::AttrInt::new_letter_spacing(tracking * pango::SCALE));
    label.set_attributes(Some(&attributes));
    label
}

/// Run `action` on the next main-loop pass.
///
/// Signal handlers that rebuild the very widget they were fired from must not destroy
/// it while GTK is still inside the emission; deferring by one iteration lets the
/// handler return first.
pub fn defer(action: impl FnOnce() + 'static) {
    glib::idle_add_local_once(action);
}

/// Colour one widget without going through the global stylesheet.
///
/// Tags carry arbitrary user colours, so each one needs its own provider rather than a
/// class the theme could define.
pub fn apply_color(widget: &impl IsA<gtk::Widget>, color: &str) {
    if !crate::theme::valid_color(color) {
        return;
    }

    let provider = gtk::CssProvider::new();
    provider.load_from_string(&format!("* {{ color: {color}; }}"));

    // Per-widget providers are the only way to apply a colour GTK's stylesheet cannot
    // know about ahead of time; the replacement API does not cover this case.
    #[allow(deprecated)]
    widget
        .as_ref()
        .style_context()
        .add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
}

/// A sidebar section heading.
pub fn section_title(text: &str) -> gtk::Label {
    let label = tracked_label(text, 1);
    label.add_css_class("teral-section-title");
    label.set_margin_start(10);
    label.set_margin_top(4);
    label.set_margin_bottom(4);
    label
}

/// A flat, compact icon button.
///
/// Centring matters: a button left at the default `Fill` alignment grows to the full
/// height of whatever row it sits in, which is what makes toolbar icons look like they
/// are floating inside oversized plates.
pub fn icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon_name);
    button.add_css_class("teral-icon-button");
    button.set_tooltip_text(Some(tooltip));
    button.set_has_frame(false);
    button.set_valign(gtk::Align::Center);
    button.set_halign(gtk::Align::Center);
    button
}

/// A flat, compact toggle button.
pub fn icon_toggle(icon_name: &str, tooltip: &str) -> gtk::ToggleButton {
    let button = gtk::ToggleButton::new();
    button.set_child(Some(&gtk::Image::from_icon_name(icon_name)));
    button.add_css_class("teral-icon-button");
    button.set_tooltip_text(Some(tooltip));
    button.set_has_frame(false);
    button.set_valign(gtk::Align::Center);
    button.set_halign(gtk::Align::Center);
    button
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_real_folder_can_be_acted_in() {
        let folder = Location::Directory(PathBuf::from("/home/zub/Documents"));
        assert_eq!(
            folder.working_directory(),
            Some(Path::new("/home/zub/Documents"))
        );
        assert!(folder.accepts_new_files());
        assert!(!folder.is_virtual());
        assert!(!folder.is_trash());

        // The trash is a real path, but its items belong somewhere else, so nothing new
        // is created in it and no shell is opened there.
        let trash = Location::Trash(PathBuf::from("/home/zub/.local/share/Trash/files"));
        assert_eq!(trash.working_directory(), None);
        assert!(!trash.accepts_new_files());
        assert!(trash.is_trash());
        assert!(!trash.is_virtual());

        // A tag view gathers files from all over the filesystem and is not one place.
        let tagged = Location::Tag("Important".to_owned());
        assert_eq!(tagged.working_directory(), None);
        assert!(!tagged.accepts_new_files());
        assert!(tagged.is_virtual());
        assert!(!tagged.is_trash());
    }

    #[test]
    fn a_tab_remembers_the_view_it_was_showing() {
        let mut tab = Tab::new(PathBuf::from("/home/zub"));
        assert_eq!(tab.tag, None);
        assert!(tab.back.is_empty());
        assert!(tab.forward.is_empty());

        // Switching away from a tag view and back must return to the tag view, not to
        // whichever directory the tab happened to be in beforehand.
        tab.tag = Some("Important".to_owned());
        let restored = tab.clone();
        assert_eq!(restored.tag.as_deref(), Some("Important"));
        assert_eq!(restored.path, PathBuf::from("/home/zub"));
    }
}
