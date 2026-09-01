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
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};

const GLOBAL_SEARCH_PAGE: usize = 96;
const GLOBAL_SEARCH_BUFFERED_BATCHES: usize = 4;

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
    /// Results from a recursive search rooted in one local directory.
    Search { root: PathBuf, query: String },
}

impl Location {
    /// The directory an action should act in, for the views that have one.
    ///
    /// `None` means the view is not one place on disk, so creating a file, pasting into
    /// it, or opening a shell there has no defined meaning.
    pub fn working_directory(&self) -> Option<&Path> {
        match self {
            Self::Directory(path) => Some(path),
            Self::Trash(_) | Self::Tag(_) | Self::Search { .. } => None,
        }
    }

    /// True when the view gathers entries rather than showing one folder's contents.
    pub const fn is_virtual(&self) -> bool {
        matches!(self, Self::Tag(_) | Self::Search { .. })
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
    /// A short debounce keeps a large folder from being rebuilt for every key event.
    pub filter_source: Cell<Option<glib::SourceId>>,
    /// Increments on every navigation so stale async results are discarded.
    pub generation: Cell<u64>,
    pub preview_generation: Cell<u64>,
    pub pinned: RefCell<Vec<PathBuf>>,
    pub clipboard: RefCell<Option<Clipboard>>,
    pub icon_size: Cell<i32>,
    pub view_mode: Cell<ViewMode>,
    /// True while a Quick Command is attached to the console terminal.
    pub running_command: Cell<bool>,
    pub running_pid: Cell<Option<glib::Pid>>,
    pub command_stop_requested: Cell<bool>,
    pub command_close_requested: Cell<bool>,
    pub command_history: RefCell<Vec<String>>,
    pub command_history_index: Cell<usize>,
    pub running_transfer: RefCell<Option<CancelFlag>>,
    /// Guards against feedback loops while widgets are being synchronised.
    pub updating: Cell<bool>,
    pub theme_apply_source: Cell<Option<glib::SourceId>>,
    /// Configuration writes are serialized off the GTK thread. Repeated slider and
    /// text-entry changes replace the pending value instead of queuing one fsync each.
    pub config_save_running: Cell<bool>,
    pub pending_config_save: RefCell<Option<Config>>,
    pub bookmark_save_running: Cell<bool>,
    pub pending_bookmark_save: RefCell<Option<(PathBuf, Vec<u8>)>>,
    pub config_reload_queued: Cell<bool>,
    pub theme_reload_requested: Cell<bool>,
    /// Open tabs, and which one is showing.
    pub tabs: RefCell<Vec<Tab>>,
    pub active_tab: Cell<usize>,
    /// Watches the directory on screen so external changes appear on their own.
    pub directory_monitor: RefCell<Option<gio::FileMonitor>>,
    pub refresh_queued: Cell<bool>,
    /// Kept for the window lifetime so hot-plug and mount signals remain connected.
    pub volume_monitor: gio::VolumeMonitor,
    pub volume_handlers: RefCell<Vec<glib::SignalHandlerId>>,
    pub device_refresh_queued: Cell<bool>,
    /// Kept alive so configuration and theme changes keep being delivered.
    pub config_monitors: RefCell<Vec<gio::FileMonitor>>,
    /// Signal handlers installed on global GTK settings, disconnected with the window.
    pub desktop_handlers: RefCell<Vec<(gtk::Settings, glib::SignalHandlerId)>>,
    /// Height the console had when it was last visible.
    pub console_height: Cell<i32>,
    /// Set while the file view is showing everything carrying one tag.
    pub tag_view: RefCell<Option<String>>,
    /// Root and query of the recursive search currently displayed.
    pub global_search: RefCell<Option<(PathBuf, String)>>,
    pub global_search_return: RefCell<Option<PathBuf>>,
    pub global_search_cancel: RefCell<Option<Arc<AtomicBool>>>,
    pub global_search_receiver:
        RefCell<Option<mpsc::Receiver<crate::files::search::SearchEvent>>>,
    pub global_search_pending: RefCell<VecDeque<Vec<PathBuf>>>,
    pub global_search_scan_running: Cell<bool>,
    pub global_search_finished: Cell<bool>,
    pub global_search_unreadable: Cell<usize>,
    pub global_search_visible_limit: Cell<usize>,
    pub global_search_source: Cell<Option<glib::SourceId>>,
    /// True while a directory read is in flight, so a filesystem event arriving in the
    /// meantime cannot restart the view and cancel the navigation already under way.
    pub loading: Cell<bool>,
    /// An entry to select once the view it lives in has finished loading, used when
    /// another application asks Teral to open a file rather than a folder.
    pub pending_selection: RefCell<Option<PathBuf>>,
    pub retained_selection: RefCell<Vec<PathBuf>>,
    /// Pending write of a new icon size, so dragging the slider writes once.
    pub icon_size_save: Cell<Option<glib::SourceId>>,
    /// At most one grid-factory replacement is queued per rendered frame.
    pub icon_refresh_queued: Cell<bool>,
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
    pub search: Option<String>,
}

impl Tab {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            back: Vec::new(),
            forward: Vec::new(),
            tag: None,
            search: None,
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
        if let Some((root, query)) = self.state.global_search.borrow().clone() {
            return Location::Search { root, query };
        }
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
        if self.state.global_search.borrow().is_none() && *self.state.current.borrow() == path {
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
        if let Some((_, query)) = self.state.global_search.borrow().clone() {
            self.show_global_search(&query);
            return;
        }
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
        self.cancel_global_search();
        self.state.global_search_return.borrow_mut().take();
        header::hide_global_search_controls(self);
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

    /// Recursively search the user's home directory, publishing bounded result batches.
    pub fn show_global_search(self: &App, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            self.set_message("Enter a name to search for", false);
            return;
        }

        if self.state.global_search.borrow().is_none()
            && self.state.global_search_return.borrow().is_none()
        {
            *self.state.global_search_return.borrow_mut() = Some(self.current_dir());
        }
        self.cancel_global_search();
        search::close(self);
        header::show_global_search_controls(self);
        self.widgets.global_search_entry.set_text(query);
        let root = crate::theme::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        *self.state.tag_view.borrow_mut() = None;
        *self.state.global_search.borrow_mut() = Some((root.clone(), query.to_owned()));
        if let Some(tab) = self
            .state
            .tabs
            .borrow_mut()
            .get_mut(self.state.active_tab.get())
        {
            tab.path = root.clone();
            tab.tag = None;
            tab.search = Some(query.to_owned());
        }

        let generation = self.state.generation.get().wrapping_add(1);
        self.state.generation.set(generation);
        self.state.loading.set(true);
        self.state.directory_monitor.borrow_mut().take();
        self.state.store.remove_all();
        self.state.all.borrow_mut().clear();
        self.state.selection.unselect_all();
        self.state.global_search_finished.set(false);
        self.state.global_search_unreadable.set(0);
        self.state
            .global_search_visible_limit
            .set(GLOBAL_SEARCH_PAGE);
        self.state.global_search_pending.borrow_mut().clear();
        self.refresh_chrome();
        self.update_counts();
        self.set_message("Searching Home…", false);

        let (sender, receiver) = mpsc::sync_channel(8);
        *self.state.global_search_receiver.borrow_mut() = Some(receiver);
        let cancelled = Arc::new(AtomicBool::new(false));
        *self.state.global_search_cancel.borrow_mut() = Some(Arc::clone(&cancelled));
        let worker_root = root;
        let worker_query = query.to_owned();
        let show_hidden = self.state.show_hidden.get();
        let _worker = std::thread::spawn(move || {
            crate::files::search::run(
                worker_root,
                worker_query,
                show_hidden,
                cancelled,
                sender,
            );
        });

        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_local(std::time::Duration::from_millis(35), move || {
            let Some(app) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if app.state.generation.get() != generation
                || app.state.global_search.borrow().is_none()
            {
                app.state.global_search_source.set(None);
                return glib::ControlFlow::Break;
            }

            let capacity = GLOBAL_SEARCH_BUFFERED_BATCHES
                .saturating_sub(app.state.global_search_pending.borrow().len());
            let events = app
                .state
                .global_search_receiver
                .borrow()
                .as_ref()
                .map(|receiver| {
                    (0..capacity)
                        .filter_map(|_| receiver.try_recv().ok())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for event in events {
                match event {
                    crate::files::search::SearchEvent::Batch(paths) => {
                        app.state.global_search_pending.borrow_mut().push_back(paths);
                    }
                    crate::files::search::SearchEvent::Finished { unreadable } => {
                        app.state.global_search_unreadable.set(unreadable);
                        app.state.global_search_finished.set(true);
                    }
                }
            }
            app.pump_global_search(generation);

            if app.state.global_search_finished.get()
                && app.state.global_search_pending.borrow().is_empty()
                && !app.state.global_search_scan_running.get()
            {
                app.finish_global_search();
                app.state.global_search_source.set(None);
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
        self.state.global_search_source.set(Some(source));
    }

    fn pump_global_search(self: &App, generation: u64) {
        if self.state.global_search_scan_running.get() {
            return;
        }
        if self.state.all.borrow().len() >= self.state.global_search_visible_limit.get() {
            self.set_message("Scroll down to load more results", false);
            return;
        }
        let Some(paths) = self.state.global_search_pending.borrow_mut().pop_front() else {
            return;
        };
        self.state.global_search_scan_running.set(true);
        let app = Rc::clone(self);
        glib::spawn_future_local(async move {
            let entries = scan::scan_paths(&paths).await;
            if app.state.generation.get() != generation
                || app.state.global_search.borrow().is_none()
            {
                return;
            }
            let objects: Vec<glib::Object> = entries
                .iter()
                .cloned()
                .map(FileEntry::new)
                .map(|entry| entry.upcast::<glib::Object>())
                .collect();
            app.state.all.borrow_mut().extend(entries);
            app.state
                .store
                .splice(app.state.store.n_items(), 0, &objects);
            app.state.global_search_scan_running.set(false);
            if app.state.all.borrow().len() >= GLOBAL_SEARCH_PAGE {
                app.state.loading.set(false);
            }
            app.update_counts();
            app.pump_global_search(generation);
        });
    }

    /// Allow another bounded page of recursive-search results to enter the GTK model.
    pub fn load_more_global_search(self: &App) {
        if self.state.global_search.borrow().is_none() {
            return;
        }
        if self.state.global_search_scan_running.get()
            || self.state.all.borrow().len() < self.state.global_search_visible_limit.get()
        {
            return;
        }
        let next = self
            .state
            .global_search_visible_limit
            .get()
            .saturating_add(GLOBAL_SEARCH_PAGE);
        self.state.global_search_visible_limit.set(next);
        self.set_message("Loading more results…", false);
        self.pump_global_search(self.state.generation.get());
    }

    /// Leave recursive search and restore the directory that was visible before it.
    pub fn exit_global_search(self: &App) {
        let target = self
            .state
            .global_search_return
            .borrow_mut()
            .take()
            .unwrap_or_else(|| crate::theme::home_dir().unwrap_or_else(|| PathBuf::from("/")));
        self.cancel_global_search();
        self.load(&target, None);
    }

    fn finish_global_search(self: &App) {
        self.state.loading.set(false);
        self.state.global_search_receiver.borrow_mut().take();
        self.state.global_search_cancel.borrow_mut().take();
        self.apply_filter();
        let unreadable = self.state.global_search_unreadable.get();
        if unreadable == 0 {
            self.clear_message();
        } else {
            self.set_message(
                &format!("Search completed; {unreadable} folders could not be read"),
                false,
            );
        }
    }

    fn cancel_global_search(&self) {
        if let Some(cancelled) = self.state.global_search_cancel.borrow_mut().take() {
            cancelled.store(true, Ordering::Relaxed);
        }
        if let Some(source) = self.state.global_search_source.replace(None) {
            source.remove();
        }
        self.state.global_search_receiver.borrow_mut().take();
        self.state.global_search_pending.borrow_mut().clear();
        self.state.global_search_scan_running.set(false);
        self.state.global_search_finished.set(false);
        self.state.global_search.borrow_mut().take();
    }

    /// Read a directory asynchronously and swap it in when it arrives.
    pub fn load(self: &App, path: &Path, history: Option<HistoryStep>) {
        self.cancel_global_search();
        self.state.global_search_return.borrow_mut().take();
        header::hide_global_search_controls(self);
        *self.state.tag_view.borrow_mut() = None;
        if let Some(tab) = self
            .state
            .tabs
            .borrow_mut()
            .get_mut(self.state.active_tab.get())
        {
            tab.tag = None;
            tab.search = None;
        }

        let app = Rc::clone(self);
        let path = path.to_path_buf();
        let generation = self.state.generation.get().wrapping_add(1);
        self.state.generation.set(generation);
        crate::icons::cancel_pending();
        self.state.loading.set(true);

        let retained = if self.state.current.borrow().as_path() == path {
            self.selected_entries()
                .into_iter()
                .map(|entry| entry.path().to_path_buf())
                .collect()
        } else {
            Vec::new()
        };
        *self.state.retained_selection.borrow_mut() = retained;

        // A new physical directory publishes bounded batches immediately. The final
        // commit applies the selected sort/filter once, avoiding a full clone and model
        // rebuild per batch.
        self.state.store.remove_all();
        self.state.all.borrow_mut().clear();
        self.update_counts();

        glib::spawn_future_local(async move {
            let result = scan::scan_directory_batched(&path, {
                let app = Rc::clone(&app);
                move |batch| {
                    if app.state.generation.get() != generation {
                        return false;
                    }
                    let show_hidden = app.state.show_hidden.get();
                    let query = app.state.query.borrow().to_lowercase();
                    let visible: Vec<FileEntry> = batch
                        .iter()
                        .filter(|entry| scan::matches(entry, show_hidden, &query))
                        .cloned()
                        .map(FileEntry::new)
                        .collect();
                    app.state.all.borrow_mut().extend(batch);
                    let objects: Vec<glib::Object> = visible
                        .into_iter()
                        .map(|entry| entry.upcast::<glib::Object>())
                        .collect();
                    app.state
                        .store
                        .splice(app.state.store.n_items(), 0, &objects);
                    app.update_counts();
                    true
                }
            })
            .await;
            if app.state.generation.get() != generation {
                // A newer navigation has already taken over; it owns the flag now.
                return;
            }
            app.state.loading.set(false);

            match result {
                Ok(entries) => {
                    app.commit_directory(path, entries, history);
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
            self.state.updating.set(true);
            self.widgets.search.entry.set_text("");
            self.state.updating.set(false);
            if let Some(source) = self.state.filter_source.replace(None) {
                source.remove();
            }
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
        let retained = std::mem::take(&mut *self.state.retained_selection.borrow_mut());
        let wanted: Vec<&Path> = requested
            .as_deref()
            .into_iter()
            .chain(previously_selected.iter().map(PathBuf::as_path))
            .chain(retained.iter().map(PathBuf::as_path))
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

    /// Apply the newest search text after the current burst of key events settles.
    pub fn queue_filter(self: &App) {
        if let Some(source) = self.state.filter_source.replace(None) {
            source.remove();
        }
        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_local_once(
            std::time::Duration::from_millis(90),
            move || {
                let Some(app) = weak.upgrade() else {
                    return;
                };
                app.state.filter_source.set(None);
                app.apply_filter();
            },
        );
        self.state.filter_source.set(Some(source));
    }

    /// Refresh everything that depends on the current directory.
    pub fn refresh_chrome(self: &App) {
        let current = self.current_dir();

        if let Some((_, query)) = self.state.global_search.borrow().clone() {
            self.widgets.new_folder.set_visible(false);
            self.widgets.folder_title.set_text("Search results");
            header::show_search_crumb(self, &query);
            sidebar::mark_active(self, Path::new(""));
            sidebar::mark_active_tag(self, None);
            self.widgets.back.set_sensitive(!self.state.back.borrow().is_empty());
            self.widgets
                .forward
                .set_sensitive(!self.state.forward.borrow().is_empty());
            self.widgets.up.set_sensitive(false);
            tabs::rebuild(self);
            return;
        }

        let tag_view = self.state.tag_view.borrow().clone();
        if let Some(tag) = tag_view {
            self.widgets.new_folder.set_visible(false);
            self.widgets.folder_title.set_text(&tag);
            header::show_tag_crumb(self, &tag);
            sidebar::mark_active(self, Path::new(""));
            sidebar::mark_active_tag(self, Some(&tag));
            tabs::rebuild(self);
            return;
        }
        sidebar::mark_active_tag(self, None);
        self.widgets
            .new_folder
            .set_visible(self.location().accepts_new_files());

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
        }
        self.queue_bookmark_save();
        sidebar::rebuild_pinned(self);
        sidebar::mark_active(self, &self.current_dir());
    }

    pub fn is_pinned(&self, path: &Path) -> bool {
        self.state.pinned.borrow().iter().any(|pin| pin == path)
    }

    /// Move a bookmark without changing the location it points to.
    pub fn reorder_pin(self: &App, path: &Path, offset: isize) {
        {
            let mut pinned = self.state.pinned.borrow_mut();
            let Some(index) = pinned.iter().position(|pin| pin == path) else {
                return;
            };
            let target = index.saturating_add_signed(offset).min(pinned.len() - 1);
            if index == target {
                return;
            }
            pinned.swap(index, target);
        }
        self.queue_bookmark_save();
        sidebar::rebuild_pinned(self);
    }

    /// Change only the sidebar label, never the directory's real name.
    pub fn label_pin(self: &App, path: &Path, label: String) {
        places::set_bookmark_label(path, Some(label));
        self.queue_bookmark_save();
        sidebar::rebuild_pinned(self);
    }

    /// Serialize labels on the GTK context, then keep only the newest pending write.
    fn queue_bookmark_save(self: &App) {
        let payload = places::pinned_payload(&self.state.pinned.borrow());
        *self.state.pending_bookmark_save.borrow_mut() = Some(payload);
        if self.state.bookmark_save_running.replace(true) {
            return;
        }
        self.start_next_bookmark_save();
    }

    fn start_next_bookmark_save(self: &App) {
        let Some((path, document)) = self.state.pending_bookmark_save.borrow_mut().take() else {
            self.state.bookmark_save_running.set(false);
            return;
        };
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                places::write_pinned_payload(path, document)
            })
            .await;
            let Some(app) = weak.upgrade() else {
                return;
            };
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => app.show_error(&format!("Could not save bookmarks: {error}")),
                Err(_) => app.show_error("The bookmark writer stopped unexpectedly"),
            }
            app.start_next_bookmark_save();
        });
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
            tab.search = self
                .state
                .global_search
                .borrow()
                .as_ref()
                .map(|(_, query)| query.clone());
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
        match (&tab.search, &tab.tag) {
            (Some(query), _) => {
                *self.state.current.borrow_mut() = tab.path.clone();
                self.show_global_search(query);
            }
            (None, Some(tag)) => {
                *self.state.current.borrow_mut() = tab.path.clone();
                self.show_tag(tag);
            }
            (None, None) => self.load(&tab.path, None),
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

    /// Replace the initial tab with a validated saved session.
    pub fn restore_session(self: &App, session: crate::session::Session) {
        let tabs: Vec<Tab> = session
            .tabs
            .into_iter()
            .map(|saved| Tab {
                path: saved.path,
                back: saved.back,
                forward: saved.forward,
                tag: saved.tag,
                search: saved.search,
            })
            .collect();
        if tabs.is_empty() {
            return;
        }

        let requested = session.active.min(tabs.len() - 1);
        let active = if tabs[requested].search.is_some()
            || tabs[requested].tag.is_some()
            || tabs[requested].path.is_dir()
        {
            requested
        } else {
            tabs.iter()
                .position(|tab| tab.search.is_some() || tab.tag.is_some() || tab.path.is_dir())
                .unwrap_or(requested)
        };
        *self.state.tabs.borrow_mut() = tabs;
        self.state.active_tab.set(active);

        let tab = self.state.tabs.borrow()[active].clone();
        *self.state.back.borrow_mut() = tab.back;
        *self.state.forward.borrow_mut() = tab.forward;
        match (tab.search, tab.tag) {
            (Some(query), _) => self.show_global_search(&query),
            (None, Some(tag)) if crate::tags::current().get(&tag).is_some() => {
                self.show_tag(&tag)
            }
            _ if tab.path.is_dir() => self.load(&tab.path, None),
            _ => {
                let fallback = crate::theme::home_dir().unwrap_or_else(|| PathBuf::from("/"));
                self.load(&fallback, None);
                self.set_message("The last location is unavailable; showing Home", false);
            }
        }
    }

    /// Persist the exact per-tab location and navigation history.
    pub fn save_session(&self) -> Result<(), String> {
        self.sync_active_tab();
        let tabs = self
            .state
            .tabs
            .borrow()
            .iter()
            .map(|tab| crate::session::SavedTab {
                path: tab.path.clone(),
                tag: tab.tag.clone(),
                search: tab.search.clone(),
                back: tab.back.clone(),
                forward: tab.forward.clone(),
            })
            .collect();
        crate::session::save(&crate::session::Session {
            active: self.state.active_tab.get(),
            tabs,
        })
    }

    pub fn disconnect_desktop_handlers(&self) {
        self.cancel_global_search();
        for (settings, handler) in self.state.desktop_handlers.borrow_mut().drain(..) {
            settings.disconnect(handler);
        }
        for handler in self.state.volume_handlers.borrow_mut().drain(..) {
            self.state.volume_monitor.disconnect(handler);
        }
    }

    // ----------------------------------------------------------------- theme ----

    /// Apply a new configuration, touching only the runtime areas that changed.
    pub fn apply_config(self: &App, config: Config, persist: bool) {
        let previous = self.config.borrow().clone();
        if config == previous {
            return;
        }

        let mut previous_layout_without_icon = previous.layout.clone();
        previous_layout_without_icon.grid_icon_size = config.layout.grid_icon_size;
        let layout_changed = previous_layout_without_icon != config.layout;
        let external_icon_change = config.layout.grid_icon_size != previous.layout.grid_icon_size
            && config.layout.grid_icon_size != Some(self.state.icon_size.get());
        let appearance_changed = config.mode != previous.mode
            || config.accent != previous.accent
            || config.colors != previous.colors
            || layout_changed
            || external_icon_change;

        crate::config::set_current(config.clone());
        *self.config.borrow_mut() = config.clone();

        if persist {
            self.queue_config_save(config.clone());
        }

        if !appearance_changed {
            return;
        }

        self.queue_theme_apply();
    }

    /// Collapse slider movement and clustered desktop notifications into one style
    /// application. GTK CSS replacement is main-thread work, so doing it once per
    /// pointer event makes an otherwise cheap slider appear frozen.
    pub(crate) fn queue_theme_apply(self: &App) {
        if let Some(source) = self.state.theme_apply_source.replace(None) {
            source.remove();
        }
        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_local_once(
            std::time::Duration::from_millis(75),
            move || {
                let Some(app) = weak.upgrade() else {
                    return;
                };
                app.state.theme_apply_source.set(None);
                let config = app.config.borrow().clone();
                app.apply_resolved_theme(&config);
            },
        );
        self.state.theme_apply_source.set(Some(source));
    }

    pub(crate) fn apply_resolved_theme(self: &App, config: &Config) {
        if let Some(source) = self.state.theme_apply_source.replace(None) {
            source.remove();
        }
        let theme = ThemeConfig::resolve(config);
        let icon_size = theme.grid_icon_size();
        crate::style::apply(&theme);
        crate::command::style_terminal(&self.widgets.console.terminal, &theme);
        *self.theme.borrow_mut() = theme;
        if icon_size != self.state.icon_size.get() {
            self.state.icon_size.set(icon_size);
            self.state.updating.set(true);
            self.widgets.zoom.set_value(f64::from(icon_size));
            self.state.updating.set(false);
            self.widgets.zoom_value.set_text(&format!("{icon_size} px"));
            self.queue_icon_refresh();
        }
    }

    /// Keep only the newest requested configuration and write it away from GTK.
    fn queue_config_save(self: &App, config: Config) {
        *self.state.pending_config_save.borrow_mut() = Some(config);
        if self.state.config_save_running.replace(true) {
            return;
        }
        self.start_next_config_save();
    }

    fn start_next_config_save(self: &App) {
        let Some(config) = self.state.pending_config_save.borrow_mut().take() else {
            self.state.config_save_running.set(false);
            return;
        };

        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || config.save()).await;
            let Some(app) = weak.upgrade() else {
                return;
            };
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => app.show_error(&format!("Could not save settings: {error}")),
                Err(_) => app.show_error("The settings writer stopped unexpectedly"),
            }
            app.start_next_config_save();
        });
    }

    /// Push the configuration's file preferences into the live browsing state.
    pub fn apply_preferences(self: &App) {
        let config = self.config.borrow().clone();

        let previous_hidden = self.state.show_hidden.get();
        let previous_sorting = self.state.sorting.get();
        let previous_view = self.state.view_mode.get();
        let next_sorting = Sorting {
            key: config.sort,
            descending: config.descending,
            folders_first: config.folders_first,
        };
        let next_view = match config.view {
            crate::config::ViewPreference::Grid => ViewMode::Grid,
            crate::config::ViewPreference::List => ViewMode::List,
        };

        self.state.show_hidden.set(config.show_hidden);
        self.state.sorting.set(next_sorting);

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

        if previous_view != next_view {
            self.set_view_mode(next_view);
        }
        if previous_hidden != config.show_hidden || previous_sorting != next_sorting {
            self.apply_filter();
        }
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
        self.queue_icon_refresh();

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
                app.apply_config(config, true);
            });
        self.state.icon_size_save.set(Some(source));
    }

