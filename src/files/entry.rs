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
    /// A symlink whose target does not exist. It is still listed, still selectable, and
    /// still deletable; only following it is refused.
    pub is_broken_symlink: bool,
    /// A FIFO, socket, block or character device. Opening one can block forever, so
    /// these are described but never launched.
    pub is_special: bool,
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
        let resolved = Resolution::of(&path, is_symlink, file_type);

        let kind = if resolved.is_directory {
            "Folder".to_owned()
        } else if resolved.is_broken_symlink {
            "Broken link".to_owned()
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
            is_directory: resolved.is_directory,
            is_symlink,
            is_broken_symlink: resolved.is_broken_symlink,
            is_special: resolved.is_special,
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

    /// True when opening this entry with an application makes sense.
    ///
    /// A broken link has nothing to open, and a FIFO or device can block the process
    /// that opens it indefinitely, so neither is handed to another application.
    pub fn is_openable(&self) -> bool {
        !self.is_broken_symlink && !self.is_special
    }

    /// What a symlink points at, and whether that target is still there.
    pub fn link_summary(&self) -> Option<String> {
        let target = self.symlink_target.as_ref()?;
        let target = target.to_string_lossy();
        Some(if self.is_broken_symlink {
            format!("{target} (missing)")
        } else {
            target.into_owned()
        })
    }

    /// True when Teral can render a real thumbnail for this entry.
    pub fn is_thumbnailable(&self) -> bool {
        !self.is_directory
            && !self.is_special
            && !self.is_broken_symlink
            && self.size > 0
            && self.size <= MAX_THUMBNAIL_BYTES
            && self
                .content_type
                .as_deref()
                .is_some_and(|value| value.starts_with("image/"))
    }
}

/// What an entry turns out to be once a symlink has been followed.
///
/// A directory listing is read with `NOFOLLOW_SYMLINKS`, which is what keeps the entry's
/// own name, size and timestamps honest — but it also means every symlink reports the
/// content type `inode/symlink`, so a link to a folder would never be recognised as one.
/// Following the link once, here, is what makes it navigable, and a failure to follow it
/// is exactly what identifies a broken link. Only symlinks pay for the extra look.
struct Resolution {
    is_directory: bool,
    is_broken_symlink: bool,
    is_special: bool,
}

impl Resolution {
    fn of(path: &Path, is_symlink: bool, file_type: gio::FileType) -> Self {
        if !is_symlink {
            return Self {
                is_directory: file_type == gio::FileType::Directory,
                is_broken_symlink: false,
                is_special: file_type == gio::FileType::Special,
            };
        }

        match std::fs::metadata(path) {
            Ok(target) => Self {
                is_directory: target.is_dir(),
                is_broken_symlink: false,
                is_special: !target.is_dir() && !target.is_file(),
            },
            // The target is gone, or a link loop, or unreadable. Either way it must not
            // be presented as something that can be entered or opened.
            Err(_) => Self {
                is_directory: false,
                is_broken_symlink: true,
                is_special: false,
            },
        }
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

    pub fn is_openable(&self) -> bool {
        self.data().is_openable()
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

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::gio::prelude::*;

    /// Enumerate a directory the way the file view does, and find one entry by name.
    fn entry(dir: &Path, name: &str) -> EntryData {
        let info = gio::File::for_path(dir.join(name))
            .query_info(
                "standard::name,standard::display-name,standard::type,standard::size,\
standard::content-type,standard::is-symlink,standard::symlink-target",
                gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                gio::Cancellable::NONE,
            )
            .expect("query");
        EntryData::from_info(dir, &info)
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("teral-entry-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    #[test]
    fn a_symlink_to_a_directory_is_a_directory() {
        let dir = scratch("symlink-dir");
        std::fs::create_dir(dir.join("real")).unwrap();
        std::os::unix::fs::symlink(dir.join("real"), dir.join("link")).unwrap();

        // Listings are read without following symlinks, which reports every link as
        // "inode/symlink". Without resolving it, a link to a folder is not enterable.
        let link = entry(&dir, "link");
        assert!(link.is_symlink);
        assert!(link.is_directory, "a link to a folder must be enterable");
        assert!(!link.is_broken_symlink);
        assert!(link.is_openable());
        assert_eq!(link.kind, "Folder");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_symlink_to_a_file_is_not_a_directory() {
        let dir = scratch("symlink-file");
        std::fs::write(dir.join("real.txt"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.join("real.txt"), dir.join("link")).unwrap();

        let link = entry(&dir, "link");
        assert!(link.is_symlink);
        assert!(!link.is_directory);
        assert!(!link.is_broken_symlink);
        assert!(link.is_openable());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_broken_symlink_is_listed_but_never_followed() {
        let dir = scratch("broken");
        std::os::unix::fs::symlink(dir.join("nowhere"), dir.join("broken")).unwrap();

        let link = entry(&dir, "broken");
        assert!(link.is_symlink);
        assert!(link.is_broken_symlink);
        assert!(!link.is_directory, "a broken link must not be enterable");
        assert!(!link.is_openable(), "there is nothing behind it to open");
        assert!(!link.is_thumbnailable());
        assert_eq!(link.kind, "Broken link");
        // It still says what it pointed at, and that the target is gone.
        let summary = link.link_summary().expect("summary");
        assert!(summary.contains("nowhere"), "{summary}");
        assert!(summary.contains("missing"), "{summary}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_link_pointing_at_itself_is_treated_as_broken() {
        let dir = scratch("loop");
        std::os::unix::fs::symlink(dir.join("loop"), dir.join("loop")).unwrap();

        // Resolving this fails with ELOOP rather than "not found"; either way it must
        // not be presented as something that can be entered.
        let link = entry(&dir, "loop");
        assert!(link.is_broken_symlink);
        assert!(!link.is_directory);
        assert!(!link.is_openable());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_fifo_is_described_but_never_opened() {
        let dir = scratch("fifo");
        let made = std::process::Command::new("mkfifo")
            .arg(dir.join("pipe"))
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !made {
            // mkfifo is not present; nothing to assert about a FIFO that was not made.
            std::fs::remove_dir_all(dir).unwrap();
            return;
        }

        let pipe = entry(&dir, "pipe");
        assert!(pipe.is_special, "a FIFO is a special entry");
        assert!(!pipe.is_directory);
        assert!(
            !pipe.is_openable(),
            "opening a FIFO blocks until the other end is opened"
        );
        assert!(!pipe.is_thumbnailable());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_ordinary_file_and_folder_are_unaffected() {
        let dir = scratch("plain");
        std::fs::write(dir.join("notes.txt"), b"payload").unwrap();
        std::fs::create_dir(dir.join("folder")).unwrap();

        let file = entry(&dir, "notes.txt");
        assert!(!file.is_symlink && !file.is_broken_symlink && !file.is_special);
        assert!(file.is_openable());
        assert_eq!(file.link_summary(), None);

        let folder = entry(&dir, "folder");
        assert!(folder.is_directory);
        assert!(folder.is_openable());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
