//! Layered Teral theme resolution.
//!
//! Layers, lowest priority first:
//!
//! 1. a built-in Teral palette (dark, or light when the desktop asks for light)
//! 2. the desktop's own appearance, when the user picked "Follow the system"
//! 3. the active Omarchy theme, when the user picked "Follow Omarchy"
//! 4. the user's own colour and layout overrides
//!
//! Every field is optional at every layer, so partial overrides inherit safely and a
//! broken theme can never leave Teral without a usable appearance.

use crate::config::{Config, ThemeMode};
use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;
use gtk::prelude::*;
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const DARK_THEME: &str = include_str!("../themes/default/teral.toml");
const LIGHT_THEME: &str = include_str!("../themes/default/teral-light.toml");

/// Theme format version understood by this build of Teral.
pub const THEME_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ThemeConfig {
    pub version: Option<u32>,
    pub name: Option<String>,
    #[serde(default)]
    pub colors: ThemeColors,
    #[serde(default)]
    pub layout: ThemeLayout,
    /// True when the resolved palette is a dark one, so GTK can be told to match.
    #[serde(skip)]
    pub dark: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ThemeColors {
    pub background: Option<String>,
    pub surface: Option<String>,
    pub surface_alt: Option<String>,
    pub elevated: Option<String>,
    pub border: Option<String>,
    pub text: Option<String>,
    pub text_bright: Option<String>,
    pub text_muted: Option<String>,
    pub accent: Option<String>,
    pub selection: Option<String>,
    pub danger: Option<String>,
    pub warning: Option<String>,
    pub success: Option<String>,
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
    pub grid_icon_size: Option<i32>,
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
    /// Resolve the effective theme for a configuration.
    pub fn resolve(config: &Config) -> Self {
        // Following the system means following the active Omarchy theme when Teral is
        // running under Omarchy, because that theme *is* that desktop's appearance and
        // is more specific than anything GTK publishes. Every other desktop — and
        // Omarchy with no readable theme — keeps the GTK-derived palette.
        let omarchy = match config.mode {
            ThemeMode::System => omarchy_active_theme_dir(),
            ThemeMode::Teral => None,
        };

        let prefer_dark = match (config.mode, &omarchy) {
            (ThemeMode::System, Some(directory)) => omarchy_prefers_dark(directory),
            (ThemeMode::System, None) => system_prefers_dark(),
            (ThemeMode::Teral, _) => true,
        };

        let source = if prefer_dark { DARK_THEME } else { LIGHT_THEME };
        let mut theme =
            toml::from_str::<Self>(source).expect("the built-in Teral themes must be valid TOML");
        theme.dark = prefer_dark;

        if config.mode == ThemeMode::System {
            // Tell GTK which way to lean before reading its colours back, so the
            // palette Teral derives is the one the desktop is actually drawing.
            if let Some(settings) = gtk::Settings::default() {
                settings.set_gtk_application_prefer_dark_theme(prefer_dark);
            }

            match omarchy.as_deref().and_then(omarchy_overlay) {
                Some(overlay) => theme.overlay(overlay),
                None => {
                    if let Some(palette) = system_palette(prefer_dark) {
                        theme.colors.overlay(palette);
                    }
                    if let Some(accent) = system_accent() {
                        theme.colors.accent = Some(accent);
                    }
                    theme.name = Some("System".to_owned());
                }
            }
        }

        // The user's own overrides always win.
        theme.colors.overlay(config.colors.clone());
        theme.layout.overlay(config.layout.clone());
        if let Some(accent) = config.accent.clone() {
            theme.colors.accent = Some(accent);
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
        self.layout.window_width.unwrap_or(1360)
    }

    pub fn window_height(&self) -> i32 {
        self.layout.window_height.unwrap_or(840)
    }

    pub fn sidebar_width(&self) -> i32 {
        self.layout.sidebar_width.unwrap_or(238)
    }

    pub fn details_width(&self) -> i32 {
        self.layout.details_width.unwrap_or(316)
    }

    pub fn spacing(&self) -> i32 {
        self.layout.spacing.unwrap_or(12)
    }

    pub fn radius(&self) -> i32 {
        self.layout.radius.unwrap_or(10)
    }

    pub fn row_height(&self) -> i32 {
        self.layout.row_height.unwrap_or(30)
    }

    pub fn grid_icon_size(&self) -> i32 {
        self.layout.grid_icon_size.unwrap_or(64)
    }

    /// A colour resolved through the theme layers, falling back to the built-in value.
    pub fn color(&self, role: ColorRole) -> &str {
        let configured = match role {
            ColorRole::Background => self.colors.background.as_deref(),
            ColorRole::Surface => self.colors.surface.as_deref(),
            ColorRole::SurfaceAlt => self.colors.surface_alt.as_deref(),
            ColorRole::Elevated => self.colors.elevated.as_deref(),
            ColorRole::Border => self.colors.border.as_deref(),
            ColorRole::Text => self.colors.text.as_deref(),
            ColorRole::TextBright => self.colors.text_bright.as_deref(),
            ColorRole::TextMuted => self.colors.text_muted.as_deref(),
            ColorRole::Accent => self.colors.accent.as_deref(),
            ColorRole::Selection => self.colors.selection.as_deref(),
            ColorRole::Danger => self.colors.danger.as_deref(),
            ColorRole::Warning => self.colors.warning.as_deref(),
            ColorRole::Success => self.colors.success.as_deref(),
        };

        configured.unwrap_or_else(|| role.fallback())
    }
}

/// Semantic colour roles exposed to the stylesheet and to theme authors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorRole {
    Background,
    Surface,
    SurfaceAlt,
    Elevated,
    Border,
    Text,
    TextBright,
    TextMuted,
    Accent,
    Selection,
    Danger,
    Warning,
    Success,
}

impl ColorRole {
    /// Every role Teral defines, in stylesheet declaration order.
    pub const ALL: [Self; 13] = [
        Self::Background,
        Self::Surface,
        Self::SurfaceAlt,
        Self::Elevated,
        Self::Border,
        Self::Text,
        Self::TextBright,
        Self::TextMuted,
        Self::Accent,
        Self::Selection,
        Self::Danger,
        Self::Warning,
        Self::Success,
    ];

    /// The GTK `@define-color` name used by the Teral stylesheet.
    pub const fn css_name(self) -> &'static str {
        match self {
            Self::Background => "teral_bg",
            Self::Surface => "teral_surface",
            Self::SurfaceAlt => "teral_surface_alt",
            Self::Elevated => "teral_elevated",
            Self::Border => "teral_border",
            Self::Text => "teral_text",
            Self::TextBright => "teral_text_bright",
            Self::TextMuted => "teral_muted",
            Self::Accent => "teral_accent",
            Self::Selection => "teral_selection",
            Self::Danger => "teral_danger",
            Self::Warning => "teral_warning",
            Self::Success => "teral_success",
        }
    }

    /// Used when neither the built-in theme file nor any overlay supplies the role.
    const fn fallback(self) -> &'static str {
        match self {
            Self::Background => "#0e0e11",
            Self::Surface => "#0a0a0c",
            Self::SurfaceAlt => "#1a1a1f",
            Self::Elevated => "#141418",
            Self::Border => "#232329",
            Self::Text => "#e6e3de",
            Self::TextBright => "#ffffff",
            Self::TextMuted => "#8a8680",
            Self::Accent => "#e0a63c",
            Self::Selection => "#2a2117",
            Self::Danger => "#d9634f",
            Self::Warning => "#e0a63c",
            Self::Success => "#6fbf73",
        }
    }
}

