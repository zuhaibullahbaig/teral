//! Presentation layer.
//!
//! `ui` owns the shared application object: browsing state, the list model every view
//! shares, and the operations the widgets in the submodules call into. The submodules
//! only build and update widgets.

pub mod details;
pub mod dialogs;
pub mod fileview;
pub mod header;
pub mod sidebar;
pub mod statusbar;
pub mod window;

use crate::command::RunningCommand;
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

pub use window::build_window;

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
    pub running_command: RefCell<Option<RunningCommand>>,
    pub running_transfer: RefCell<Option<CancelFlag>>,
    /// Guards against feedback loops while widgets are being synchronised.
    pub updating: Cell<bool>,
}

/// The application object. Widgets hold an `App` and call back into it.
pub struct AppInner {
    pub state: State,
    pub widgets: window::Widgets,
    pub theme: ThemeConfig,
}

pub type App = Rc<AppInner>;

impl AppInner {
    pub fn current_dir(&self) -> PathBuf {
        self.state.current.borrow().clone()
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

    /// Re-read the current directory, keeping history and selection intact.
    pub fn reload(self: &App) {
        let current = self.current_dir();
        self.load(&current, None);
    }

    /// Read a directory asynchronously and swap it in when it arrives.
    pub fn load(self: &App, path: &Path, history: Option<HistoryStep>) {
        let app = Rc::clone(self);
        let path = path.to_path_buf();
        let generation = self.state.generation.get().wrapping_add(1);
        self.state.generation.set(generation);

        glib::spawn_future_local(async move {
            let result = scan::scan_directory(&path).await;
            if app.state.generation.get() != generation {
                return;
            }

            match result {
                Ok(entries) => {
                    app.commit_directory(path, entries, history);
                    app.load_child_counts(generation);
                    app.update_free_space();
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
        *self.state.current.borrow_mut() = path;
        *self.state.all.borrow_mut() = entries;

        if changed_directory {
            self.state.query.borrow_mut().clear();
            self.widgets.search_entry.set_text("");
            self.widgets.search_bar.set_search_mode(false);
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

        if !previously_selected.is_empty() {
            for (index, entry) in objects.iter().enumerate() {
                if previously_selected.iter().any(|path| path == entry.path()) {
                    let index = u32::try_from(index).unwrap_or(u32::MAX);
                    self.state.selection.select_item(index, false);
                }
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

    fn update_free_space(self: &App) {
        let app = Rc::clone(self);
        let path = self.current_dir();
        glib::spawn_future_local(async move {
            let usage = scan::filesystem_usage(&path).await;
            if app.current_dir() != path {
                return;
            }
            app.widgets.status_free.set_text(&match usage {
                Some((free, _total)) => format!("Free: {}", crate::files::format_size(free)),
                None => String::new(),
            });
        });
    }

    /// Refresh everything that depends on the current directory.
    pub fn refresh_chrome(self: &App) {
        let current = self.current_dir();

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
    }

    pub fn update_counts(&self) {
        let total = self.state.all.borrow().len();
        let visible = self.state.store.n_items() as usize;

        let text = if visible == total {
            crate::files::item_count_label(total)
        } else {
            format!("{visible} of {}", crate::files::item_count_label(total))
        };
        self.widgets.folder_subtitle.set_text(&text);
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
        let size = self.theme.grid_icon_size();
        self.widgets.zoom.set_value(f64::from(size));
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
pub fn icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon_name);
    button.add_css_class("teral-icon-button");
    button.set_tooltip_text(Some(tooltip));
    button.set_has_frame(false);
    button
}

/// A flat, compact toggle button.
pub fn icon_toggle(icon_name: &str, tooltip: &str) -> gtk::ToggleButton {
    let button = gtk::ToggleButton::new();
    button.set_child(Some(&gtk::Image::from_icon_name(icon_name)));
    button.add_css_class("teral-icon-button");
    button.set_tooltip_text(Some(tooltip));
    button.set_has_frame(false);
    button
}
