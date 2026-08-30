//! Sidebar locations: XDG user directories, mounted volumes and user pins.

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

    if let Some(trash) = trash_directory() {
        places.push(Place {
            label: "Trash".to_owned(),
            icon_name: "user-trash-symbolic".to_owned(),
            path: trash,
        });
    }

    places
}

/// The FreeDesktop trash directory for the home filesystem, when it exists.
pub fn trash_directory() -> Option<PathBuf> {
    let path = data_home().join("Trash/files");
    path.is_dir().then_some(path)
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

    if Some(path) == trash_directory().as_deref() {
        return "Trash".to_owned();
    }

    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

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
