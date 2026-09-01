//! Teral's user configuration.
//!
//! One file — `~/.config/teral/teral.toml` — carries appearance, file and command
//! preferences. Teral's Settings window writes the same file, so hand-editing and the
//! GUI stay in sync instead of fighting over two independent stores.

use crate::files::SortKey;
use crate::theme::{ThemeColors, ThemeLayout, config_home};
use serde::Deserialize;
use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;

/// Configuration format version understood by this build.
pub const CONFIG_VERSION: u32 = 1;

/// Where Teral's colours come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    /// Teral's own palette, identical on every desktop.
    #[default]
    Teral,
    /// Follow the desktop's own appearance, including the active Omarchy theme.
    System,
}

impl ThemeMode {
    pub const ALL: [Self; 2] = [Self::Teral, Self::System];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Teral => "teral",
            Self::System => "system",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Teral => "Teral",
            Self::System => "Follow the system",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Teral => "Teral's own dark palette, the same on every desktop.",
            Self::System => {
                "Take the desktop's colours: the active Omarchy theme under Omarchy, \
                 and the GTK theme and accent colour elsewhere."
            }
        }
    }

    fn parse(value: &str) -> Option<Self> {
        // `omarchy` was its own mode before Omarchy became part of following the
        // system, so existing configuration files keep working unchanged.
        if value.eq_ignore_ascii_case("omarchy") {
            return Some(Self::System);
        }
        Self::ALL
            .into_iter()
            .find(|mode| mode.key().eq_ignore_ascii_case(value))
    }
}

/// Which file view Teral opens with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewPreference {
    #[default]
    Grid,
    List,
}

impl ViewPreference {
    const fn key(self) -> &'static str {
        match self {
            Self::Grid => "grid",
            Self::List => "list",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "grid" => Some(Self::Grid),
            "list" => Some(Self::List),
            _ => None,
        }
    }
}

fn sort_key_name(key: SortKey) -> &'static str {
    match key {
        SortKey::Name => "name",
        SortKey::Size => "size",
        SortKey::Kind => "type",
        SortKey::Modified => "modified",
    }
}

fn parse_sort_key(value: &str) -> Option<SortKey> {
    match value.to_ascii_lowercase().as_str() {
        "name" => Some(SortKey::Name),
        "size" => Some(SortKey::Size),
        "type" | "kind" => Some(SortKey::Kind),
        "modified" | "time" => Some(SortKey::Modified),
        _ => None,
    }
}

