//! Icon and thumbnail resolution.
//!
//! Icons come from the active system icon theme through GIO, so changing the desktop's
//! icon theme changes Teral. Thumbnails are only generated for real image files: files
//! Teral cannot thumbnail keep their MIME icon rather than showing an invented preview.

use crate::files::FileEntry;
use gtk::gdk;
use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

/// Longest edge, in pixels, of a generated thumbnail.
const THUMBNAIL_EDGE: i32 = 192;

/// How many thumbnails may be loading at once.
const MAX_IN_FLIGHT: usize = 3;

/// Cache keyed by path and modification time so edited images refresh naturally.
type CacheKey = (PathBuf, i64);

thread_local! {
    static CACHE: RefCell<HashMap<CacheKey, gdk::Texture>> = RefCell::new(HashMap::new());
    static RESOLVED: RefCell<HashMap<&'static str, &'static str>> =
        RefCell::new(HashMap::new());
    static QUEUE: RefCell<VecDeque<FileEntry>> = const { RefCell::new(VecDeque::new()) };
    static IN_FLIGHT: RefCell<usize> = const { RefCell::new(0) };
}

/// Candidate icon names for each control Teral draws, most preferred first.
///
/// Icon themes differ in what they ship, so Teral asks the active theme which of these
/// it actually has instead of assuming one name exists everywhere.
pub mod names {
    pub const BACK: &[&str] = &["go-previous-symbolic"];
    pub const FORWARD: &[&str] = &["go-next-symbolic"];
    pub const UP: &[&str] = &["go-up-symbolic"];
    pub const SEARCH: &[&str] = &["system-search-symbolic", "edit-find-symbolic"];
    pub const GRID: &[&str] = &["view-grid-symbolic", "view-app-grid-symbolic"];
    pub const LIST: &[&str] = &["view-list-symbolic"];
    pub const SORT: &[&str] = &[
        "view-sort-descending-symbolic",
        "view-list-ordered-symbolic",
        "emblem-system-symbolic",
    ];
    pub const MENU: &[&str] = &["view-more-symbolic", "open-menu-symbolic"];
    pub const NEW_FOLDER: &[&str] = &["folder-new-symbolic", "list-add-symbolic"];
    pub const ADD: &[&str] = &["list-add-symbolic"];
    pub const PASTE: &[&str] = &["edit-paste-symbolic"];
    pub const COPY: &[&str] = &["edit-copy-symbolic"];
    pub const CUT: &[&str] = &["edit-cut-symbolic"];
    pub const TRASH: &[&str] = &["user-trash-symbolic"];
    pub const REFRESH: &[&str] = &["view-refresh-symbolic"];
    pub const RENAME: &[&str] = &[
        "document-edit-symbolic",
        "text-editor-symbolic",
        "document-properties-symbolic",
        "document-save-symbolic",
    ];
    pub const COPY_PATH: &[&str] = &["insert-link-symbolic", "edit-copy-symbolic"];
    pub const TERMINAL: &[&str] = &[
        "utilities-terminal-symbolic",
        "text-x-script-symbolic",
        "system-run-symbolic",
    ];
    pub const PIN: &[&str] = &[
        "view-pin-symbolic",
        "starred-symbolic",
        "bookmark-new-symbolic",
    ];
    pub const OPEN: &[&str] = &["document-open-symbolic"];
    pub const OPEN_FOLDER: &[&str] = &["folder-open-symbolic", "folder-symbolic"];
    pub const OPEN_WITH: &[&str] = &["application-x-executable-symbolic"];
    pub const SELECTED: &[&str] = &["object-select-symbolic", "emblem-ok-symbolic"];
    pub const STOP: &[&str] = &["process-stop-symbolic", "window-close-symbolic"];
    pub const CLOSE: &[&str] = &["window-close-symbolic"];
    pub const DRIVE: &[&str] = &["drive-harddisk-symbolic"];
    pub const SETTINGS: &[&str] = &[
        "emblem-system-symbolic",
        "preferences-system-symbolic",
        "applications-system-symbolic",
    ];
    pub const HELP: &[&str] = &["help-browser-symbolic", "dialog-question-symbolic"];
    pub const ABOUT: &[&str] = &["help-about-symbolic", "dialog-information-symbolic"];
    pub const DELETE: &[&str] = &["edit-delete-symbolic", "user-trash-symbolic"];
    pub const PANEL: &[&str] = &[
        "sidebar-show-right-symbolic",
        "view-dual-symbolic",
        "view-paged-symbolic",
        "edit-select-all-symbolic",
    ];
    pub const RESTORE: &[&str] = &[
        "edit-undo-symbolic",
        "document-revert-symbolic",
        "go-up-symbolic",
    ];
}

