//! File operations.
//!
//! Metadata-only operations use GIO's asynchronous APIs. Recursive copies and moves run
//! on a worker thread through [`gio::spawn_blocking`]. The current transfer engine uses
//! conflict-renamed destinations, but it remains under safety hardening for the 0.1 release.

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

/// True when both paths live on the same filesystem, so a move can be a rename.
pub fn same_filesystem(source: &Path, destination: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let source = source
        .parent()
        .and_then(|parent| fs::metadata(parent).ok())
        .map(|metadata| metadata.dev());
    let destination = fs::metadata(destination)
        .ok()
        .map(|metadata| metadata.dev());

    matches!((source, destination), (Some(a), Some(b)) if a == b)
}

/// Copy entries beside themselves under a free name.
pub async fn duplicate(paths: Vec<PathBuf>) -> TransferReport {
    let cancel = CancelFlag::new();
    gio::spawn_blocking(move || {
        let mut report = TransferReport::default();
        for source in paths {
            let Some(parent) = source.parent().map(Path::to_path_buf) else {
                report.failures.push(format!(
                    "{}: it has no parent folder",
                    display_name(&source)
                ));
                continue;
            };

            match duplicate_one(&source, &parent, &cancel) {
                Ok(()) => report.succeeded += 1,
                Err(error) => report
                    .failures
                    .push(format!("{}: {error}", display_name(&source))),
            }
        }
        report
    })
    .await
    .unwrap_or_else(|_| TransferReport {
        failures: vec!["the copy worker stopped unexpectedly".to_owned()],
        ..TransferReport::default()
    })
}

fn duplicate_one(source: &Path, parent: &Path, cancel: &CancelFlag) -> io::Result<()> {
    let Some(name) = source.file_name() else {
        return Err(io::Error::other("the source has no file name"));
    };
    let target = unique_destination(parent, name)?;
    copy_recursively(source, &target, cancel)
}

// -------------------------------------------------------------------- trash ----

/// The FreeDesktop trash directory for the home filesystem.
pub fn trash_root() -> PathBuf {
    crate::theme::data_home().join("Trash")
}

/// True when `path` is inside the trash, so restore and emptying make sense.
pub fn is_in_trash(path: &Path) -> bool {
    path.starts_with(trash_root().join("files"))
}

/// Restore trashed entries to the locations recorded in their `.trashinfo` files.
pub async fn restore_from_trash(paths: Vec<PathBuf>) -> TransferReport {
    gio::spawn_blocking(move || {
        let mut report = TransferReport::default();
        for path in paths {
            match restore_one(&path) {
                Ok(()) => report.succeeded += 1,
                Err(error) => report
                    .failures
                    .push(format!("{}: {error}", display_name(&path))),
            }
        }
        report
    })
    .await
    .unwrap_or_else(|_| TransferReport {
        failures: vec!["the restore worker stopped unexpectedly".to_owned()],
        ..TransferReport::default()
    })
}

fn restore_one(path: &Path) -> io::Result<()> {
    let Some(name) = path.file_name() else {
        return Err(io::Error::other("the entry has no file name"));
    };

    let mut info_name = OsString::from(name);
    info_name.push(".trashinfo");
    let info_path = trash_root().join("info").join(&info_name);

    let info = fs::read_to_string(&info_path)
        .map_err(|error| io::Error::other(format!("its trash record is unreadable: {error}")))?;

    let original = info
        .lines()
        .find_map(|line| line.strip_prefix("Path="))
        .map(percent_decode)
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("its trash record has no original path"))?;

    // A relative Path= is relative to the trash directory's filesystem root.
    let original = if original.is_absolute() {
        original
    } else {
        crate::theme::home_dir()
            .unwrap_or_else(|| PathBuf::from("/"))
            .join(original)
    };

    let Some(parent) = original.parent() else {
        return Err(io::Error::other("its original folder is unknown"));
    };
    fs::create_dir_all(parent)?;

    let target = if original.symlink_metadata().is_ok() {
        let name = original
            .file_name()
            .ok_or_else(|| io::Error::other("its original name is unknown"))?;
        unique_destination(parent, name)?
    } else {
        original
    };

    if fs::rename(path, &target).is_err() {
        copy_recursively(path, &target, &CancelFlag::new())?;
        remove_recursively(path)?;
    }

    let _ = fs::remove_file(&info_path);
    Ok(())
}

