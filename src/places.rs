//! Sidebar locations: XDG user directories, mounted volumes and user pins.

use crate::files::trash::TrashDir;
use crate::theme::{data_home, home_dir};
use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A navigable location shown in the sidebar.
#[derive(Debug, Clone)]
pub struct Place {
    pub label: String,
    pub icon_name: String,
    pub path: PathBuf,
}

/// A mountable volume or mounted filesystem shown in the sidebar.
#[derive(Debug, Clone)]
pub struct Device {
    pub label: String,
    pub icon: Option<gio::Icon>,
    pub path: Option<PathBuf>,
    pub volume: Option<gio::Volume>,
}

/// Standard XDG user directories that actually exist on this system.
pub fn user_places() -> Vec<Place> {
    let mut places = Vec::new();

    if let Some(home) = home_dir() {
        places.push(Place {
            label: "Home".to_owned(),
            icon_name: "user-home-symbolic".to_owned(),
            path: home,
        });
    }

    const DIRECTORIES: [(glib::UserDirectory, &str, &str); 6] = [
        (
            glib::UserDirectory::Desktop,
            "Desktop",
            "user-desktop-symbolic",
        ),
        (
            glib::UserDirectory::Documents,
            "Documents",
            "folder-documents-symbolic",
        ),
        (
            glib::UserDirectory::Downloads,
            "Downloads",
            "folder-download-symbolic",
        ),
        (
            glib::UserDirectory::Pictures,
            "Pictures",
            "folder-pictures-symbolic",
        ),
        (glib::UserDirectory::Music, "Music", "folder-music-symbolic"),
        (
            glib::UserDirectory::Videos,
            "Videos",
            "folder-videos-symbolic",
        ),
    ];

    for (directory, label, icon_name) in DIRECTORIES {
        let Some(path) = glib::user_special_dir(directory) else {
            continue;
        };
        // A user who has disabled a directory should not see a broken entry.
        if !path.is_dir() || Some(path.as_path()) == home_dir().as_deref() {
            continue;
        }
        places.push(Place {
            label: label.to_owned(),
            icon_name: icon_name.to_owned(),
            path,
        });
    }

    places.extend(trash_places());
    places
}

/// Every trash Teral can currently browse.
///
/// The home trash keeps the plain "Trash" label. A trash on another filesystem is
/// labelled by the mount it belongs to, because two entries called "Trash" would give
/// no way to tell which disk's deleted files are being looked at. A device that is
/// unplugged simply stops appearing.
pub fn trash_places() -> Vec<Place> {
    let home = crate::files::trash::home_trash(&data_home());
    let mut places = vec![Place {
        label: "Trash".to_owned(),
        icon_name: "user-trash-symbolic".to_owned(),
        path: home.files(),
    }];
    places.extend(
        crate::files::ops::trash_dirs()
            .into_iter()
            .filter(|dir| dir.root != home.root)
            .map(|dir| Place {
                label: trash_label(&dir, &home),
                icon_name: "user-trash-symbolic".to_owned(),
                path: dir.files(),
            }),
    );
    places
}

/// The label one trash location should carry in the sidebar.
///
/// The mount point comes from the trash directory itself rather than by counting path
/// components, because the two forms the specification allows —
/// `<mount>/.Trash-<uid>` and `<mount>/.Trash/<uid>` — sit at different depths.
fn trash_label(dir: &TrashDir, home: &TrashDir) -> String {
    if dir.root == home.root {
        return "Trash".to_owned();
    }
    match dir.top_dir.file_name() {
        Some(mount) => format!("Trash on {}", mount.to_string_lossy()),
        None => "Trash".to_owned(),
    }
}

/// Mountable volumes and mounted filesystems discovered through GIO, plus root.
pub fn devices() -> Vec<Device> {
    let mut devices = vec![Device {
        label: "Filesystem".to_owned(),
        icon: Some(gio::Icon::for_string("drive-harddisk-symbolic").expect("static icon name")),
        path: Some(PathBuf::from("/")),
        volume: None,
    }];

    let monitor = gio::VolumeMonitor::get();
    for volume in monitor.volumes() {
        let path = volume.get_mount().and_then(|mount| mount.root().path());
        if path.as_deref() == Some(Path::new("/")) {
            continue;
        }
        devices.push(Device {
            label: volume.name().to_string(),
            icon: Some(volume.symbolic_icon()),
            path,
            volume: Some(volume),
        });
    }

    // Some mounts, including manually-added remote locations, have no GVolume. Keep
    // those navigable without duplicating the mounts already represented above.
    for mount in monitor.mounts() {
        if mount.is_shadowed() {
            continue;
        }
        if mount.volume().is_some() {
            continue;
        }
        let Some(path) = mount.root().path() else {
            continue;
        };
        if path == Path::new("/")
            || devices
                .iter()
                .any(|device| device.path.as_deref() == Some(path.as_path()))
        {
            continue;
        }

        devices.push(Device {
            label: mount.name().to_string(),
            icon: Some(mount.symbolic_icon()),
            path: Some(path),
            volume: None,
        });
    }

    DEVICE_LABELS.with_borrow_mut(|labels| {
        labels.clear();
        labels.extend(devices.iter().filter_map(|device| {
            device
                .path
                .clone()
                .map(|path| (path, device.label.clone()))
        }));
    });
    devices
}

