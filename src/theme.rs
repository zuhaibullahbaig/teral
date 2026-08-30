use gtk::gdk::Display;
use gtk::prelude::*;
use gtk::CssProvider;
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_THEME: &str = include_str!("../themes/default/teral.toml");

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ThemeConfig {
    pub version: Option<u32>,
    pub name: Option<String>,
    #[serde(default)]
    pub colors: ThemeColors,
    #[serde(default)]
    pub layout: ThemeLayout,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ThemeColors {
    pub background: Option<String>,
    pub surface: Option<String>,
    pub surface_alt: Option<String>,
    pub text: Option<String>,
    pub text_muted: Option<String>,
    pub accent: Option<String>,
    pub selection: Option<String>,
    pub danger: Option<String>,
    pub warning: Option<String>,
    pub success: Option<String>,
    pub border: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ThemeLayout {
    pub window_width: Option<i32>,
    pub window_height: Option<i32>,
    pub sidebar_width: Option<i32>,
    pub details_width: Option<i32>,
    pub spacing: Option<i32>,
    pub radius: Option<i32>,
    pub row_height: Option<i32>,
}

#[derive(Debug, Default, Deserialize)]
struct OmarchyColors {
    background: Option<String>,
    dark_background: Option<String>,
    darker_background: Option<String>,
    lighter_background: Option<String>,
    foreground: Option<String>,
    dark_foreground: Option<String>,
    bright_foreground: Option<String>,
    accent: Option<String>,
    selection: Option<String>,
    muted: Option<String>,
    red: Option<String>,
    yellow: Option<String>,
    green: Option<String>,
}

impl ThemeConfig {
    pub fn load() -> Self {
        let mut theme = toml::from_str::<Self>(DEFAULT_THEME)
            .expect("the built-in Teral theme must be valid TOML");

        if let Some(omarchy_theme_dir) = omarchy_active_theme_dir() {
            let teral_theme = omarchy_theme_dir.join("teral.toml");
            let colors_theme = omarchy_theme_dir.join("colors.toml");

            if teral_theme.is_file() {
                match read_theme(&teral_theme) {
                    Ok(overlay) => theme.overlay(overlay),
                    Err(error) => {
                        eprintln!("Teral: could not load {}: {error}", teral_theme.display());
                        if let Some(derived) = derive_omarchy_theme(&colors_theme) {
                            theme.overlay(derived);
                        }
                    }
                }
            } else if let Some(derived) = derive_omarchy_theme(&colors_theme) {
                theme.overlay(derived);
            }
        }

        let user_theme = config_home().join("teral/teral.toml");
        if user_theme.is_file() {
            match read_theme(&user_theme) {
                Ok(overlay) => theme.overlay(overlay),
                Err(error) => {
                    eprintln!("Teral: could not load {}: {error}", user_theme.display());
                }
            }
        }

        theme.sanitize();
        theme
    }

    fn overlay(&mut self, other: Self) {
        overlay_option(&mut self.version, other.version);
        overlay_option(&mut self.name, other.name);
        self.colors.overlay(other.colors);
        self.layout.overlay(other.layout);
    }

    fn sanitize(&mut self) {
        self.colors.sanitize();
        self.layout.sanitize();
    }

    pub fn window_width(&self) -> i32 {
        self.layout.window_width.unwrap_or(1320)
    }

    pub fn window_height(&self) -> i32 {
        self.layout.window_height.unwrap_or(820)
    }

    pub fn sidebar_width(&self) -> i32 {
        self.layout.sidebar_width.unwrap_or(220)
    }

    pub fn details_width(&self) -> i32 {
        self.layout.details_width.unwrap_or(320)
    }

    pub fn spacing(&self) -> i32 {
        self.layout.spacing.unwrap_or(12)
    }

    pub fn row_height(&self) -> i32 {
        self.layout.row_height.unwrap_or(44)
    }

    pub fn apply_css(&self) {
        let css = self.to_css();
        let provider = CssProvider::new();
        provider.load_from_string(&css);

        if let Some(display) = Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    }

    fn to_css(&self) -> String {
        let mut css = String::from(
            ".teral-root { }\n\
             .teral-toolbar { padding: 10px 12px; }\n\
             .teral-sidebar { padding: 10px; }\n\
             .teral-details { padding: 16px; }\n\
             .teral-title { font-size: 15px; font-weight: 700; }\n\
             .teral-section-title { font-size: 11px; font-weight: 700; opacity: 0.72; }\n\
             .teral-muted { opacity: 0.66; }\n\
             .teral-path { font-family: monospace; opacity: 0.78; }\n\
             .teral-file-list row { padding: 7px 10px; }\n\
             .teral-file-name { font-weight: 600; }\n",
        );

        let radius = self.layout.radius.unwrap_or(10);
        css.push_str(&format!(
            ".teral-card {{ border-radius: {radius}px; }}\n\
             .teral-file-list row {{ min-height: {}px; }}\n",
            self.row_height()
        ));

        push_rule(&mut css, ".teral-root", "background-color", self.colors.background.as_deref());
        push_rule(&mut css, ".teral-root", "color", self.colors.text.as_deref());
        push_rule(
            &mut css,
            ".teral-sidebar, .teral-details, .teral-toolbar",
            "background-color",
            self.colors.surface.as_deref(),
        );
        push_rule(
            &mut css,
            ".teral-muted, .teral-path, .teral-section-title",
            "color",
            self.colors.text_muted.as_deref(),
        );
        push_rule(
            &mut css,
            ".teral-file-list row:selected",
            "background-color",
            self.colors.selection.as_deref().or(self.colors.accent.as_deref()),
        );
        push_rule(
            &mut css,
            ".teral-file-list row:hover",
            "background-color",
            self.colors.surface_alt.as_deref(),
        );
        push_rule(
            &mut css,
            ".teral-accent",
            "color",
            self.colors.accent.as_deref(),
        );

        if let Some(border) = self.colors.border.as_deref() {
            if valid_color(border) {
                css.push_str(&format!(
                    ".teral-sidebar {{ border-right: 1px solid {border}; }}\n\
                     .teral-details {{ border-left: 1px solid {border}; }}\n\
                     .teral-toolbar {{ border-bottom: 1px solid {border}; }}\n"
                ));
            }
        }

        css
    }
}

impl ThemeColors {
    fn overlay(&mut self, other: Self) {
        overlay_option(&mut self.background, other.background);
        overlay_option(&mut self.surface, other.surface);
        overlay_option(&mut self.surface_alt, other.surface_alt);
        overlay_option(&mut self.text, other.text);
        overlay_option(&mut self.text_muted, other.text_muted);
        overlay_option(&mut self.accent, other.accent);
        overlay_option(&mut self.selection, other.selection);
        overlay_option(&mut self.danger, other.danger);
        overlay_option(&mut self.warning, other.warning);
        overlay_option(&mut self.success, other.success);
        overlay_option(&mut self.border, other.border);
    }

    fn sanitize(&mut self) {
        for color in [
            &mut self.background,
            &mut self.surface,
            &mut self.surface_alt,
            &mut self.text,
            &mut self.text_muted,
            &mut self.accent,
            &mut self.selection,
            &mut self.danger,
            &mut self.warning,
            &mut self.success,
            &mut self.border,
        ] {
            if color.as_deref().is_some_and(|value| !valid_color(value)) {
                *color = None;
            }
        }
    }
}

impl ThemeLayout {
    fn overlay(&mut self, other: Self) {
        overlay_option(&mut self.window_width, other.window_width);
        overlay_option(&mut self.window_height, other.window_height);
        overlay_option(&mut self.sidebar_width, other.sidebar_width);
        overlay_option(&mut self.details_width, other.details_width);
        overlay_option(&mut self.spacing, other.spacing);
        overlay_option(&mut self.radius, other.radius);
        overlay_option(&mut self.row_height, other.row_height);
    }

    fn sanitize(&mut self) {
        clamp_option(&mut self.window_width, 720, 3840);
        clamp_option(&mut self.window_height, 480, 2160);
        clamp_option(&mut self.sidebar_width, 160, 520);
        clamp_option(&mut self.details_width, 220, 720);
        clamp_option(&mut self.spacing, 0, 48);
        clamp_option(&mut self.radius, 0, 40);
        clamp_option(&mut self.row_height, 28, 96);
    }
}

fn read_theme(path: &Path) -> Result<ThemeConfig, String> {
    let raw = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let theme = toml::from_str::<ThemeConfig>(&raw).map_err(|error| error.to_string())?;

    if theme.version.is_some_and(|version| version != 1) {
        return Err("unsupported theme version; Teral currently supports version = 1".to_owned());
    }

    Ok(theme)
}

fn derive_omarchy_theme(colors_path: &Path) -> Option<ThemeConfig> {
    let raw = fs::read_to_string(colors_path).ok()?;
    let colors = toml::from_str::<OmarchyColors>(&raw).ok()?;

    Some(ThemeConfig {
        version: Some(1),
        name: Some("Omarchy Active Theme".to_owned()),
        colors: ThemeColors {
            background: colors.background,
            surface: colors.dark_background.or(colors.darker_background),
            surface_alt: colors.lighter_background,
            text: colors.foreground.or(colors.bright_foreground),
            text_muted: colors.muted.or(colors.dark_foreground),
            accent: colors.accent,
            selection: colors.selection,
            danger: colors.red,
            warning: colors.yellow,
            success: colors.green,
            border: None,
        },
        layout: ThemeLayout::default(),
    })
}

fn omarchy_active_theme_dir() -> Option<PathBuf> {
    let path = state_home().join("omarchy/current/theme");
    path.is_dir().then_some(path)
}

fn config_home() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"))
}

fn state_home() -> PathBuf {
    env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".local/state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"))
}

pub fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn overlay_option<T>(base: &mut Option<T>, overlay: Option<T>) {
    if overlay.is_some() {
        *base = overlay;
    }
}

fn clamp_option(value: &mut Option<i32>, min: i32, max: i32) {
    if let Some(current) = value.as_mut() {
        *current = (*current).clamp(min, max);
    }
}

fn valid_color(value: &str) -> bool {
    let Some(hex) = value.strip_prefix('#') else {
        return false;
    };

    matches!(hex.len(), 6 | 8) && hex.chars().all(|character| character.is_ascii_hexdigit())
}

fn push_rule(css: &mut String, selector: &str, property: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| valid_color(value)) {
        css.push_str(&format!("{selector} {{ {property}: {value}; }}\n"));
    }
}