/// Resolve the first candidate the active icon theme can actually draw.
pub fn ui(candidates: &'static [&'static str]) -> &'static str {
    // The first candidate names the role and is unique, so it keys the cache.
    let key = candidates.first().copied().unwrap_or("image-missing");
    let fallback = candidates.last().copied().unwrap_or("image-missing");

    RESOLVED.with_borrow_mut(|cache| {
        if let Some(name) = cache.get(key) {
            return *name;
        }

        let resolved = resolve(candidates).unwrap_or(fallback);
        cache.insert(key, resolved);
        resolved
    })
}

fn resolve(candidates: &'static [&'static str]) -> Option<&'static str> {
    let display = gdk::Display::default()?;
    let theme = gtk::IconTheme::for_display(&display);

    candidates.iter().copied().find(|name| {
        let paintable = theme.lookup_icon(
            name,
            &[],
            16,
            1,
            gtk::TextDirection::Ltr,
            gtk::IconLookupFlags::empty(),
        );
        paintable
            .icon_name()
            .is_some_and(|resolved| resolved != std::path::Path::new("image-missing"))
    })
}

/// Fallback icon used when GIO reports nothing for an entry.
pub fn fallback_icon_name(entry: &FileEntry) -> &'static str {
    if entry.is_directory() {
        "folder-symbolic"
    } else {
        "text-x-generic-symbolic"
    }
}

/// Apply an entry's system icon to an image widget.
pub fn set_entry_icon(image: &gtk::Image, entry: &FileEntry) {
    match entry.data().icon.as_ref() {
        Some(icon) => image.set_from_gicon(icon),
        None => image.set_icon_name(Some(fallback_icon_name(entry))),
    }
}

/// Ask for a thumbnail. The entry's `thumbnail` property is set once one is ready.
pub fn request_thumbnail(entry: &FileEntry) {
    if entry.thumbnail_attempted() || !entry.data().is_thumbnailable() {
        return;
    }

    if let Some(key) = cache_key(entry) {
        let cached = CACHE.with_borrow(|cache| cache.get(&key).cloned());
        if let Some(texture) = cached {
            entry.mark_thumbnail_attempted();
            entry.set_thumbnail(Some(texture));
            return;
        }
    }

    entry.mark_thumbnail_attempted();
    QUEUE.with_borrow_mut(|queue| queue.push_back(entry.clone()));
    pump();
}

fn pump() {
    loop {
        if IN_FLIGHT.with_borrow(|count| *count >= MAX_IN_FLIGHT) {
            return;
        }

        let Some(entry) = QUEUE.with_borrow_mut(VecDeque::pop_front) else {
            return;
        };

        IN_FLIGHT.with_borrow_mut(|count| *count += 1);
        glib::spawn_future_local(async move {
            load(entry).await;
            IN_FLIGHT.with_borrow_mut(|count| *count = count.saturating_sub(1));
            pump();
        });
    }
}

async fn load(entry: FileEntry) {
    // Reading happens asynchronously; only the decode runs on the main loop, and it is
    // bounded by MAX_IN_FLIGHT so scrolling stays smooth.
    let Ok((bytes, _etag)) = entry.file().load_bytes_future().await else {
        return;
    };

    let stream = gio::MemoryInputStream::from_bytes(&bytes);
    let Ok(pixbuf) = gdk::gdk_pixbuf::Pixbuf::from_stream_at_scale(
        &stream,
        THUMBNAIL_EDGE,
        THUMBNAIL_EDGE,
        true,
        gio::Cancellable::NONE,
    ) else {
        return;
    };

    let texture = gdk::Texture::for_pixbuf(&pixbuf);
    if let Some(key) = cache_key(&entry) {
        CACHE.with_borrow_mut(|cache| {
            // A simple bound: file managers walk folders, they do not need history.
            if cache.len() > 512 {
                cache.clear();
            }
            cache.insert(key, texture.clone());
        });
    }
    entry.set_thumbnail(Some(texture));
}

fn cache_key(entry: &FileEntry) -> Option<CacheKey> {
    let modified = entry
        .data()
        .modified
        .as_ref()
        .map(glib::DateTime::to_unix)?;
    Some((entry.path().to_path_buf(), modified))
}
