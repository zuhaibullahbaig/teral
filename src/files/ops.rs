//! File operations.
//!
//! Metadata-only operations use GIO's asynchronous APIs. Recursive copies and moves run
//! on a worker thread through [`gio::spawn_blocking`] so a large transfer can never
//! stall the GTK main loop, and they resolve name conflicts by picking a free name
//! rather than by overwriting anything.

use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether a pending transfer copies or moves its sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    Copy,
    Move,
}

impl TransferKind {
    pub const fn verb(self) -> &'static str {
        match self {
            Self::Copy => "Copying",
            Self::Move => "Moving",
        }
    }

    pub const fn past_tense(self) -> &'static str {
        match self {
            Self::Copy => "Copied",
            Self::Move => "Moved",
        }
    }
}

/// Sources staged by Copy or Move, waiting for a Paste.
#[derive(Debug, Clone)]
pub struct Clipboard {
    pub kind: TransferKind,
    pub sources: Vec<PathBuf>,
}

/// Outcome of a completed transfer.
#[derive(Debug, Default)]
pub struct TransferReport {
    pub succeeded: usize,
    pub failures: Vec<String>,
    pub cancelled: bool,
}

/// Cancellation flag shared with a running transfer.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Create a directory, reporting the GIO error if it already exists or is refused.
pub async fn create_directory(parent: &Path, name: &OsStr) -> Result<PathBuf, glib::Error> {
    let path = parent.join(name);
    gio::File::for_path(&path)
        .make_directory_future(glib::Priority::DEFAULT)
        .await?;
    Ok(path)
}

/// Rename an entry through GIO so display-name semantics stay correct.
pub async fn rename(path: &Path, new_name: &str) -> Result<PathBuf, glib::Error> {
    let renamed = gio::File::for_path(path)
        .set_display_name_future(new_name, glib::Priority::DEFAULT)
        .await?;

    Ok(renamed
        .path()
        .unwrap_or_else(|| path.with_file_name(new_name)))
}

/// Move entries to the FreeDesktop trash.
pub async fn trash(paths: Vec<PathBuf>) -> TransferReport {
    let mut report = TransferReport::default();

    for path in paths {
        match gio::File::for_path(&path)
            .trash_future(glib::Priority::DEFAULT)
            .await
        {
            Ok(()) => report.succeeded += 1,
            Err(error) => report.failures.push(format!(
                "{}: {}",
                display_name(&path),
                error.message().trim()
            )),
        }
    }

    report
}

/// Copy or move `sources` into `destination` on a worker thread.
pub async fn transfer(
    kind: TransferKind,
    sources: Vec<PathBuf>,
    destination: PathBuf,
    cancel: CancelFlag,
) -> TransferReport {
    gio::spawn_blocking(move || run_transfer(kind, &sources, &destination, &cancel))
        .await
        .unwrap_or_else(|_| TransferReport {
            failures: vec!["the transfer worker stopped unexpectedly".to_owned()],
            ..TransferReport::default()
        })
}

fn run_transfer(
    kind: TransferKind,
    sources: &[PathBuf],
    destination: &Path,
    cancel: &CancelFlag,
) -> TransferReport {
    let mut report = TransferReport::default();

    for source in sources {
        if cancel.is_cancelled() {
            report.cancelled = true;
            break;
        }

        if let Err(error) = transfer_one(kind, source, destination, cancel) {
            report
                .failures
                .push(format!("{}: {error}", display_name(source)));
        } else {
            report.succeeded += 1;
        }
    }

    report
}

fn transfer_one(
    kind: TransferKind,
    source: &Path,
    destination_dir: &Path,
    cancel: &CancelFlag,
) -> io::Result<()> {
    let Some(name) = source.file_name() else {
        return Err(io::Error::other("the source has no file name"));
    };

    if source.parent() == Some(destination_dir) && kind == TransferKind::Move {
        return Ok(());
    }

    if destination_dir.starts_with(source) {
        return Err(io::Error::other("a folder cannot be copied into itself"));
    }

    let target = unique_destination(destination_dir, name)?;

    if kind == TransferKind::Move && fs::rename(source, &target).is_ok() {
        return Ok(());
    }

    copy_recursively(source, &target, cancel)?;

    if kind == TransferKind::Move && !cancel.is_cancelled() {
        remove_recursively(source)?;
    }

    Ok(())
}

fn copy_recursively(source: &Path, target: &Path, cancel: &CancelFlag) -> io::Result<()> {
    if cancel.is_cancelled() {
        return Err(io::Error::other("cancelled"));
    }

    let metadata = fs::symlink_metadata(source)?;

    if metadata.file_type().is_symlink() {
        let link = fs::read_link(source)?;
        return std::os::unix::fs::symlink(link, target);
    }

    if metadata.is_dir() {
        fs::create_dir(target)?;
        for child in fs::read_dir(source)? {
            let child = child?;
            copy_recursively(&child.path(), &target.join(child.file_name()), cancel)?;
        }
        return Ok(());
    }

    fs::copy(source, target).map(|_| ())
}

