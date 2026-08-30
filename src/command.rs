//! Quick Command: run a shell command with the browsed directory as its working
//! directory, in a real terminal.
//!
//! The command runs on a pseudo-terminal inside Teral's console, so interactive
//! programs — `vim`, `git rebase -i`, `htop`, anything that expects a TTY — work and
//! can be typed into. Only commands the user types are ever executed, the process is
//! spawned asynchronously so the UI cannot freeze, and Teral never escalates
//! privileges.

use gtk::glib;
use gtk::prelude::*;
use std::path::Path;
use vte::prelude::*;

/// Build the terminal widget used by the console.
pub fn build_terminal() -> vte::Terminal {
    let terminal = vte::Terminal::new();
    terminal.add_css_class("teral-terminal");
    terminal.set_scrollback_lines(10_000);
    terminal.set_scroll_on_output(true);
    terminal.set_scroll_on_keystroke(true);
    terminal.set_mouse_autohide(true);
    terminal.set_hexpand(true);
    terminal.set_vexpand(true);
    terminal
}

/// Apply Teral's palette to the terminal so it matches the rest of the window.
pub fn style_terminal(terminal: &vte::Terminal, theme: &crate::theme::ThemeConfig) {
    use crate::theme::ColorRole;

    let color = |role: ColorRole| {
        theme
            .color(role)
            .parse::<gtk::gdk::RGBA>()
            .unwrap_or(gtk::gdk::RGBA::BLACK)
    };

    terminal.set_color_background(&color(ColorRole::Background));
    terminal.set_color_foreground(&color(ColorRole::Text));
    terminal.set_color_cursor(Some(&color(ColorRole::Accent)));
}

/// Start `command` on a pseudo-terminal rooted at `directory`.
///
/// The returned future resolves once the child has been spawned; the terminal's
/// `child-exited` signal reports when it finishes.
pub fn run(
    terminal: &vte::Terminal,
    command: &str,
    directory: &Path,
) -> impl std::future::Future<Output = Result<glib::Pid, glib::Error>> + use<> {
    let shell = shell();
    let directory = directory.to_string_lossy().into_owned();
    let command = command.to_owned();

    terminal.spawn_future(
        vte::PtyFlags::DEFAULT,
        Some(&directory),
        &[&shell, "-c", &command],
        &[],
        glib::SpawnFlags::DEFAULT,
        || {},
        -1,
    )
}

/// The shell used for Quick Command.
///
/// Teral's own setting wins, then `TERAL_SHELL`, then the user's `SHELL`, then
/// `/bin/sh`.
pub fn shell() -> String {
    let configured = crate::config::current().shell;
    if !configured.trim().is_empty() {
        return configured.trim().to_owned();
    }

    std::env::var("TERAL_SHELL")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var("SHELL")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| "/bin/sh".to_owned())
}

/// A short, readable label for a command, for the console header.
pub fn summarise(command: &str, directory: &Path) -> String {
    format!("$ {}   ·   {}", command.trim(), directory.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shell_is_always_available() {
        assert!(!shell().is_empty());
    }

    #[test]
    fn summaries_show_the_command_and_its_folder() {
        let summary = summarise("  git status  ", Path::new("/tmp"));
        assert!(summary.starts_with("$ git status"));
        assert!(summary.ends_with("/tmp"));
    }
}
