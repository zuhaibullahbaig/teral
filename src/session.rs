//! Deliberate restoration of the last window's tabs.

use crate::persistence::{atomic_write, decode_path, encode_path};
use crate::theme::data_home;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

const SESSION_VERSION: u32 = 1;
const MAX_TABS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedTab {
    pub path: PathBuf,
    pub tag: Option<String>,
    pub back: Vec<PathBuf>,
    pub forward: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub active: usize,
    pub tabs: Vec<SavedTab>,
}

#[derive(Debug, Default, Deserialize)]
struct RawSession {
    active: Option<usize>,
    #[serde(default)]
    tab: Vec<RawTab>,
}

#[derive(Debug, Deserialize)]
struct RawTab {
    path_hex: String,
    tag: Option<String>,
    #[serde(default)]
    back_hex: Vec<String>,
    #[serde(default)]
    forward_hex: Vec<String>,
}

pub fn path() -> PathBuf {
    data_home().join("teral/session.toml")
}

pub fn load() -> Result<Option<Session>, String> {
    let path = path();
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not read {}: {error}", path.display())),
    };
    let raw: RawSession = toml::from_str(&raw)
        .map_err(|error| format!("could not parse {}: {error}", path.display()))?;
    if raw.tab.is_empty() {
        return Ok(None);
    }

    let mut tabs = Vec::with_capacity(raw.tab.len().min(MAX_TABS));
    for tab in raw.tab.into_iter().take(MAX_TABS) {
        let path = decode_path(&tab.path_hex)?;
        let back = decode_paths(tab.back_hex)?;
        let forward = decode_paths(tab.forward_hex)?;
        tabs.push(SavedTab {
            path,
            tag: tab.tag.filter(|tag| !tag.trim().is_empty()),
            back,
            forward,
        });
    }
    let active = raw.active.unwrap_or_default().min(tabs.len() - 1);
    Ok(Some(Session { active, tabs }))
}

pub fn save(session: &Session) -> Result<(), String> {
    if session.tabs.is_empty() {
        return Err("a session must contain at least one tab".to_owned());
    }
    let mut document = format!(
        "version = {SESSION_VERSION}\nactive = {}\n",
        session.active.min(session.tabs.len() - 1)
    );
    for tab in session.tabs.iter().take(MAX_TABS) {
        document.push_str("\n[[tab]]\n");
        document.push_str(&format!("path_hex = \"{}\"\n", encode_path(&tab.path)));
        if let Some(tag) = &tab.tag {
            document.push_str(&format!("tag = \"{}\"\n", escape(tag)));
        }
        write_paths(&mut document, "back_hex", &tab.back);
        write_paths(&mut document, "forward_hex", &tab.forward);
    }
    atomic_write(&path(), document.as_bytes()).map_err(|error| error.to_string())
}

fn decode_paths(values: Vec<String>) -> Result<Vec<PathBuf>, String> {
    values.into_iter().map(|value| decode_path(&value)).collect()
}

fn write_paths(document: &mut String, key: &str, paths: &[PathBuf]) {
    document.push_str(&format!("{key} = [\n"));
    for path in paths.iter().rev().take(128).rev() {
        document.push_str(&format!("  \"{}\",\n", encode_path(path)));
    }
    document.push_str("]\n");
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn session_paths_are_serialized_without_utf8_loss() {
        let path = PathBuf::from(OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xfe]));
        let tab = SavedTab {
            path: path.clone(),
            tag: None,
            back: vec![path.clone()],
            forward: Vec::new(),
        };
        let mut document = String::new();
        write_paths(&mut document, "back_hex", &tab.back);
        let raw: toml::Value = toml::from_str(&document).expect("valid TOML");
        let encoded = raw["back_hex"][0].as_str().expect("path");
        assert_eq!(decode_path(encoded).expect("decode"), path);
    }

    #[test]
    fn history_is_bounded_to_the_most_recent_entries() {
        let paths: Vec<PathBuf> = (0..200).map(|index| PathBuf::from(format!("/{index}"))).collect();
        let mut document = String::new();
        write_paths(&mut document, "back_hex", &paths);
        let raw: toml::Value = toml::from_str(&document).expect("valid TOML");
        assert_eq!(raw["back_hex"].as_array().expect("array").len(), 128);
    }
}