/// The resolved user configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub mode: ThemeMode,
    /// Overrides the accent colour of whichever palette is in use.
    pub accent: Option<String>,
    /// Advanced per-colour overrides.
    pub colors: ThemeColors,
    pub layout: ThemeLayout,
    pub show_hidden: bool,
    pub folders_first: bool,
    pub sort: SortKey,
    pub descending: bool,
    pub view: ViewPreference,
    /// Whether the information panel is visible when a window opens.
    pub details_visible: bool,
    /// Shell used by Quick Command. Empty means "ask the environment".
    pub shell: String,
    /// Terminal used by Open Terminal Here. Empty means "detect one".
    pub terminal: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: ThemeMode::default(),
            accent: None,
            colors: ThemeColors::default(),
            layout: ThemeLayout::default(),
            show_hidden: false,
            folders_first: true,
            sort: SortKey::Name,
            descending: false,
            view: ViewPreference::default(),
            details_visible: true,
            shell: String::new(),
            terminal: String::new(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct RawConfig {
    #[serde(default)]
    appearance: RawAppearance,
    #[serde(default)]
    colors: ThemeColors,
    #[serde(default)]
    layout: ThemeLayout,
    #[serde(default)]
    files: RawFiles,
    #[serde(default)]
    commands: RawCommands,
}

#[derive(Debug, Default, Deserialize)]
struct RawAppearance {
    mode: Option<String>,
    accent: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawFiles {
    show_hidden: Option<bool>,
    folders_first: Option<bool>,
    sort: Option<String>,
    descending: Option<bool>,
    view: Option<String>,
    details_visible: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct RawCommands {
    shell: Option<String>,
    terminal: Option<String>,
}

/// Path of the user configuration file.
pub fn config_path() -> PathBuf {
    config_home().join("teral/teral.toml")
}

impl Config {
    /// Read the user configuration, falling back to defaults for anything missing.
    pub fn load() -> Result<Self, String> {
        let path = config_path();
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(format!("could not read {}: {error}", path.display())),
        };

        match toml::from_str::<RawConfig>(&raw) {
            Ok(raw) => Ok(Self::from_raw(raw)),
            Err(error) => Err(format!("could not parse {}: {error}", path.display())),
        }
    }

    fn from_raw(raw: RawConfig) -> Self {
        let defaults = Self::default();
        let mut config = Self {
            mode: raw
                .appearance
                .mode
                .as_deref()
                .and_then(ThemeMode::parse)
                .unwrap_or(defaults.mode),
            accent: raw.appearance.accent,
            colors: raw.colors,
            layout: raw.layout,
            show_hidden: raw.files.show_hidden.unwrap_or(defaults.show_hidden),
            folders_first: raw.files.folders_first.unwrap_or(defaults.folders_first),
            sort: raw
                .files
                .sort
                .as_deref()
                .and_then(parse_sort_key)
                .unwrap_or(defaults.sort),
            descending: raw.files.descending.unwrap_or(defaults.descending),
            view: raw
                .files
                .view
                .as_deref()
                .and_then(ViewPreference::parse)
                .unwrap_or(defaults.view),
            details_visible: raw
                .files
                .details_visible
                .unwrap_or(defaults.details_visible),
            shell: raw.commands.shell.unwrap_or_default(),
            terminal: raw.commands.terminal.unwrap_or_default(),
        };

        config.colors.sanitize();
        config.layout.sanitize();
        if config
            .accent
            .as_deref()
            .is_some_and(|value| !crate::theme::valid_color(value))
        {
            config.accent = None;
        }
        config
    }

    /// Write the configuration back, creating the directory when needed.
    pub fn save(&self) -> Result<(), String> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        crate::persistence::atomic_write(&path, self.to_toml().as_bytes())
            .map_err(|error| error.to_string())
    }

    fn to_toml(&self) -> String {
        let mut document = String::new();
        document.push_str("# Teral configuration. Teral's Settings window writes this file,\n");
        document
            .push_str("# and every key is optional: anything missing uses Teral's default.\n\n");
        document.push_str(&format!("version = {CONFIG_VERSION}\n\n"));

        document.push_str("[appearance]\n");
        document.push_str(&format!("mode = \"{}\"\n", self.mode.key()));
        if let Some(accent) = &self.accent {
            document.push_str(&format!("accent = \"{accent}\"\n"));
        }
        document.push('\n');

        let colors = self.colors.entries();
        if !colors.is_empty() {
            document.push_str("[colors]\n");
            for (key, value) in colors {
                document.push_str(&format!("{key} = \"{value}\"\n"));
            }
            document.push('\n');
        }

        document.push_str("[layout]\n");
        for (key, value) in self.layout.entries() {
            document.push_str(&format!("{key} = {value}\n"));
        }
        document.push('\n');

        document.push_str("[files]\n");
        document.push_str(&format!("show_hidden = {}\n", self.show_hidden));
        document.push_str(&format!("folders_first = {}\n", self.folders_first));
        document.push_str(&format!("sort = \"{}\"\n", sort_key_name(self.sort)));
        document.push_str(&format!("descending = {}\n", self.descending));
        document.push_str(&format!("view = \"{}\"\n\n", self.view.key()));
        document.push_str(&format!("details_visible = {}\n\n", self.details_visible));

        document.push_str("[commands]\n");
        document.push_str(&format!("shell = \"{}\"\n", escape(&self.shell)));
        document.push_str(&format!("terminal = \"{}\"\n", escape(&self.terminal)));

        document
    }
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

thread_local! {
    static CURRENT: RefCell<Config> = RefCell::new(Config::default());
}

/// The configuration currently in effect.
pub fn current() -> Config {
    CURRENT.with_borrow(Clone::clone)
}

/// Replace the configuration in effect. Callers are responsible for re-applying it.
pub fn set_current(config: Config) {
    CURRENT.with_borrow_mut(|current| *current = config);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_through_toml() {
        let config = Config::default();
        let parsed = toml::from_str::<RawConfig>(&config.to_toml()).expect("valid TOML");
        let restored = Config::from_raw(parsed);

        assert_eq!(restored.mode, config.mode);
        assert_eq!(restored.show_hidden, config.show_hidden);
        assert_eq!(restored.folders_first, config.folders_first);
        assert_eq!(restored.view, config.view);
    }

    #[test]
    fn every_setting_round_trips() {
        let config = Config {
            mode: ThemeMode::System,
            accent: Some("#123456".to_owned()),
            show_hidden: true,
            folders_first: false,
            sort: SortKey::Modified,
            descending: true,
            view: ViewPreference::List,
            details_visible: false,
            shell: "/bin/dash".to_owned(),
            terminal: "kitty".to_owned(),
            ..Config::default()
        };

        let parsed = toml::from_str::<RawConfig>(&config.to_toml()).expect("valid TOML");
        let restored = Config::from_raw(parsed);

        assert_eq!(restored.mode, ThemeMode::System);
        assert_eq!(restored.accent.as_deref(), Some("#123456"));
        assert!(restored.show_hidden);
        assert!(!restored.folders_first);
        assert_eq!(restored.sort, SortKey::Modified);
        assert!(restored.descending);
        assert_eq!(restored.view, ViewPreference::List);
        assert!(!restored.details_visible);
        assert_eq!(restored.shell, "/bin/dash");
        assert_eq!(restored.terminal, "kitty");
    }

    #[test]
    fn an_unreadable_accent_is_discarded_rather_than_applied() {
        let raw = toml::from_str::<RawConfig>("[appearance]\naccent = \"purple\"\n").expect("toml");
        assert!(Config::from_raw(raw).accent.is_none());
    }

    #[test]
    fn the_old_omarchy_mode_now_means_following_the_system() {
        let raw = toml::from_str::<RawConfig>("[appearance]\nmode = \"omarchy\"\n").expect("toml");
        assert_eq!(Config::from_raw(raw).mode, ThemeMode::System);
    }

    #[test]
    fn an_unknown_mode_falls_back_to_the_default() {
        let raw = toml::from_str::<RawConfig>("[appearance]\nmode = \"nonsense\"\n").expect("toml");
        assert_eq!(Config::from_raw(raw).mode, ThemeMode::Teral);
    }
}
