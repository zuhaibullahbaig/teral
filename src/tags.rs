//! User tags.
//!
//! Tags are Teral's own metadata, not a filesystem feature: a name, a colour, and the
//! set of files carrying it. They live in `~/.local/share/teral/tags.toml` so they
//! survive restarts, and Teral rewrites the stored paths when it renames or moves a
//! file itself, so a tag follows its file instead of pointing at a hole.

use crate::theme::data_home;
use serde::Deserialize;
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

/// Tag file format version understood by this build.
pub const TAGS_VERSION: u32 = 1;

/// One tag and the files carrying it.
#[derive(Debug, Clone)]
pub struct Tag {
    pub name: String,
    pub color: String,
    pub paths: Vec<PathBuf>,
}

impl Tag {
    /// Tags are identified by name, compared without case so "Work" and "work" are one.
    pub fn matches(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }
}

/// The whole tag store.
#[derive(Debug, Clone, Default)]
pub struct Tags {
    pub tags: Vec<Tag>,
}

/// The tags Teral starts with, so the section is useful before anyone configures it.
const DEFAULTS: [(&str, &str); 4] = [
    ("Important", "#e0a63c"),
    ("Work", "#6f9fd8"),
    ("Personal", "#8fd08f"),
    ("Archive", "#b58bd0"),
];

#[derive(Debug, Default, Deserialize)]
struct RawTags {
    #[serde(default)]
    tag: Vec<RawTag>,
}

#[derive(Debug, Deserialize)]
struct RawTag {
    name: String,
    color: Option<String>,
    #[serde(default)]
    paths: Vec<String>,
}

fn tags_path() -> PathBuf {
    data_home().join("teral/tags.toml")
}

impl Tags {
    /// Teral's starting set of tags, with nothing tagged yet.
    pub fn defaults() -> Self {
        Self {
            tags: DEFAULTS
                .iter()
                .map(|(name, color)| Tag {
                    name: (*name).to_owned(),
                    color: (*color).to_owned(),
                    paths: Vec::new(),
                })
                .collect(),
        }
    }

    /// Read the store, falling back to the defaults when there is nothing to read.
    pub fn load() -> Self {
        let path = tags_path();
        let Ok(raw) = fs::read_to_string(&path) else {
            return Self::defaults();
        };

        match toml::from_str::<RawTags>(&raw) {
            Ok(raw) => Self {
                tags: raw
                    .tag
                    .into_iter()
                    .map(|tag| Tag {
                        color: tag
                            .color
                            .filter(|color| crate::theme::valid_color(color))
                            .unwrap_or_else(|| "#e0a63c".to_owned()),
                        name: tag.name,
                        // Files that have since disappeared are dropped quietly.
                        paths: tag
                            .paths
                            .into_iter()
                            .map(PathBuf::from)
                            .filter(|path| path.symlink_metadata().is_ok())
                            .collect(),
                    })
                    .filter(|tag| !tag.name.trim().is_empty())
                    .collect(),
            },
            Err(error) => {
                eprintln!("Teral: could not read {}: {error}", path.display());
                Self::defaults()
            }
        }
    }

    /// Persist the store.
    pub fn save(&self) {
        let path = tags_path();
        if let Some(parent) = path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            eprintln!("Teral: could not create {}: {error}", parent.display());
            return;
        }

        let mut document = String::from("# Teral tags. Edit by hand if you like.\n\n");
        document.push_str(&format!("version = {TAGS_VERSION}\n"));

        for tag in &self.tags {
            document.push_str("\n[[tag]]\n");
            document.push_str(&format!("name = \"{}\"\n", escape(&tag.name)));
            document.push_str(&format!("color = \"{}\"\n", escape(&tag.color)));
            document.push_str("paths = [\n");
            for entry in &tag.paths {
                match entry.to_str() {
                    Some(text) => document.push_str(&format!("  \"{}\",\n", escape(text))),
                    None => eprintln!(
                        "Teral: cannot tag {} because its name is not valid UTF-8",
                        entry.display()
                    ),
                }
            }
            document.push_str("]\n");
        }

