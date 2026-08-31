//! Asynchronous directory enumeration and sorting.
//!
//! Enumeration goes through GIO's asynchronous file APIs, so a slow or unresponsive
//! mount never blocks the GTK main loop.

use super::entry::EntryData;
use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;
use std::path::Path;

/// Attributes Teral needs for a directory listing and for the details panel.
const ATTRIBUTES: &str = "standard::name,standard::display-name,standard::type,\
standard::size,standard::content-type,standard::icon,standard::is-hidden,\
standard::is-backup,standard::is-symlink,standard::symlink-target,\
time::modified,time::created,time::access,owner::user,owner::group,unix::mode";

/// Entries fetched per asynchronous batch.
const BATCH: i32 = 256;

/// How a directory listing is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Size,
    Kind,
    Modified,
}

impl SortKey {
    pub const ALL: [Self; 4] = [Self::Name, Self::Size, Self::Kind, Self::Modified];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Size => "Size",
            Self::Kind => "Type",
            Self::Modified => "Modified",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Sorting {
    pub key: SortKey,
    pub descending: bool,
    pub folders_first: bool,
}

impl Default for Sorting {
    fn default() -> Self {
        Self {
            key: SortKey::Name,
            descending: false,
            folders_first: true,
        }
    }
}

/// Enumerate a directory without blocking the main loop.
pub async fn scan_directory(path: &Path) -> Result<Vec<EntryData>, glib::Error> {
    let directory = gio::File::for_path(path);
    let enumerator = directory
        .enumerate_children_future(
            ATTRIBUTES,
            gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
            glib::Priority::DEFAULT,
        )
        .await?;

    let mut entries = Vec::new();
    loop {
        let batch = enumerator
            .next_files_future(BATCH, glib::Priority::DEFAULT)
            .await?;
        if batch.is_empty() {
            break;
        }
        entries.extend(batch.iter().map(|info| EntryData::from_info(path, info)));
    }

    Ok(entries)
}

/// Read metadata for an explicit list of files, for views that are not a directory.
pub async fn scan_paths(paths: &[std::path::PathBuf]) -> Vec<EntryData> {
    let mut entries = Vec::with_capacity(paths.len());

    for path in paths {
        let Some(parent) = path.parent() else {
            continue;
        };
        let file = gio::File::for_path(path);
        let info = file
            .query_info_future(
                ATTRIBUTES,
                gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                glib::Priority::DEFAULT,
            )
            .await;

        // A tagged file that has since been deleted is simply left out.
        if let Ok(info) = info {
            entries.push(EntryData::from_info(parent, &info));
        }
    }

    entries
}

/// Count the children of a directory, used for the item count under folder tiles.
pub async fn count_children(path: &Path) -> Result<usize, glib::Error> {
    let directory = gio::File::for_path(path);
    let enumerator = directory
        .enumerate_children_future(
            gio::FILE_ATTRIBUTE_STANDARD_NAME,
            gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
            glib::Priority::LOW,
        )
        .await?;

    let mut count = 0usize;
    loop {
        let batch = enumerator
            .next_files_future(BATCH, glib::Priority::LOW)
            .await?;
        if batch.is_empty() {
            break;
        }
        count += batch.len();
    }

    Ok(count)
}

/// Free and total bytes of the filesystem holding `path`.
pub async fn filesystem_usage(path: &Path) -> Option<(u64, u64)> {
    let info = gio::File::for_path(path)
        .query_filesystem_info_future("filesystem::size,filesystem::free", glib::Priority::LOW)
        .await
        .ok()?;

    let size = info.attribute_uint64(gio::FILE_ATTRIBUTE_FILESYSTEM_SIZE);
    let free = info.attribute_uint64(gio::FILE_ATTRIBUTE_FILESYSTEM_FREE);
    (size > 0).then_some((free, size))
}

/// Order entries in place according to `sorting`.
pub fn sort(entries: &mut [EntryData], sorting: Sorting) {
    entries.sort_by(|left, right| {
        if sorting.folders_first && left.is_directory != right.is_directory {
            return right.is_directory.cmp(&left.is_directory);
        }

        let ordering = match sorting.key {
            SortKey::Name => left.sort_key.cmp(&right.sort_key),
            SortKey::Size => left.size.cmp(&right.size),
            SortKey::Kind => left.kind.to_lowercase().cmp(&right.kind.to_lowercase()),
            SortKey::Modified => timestamp(left).cmp(&timestamp(right)),
        };

        let ordering = if sorting.descending {
            ordering.reverse()
        } else {
            ordering
        };

        ordering.then_with(|| left.sort_key.cmp(&right.sort_key))
    });
}

fn timestamp(entry: &EntryData) -> i64 {
    entry
        .modified
        .as_ref()
        .map(glib::DateTime::to_unix)
        .unwrap_or(i64::MIN)
}

/// True when `entry` should be visible for the current filter settings.
pub fn matches(entry: &EntryData, show_hidden: bool, query: &str) -> bool {
    if entry.is_hidden && !show_hidden {
        return false;
    }

    if query.is_empty() {
        return true;
    }

    entry.display_name.to_lowercase().contains(query)
}