/// Delete entries without going through the trash.
pub async fn delete_permanently(paths: Vec<PathBuf>) -> TransferReport {
    gio::spawn_blocking(move || {
        let mut report = TransferReport::default();
        for path in paths {
            match remove_with_trash_record(&path) {
                Ok(()) => report.succeeded += 1,
                Err(error) => report
                    .failures
                    .push(format!("{}: {error}", display_name(&path))),
            }
        }
        report
    })
    .await
    .unwrap_or_else(|_| TransferReport {
        failures: vec!["the delete worker stopped unexpectedly".to_owned()],
        ..TransferReport::default()
    })
}

/// Remove an entry, cleaning up its trash record when it lives in the trash.
fn remove_with_trash_record(path: &Path) -> io::Result<()> {
    remove_recursively(path)?;

    if is_in_trash(path)
        && let Some(name) = path.file_name()
    {
        let mut info_name = OsString::from(name);
        info_name.push(".trashinfo");
        let _ = fs::remove_file(trash_root().join("info").join(info_name));
    }

    Ok(())
}

/// Permanently delete everything in the trash.
pub async fn empty_trash() -> TransferReport {
    gio::spawn_blocking(move || {
        let root = trash_root();
        let mut report = TransferReport::default();

        for directory in ["files", "info"] {
            let directory = root.join(directory);
            let Ok(entries) = fs::read_dir(&directory) else {
                continue;
            };
            for entry in entries.flatten() {
                match remove_recursively(&entry.path()) {
                    Ok(()) => report.succeeded += 1,
                    Err(error) => report
                        .failures
                        .push(format!("{}: {error}", display_name(&entry.path()))),
                }
            }
        }

        report
    })
    .await
    .unwrap_or_else(|_| TransferReport {
        failures: vec!["the delete worker stopped unexpectedly".to_owned()],
        ..TransferReport::default()
    })
}

/// Decode the percent-encoding the trash specification uses for `Path=`.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
            if let Some(byte) = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                decoded.push(byte);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&decoded).into_owned()
}

// ------------------------------------------------------------------ archives ----

/// Content types Teral offers to extract.
const ARCHIVE_TYPES: [&str; 12] = [
    "application/zip",
    "application/x-tar",
    "application/gzip",
    "application/x-gzip",
    "application/x-bzip2",
    "application/x-xz",
    "application/zstd",
    "application/x-7z-compressed",
    "application/vnd.rar",
    "application/x-rar",
    "application/x-rar-compressed",
    "application/x-compressed-tar",
];

/// True when Teral knows how to unpack this entry.
pub fn is_archive(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|value| ARCHIVE_TYPES.contains(&value))
}

/// The name an archive's contents should be extracted into.
pub fn archive_stem(path: &Path) -> OsString {
    let mut stem = path.file_stem().unwrap_or_default().to_os_string();
    // "project.tar.gz" should become "project", not "project.tar".
    if Path::new(&stem).extension().is_some_and(|ext| ext == "tar") {
        stem = Path::new(&stem)
            .file_stem()
            .unwrap_or_default()
            .to_os_string();
    }
    if stem.is_empty() {
        stem = OsString::from("extracted");
    }
    stem
}