fn remove_recursively(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Pick a free path inside `directory`, never replacing an existing entry.
fn unique_destination(directory: &Path, name: &OsStr) -> io::Result<PathBuf> {
    let candidate = directory.join(name);
    if candidate.symlink_metadata().is_err() {
        return Ok(candidate);
    }

    let path = Path::new(name);
    let stem = path.file_stem().unwrap_or(name);
    let extension = path.extension();

    for attempt in 1..10_000u32 {
        let mut candidate_name = OsString::from(stem);
        candidate_name.push(if attempt == 1 {
            " (copy)".to_owned()
        } else {
            format!(" (copy {attempt})")
        });
        if let Some(extension) = extension {
            candidate_name.push(".");
            candidate_name.push(extension);
        }

        let candidate = directory.join(&candidate_name);
        if candidate.symlink_metadata().is_err() {
            return Ok(candidate);
        }
    }

    Err(io::Error::other(
        "could not find an unused name in the destination folder",
    ))
}

/// Launch an entry with the desktop's default application.
pub fn open(path: &Path) -> Result<(), glib::Error> {
    let uri = gio::File::for_path(path).uri();
    gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>)
}

/// Applications the desktop recommends for an entry's content type.
pub fn applications_for(content_type: Option<&str>) -> Vec<gio::AppInfo> {
    let Some(content_type) = content_type else {
        return Vec::new();
    };

    let mut applications = gio::AppInfo::recommended_for_type(content_type);
    if applications.is_empty() {
        applications = gio::AppInfo::all_for_type(content_type);
    }
    applications
}

/// Launch `path` with a specific application.
pub fn open_with(application: &gio::AppInfo, path: &Path) -> Result<(), glib::Error> {
    let file = gio::File::for_path(path);
    application.launch(&[file], None::<&gio::AppLaunchContext>)
}

/// Open the user's terminal emulator in `directory`.
///
/// The command comes from `TERAL_TERMINAL` when set, otherwise from the first terminal
/// found on `PATH`, so the behaviour stays configurable without a Teral-only registry.
pub fn open_terminal(directory: &Path) -> Result<(), String> {
    let configured = std::env::var_os("TERAL_TERMINAL")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);

    const CANDIDATES: [&str; 10] = [
        "ghostty",
        "alacritty",
        "kitty",
        "wezterm",
        "foot",
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "tilix",
        "xterm",
    ];

    let program = configured
        .and_then(|value| glib::find_program_in_path(&value))
        .or_else(|| CANDIDATES.iter().find_map(glib::find_program_in_path))
        .ok_or_else(|| "no terminal emulator was found on PATH".to_owned())?;

    std::process::Command::new(&program)
        .current_dir(directory)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("{}: {error}", program.display()))
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("teral-tests-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch directory");
        dir
    }

    #[test]
    fn unique_destination_leaves_existing_files_alone() {
        let dir = scratch("unique");
        fs::write(dir.join("notes.txt"), b"one").expect("write");

        let target = unique_destination(&dir, OsStr::new("notes.txt")).expect("unique name");
        assert_eq!(target, dir.join("notes (copy).txt"));

        fs::write(&target, b"two").expect("write");
        let next = unique_destination(&dir, OsStr::new("notes.txt")).expect("unique name");
        assert_eq!(next, dir.join("notes (copy 2).txt"));

        // Nothing was overwritten.
        assert_eq!(fs::read(dir.join("notes.txt")).expect("read"), b"one");
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn unique_destination_keeps_the_extension() {
        let dir = scratch("extension");
        fs::write(dir.join("archive.tar.gz"), b"x").expect("write");
        let target = unique_destination(&dir, OsStr::new("archive.tar.gz")).expect("unique name");
        assert_eq!(target, dir.join("archive.tar (copy).gz"));
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn directories_copy_recursively() {
        let dir = scratch("recursive");
        let source = dir.join("source");
        fs::create_dir_all(source.join("nested")).expect("tree");
        fs::write(source.join("nested/file.txt"), b"payload").expect("write");

        let destination = dir.join("destination");
        fs::create_dir(&destination).expect("destination");

        let report = run_transfer(
            TransferKind::Copy,
            std::slice::from_ref(&source),
            &destination,
            &CancelFlag::new(),
        );

        assert_eq!(report.succeeded, 1);
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert_eq!(
            fs::read(destination.join("source/nested/file.txt")).expect("copied file"),
            b"payload"
        );
        assert!(source.exists(), "a copy must leave the source in place");
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn moves_remove_the_source() {
        let dir = scratch("move");
        let source = dir.join("source");
        fs::create_dir(&source).expect("source");
        fs::write(source.join("file.txt"), b"payload").expect("write");

        let destination = dir.join("destination");
        fs::create_dir(&destination).expect("destination");

        let report = run_transfer(
            TransferKind::Move,
            std::slice::from_ref(&source),
            &destination,
            &CancelFlag::new(),
        );

        assert_eq!(report.succeeded, 1);
        assert!(!source.exists());
        assert!(destination.join("source/file.txt").exists());
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_folder_cannot_be_copied_into_itself() {
        let dir = scratch("self");
        let source = dir.join("source");
        fs::create_dir_all(source.join("inner")).expect("tree");

        let report = run_transfer(
            TransferKind::Copy,
            std::slice::from_ref(&source),
            &source.join("inner"),
            &CancelFlag::new(),
        );

        assert_eq!(report.succeeded, 0);
        assert_eq!(report.failures.len(), 1);
        fs::remove_dir_all(&dir).expect("cleanup");
    }
}