    fn queue_icon_refresh(self: &App) {
        if self.state.icon_refresh_queued.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(
            std::time::Duration::from_millis(16),
            move || {
                let Some(app) = weak.upgrade() else {
                    return;
                };
                app.state.icon_refresh_queued.set(false);
                fileview::refresh_grid_factory(&app);
            },
        );
    }

    pub fn set_view_mode(self: &App, mode: ViewMode) {
        self.state.view_mode.set(mode);
        self.widgets
            .view_stack
            .set_visible_child_name(mode.stack_name());
    }

    /// Persist the live file controls through the same configuration model Settings
    /// uses. Callers invoke this after changing toolbar or shortcut state.
    pub fn persist_file_preferences(self: &App) {
        let sorting = self.state.sorting.get();
        let mut config = self.config.borrow().clone();
        config.show_hidden = self.state.show_hidden.get();
        config.folders_first = sorting.folders_first;
        config.sort = sorting.key;
        config.descending = sorting.descending;
        config.view = match self.state.view_mode.get() {
            ViewMode::Grid => crate::config::ViewPreference::Grid,
            ViewMode::List => crate::config::ViewPreference::List,
        };
        self.apply_config(config, true);
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

        let searched = Location::Search {
            root: PathBuf::from("/home/zub"),
            query: "report".to_owned(),
        };
        assert_eq!(searched.working_directory(), None);
        assert!(!searched.accepts_new_files());
        assert!(searched.is_virtual());
        assert!(!searched.is_trash());
    }

    #[test]
    fn a_tab_remembers_the_view_it_was_showing() {
        let mut tab = Tab::new(PathBuf::from("/home/zub"));
        assert_eq!(tab.tag, None);
        assert_eq!(tab.search, None);
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