/// Build the command that unpacks `archive` into `destination`.
///
/// `bsdtar` handles every format Teral offers, so it is preferred; the per-format tools
/// are the fallback for systems that only have those.
fn extract_command(
    archive: &Path,
    destination: &Path,
    content_type: &str,
) -> Option<Vec<OsString>> {
    let arg = |value: &str| OsString::from(value);
    let program = |name: &str| glib::find_program_in_path(name);

    if let Some(bsdtar) = program("bsdtar") {
        return Some(vec![
            bsdtar.into_os_string(),
            arg("-x"),
            arg("-f"),
            archive.as_os_str().to_os_string(),
            arg("-C"),
            destination.as_os_str().to_os_string(),
        ]);
    }

    match content_type {
        "application/zip" => program("unzip").map(|unzip| {
            vec![
                unzip.into_os_string(),
                arg("-o"),
                arg("-q"),
                archive.as_os_str().to_os_string(),
                arg("-d"),
                destination.as_os_str().to_os_string(),
            ]
        }),
        "application/x-7z-compressed" => program("7z").or_else(|| program("7za")).map(|seven| {
            let mut output = OsString::from("-o");
            output.push(destination.as_os_str());
            vec![
                seven.into_os_string(),
                arg("x"),
                arg("-y"),
                output,
                archive.as_os_str().to_os_string(),
            ]
        }),
        "application/vnd.rar" | "application/x-rar" | "application/x-rar-compressed" => {
            program("unrar").map(|unrar| {
                vec![
                    unrar.into_os_string(),
                    arg("x"),
                    arg("-y"),
                    archive.as_os_str().to_os_string(),
                    destination.as_os_str().to_os_string(),
                ]
            })
        }
        _ => program("tar").map(|tar| {
            vec![
                tar.into_os_string(),
                arg("-x"),
                arg("-f"),
                archive.as_os_str().to_os_string(),
                arg("-C"),
                destination.as_os_str().to_os_string(),
            ]
        }),
    }
}

/// Unpack `archive` into `destination`, which is created if it does not exist.
pub async fn extract(
    archive: PathBuf,
    destination: PathBuf,
    content_type: String,
) -> Result<PathBuf, String> {
    let Some(command) = extract_command(&archive, &destination, &content_type) else {
        return Err("no extraction tool was found; install bsdtar, unzip, 7z or unrar".to_owned());
    };

    if let Err(error) = fs::create_dir_all(&destination) {
        return Err(format!("{}: {error}", destination.display()));
    }

    let arguments: Vec<&std::ffi::OsStr> = command.iter().map(OsString::as_os_str).collect();
    let process = gio::Subprocess::newv(
        &arguments,
        gio::SubprocessFlags::STDOUT_SILENCE | gio::SubprocessFlags::STDERR_PIPE,
    )
    .map_err(|error| error.message().trim().to_owned())?;

    let (_stdout, stderr) = process
        .communicate_future(None)
        .await
        .map_err(|error| error.message().trim().to_owned())?;

    if process.exit_status() == 0 {
        return Ok(destination);
    }

    let message = stderr
        .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_owned())
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| format!("extraction failed with status {}", process.exit_status()));
    Err(message)
}

/// The name a new archive of `paths` should take, inside `directory`.
///
/// One item keeps its own name; several become the folder's name, which is what every
/// other file manager does and what people expect to find afterwards.
pub fn archive_name(directory: &Path, paths: &[PathBuf]) -> OsString {
    let mut name = match paths {
        [single] => single
            .file_name()
            .map(std::ffi::OsStr::to_os_string)
            .unwrap_or_else(|| OsString::from("archive")),
        _ => directory
            .file_name()
            .map(std::ffi::OsStr::to_os_string)
            .unwrap_or_else(|| OsString::from("archive")),
    };
    name.push(".zip");
    name
}

/// True when a tool that can write an archive is installed.
///
/// Teral only offers Compress when something on this machine can carry it out; an
/// action that always fails is worse than one that is absent.
pub fn can_compress() -> bool {
    glib::find_program_in_path("bsdtar").is_some() || glib::find_program_in_path("zip").is_some()
}

/// Build the command that packs `names` — relative to `directory` — into `archive`.
///
/// `bsdtar` is preferred for the same reason it is preferred for extraction: it is one
/// tool for every format. `zip` is the fallback, since zip is the format Teral writes.
fn compress_command(directory: &Path, archive: &Path, names: &[OsString]) -> Option<Vec<OsString>> {
    let arg = |value: &str| OsString::from(value);

    let mut command = if let Some(bsdtar) = glib::find_program_in_path("bsdtar") {
        vec![
            bsdtar.into_os_string(),
            arg("-a"),
            arg("-c"),
            arg("-f"),
            archive.as_os_str().to_os_string(),
            arg("-C"),
            directory.as_os_str().to_os_string(),
        ]
    } else {
        let zip = glib::find_program_in_path("zip")?;
        vec![
            zip.into_os_string(),
            arg("-r"),
            arg("-q"),
            archive.as_os_str().to_os_string(),
        ]
    };

    command.extend(names.iter().cloned());
    Some(command)
}