#[derive(Debug, Default, Deserialize)]
struct PinnedFile {
    #[serde(default)]
    pinned: Vec<String>,
    #[serde(default)]
    bookmark: Vec<RawBookmark>,
}

#[derive(Debug, Deserialize)]
struct RawBookmark {
    path_hex: String,
    label: Option<String>,
}

thread_local! {
    static BOOKMARK_LABELS: RefCell<HashMap<PathBuf, String>> = RefCell::new(HashMap::new());
    static DEVICE_LABELS: RefCell<HashMap<PathBuf, String>> = RefCell::new(HashMap::new());
}

fn pinned_path() -> PathBuf {
    data_home().join("teral/places.toml")
}

/// Load the user's pinned locations, dropping any that no longer exist.
pub fn load_pinned() -> Result<Vec<PathBuf>, String> {
    let path = pinned_path();
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("could not read {}: {error}", path.display())),
    };

    match toml::from_str::<PinnedFile>(&raw) {
        Ok(file) => {
            let mut pinned: Vec<PathBuf> = file.pinned.into_iter().map(PathBuf::from).collect();
            let mut labels = HashMap::new();
            for bookmark in file.bookmark {
                let path = crate::persistence::decode_path(&bookmark.path_hex)?;
                if !pinned.contains(&path) {
                    pinned.push(path.clone());
                }
                if let Some(label) = bookmark.label.filter(|label| !label.trim().is_empty()) {
                    labels.insert(path, label);
                }
            }
            BOOKMARK_LABELS.with_borrow_mut(|current| *current = labels);
            Ok(pinned)
        }
        Err(error) => Err(format!("could not parse {}: {error}", path.display())),
    }
}

/// Serialize bookmarks on the owning main context, where display-label overrides live.
/// The returned plain data can then be written safely on a worker.
pub fn pinned_payload(pinned: &[PathBuf]) -> (PathBuf, Vec<u8>) {
    let path = pinned_path();
    let mut document = String::from("version = 2\n");
    for pin in pinned {
        document.push_str("\n[[bookmark]]\n");
        document.push_str(&format!(
            "path_hex = \"{}\"\n",
            crate::persistence::encode_path(pin)
        ));
        if let Some(label) = bookmark_label_override(pin) {
            document.push_str(&format!("label = \"{}\"\n", escape(&label)));
        }
    }
    (path, document.into_bytes())
}

/// Complete a previously prepared bookmark write.
pub fn write_pinned_payload(path: PathBuf, document: Vec<u8>) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return Err(format!("could not create {}: {error}", parent.display()));
    }

    crate::persistence::atomic_write(&path, &document)
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

pub fn bookmark_label(path: &Path) -> String {
    bookmark_label_override(path).unwrap_or_else(|| display_label(path))
}

pub fn set_bookmark_label(path: &Path, label: Option<String>) {
    BOOKMARK_LABELS.with_borrow_mut(|labels| match label.filter(|label| !label.trim().is_empty()) {
        Some(label) => {
            labels.insert(path.to_path_buf(), label);
        }
        None => {
            labels.remove(path);
        }
    });
}

fn bookmark_label_override(path: &Path) -> Option<String> {
    BOOKMARK_LABELS.with_borrow(|labels| labels.get(path).cloned())
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// A short, friendly label for a directory shown in the sidebar or the title.
pub fn display_label(path: &Path) -> String {
    if path == Path::new("/") {
        return "Filesystem".to_owned();
    }

    if Some(path) == home_dir().as_deref() {
        return "Home".to_owned();
    }

    if let Some(label) = DEVICE_LABELS.with_borrow(|labels| labels.get(path).cloned()) {
        return label;
    }

    // Every browsable trash path ends in "files". Checking that first keeps the
    // volume-monitor lookup out of ordinary breadcrumb and title rendering.
    if path.file_name() == Some(std::ffi::OsStr::new("files")) {
        let home = crate::files::trash::home_trash(&data_home());
        if home.files() == path {
            return "Trash".to_owned();
        }
        if let Some(dir) = crate::files::ops::trash_dirs()
            .iter()
            .find(|dir| dir.files() == path)
        {
            return trash_label(dir, &home);
        }
    }

    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secondary_trash_is_labelled_by_the_disk_it_belongs_to() {
        let home = TrashDir::new(
            PathBuf::from("/home/zub/.local/share/Trash"),
            PathBuf::from("/"),
        );
        assert_eq!(trash_label(&home, &home), "Trash");

        // The two forms the specification allows sit at different depths, and both must
        // be named after the disk rather than after a directory inside the path.
        let unshared = TrashDir::new(
            PathBuf::from("/media/zub/backup/.Trash-1000"),
            PathBuf::from("/media/zub/backup"),
        );
        assert_eq!(trash_label(&unshared, &home), "Trash on backup");

        let shared = TrashDir::new(
            PathBuf::from("/media/zub/backup/.Trash/1000"),
            PathBuf::from("/media/zub/backup"),
        );
        assert_eq!(trash_label(&shared, &home), "Trash on backup");
    }

    #[test]
    fn labels_prefer_the_final_component() {
        assert_eq!(display_label(Path::new("/usr/share")), "share");
        assert_eq!(display_label(Path::new("/")), "Filesystem");
    }

    #[test]
    fn escaping_keeps_toml_valid() {
        assert_eq!(escape(r#"a"b\c"#), r#"a\"b\\c"#);
    }
}
