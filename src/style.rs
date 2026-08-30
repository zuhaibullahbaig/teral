//! Teral's stylesheet.
//!
//! The stylesheet is written against stable semantic classes (`.teral-*`) and against
//! GTK `@define-color` names derived from [`ColorRole`]. Theme authors therefore only
//! need to supply colors and a handful of layout numbers; they never have to target
//! GTK's internal widget tree.

use crate::theme::{ColorRole, ThemeConfig};
use gtk::CssProvider;
use gtk::gdk::Display;

/// The static part of the stylesheet. Everything colour-related resolves through the
/// `@teral_*` names emitted by [`color_definitions`].
const SHEET: &str = include_str!("../themes/default/teral.css");

/// Install Teral's stylesheet on the default display.
pub fn apply(theme: &ThemeConfig) {
    let provider = CssProvider::new();
    provider.load_from_string(&stylesheet(theme));

    if let Some(display) = Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// Build the complete stylesheet for a resolved theme.
pub fn stylesheet(theme: &ThemeConfig) -> String {
    let mut css = color_definitions(theme);
    css.push_str(SHEET);
    css.push_str(&metrics(theme));
    css
}

fn color_definitions(theme: &ThemeConfig) -> String {
    let mut css = String::new();
    for role in ColorRole::ALL {
        css.push_str(&format!(
            "@define-color {} {};\n",
            role.css_name(),
            theme.color(role)
        ));
    }
    css
}

fn metrics(theme: &ThemeConfig) -> String {
    let radius = theme.radius();
    let row_height = theme.row_height();

    format!(
        ".teral-tile {{ border-radius: {radius}px; }}\n\
         .teral-preview {{ border-radius: {radius}px; }}\n\
         .teral-action {{ border-radius: {}px; }}\n\
         .teral-command {{ border-radius: {}px; }}\n\
         columnview.teral-list row cell {{ min-height: {row_height}px; }}\n",
        radius.saturating_sub(1).max(0),
        radius.saturating_sub(1).max(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stylesheet_defines_every_color_role() {
        let theme = ThemeConfig::default();
        let css = stylesheet(&theme);
        for role in ColorRole::ALL {
            assert!(
                css.contains(&format!("@define-color {} ", role.css_name())),
                "missing {}",
                role.css_name()
            );
        }
    }

    #[test]
    fn stylesheet_is_accepted_by_gtk() {
        if gtk::init().is_err() {
            // No display available in this environment; parsing is exercised elsewhere.
            return;
        }

        let provider = CssProvider::new();
        let failures = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let seen = std::rc::Rc::clone(&failures);
        provider.connect_parsing_error(move |_, section, error| {
            eprintln!("css error at {section}: {error}");
            seen.set(seen.get() + 1);
        });
        provider.load_from_string(&stylesheet(&ThemeConfig::default()));
        assert_eq!(failures.get(), 0, "stylesheet must parse without errors");
    }
}