impl ThemeColors {
    fn overlay(&mut self, other: Self) {
        overlay_option(&mut self.background, other.background);
        overlay_option(&mut self.surface, other.surface);
        overlay_option(&mut self.surface_alt, other.surface_alt);
        overlay_option(&mut self.elevated, other.elevated);
        overlay_option(&mut self.border, other.border);
        overlay_option(&mut self.text, other.text);
        overlay_option(&mut self.text_bright, other.text_bright);
        overlay_option(&mut self.text_muted, other.text_muted);
        overlay_option(&mut self.accent, other.accent);
        overlay_option(&mut self.selection, other.selection);
        overlay_option(&mut self.danger, other.danger);
        overlay_option(&mut self.warning, other.warning);
        overlay_option(&mut self.success, other.success);
    }

    /// Drop any value that is not a colour Teral can render.
    pub fn sanitize(&mut self) {
        for color in [
            &mut self.background,
            &mut self.surface,
            &mut self.surface_alt,
            &mut self.elevated,
            &mut self.border,
            &mut self.text,
            &mut self.text_bright,
            &mut self.text_muted,
            &mut self.accent,
            &mut self.selection,
            &mut self.danger,
            &mut self.warning,
            &mut self.success,
        ] {
            if color.as_deref().is_some_and(|value| !valid_color(value)) {
                *color = None;
            }
        }
    }