        if let Err(error) = fs::write(&path, document) {
            eprintln!("Teral: could not write {}: {error}", path.display());
        }
    }

    pub fn get(&self, name: &str) -> Option<&Tag> {
        self.tags.iter().find(|tag| tag.matches(name))
    }

    /// Create a tag, refusing a name that is already taken.
    pub fn create(&mut self, name: &str, color: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a tag needs a name".to_owned());
        }
        if self.get(name).is_some() {
            return Err(format!("there is already a tag called {name}"));
        }

        self.tags.push(Tag {
            name: name.to_owned(),
            color: color.to_owned(),
            paths: Vec::new(),
        });
        Ok(())
    }

    /// Rename and recolour a tag, keeping everything it is attached to.
    pub fn update(&mut self, current: &str, name: &str, color: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a tag needs a name".to_owned());
        }
        if !name.eq_ignore_ascii_case(current) && self.get(name).is_some() {
            return Err(format!("there is already a tag called {name}"));
        }

        let Some(tag) = self.tags.iter_mut().find(|tag| tag.matches(current)) else {
            return Err(format!("there is no tag called {current}"));
        };
        tag.name = name.to_owned();
        tag.color = color.to_owned();
        Ok(())
    }

    pub fn delete(&mut self, name: &str) {
        self.tags.retain(|tag| !tag.matches(name));
    }

    /// Whether `path` carries `name`.
    pub fn is_tagged(&self, name: &str, path: &Path) -> bool {
        self.get(name)
            .is_some_and(|tag| tag.paths.iter().any(|tagged| tagged == path))
    }

    /// Add or remove a tag on a set of files.
    pub fn set_tagged(&mut self, name: &str, paths: &[PathBuf], tagged: bool) {
        let Some(tag) = self.tags.iter_mut().find(|tag| tag.matches(name)) else {
            return;
        };

        for path in paths {
            let existing = tag.paths.iter().position(|tagged| tagged == path);
            match (tagged, existing) {
                (true, None) => tag.paths.push(path.clone()),
                (false, Some(index)) => {
                    tag.paths.remove(index);
                }
                _ => {}
            }
        }
    }

    /// Every tag carried by `path`.
    pub fn for_path(&self, path: &Path) -> Vec<&Tag> {
        self.tags
            .iter()
            .filter(|tag| tag.paths.iter().any(|tagged| tagged == path))
            .collect()
    }

    /// Follow a file that Teral moved or renamed, including anything inside it.
    pub fn relocate(&mut self, from: &Path, to: &Path) {
        for tag in &mut self.tags {
            for path in &mut tag.paths {
                if path == from {
                    *path = to.to_path_buf();
                } else if let Ok(relative) = path.strip_prefix(from) {
                    *path = to.join(relative);
                }
            }
        }
    }

    /// Forget a file Teral deleted, including anything inside it.
    pub fn forget(&mut self, path: &Path) {
        for tag in &mut self.tags {
            tag.paths
                .retain(|tagged| tagged != path && !tagged.starts_with(path));
        }
    }
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

thread_local! {
    static CURRENT: RefCell<Tags> = RefCell::new(Tags::default());
}

/// The tag store currently in memory.
pub fn current() -> Tags {
    CURRENT.with_borrow(Clone::clone)
}

/// Replace the store in memory and write it out.
pub fn set_current(tags: Tags) {
    tags.save();
    CURRENT.with_borrow_mut(|current| *current = tags);
}

/// Load the store at start-up without writing it back.
pub fn init() {
    CURRENT.with_borrow_mut(|current| *current = Tags::load());
}

/// Edit the store in place and save the result.
pub fn edit(change: impl FnOnce(&mut Tags)) {
    let mut tags = current();
    change(&mut tags);
    set_current(tags);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagging_is_idempotent() {
        let mut tags = Tags::defaults();
        let path = PathBuf::from("/tmp/a");

        tags.set_tagged("Important", std::slice::from_ref(&path), true);
        tags.set_tagged("Important", std::slice::from_ref(&path), true);
        assert_eq!(tags.get("Important").expect("tag").paths.len(), 1);

        tags.set_tagged("Important", std::slice::from_ref(&path), false);
        assert!(tags.get("Important").expect("tag").paths.is_empty());
    }

    #[test]
    fn names_are_compared_without_case() {
        let mut tags = Tags::defaults();
        assert!(tags.create("important", "#ffffff").is_err());
        assert!(tags.create("Reference", "#ffffff").is_ok());
    }

    #[test]
    fn renaming_keeps_the_tagged_files() {
        let mut tags = Tags::defaults();
        let path = PathBuf::from("/tmp/a");
        tags.set_tagged("Work", std::slice::from_ref(&path), true);

        tags.update("Work", "Clients", "#123456").expect("update");
        assert!(tags.get("Work").is_none());

        let renamed = tags.get("Clients").expect("renamed tag");
        assert_eq!(renamed.color, "#123456");
        assert_eq!(renamed.paths, vec![path]);
    }

    #[test]
    fn tags_follow_a_moved_folder() {
        let mut tags = Tags::defaults();
        let inside = PathBuf::from("/tmp/project/notes.txt");
        tags.set_tagged("Important", std::slice::from_ref(&inside), true);

        tags.relocate(Path::new("/tmp/project"), Path::new("/tmp/archive/project"));
        assert_eq!(
            tags.get("Important").expect("tag").paths,
            vec![PathBuf::from("/tmp/archive/project/notes.txt")]
        );
    }

    #[test]
    fn deleting_a_folder_forgets_what_was_inside_it() {
        let mut tags = Tags::defaults();
        let inside = PathBuf::from("/tmp/project/notes.txt");
        tags.set_tagged("Important", std::slice::from_ref(&inside), true);

        tags.forget(Path::new("/tmp/project"));
        assert!(tags.get("Important").expect("tag").paths.is_empty());
    }
}