/// Pack `paths` into a new zip archive beside them, and return the archive.
///
/// The output path is conflict-renamed before the archiver starts, and `zip` is run with
/// the folder as its working directory so the archive holds plain relative names.
pub async fn compress(directory: PathBuf, paths: Vec<PathBuf>) -> Result<PathBuf, String> {
    if paths.is_empty() {
        return Err("nothing was selected to compress".to_owned());
    }

    let archive = unique_destination(&directory, &archive_name(&directory, &paths))
        .map_err(|error| format!("{}: {error}", directory.display()))?;

    let names: Vec<OsString> = paths
        .iter()
        .map(|path| path.file_name().unwrap_or(path.as_os_str()).to_os_string())
        .collect();

    let Some(command) = compress_command(&directory, &archive, &names) else {
        return Err("no archiving tool was found; install bsdtar or zip".to_owned());
    };

    let arguments: Vec<&std::ffi::OsStr> = command.iter().map(OsString::as_os_str).collect();
    let launcher = gio::SubprocessLauncher::new(
        gio::SubprocessFlags::STDOUT_SILENCE | gio::SubprocessFlags::STDERR_PIPE,
    );
    launcher.set_cwd(&directory);
    let process = launcher
        .spawn(&arguments)
        .map_err(|error| error.message().trim().to_owned())?;

    let (_stdout, stderr) = process
        .communicate_future(None)
        .await
        .map_err(|error| error.message().trim().to_owned())?;

    if process.exit_status() == 0 {
        return Ok(archive);
    }

    // A failed run can leave a half-written archive behind; it is Teral's file, so
    // Teral removes it rather than leaving a broken zip in the user's folder.
    let _ = fs::remove_file(&archive);

    let message = stderr
        .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_owned())
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| format!("compression failed with status {}", process.exit_status()));
    Err(message)
}

/// Launch an entry with the desktop's default application.
pub fn open(path: &Path) -> Result<(), glib::Error> {
    let uri = gio::File::for_path(path).uri();
    gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>)
}