    /// The colours that are actually set, ready to be written back to TOML.
    pub fn entries(&self) -> Vec<(&'static str, String)> {
        [
            ("background", &self.background),
            ("surface", &self.surface),
            ("surface_alt", &self.surface_alt),
            ("elevated", &self.elevated),
            ("border", &self.border),
            ("text", &self.text),
            ("text_bright", &self.text_bright),
            ("text_muted", &self.text_muted),
            ("accent", &self.accent),
            ("selection", &self.selection),
            ("danger", &self.danger),
            ("warning", &self.warning),
            ("success", &self.success),
        ]
        .into_iter()
        .filter_map(|(key, value)| value.clone().map(|value| (key, value)))
        .collect()
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
        overlay_option(&mut self.grid_icon_size, other.grid_icon_size);
    }

    /// Clamp every value into a range Teral can actually lay out.
    pub fn sanitize(&mut self) {
        clamp_option(&mut self.window_width, 720, 3840);
        clamp_option(&mut self.window_height, 480, 2160);
        clamp_option(&mut self.sidebar_width, 180, 420);
        clamp_option(&mut self.details_width, 240, 520);
        clamp_option(&mut self.spacing, 0, 32);
        clamp_option(&mut self.radius, 0, 24);
        clamp_option(&mut self.row_height, 22, 64);
        clamp_option(&mut self.grid_icon_size, MIN_ICON_SIZE, MAX_ICON_SIZE);
    }

    /// The layout values that are actually set, ready to be written back to TOML.
    pub fn entries(&self) -> Vec<(&'static str, i32)> {
        [
            ("window_width", self.window_width),
            ("window_height", self.window_height),
            ("sidebar_width", self.sidebar_width),
            ("details_width", self.details_width),
            ("spacing", self.spacing),
            ("radius", self.radius),
            ("row_height", self.row_height),
            ("grid_icon_size", self.grid_icon_size),
        ]
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key, value)))
        .collect()
    }
}

/// Smallest and largest grid icon Teral will draw.
pub const MIN_ICON_SIZE: i32 = 32;
pub const MAX_ICON_SIZE: i32 = 160;

fn read_theme(path: &Path) -> Result<ThemeConfig, String> {
    let raw = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let theme = toml::from_str::<ThemeConfig>(&raw).map_err(|error| error.to_string())?;

    if theme
        .version
        .is_some_and(|version| version != THEME_FORMAT_VERSION)
    {
        return Err(format!(
            "unsupported theme version; Teral currently supports version = {THEME_FORMAT_VERSION}"
        ));
    }

    Ok(theme)
}

/// Whether the active Omarchy theme is a dark one.
///
/// Omarchy marks its light themes with a `light.mode` file in the theme directory;
/// everything else is dark, which is also the safer assumption for a theme that says
/// nothing.
fn omarchy_prefers_dark(directory: &Path) -> bool {
    !directory.join("light.mode").exists()
}

/// The Omarchy overlay: the active theme's `teral.toml`, or its palette.
fn omarchy_overlay(directory: &Path) -> Option<ThemeConfig> {
    let teral_theme = directory.join("teral.toml");
    let colors_theme = directory.join("colors.toml");

    if teral_theme.is_file() {
        match read_theme(&teral_theme) {
            Ok(overlay) => return Some(overlay),
            Err(error) => eprintln!("Teral: could not load {}: {error}", teral_theme.display()),
        }
    }

    derive_omarchy_theme(&colors_theme)
}

fn derive_omarchy_theme(colors_path: &Path) -> Option<ThemeConfig> {
    let raw = fs::read_to_string(colors_path).ok()?;
    let colors = toml::from_str::<OmarchyColors>(&raw).ok()?;

    Some(ThemeConfig {
        version: Some(THEME_FORMAT_VERSION),
        name: Some("Omarchy Active Theme".to_owned()),
        colors: ThemeColors {
            background: colors.background.clone(),
            surface: colors
                .dark_background
                .clone()
                .or_else(|| colors.darker_background.clone()),
            surface_alt: colors.lighter_background.clone(),
            elevated: colors
                .lighter_background
                .or_else(|| colors.dark_background.clone()),
            border: colors.dark_background,
            text: colors.foreground.clone(),
            text_bright: colors.bright_foreground,
            text_muted: colors.muted.or(colors.dark_foreground),
            accent: colors.accent,
            selection: colors.selection,
            danger: colors.red,
            warning: colors.yellow,
            success: colors.green,
        },
        layout: ThemeLayout::default(),
        dark: true,
    })
}

