//! Quick Command: run a shell command with the browsed directory as its working
//! directory.
//!
//! Only commands the user types are ever executed, the process is spawned
//! asynchronously so the UI cannot freeze, and Teral never escalates privileges.

use gtk::gio;
use gtk::glib;
use std::path::Path;

/// Result of a finished Quick Command.
#[derive(Debug)]
pub struct CommandOutput {
    pub text: String,
    pub exit_status: i32,
}

impl CommandOutput {
    pub fn succeeded(&self) -> bool {
        self.exit_status == 0
    }
}

/// A running Quick Command.
#[derive(Debug, Clone)]
pub struct RunningCommand {
    process: gio::Subprocess,
}

impl RunningCommand {
    /// Spawn `command` through the configured shell inside `directory`.
    pub fn spawn(command: &str, directory: &Path) -> Result<Self, glib::Error> {
        let launcher = gio::SubprocessLauncher::new(
            gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_MERGE,
        );
        launcher.set_cwd(directory);

        let shell = shell();
        let process = launcher.spawn(&[shell.as_ref(), "-c".as_ref(), command.as_ref()])?;
        Ok(Self { process })
    }

    /// Wait for the command to finish and collect its merged output.
    pub async fn wait(&self) -> Result<CommandOutput, glib::Error> {
        let (stdout, _stderr) = self.process.communicate_future(None).await?;

        let text = stdout
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();

        Ok(CommandOutput {
            text,
            exit_status: self.process.exit_status(),
        })
    }

    /// Stop a long-running command.
    pub fn cancel(&self) {
        self.process.force_exit();
    }
}

/// The shell used for Quick Command.
///
/// `TERAL_SHELL` wins, then the user's `SHELL`, then `/bin/sh`.
fn shell() -> std::ffi::OsString {
    std::env::var_os("TERAL_SHELL")
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var_os("SHELL").filter(|value| !value.is_empty()))
        .unwrap_or_else(|| std::ffi::OsString::from("/bin/sh"))
}

/// Trim trailing blank lines so the console does not grow empty space.
pub fn tidy_output(text: &str) -> String {
    text.trim_end_matches(['\n', '\r']).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_trimmed_at_the_end_only() {
        assert_eq!(tidy_output("  a\nb\n\n\n"), "  a\nb");
        assert_eq!(tidy_output(""), "");
    }

    #[test]
    fn a_shell_is_always_available() {
        assert!(!shell().is_empty());
    }
}
