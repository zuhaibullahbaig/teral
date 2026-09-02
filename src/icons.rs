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
use std::cell::{Cell, RefCell};
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
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

/// Candidate icon names for each control Teral draws, most preferred first.
///
/// Icon themes differ in what they ship, so Teral asks the active theme which of these
/// it actually has instead of assuming one name exists everywhere.
pub mod names {
    pub const BACK: &[&str] = &["go-previous-symbolic"];
    pub const FORWARD: &[&str] = &["go-next-symbolic"];
    pub const UP: &[&str] = &["go-up-symbolic"];
    pub const DOWN: &[&str] = &["go-down-symbolic"];
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
    pub const NEW_FILE: &[&str] = &["document-new-symbolic", "list-add-symbolic"];
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
    pub const EXTRACT: &[&str] = &[
        "archive-extract-symbolic",
        "folder-download-symbolic",
        "document-save-symbolic",
    ];
    pub const SELECT_ALL: &[&str] = &["edit-select-all-symbolic"];
    pub const TAG: &[&str] = &["tag-symbolic", "bookmark-new-symbolic"];
    pub const WINDOW: &[&str] = &[
        "window-new-symbolic",
        "view-dual-symbolic",
        "list-add-symbolic",
    ];
    pub const ZOOM_OUT: &[&str] = &["zoom-out-symbolic", "list-remove-symbolic"];
    pub const ZOOM_IN: &[&str] = &["zoom-in-symbolic", "list-add-symbolic"];
    pub const EXECUTABLE: &[&str] = &["application-x-executable-symbolic", "system-run-symbolic"];
    pub const PERMISSIONS: &[&str] = &["changes-prevent-symbolic", "dialog-password-symbolic"];
    pub const HELP: &[&str] = &["help-browser-symbolic", "dialog-question-symbolic"];
    pub const ABOUT: &[&str] = &["help-about-symbolic", "dialog-information-symbolic"];
    pub const DELETE: &[&str] = &["edit-delete-symbolic", "user-trash-symbolic"];
    pub const PANEL: &[&str] = &[
        "sidebar-show-right-symbolic",
        "view-dual-symbolic",
        "view-paged-symbolic",
        "edit-select-all-symbolic",
    ];
    pub const SIDEBAR: &[&str] = &[
        "sidebar-show-left-symbolic",
        "view-dual-symbolic",
        "open-menu-symbolic",
    ];
    pub const RESTORE: &[&str] = &[
        "edit-undo-symbolic",
        "document-revert-symbolic",
        "go-up-symbolic",
    ];
    pub const COMPRESS: &[&str] = &[
        "package-x-generic-symbolic",
        "folder-download-symbolic",
        "document-save-symbolic",
    ];
}

/// The window icon Teral should wear on this desktop.
///
/// An installed Teral has an icon of its own, and that always wins. Running from a build
/// directory it has none, and a window with no icon shows the desktop's blank
/// placeholder — so rather than invent artwork, Teral asks the desktop which application
/// it opens folders with and borrows that icon, then falls back to the standard names.
pub fn file_manager_icon_name() -> Option<String> {
    if theme_has_icon(crate::APP_ID) {
        return Some(crate::APP_ID.to_owned());
    }

    let handler = gio::AppInfo::default_for_type("inode/directory", false)
        .and_then(|application| application.icon())
        .and_downcast::<gio::ThemedIcon>()
        .map(|icon| icon.names())
        .unwrap_or_default();

    handler
        .iter()
        .map(glib::GString::as_str)
        .chain(["system-file-manager", "folder"])
        .find(|name| theme_has_icon(name))
        .map(str::to_owned)
}

/// True when the active icon theme can actually draw `name`.
fn theme_has_icon(name: &str) -> bool {
    let Some(display) = gdk::Display::default() else {
        return false;
    };
    gtk::IconTheme::for_display(&display).has_icon(name)
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

/// Forget icon-name and thumbnail results after the desktop icon theme changes.
pub fn clear_caches() {
    RESOLVED.with_borrow_mut(HashMap::clear);
    CACHE.with_borrow_mut(HashMap::clear);
    QUEUE.with_borrow_mut(VecDeque::clear);
}

/// Cancel queued thumbnail work and make completions from the old location harmless.
pub fn cancel_pending() {
    GENERATION.set(GENERATION.get().wrapping_add(1));
    QUEUE.with_borrow_mut(VecDeque::clear);
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
        let generation = GENERATION.get();
        glib::spawn_future_local(async move {
            load(entry, generation).await;
            IN_FLIGHT.with_borrow_mut(|count| *count = count.saturating_sub(1));
            pump();
        });
    }
}

async fn load(entry: FileEntry, generation: u64) {
    struct DecodedThumbnail {
        pixels: Vec<u8>,
        width: i32,
        height: i32,
        rowstride: i32,
        has_alpha: bool,
    }

    let path = entry.path().to_path_buf();
    let Ok(decoded) = gio::spawn_blocking(move || {
        let pixbuf = gdk::gdk_pixbuf::Pixbuf::from_file_at_scale(
            path,
            THUMBNAIL_EDGE,
            THUMBNAIL_EDGE,
            true,
        )?;
        if pixbuf.width() > THUMBNAIL_EDGE
            || pixbuf.height() > THUMBNAIL_EDGE
            || pixbuf.byte_length() > 4 * 1024 * 1024
        {
            return Ok::<_, glib::Error>(None);
        }
        Ok(Some(DecodedThumbnail {
            pixels: pixbuf.read_pixel_bytes().as_ref().to_vec(),
            width: pixbuf.width(),
            height: pixbuf.height(),
            rowstride: pixbuf.rowstride(),
            has_alpha: pixbuf.has_alpha(),
        }))
    })
    .await
    else {
        return;
    };
    let Ok(Some(decoded)) = decoded else {
        return;
    };
    if GENERATION.get() != generation {
        return;
    }

    let bytes = glib::Bytes::from_owned(decoded.pixels);
    let pixbuf = gdk::gdk_pixbuf::Pixbuf::from_bytes(
        &bytes,
        gdk::gdk_pixbuf::Colorspace::Rgb,
        decoded.has_alpha,
        8,
        decoded.width,
        decoded.height,
        decoded.rowstride,
    );
    let texture = gdk::Texture::for_pixbuf(&pixbuf);
    if let Some(key) = cache_key(&entry) {
        CACHE.with_borrow_mut(|cache| {
            // A simple bound: file managers walk folders, they do not need history.
            if cache.len() >= 256 {
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