/// Where an Omarchy installation can keep the link to its active theme.
///
/// Omarchy has moved this between XDG directories over its life, and Teral cannot ask
/// it, so Teral looks in each place rather than betting on one. `TERAL_OMARCHY_THEME`
/// overrides all of them, which is also how the behaviour is exercised off Omarchy.
fn omarchy_theme_links() -> Vec<PathBuf> {
    if let Some(override_path) = env::var_os("TERAL_OMARCHY_THEME").filter(|v| !v.is_empty()) {
        return vec![PathBuf::from(override_path)];
    }

    vec![
        config_home().join("omarchy/current/theme"),
        state_home().join("omarchy/current/theme"),
        data_home().join("omarchy/current/theme"),
    ]
}

/// The directory holding Omarchy's active theme, when Teral is running under Omarchy.
pub fn omarchy_active_theme_dir() -> Option<PathBuf> {
    omarchy_theme_links().into_iter().find(|path| path.is_dir())
}

/// Paths to watch so a theme change restyles a running Teral.
///
/// Omarchy switches themes by repointing a symlink, and a monitor on the link follows
/// it to the theme it pointed at when the monitor was created — so the swap is invisible
/// there. Watching the directory that *holds* the link is what sees the switch; watching
/// the theme directory itself is what sees a theme edited in place. Teral watches both.
pub fn omarchy_watch_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    for link in omarchy_theme_links() {
        if let Some(parent) = link.parent().filter(|parent| parent.is_dir()) {
            paths.push(parent.to_path_buf());
        }
    }

    if let Some(directory) = omarchy_active_theme_dir()
        && let Ok(resolved) = fs::canonicalize(&directory)
    {
        paths.push(resolved);
    }

    paths.dedup();
    paths
}

// ------------------------------------------------------- desktop integration ----

/// Ask the desktop whether it prefers a dark appearance.
///
/// The FreeDesktop appearance portal is the cross-desktop answer; GTK's own settings
/// are the fallback for desktops that do not run a portal.
pub fn system_prefers_dark() -> bool {
    if let Some(value) = portal_setting("color-scheme").and_then(|value| value.get::<u32>()) {
        // 1 = prefer dark, 2 = prefer light, 0 = no preference.
        match value {
            1 => return true,
            2 => return false,
            _ => {}
        }
    }

    gtk::Settings::default().is_some_and(|settings| {
        settings.is_gtk_application_prefer_dark_theme()
            || settings
                .gtk_theme_name()
                .is_some_and(|name| name.to_lowercase().contains("dark"))
    })
}

/// Derive Teral's palette from the colours the running GTK theme actually uses.
///
/// GTK themes publish a small set of named colours. Teral reads those and computes the
/// surfaces, borders and muted text it needs from them, so "Follow the system" adopts
/// the desktop's real colours rather than only its light/dark preference.
fn system_palette(dark: bool) -> Option<ThemeColors> {
    if !gtk::is_initialized() {
        return None;
    }

    // `lookup_color` is the only way GTK exposes a theme's named colours to code.
    #[allow(deprecated)]
    let (background, foreground, base, selected, borders) = {
        let context = gtk::Label::new(None).style_context();
        (
            context.lookup_color("theme_bg_color")?,
            context.lookup_color("theme_fg_color")?,
            context.lookup_color("theme_base_color"),
            context
                .lookup_color("accent_bg_color")
                .or_else(|| context.lookup_color("theme_selected_bg_color")),
            context.lookup_color("borders"),
        )
    };

    // Surfaces step away from the window background in the direction of the palette.
    let (surface, elevated, alt, border) = if dark {
        (0.82, 1.16, 1.32, 1.55)
    } else {
        (1.03, 1.06, 0.94, 0.88)
    };

    let base = base.unwrap_or(background);
    let border = borders.unwrap_or_else(|| shade(background, border));

    Some(ThemeColors {
        background: Some(hex(background)),
        surface: Some(hex(shade(background, surface))),
        surface_alt: Some(hex(shade(background, alt))),
        elevated: Some(hex(shade(base, elevated))),
        border: Some(hex(border)),
        text: Some(hex(foreground)),
        text_bright: Some(hex(shade(foreground, if dark { 1.15 } else { 0.7 }))),
        text_muted: Some(hex(mix(foreground, background, 0.45))),
        accent: selected.map(hex),
        selection: selected.map(|accent| hex(mix(accent, background, 0.8))),
        danger: None,
        warning: None,
        success: None,
    })
}

