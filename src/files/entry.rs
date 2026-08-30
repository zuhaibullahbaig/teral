//! A single directory entry, plus the `GObject` wrapper the GTK list models need.

use gtk::gio;
use gtk::glib;
use gtk::glib::subclass::prelude::*;
use std::path::{Path, PathBuf};

/// Plain data describing one filesystem entry.
///
/// Names are kept twice on purpose: `path` carries the real, possibly non-UTF-8 name for
/// filesystem calls, while `display_name` is the UTF-8 text GIO guarantees for display.
#[derive(Debug, Clone)]
pub struct EntryData {
    pub path: PathBuf,
    pub display_name: String,
    pub sort_key: String,
    pub is_directory: bool,
    pub is_symlink: bool,
    pub is_hidden: bool,
    pub symlink_target: Option<PathBuf>,
    pub size: u64,
    pub content_type: Option<String>,
    pub kind: String,
    pub icon: Option<gio::Icon>,
    pub modified: Option<glib::DateTime>,
    pub created: Option<glib::DateTime>,
    pub accessed: Option<glib::DateTime>,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub mode: Option<u32>,
}

impl EntryData {
    /// Build an entry from a GIO `FileInfo` produced by a directory enumeration.
    pub fn from_info(parent: &Path, info: &gio::FileInfo) -> Self {
        let name = info.name();
        let path = parent.join(&name);
        let display_name = info.display_name().to_string();
        let file_type = info.file_type();
        let is_symlink = info.is_symlink() || file_type == gio::FileType::SymbolicLink;

        let content_type = info.content_type().map(|value| value.to_string());
        let is_directory = file_type == gio::FileType::Directory
            || (is_symlink && content_type.as_deref() == Some("inode/directory"));

        let kind = if is_directory {
            "Folder".to_owned()
        } else {
            content_type
                .as_deref()
                .map(|value| gio::content_type_get_description(value).to_string())
                .unwrap_or_else(|| "Unknown".to_owned())
        };

        Self {
            sort_key: display_name.to_lowercase(),
            display_name,
            path,
            is_directory,
            is_symlink,
            is_hidden: info.is_hidden() || info.is_backup(),
            symlink_target: info
                .has_attribute(gio::FILE_ATTRIBUTE_STANDARD_SYMLINK_TARGET)
                .then(|| info.symlink_target())
                .flatten(),
            size: u64::try_from(info.size()).unwrap_or(0),
            content_type,
            kind,
            icon: info.icon(),
            modified: info.modification_date_time(),
            created: attribute_time(info, gio::FILE_ATTRIBUTE_TIME_CREATED),
            accessed: attribute_time(info, gio::FILE_ATTRIBUTE_TIME_ACCESS),
            owner: info
                .attribute_string(gio::FILE_ATTRIBUTE_OWNER_USER)
                .map(|value| value.to_string()),
            group: info
                .attribute_string(gio::FILE_ATTRIBUTE_OWNER_GROUP)
                .map(|value| value.to_string()),
            mode: info
                .has_attribute(gio::FILE_ATTRIBUTE_UNIX_MODE)
                .then(|| info.attribute_uint32(gio::FILE_ATTRIBUTE_UNIX_MODE) & 0o7777),
        }
    }

    /// True when Teral can render a real thumbnail for this entry.
    pub fn is_thumbnailable(&self) -> bool {
        !self.is_directory
            && self.size > 0
            && self.size <= MAX_THUMBNAIL_BYTES
            && self
                .content_type
                .as_deref()
                .is_some_and(|value| value.starts_with("image/"))
    }
}

/// Read a `time::*` attribute as a local timestamp, if the filesystem reports one.
fn attribute_time(info: &gio::FileInfo, attribute: &str) -> Option<glib::DateTime> {
    if !info.has_attribute(attribute) {
        return None;
    }
    let seconds = i64::try_from(info.attribute_uint64(attribute)).ok()?;
    glib::DateTime::from_unix_local(seconds).ok()
}

/// Images larger than this are shown with their MIME icon instead of a thumbnail.
pub const MAX_THUMBNAIL_BYTES: u64 = 32 * 1024 * 1024;

mod imp {
    use super::EntryData;
    use gtk::gdk;
    use gtk::glib;
    use gtk::glib::prelude::*;
    use gtk::glib::subclass::prelude::*;
    use std::cell::{Cell, OnceCell, RefCell};

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::FileEntry)]
    pub struct FileEntry {
        pub data: OnceCell<EntryData>,
        /// Number of children for directories, or `-1` while it is still unknown.
        #[property(get, set)]
        pub child_count: Cell<i64>,
        #[property(get, set, nullable)]
        pub thumbnail: RefCell<Option<gdk::Texture>>,
        /// Set once a thumbnail attempt has finished, successfully or not.
        pub thumbnail_attempted: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FileEntry {
        const NAME: &'static str = "TeralFileEntry";
        type Type = super::FileEntry;
    }

    #[glib::derived_properties]
    impl ObjectImpl for FileEntry {}
}

glib::wrapper! {
    /// GObject wrapper around [`EntryData`] so entries can live in a `gio::ListStore`.
    pub struct FileEntry(ObjectSubclass<imp::FileEntry>);
}

impl FileEntry {
    pub fn new(data: EntryData) -> Self {
        let entry: Self = glib::Object::new();
        let imp = imp::FileEntry::from_obj(&entry);
        imp.child_count.set(-1);
        imp.data
            .set(data)
            .expect("a FileEntry is only initialised once");
        entry
    }

    pub fn data(&self) -> &EntryData {
        imp::FileEntry::from_obj(self)
            .data
            .get()
            .expect("a FileEntry always carries its data")
    }

    pub fn path(&self) -> &Path {
        &self.data().path
    }

    pub fn display_name(&self) -> &str {
        &self.data().display_name
    }

    pub fn is_directory(&self) -> bool {
        self.data().is_directory
    }

    pub fn file(&self) -> gio::File {
        gio::File::for_path(self.path())
    }

    /// Whether a thumbnail request has already been resolved for this entry.
    pub fn thumbnail_attempted(&self) -> bool {
        imp::FileEntry::from_obj(self).thumbnail_attempted.get()
    }

    pub fn mark_thumbnail_attempted(&self) {
        imp::FileEntry::from_obj(self).thumbnail_attempted.set(true);
    }

    /// The subtitle shown under the name in the grid.
    pub fn subtitle(&self) -> String {
        if self.is_directory() {
            let count = self.child_count();
            if count < 0 {
                String::new()
            } else {
                super::item_count_label(usize::try_from(count).unwrap_or(0))
            }
        } else {
            super::format_size(self.data().size)
        }
    }
}
