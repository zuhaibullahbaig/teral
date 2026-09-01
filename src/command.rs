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
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
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
pub async fn run(
    terminal: vte::Terminal,
    command: String,
    directory: PathBuf,
) -> Result<glib::Pid, String> {
    let directory = directory
        .to_str()
        .ok_or_else(|| "Quick Command cannot use this non-UTF-8 directory with VTE".to_owned())?;
    let mut arguments = shell_argv()?;
    arguments.push("-c".to_owned());
    arguments.push(command);
    let argv: Vec<&str> = arguments.iter().map(String::as_str).collect();

    terminal
        .spawn_future(
        vte::PtyFlags::DEFAULT,
        Some(directory),
        &argv,
        &[],
        glib::SpawnFlags::DEFAULT,
        || {},
        -1,
    )
        .await
        .map_err(|error| error.message().to_owned())
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

pub fn shell_argv() -> Result<Vec<String>, String> {
    let spec = shell();
    let argv = parse_program_spec(&spec)?;
    validate_program(&argv)?;
    Ok(argv)
}

/// Parse an executable plus literal arguments without invoking a shell.
pub fn parse_program_spec(spec: &str) -> Result<Vec<String>, String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in spec.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
        } else if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            } else {
                current.push(character);
            }
        } else if character.is_whitespace() && quote.is_none() {
            if !current.is_empty() {
                arguments.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }
    if escaped || quote.is_some() {
        return Err("the command contains an unfinished quote or escape".to_owned());
    }
    if !current.is_empty() {
        arguments.push(current);
    }
    if arguments.is_empty() {
        return Err("an executable is required".to_owned());
    }
    Ok(arguments)
}

pub fn validate_program(arguments: &[String]) -> Result<(), String> {
    let program = arguments
        .first()
        .ok_or_else(|| "an executable is required".to_owned())?;
    if glib::find_program_in_path(program).is_none() {
        return Err(format!("{program} is not an executable on PATH"));
    }
    Ok(())
}

/// Force-stop the process group VTE created for a Quick Command.
///
/// Killing only the shell can leave a foreground pipeline running after the console
/// closes. VTE starts the child as the leader of its terminal process group, so a
/// negative PID addresses the shell and every child attached to that command. If the
/// group has already disappeared, fall back to the individual PID so a race cannot
/// turn Force Stop into a no-op.
pub fn force_stop(pid: glib::Pid) -> Result<(), String> {
    let pid = pid.0;
    let group = Command::new("kill")
        .arg("-KILL")
        .arg("--")
        .arg(format!("-{pid}"))
        .status()
        .map_err(|error| format!("could not start kill: {error}"))?;
    if group.success() {
        return Ok(());
    }

    let process = Command::new("kill")
        .arg("-KILL")
        .arg("--")
        .arg(pid.to_string())
        .status()
        .map_err(|error| format!("could not start kill: {error}"))?;
    if process.success() {
        Ok(())
    } else {
        Err(format!("kill exited with status {process}"))
    }
}

const HISTORY_LIMIT: usize = 100;

#[derive(Default, Deserialize)]
struct RawHistory {
    #[serde(default)]
    command: Vec<String>,
}

fn history_path() -> std::path::PathBuf {
    crate::theme::data_home().join("teral/command-history.toml")
}

/// Command history stores only commands typed by the user, never output or paths.
pub fn load_history() -> Result<Vec<String>, String> {
    let path = history_path();
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("could not read {}: {error}", path.display())),
    };
    let parsed: RawHistory = toml::from_str(&raw)
        .map_err(|error| format!("could not parse {}: {error}", path.display()))?;
    Ok(parsed
        .command
        .into_iter()
        .filter(|command| !command.trim().is_empty())
        .rev()
        .take(HISTORY_LIMIT)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect())
}

pub fn save_history(commands: &[String]) -> Result<(), String> {
    let mut document = String::from("version = 1\ncommand = [\n");
    for command in commands.iter().rev().take(HISTORY_LIMIT).rev() {
        document.push_str(&format!(
            "  \"{}\",\n",
            command.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
        ));
    }
    document.push_str("]\n");
    crate::persistence::atomic_write(&history_path(), document.as_bytes())
        .map_err(|error| error.to_string())
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

    #[test]
    fn executable_specs_keep_arguments_without_shell_expansion() {
        assert_eq!(
            parse_program_spec("kitty --title 'Files here'").expect("parse"),
            ["kitty", "--title", "Files here"]
        );
        assert!(parse_program_spec("kitty 'unfinished").is_err());
    }
}