thread_local! {
    /// Querying the desktop's application database costs real time, and the details
    /// panel asks about the same handful of content types over and over.
    static APPLICATIONS: std::cell::RefCell<std::collections::HashMap<String, Vec<gio::AppInfo>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Applications the desktop recommends for an entry's content type.
pub fn applications_for(content_type: Option<&str>) -> Vec<gio::AppInfo> {
    let Some(content_type) = content_type else {
        return Vec::new();
    };

    APPLICATIONS.with_borrow_mut(|cache| {
        if let Some(applications) = cache.get(content_type) {
            return applications.clone();
        }

        let mut applications = gio::AppInfo::recommended_for_type(content_type);
        if applications.is_empty() {
            applications = gio::AppInfo::all_for_type(content_type);
        }
        cache.insert(content_type.to_owned(), applications.clone());
        applications
    })
}

/// True when marking this entry executable is a sensible thing to offer.
///
/// Folders already need their execute bit, and marking a document executable is never
/// what anyone wants, so the action is limited to the files it makes sense for.
pub fn can_be_executable(is_directory: bool, content_type: Option<&str>) -> bool {
    const RUNNABLE: [&str; 8] = [
        "application/x-executable",
        "application/x-sharedlib",
        "application/x-shellscript",
        "application/x-perl",
        "application/x-python-code",
        "text/x-shellscript",
        "text/x-python",
        "text/x-script",
    ];

    if is_directory {
        return false;
    }

    content_type.is_some_and(|value| {
        RUNNABLE.contains(&value) || value.starts_with("text/x-") && value.contains("script")
    })
}

/// Add or remove the execute bits on a set of files, leaving read/write alone.
pub async fn set_executable(paths: Vec<PathBuf>, executable: bool) -> TransferReport {
    gio::spawn_blocking(move || {
        use std::os::unix::fs::PermissionsExt;

        let mut report = TransferReport::default();
        for path in paths {
            let result = fs::metadata(&path).and_then(|metadata| {
                let mode = metadata.permissions().mode();
                // Execute follows read: a file readable by a group becomes runnable by
                // that group too, which is what chmod +x does.
                let executable_bits = (mode & 0o444) >> 2;
                let updated = if executable {
                    mode | executable_bits
                } else {
                    mode & !0o111
                };
                fs::set_permissions(&path, fs::Permissions::from_mode(updated))
            });

            match result {
                Ok(()) => report.succeeded += 1,
                Err(error) => report
                    .failures
                    .push(format!("{}: {error}", display_name(&path))),
            }
        }
        report
    })
    .await
    .unwrap_or_else(|_| TransferReport {
        failures: vec!["the permission worker stopped unexpectedly".to_owned()],
        ..TransferReport::default()
    })
}

/// Launch `path` with a specific application.
pub fn open_with(application: &gio::AppInfo, path: &Path) -> Result<(), glib::Error> {
    let file = gio::File::for_path(path);
    application.launch(&[file], None::<&gio::AppLaunchContext>)
}

/// Open the user's terminal emulator in `directory`.
///
/// Teral's own setting wins, then `TERAL_TERMINAL`, then the first terminal found on
/// `PATH`, so the behaviour stays configurable without a Teral-only registry.
pub fn open_terminal(directory: &Path) -> Result<(), String> {
    let setting = crate::config::current().terminal;
    let configured = Some(setting.trim().to_owned())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("TERAL_TERMINAL")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        });

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
    fn an_archive_is_named_after_what_goes_into_it() {
        let folder = Path::new("/home/someone/Reports");

        let single = vec![folder.join("March.pdf")];
        assert_eq!(
            archive_name(folder, &single),
            OsString::from("March.pdf.zip")
        );

        let several = vec![folder.join("March.pdf"), folder.join("April.pdf")];
        assert_eq!(
            archive_name(folder, &several),
            OsString::from("Reports.zip")
        );
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
    fn only_runnable_files_are_offered_the_execute_bit() {
        assert!(can_be_executable(false, Some("application/x-shellscript")));
        assert!(can_be_executable(false, Some("text/x-python")));
        assert!(!can_be_executable(false, Some("text/plain")));
        assert!(!can_be_executable(false, Some("image/png")));
        assert!(!can_be_executable(true, Some("inode/directory")));
        assert!(!can_be_executable(false, None));
    }

    #[test]
    fn archive_stems_drop_both_extensions() {
        assert_eq!(archive_stem(Path::new("/tmp/project.tar.gz")), "project");
        assert_eq!(archive_stem(Path::new("/tmp/photos.zip")), "photos");
        assert_eq!(archive_stem(Path::new("/tmp/archive")), "archive");
    }

    #[test]
    fn archives_are_recognised_by_content_type() {
        assert!(is_archive(Some("application/zip")));
        assert!(!is_archive(Some("text/plain")));
        assert!(!is_archive(None));
    }

    #[test]
    fn trash_paths_are_percent_decoded() {
        assert_eq!(
            percent_decode("/home/a/My%20File.txt"),
            "/home/a/My File.txt"
        );
        assert_eq!(percent_decode("/home/a/100%"), "/home/a/100%");
        assert_eq!(percent_decode("/home/a/plain"), "/home/a/plain");
    }

    #[test]
    fn duplicating_keeps_the_original() {
        let dir = scratch("duplicate");
        let source = dir.join("report.txt");
        fs::write(&source, b"payload").expect("write");

        let report = duplicate_one(&source, &dir, &CancelFlag::new());
        assert!(report.is_ok(), "{report:?}");
        assert!(source.exists());
        assert_eq!(
            fs::read(dir.join("report (copy).txt")).expect("duplicate"),
            b"payload"
        );
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