/// Multiply a colour's channels, keeping it inside the sRGB range.
fn shade(color: gtk::gdk::RGBA, factor: f32) -> gtk::gdk::RGBA {
    gtk::gdk::RGBA::new(
        (color.red() * factor).clamp(0.0, 1.0),
        (color.green() * factor).clamp(0.0, 1.0),
        (color.blue() * factor).clamp(0.0, 1.0),
        1.0,
    )
}

/// Blend `to` into `from` by `amount`.
fn mix(from: gtk::gdk::RGBA, to: gtk::gdk::RGBA, amount: f32) -> gtk::gdk::RGBA {
    let blend = |a: f32, b: f32| a + (b - a) * amount.clamp(0.0, 1.0);
    gtk::gdk::RGBA::new(
        blend(from.red(), to.red()),
        blend(from.green(), to.green()),
        blend(from.blue(), to.blue()),
        1.0,
    )
}

fn hex(color: gtk::gdk::RGBA) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(f64::from(color.red())),
        channel(f64::from(color.green())),
        channel(f64::from(color.blue()))
    )
}

/// The desktop's accent colour, when it publishes one.
fn system_accent() -> Option<String> {
    let value = portal_setting("accent-color")?;
    let (red, green, blue) = value.get::<(f64, f64, f64)>()?;
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        channel(red),
        channel(green),
        channel(blue)
    ))
}

fn channel(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Read one `org.freedesktop.appearance` setting through the desktop portal.
fn portal_setting(key: &str) -> Option<glib::Variant> {
    let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()?;
    let reply = connection
        .call_sync(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
            "ReadOne",
            Some(&("org.freedesktop.appearance", key).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            400,
            gio::Cancellable::NONE,
        )
        .ok()?;

    reply.child_value(0).get::<glib::Variant>()
}

// -------------------------------------------------------------------- paths ----

pub fn config_home() -> PathBuf {
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

/// The user's data directory, used for Teral's own persisted state.
pub fn data_home() -> PathBuf {
    env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from(".local/share"))
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

/// True for the `#rrggbb` and `#rrggbbaa` forms Teral themes may use.
pub fn valid_color(value: &str) -> bool {
    let Some(hex) = value.strip_prefix('#') else {
        return false;
    };

    matches!(hex.len(), 6 | 8) && hex.chars().all(|character| character.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_built_in_themes_parse() {
        for source in [DARK_THEME, LIGHT_THEME] {
            let theme = toml::from_str::<ThemeConfig>(source).expect("built-in theme parses");
            assert_eq!(theme.version, Some(THEME_FORMAT_VERSION));
        }
    }

    #[test]
    fn every_color_role_resolves_in_both_palettes() {
        for source in [DARK_THEME, LIGHT_THEME] {
            let mut theme = toml::from_str::<ThemeConfig>(source).expect("built-in theme");
            theme.sanitize();
            for role in ColorRole::ALL {
                assert!(valid_color(theme.color(role)), "{role:?} must be a colour");
            }
        }
    }

    #[test]
    fn user_overrides_beat_the_built_in_palette() {
        let config = Config {
            accent: Some("#123456".to_owned()),
            ..Config::default()
        };
        let theme = ThemeConfig::resolve(&config);
        assert_eq!(theme.color(ColorRole::Accent), "#123456");
    }

    #[test]
    fn invalid_colors_fall_back_instead_of_breaking() {
        let mut theme = ThemeConfig {
            colors: ThemeColors {
                accent: Some("not-a-colour".to_owned()),
                ..ThemeColors::default()
            },
            ..ThemeConfig::default()
        };
        theme.sanitize();
        assert_eq!(theme.color(ColorRole::Accent), ColorRole::Accent.fallback());
    }

    #[test]
    fn layout_values_are_clamped() {
        let config = Config {
            layout: ThemeLayout {
                sidebar_width: Some(4000),
                ..ThemeLayout::default()
            },
            ..Config::default()
        };
        assert_eq!(ThemeConfig::resolve(&config).sidebar_width(), 420);
    }

    #[test]
    fn layout_entries_only_list_values_that_are_set() {
        let layout = ThemeLayout {
            grid_icon_size: Some(72),
            ..ThemeLayout::default()
        };
        assert_eq!(layout.entries(), vec![("grid_icon_size", 72)]);
    }
}
