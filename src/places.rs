//! Sidebar locations: XDG user directories, mounted volumes and user pins.

use crate::files::trash::TrashDir;
use crate::theme::{data_home, home_dir};
use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

/// A navigable location shown in the sidebar.
#[derive(Debug, Clone)]
pub struct Place {
    pub label: String,
    pub icon_name: String,
    pub path: PathBuf,
}

/// A mounted filesystem, with capacity when the kernel reports it.
#[derive(Debug, Clone)]
pub struct Device {
    pub label: String,
    pub icon: Option<gio::Icon>,
    pub path: PathBuf,
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
    crate::files::ops::trash_dirs()
        .into_iter()
        .map(|dir| Place {
            label: trash_label(&dir, &home),
            icon_name: "user-trash-symbolic".to_owned(),
            path: dir.files(),
        })
        .collect()
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

/// Mounted filesystems discovered through GIO, plus the root filesystem.
pub fn devices() -> Vec<Device> {
    let mut devices = vec![Device {
        label: "Filesystem".to_owned(),
        icon: Some(gio::Icon::for_string("drive-harddisk-symbolic").expect("static icon name")),
        path: PathBuf::from("/"),
    }];

    for mount in gio::VolumeMonitor::get().mounts() {
        if mount.is_shadowed() {
            continue;
        }
        let Some(path) = mount.root().path() else {
            continue;
        };
        if path == Path::new("/") || devices.iter().any(|device| device.path == path) {
            continue;
        }

        devices.push(Device {
            label: mount.name().to_string(),
            icon: Some(mount.symbolic_icon()),
            path,
        });
    }

    devices
}

#[derive(Debug, Default, Deserialize)]
struct PinnedFile {
    #[serde(default)]
    pinned: Vec<String>,
}

fn pinned_path() -> PathBuf {
    data_home().join("teral/places.toml")
}

/// Load the user's pinned locations, dropping any that no longer exist.
pub fn load_pinned() -> Vec<PathBuf> {
    let Ok(raw) = fs::read_to_string(pinned_path()) else {
        return Vec::new();
    };

    match toml::from_str::<PinnedFile>(&raw) {
        Ok(file) => file
            .pinned
            .into_iter()
            .map(PathBuf::from)
            .filter(|path| path.is_dir())
            .collect(),
        Err(error) => {
            eprintln!("Teral: could not read pinned locations: {error}");
            Vec::new()
        }
    }
}

/// Persist the user's pinned locations.
///
/// Paths that are not valid UTF-8 cannot be represented in TOML and are reported rather
/// than silently written back in a lossy form.
pub fn save_pinned(pinned: &[PathBuf]) {
    let path = pinned_path();
    if let Some(parent) = path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        eprintln!("Teral: could not create {}: {error}", parent.display());
        return;
    }

    let mut document = String::from("version = 1\npinned = [\n");
    for pin in pinned {
        match pin.to_str() {
            Some(text) => document.push_str(&format!("  \"{}\",\n", escape(text))),
            None => eprintln!(
                "Teral: cannot pin {} because its name is not valid UTF-8",
                pin.display()
            ),
        }
    }
    document.push_str("]\n");

    if let Err(error) = fs::write(&path, document) {
        eprintln!("Teral: could not write {}: {error}", path.display());
    }
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

    // Every browsable trash path ends in "files". Checking that first keeps the
    // volume-monitor lookup out of ordinary breadcrumb and title rendering.
    if path.file_name() == Some(std::ffi::OsStr::new("files")) {
        let home = crate::files::trash::home_trash(&data_home());
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
