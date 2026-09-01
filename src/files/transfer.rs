//! Authoritative copy, move and duplicate jobs.
//!
//! Every entry point uses the same structured job runner. Destination creation is
//! atomic, partial paths are tracked, cancellation is checked while bytes are copied,
//! and callers receive the actual destination selected for every source.

use gtk::gdk;
use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;
use gtk::glib::prelude::StaticType;
use gtk::glib::value::ToValue;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};

pub const GNOME_COPIED_FILES: &str = "x-special/gnome-copied-files";
const URI_LIST: &str = "text/uri-list";
const KDE_CUT_SELECTION: &str = "application/x-kde-cutselection";
const COPY_BUFFER_SIZE: usize = 1024 * 1024;

/// Whether a job copies or moves its sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    Copy,
    Move,
    Link,
}

/// Every filesystem mutation is named up front so trash, restore, deletion and archive
/// work share this result/coordinator contract instead of inventing their own status
/// types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationKind {
    Copy,
    Move,
    Link,
    Duplicate,
    Trash,
    Restore,
    PermanentDelete,
    SetExecutable,
}

impl OperationKind {
    /// How a finished job of this kind describes itself.
    pub const fn past_tense(self) -> &'static str {
        match self {
            Self::Copy => "Copied",
            Self::Move => "Moved",
            Self::Link => "Linked",
            Self::Duplicate => "Duplicated",
            Self::Trash => "Moved to the trash",
            Self::Restore => "Restored",
            Self::PermanentDelete => "Deleted",
            Self::SetExecutable => "Updated",
        }
    }
}

impl From<TransferKind> for OperationKind {
    fn from(value: TransferKind) -> Self {
        match value {
            TransferKind::Copy => Self::Copy,
            TransferKind::Move => Self::Move,
            TransferKind::Link => Self::Link,
        }
    }
}

impl TransferKind {
    pub const fn verb(self) -> &'static str {
        match self {
            Self::Copy => "Copying",
            Self::Move => "Moving",
            Self::Link => "Linking",
        }
    }

    pub const fn past_tense(self) -> &'static str {
        match self {
            Self::Copy => "Copied",
            Self::Move => "Moved",
            Self::Link => "Linked",
        }
    }
}

/// The decision applied when a requested destination already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    Replace,
    Merge,
    RenameIncoming,
    Skip,
    Cancel,
}

/// What kind of collision was found before a transfer started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    File,
    Folder,
    SameEntry,
    SelfMove,
}

/// One source and destination that need a deliberate user decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub kind: ConflictKind,
}

/// Per-entry decisions collected by the conflict UI.
///
/// A race can create another destination after inspection. Such a late conflict uses
/// `fallback`, which is Rename for interactive transfers and the caller's uniform
/// policy for non-interactive jobs and tests.
#[derive(Debug, Clone)]
pub struct ConflictRules {
    decisions: HashMap<(PathBuf, PathBuf), ConflictPolicy>,
    fallback: ConflictPolicy,
}

impl ConflictRules {
    pub fn new(fallback: ConflictPolicy) -> Self {
        Self {
            decisions: HashMap::new(),
            fallback,
        }
    }

    pub fn uniform(policy: ConflictPolicy) -> Self {
        Self::new(policy)
    }

    pub fn set(&mut self, conflict: &Conflict, policy: ConflictPolicy) {
        self.decisions.insert(
            (conflict.source.clone(), conflict.destination.clone()),
            policy,
        );
    }

    fn policy_for(&self, source: &Path, destination: &Path) -> ConflictPolicy {
        self.decisions
            .get(&(source.to_path_buf(), destination.to_path_buf()))
            .copied()
            .unwrap_or(self.fallback)
    }
}

/// Sources advertised through the desktop clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clipboard {
    pub kind: TransferKind,
    pub sources: Vec<PathBuf>,
}

/// Final state of one requested source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemState {
    Completed,
    Skipped,
    Cancelled,
    Failed,
    /// The destination completed but a later step, such as source removal, failed.
    Partial,
}

/// Structured outcome for one source.
#[derive(Debug, Clone)]
pub struct ItemResult {
    pub source: PathBuf,
    pub requested_destination: PathBuf,
    pub actual_destination: Option<PathBuf>,
    pub bytes: u64,
    pub state: ItemState,
    pub error: Option<String>,
}

impl ItemResult {
    pub(super) fn new(source: PathBuf, requested_destination: PathBuf) -> Self {
        Self {
            source,
            requested_destination,
            actual_destination: None,
            bytes: 0,
            state: ItemState::Failed,
            error: None,
        }
    }

    pub fn completed(&self) -> bool {
        self.state == ItemState::Completed
    }

    /// True when the job could not use the destination that was asked for.
    ///
    /// A conflict-renamed file is a success, but a silent one: the user asked for one
    /// name and got another, and has to be told so they can find it.
    pub fn was_renamed(&self) -> bool {
        self.actual_destination
            .as_deref()
            .is_some_and(|actual| actual != self.requested_destination)
    }
}

/// Snapshot emitted by a running job. Updates are deliberately coarse enough not to
/// flood GTK's main loop.
#[derive(Debug, Clone)]
pub struct JobProgress {
    pub processed_items: usize,
    pub total_items: usize,
    pub completed_bytes: u64,
    pub total_bytes: u64,
    pub current: Option<PathBuf>,
}

/// Complete job result.
#[derive(Debug)]
pub struct JobReport {
    pub kind: OperationKind,
    pub items: Vec<ItemResult>,
    pub cancelled: bool,
}

impl JobReport {
    pub(super) fn new(kind: OperationKind) -> Self {
        Self {
            kind,
            items: Vec::new(),
            cancelled: false,
        }
    }

    pub fn succeeded(&self) -> usize {
        self.items.iter().filter(|item| item.completed()).count()
    }

    /// Completed items that had to take a different name than the one requested.
    pub fn renamed(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.completed() && item.was_renamed())
            .count()
    }

    pub fn skipped(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.state == ItemState::Skipped)
            .count()
    }

    pub fn problems(&self) -> Vec<String> {
        self.items
            .iter()
            .filter_map(|item| {
                item.error
                    .as_ref()
                    .map(|error| format!("{}: {error}", display_name(&item.source)))
            })
            .collect()
    }

    pub fn is_complete(&self) -> bool {
        !self.cancelled && self.items.iter().all(ItemResult::completed)
    }

    pub fn remaining_sources(&self) -> Vec<PathBuf> {
        self.items
            .iter()
            .filter(|item| !item.completed())
            .map(|item| item.source.clone())
            .collect()
    }

    /// The tag updates this report justifies, and nothing more.
    ///
    /// Only completed items appear. An item that landed somewhere carries its tags to
    /// the destination the job actually reported — never a requested or guessed one. An
    /// item that completed without landing anywhere was destroyed, so its tags go with
    /// it. Failed, partial, skipped and cancelled items are absent, because none of them
    /// has an authoritative outcome to act on.
    pub fn tag_updates(&self) -> Vec<(&Path, Option<&Path>)> {
        self.items
            .iter()
            .filter(|item| item.completed())
            .map(|item| {
                (
                    item.source.as_path(),
                    item.actual_destination
                        .as_deref()
                        .filter(|destination| *destination != item.source.as_path()),
                )
            })
            .collect()
    }

    pub fn completed_moves(&self) -> impl Iterator<Item = (&Path, &Path)> {
        self.items.iter().filter_map(|item| {
            if item.completed() {
                item.actual_destination
                    .as_deref()
                    .map(|to| (item.source.as_path(), to))
            } else {
                None
            }
        })
    }
}

/// Cancellation flag shared with a worker. File copies check it after every buffer.
#[derive(Debug, Default)]
struct CancelState {
    cancelled: AtomicBool,
    #[cfg(test)]
    after_bytes: AtomicU64,
}

#[derive(Debug, Clone, Default)]
pub struct CancelFlag(Arc<CancelState>);

impl CancelFlag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }

    #[cfg(test)]
    fn cancel_after_bytes(&self, bytes: u64) {
        self.0.after_bytes.store(bytes, Ordering::Release);
    }

    fn observe_progress(&self, bytes: u64) {
        #[cfg(test)]
        {
            let threshold = self.0.after_bytes.load(Ordering::Acquire);
            if threshold > 0 && bytes >= threshold {
                self.cancel();
            }
        }
        #[cfg(not(test))]
        let _ = bytes;
    }
}

/// Advertise a Teral Copy/Cut selection in both GTK's typed file-list format and the
/// standard URI formats understood by Nautilus, Dolphin and Thunar.
pub fn write_clipboard(clipboard: &gdk::Clipboard, staged: &Clipboard) -> Result<(), String> {
    let files: Vec<gio::File> = staged.sources.iter().map(gio::File::for_path).collect();
    if files.is_empty() {
        return Err("there are no local files to copy".to_owned());
    }

    let uris: Vec<String> = files.iter().map(|file| file.uri().to_string()).collect();
    let action = match staged.kind {
        TransferKind::Copy => "copy",
        TransferKind::Move => "cut",
        TransferKind::Link => {
            return Err("link operations cannot be stored in the clipboard".to_owned());
        }
    };
    // GNOME's own producer writes "copy" or "cut" followed by one newline-prefixed URI
    // per file, and no trailing newline. A trailing newline leaves an empty final field
    // that several consumers turn into an invalid empty URI, so it is deliberately absent.
    let special = format!("{action}\n{}", uris.join("\n"));
    let uri_list = format!("{}\r\n", uris.join("\r\n"));
    let kde_cut = if staged.kind == TransferKind::Move {
        "1"
    } else {
        "0"
    };

    let file_list = gdk::FileList::from_array(&files);
    let providers = [
        gdk::ContentProvider::for_value(&file_list.to_value()),
        gdk::ContentProvider::for_bytes(GNOME_COPIED_FILES, &glib::Bytes::from(special.as_bytes())),
        gdk::ContentProvider::for_bytes(URI_LIST, &glib::Bytes::from(uri_list.as_bytes())),
        gdk::ContentProvider::for_bytes(KDE_CUT_SELECTION, &glib::Bytes::from(kde_cut.as_bytes())),
    ];
    clipboard
        .set_content(Some(&gdk::ContentProvider::new_union(&providers)))
        .map_err(|error| error.to_string())
}

/// True when the desktop clipboard advertises a local file list Teral can paste.
pub fn clipboard_has_files(clipboard: &gdk::Clipboard) -> bool {
    let formats = clipboard.formats();
    formats.contains_type(gdk::FileList::static_type())
        || formats.contain_mime_type(GNOME_COPIED_FILES)
        || formats.contain_mime_type(URI_LIST)
}

/// Read a Copy/Cut selection from another Linux file manager.
pub async fn read_clipboard(clipboard: &gdk::Clipboard) -> Result<Clipboard, String> {
    if clipboard.formats().contain_mime_type(GNOME_COPIED_FILES) {
        let bytes = read_mime(clipboard, GNOME_COPIED_FILES).await?;
        return parse_gnome_clipboard(&bytes);
    }

    let kind = if clipboard.formats().contain_mime_type(KDE_CUT_SELECTION) {
        match read_mime(clipboard, KDE_CUT_SELECTION).await {
            Ok(value) if value.first() == Some(&b'1') => TransferKind::Move,
            _ => TransferKind::Copy,
        }
    } else {
        TransferKind::Copy
    };

    if clipboard.formats().contain_mime_type(URI_LIST) {
        let bytes = read_mime(clipboard, URI_LIST).await?;
        return parse_uri_list(&bytes, kind);
    }

    let value = clipboard
        .read_value_future(gdk::FileList::static_type(), glib::Priority::DEFAULT)
        .await
        .map_err(|error| error.message().trim().to_owned())?;
    let files = value
        .get::<gdk::FileList>()
        .map_err(|_| "the clipboard file list is invalid".to_owned())?;
    let sources = files.files().iter().filter_map(gio::File::path).collect();
    validate_clipboard(kind, sources)
}

async fn read_mime(clipboard: &gdk::Clipboard, mime: &str) -> Result<Vec<u8>, String> {
    let (stream, _) = clipboard
        .read_future(&[mime], glib::Priority::DEFAULT)
        .await
        .map_err(|error| error.message().trim().to_owned())?;
    let mut output = Vec::new();

    loop {
        let buffer = vec![0u8; 16 * 1024];
        let (buffer, read) = stream
            .read_future(buffer, glib::Priority::DEFAULT)
            .await
            .map_err(|(_, error)| error.message().trim().to_owned())?;
        if read == 0 {
            break;
        }
        output.extend_from_slice(&buffer[..read]);
        if output.len() > 8 * 1024 * 1024 {
            return Err("the clipboard file list is unexpectedly large".to_owned());
        }
    }
    Ok(output)
}

fn parse_gnome_clipboard(bytes: &[u8]) -> Result<Clipboard, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "the clipboard file list is not valid UTF-8".to_owned())?;
    let mut lines = text.lines();
    let kind = match lines.next().map(str::trim) {
        Some("copy") => TransferKind::Copy,
        Some("cut") => TransferKind::Move,
        _ => return Err("the clipboard does not contain a valid copy or cut action".to_owned()),
    };
    validate_clipboard(kind, paths_from_uris(lines))
}

fn parse_uri_list(bytes: &[u8], kind: TransferKind) -> Result<Clipboard, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "the clipboard URI list is not valid UTF-8".to_owned())?;
    validate_clipboard(kind, paths_from_uris(text.lines()))
}

fn paths_from_uris<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<PathBuf> {
    lines
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|uri| gio::File::for_uri(uri).path())
        .collect()
}

fn validate_clipboard(kind: TransferKind, sources: Vec<PathBuf>) -> Result<Clipboard, String> {
    if sources.is_empty() {
        Err("the clipboard has no local files Teral can paste".to_owned())
    } else {
        Ok(Clipboard { kind, sources })
    }
}

/// Inspect conflicts off the GTK thread. This is advisory only; the job still performs
/// atomic destination creation because the filesystem may change after this returns.
pub async fn conflicts(
    kind: TransferKind,
    sources: Vec<PathBuf>,
    destination: PathBuf,
) -> Result<Vec<Conflict>, String> {
    gio::spawn_blocking(move || inspect_conflicts(kind, &sources, &destination))
        .await
        .map_err(|_| "the conflict check worker stopped unexpectedly".to_owned())?
}

fn inspect_conflicts(
    kind: TransferKind,
    sources: &[PathBuf],
    destination: &Path,
) -> Result<Vec<Conflict>, String> {
    let mut conflicts = Vec::new();
    for source in sources {
        let Some(name) = source.file_name() else {
            return Err(format!("{} has no file name", source.display()));
        };
        let requested = destination.join(name);
        inspect_conflict(kind, source, &requested, &mut conflicts)?;
    }
    Ok(conflicts)
}

fn inspect_conflict(
    kind: TransferKind,
    source: &Path,
    destination: &Path,
    conflicts: &mut Vec<Conflict>,
) -> Result<(), String> {
    if same_entry(source, destination) {
        conflicts.push(Conflict {
            source: source.to_path_buf(),
            destination: destination.to_path_buf(),
            kind: if kind == TransferKind::Move {
                ConflictKind::SelfMove
            } else {
                ConflictKind::SameEntry
            },
        });
        return Ok(());
    }

    let destination_metadata = match destination.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("{}: {error}", destination.display())),
    };
    let source_metadata = source
        .symlink_metadata()
        .map_err(|error| format!("{}: {error}", source.display()))?;
    let folders = kind != TransferKind::Link
        && source_metadata.is_dir()
        && !source_metadata.file_type().is_symlink()
        && destination_metadata.is_dir()
        && !destination_metadata.file_type().is_symlink();
    conflicts.push(Conflict {
        source: source.to_path_buf(),
        destination: destination.to_path_buf(),
        kind: if folders {
            ConflictKind::Folder
        } else {
            ConflictKind::File
        },
    });

    if folders {
        let mut children = fs::read_dir(source)
            .map_err(|error| format!("{}: {error}", source.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("{}: {error}", source.display()))?;
        children.sort_by_key(fs::DirEntry::file_name);
        for child in children {
            inspect_conflict(
                kind,
                &child.path(),
                &destination.join(child.file_name()),
                conflicts,
            )?;
        }
    }
    Ok(())
}

/// Run a transfer using the per-entry decisions collected by the interactive UI.
pub async fn transfer_resolved(
    kind: TransferKind,
    sources: Vec<PathBuf>,
    destination: PathBuf,
    rules: ConflictRules,
    cancel: CancelFlag,
    progress: mpsc::SyncSender<JobProgress>,
) -> JobReport {
    let fallback_sources = sources.clone();
    let fallback_destination = destination.clone();
    gio::spawn_blocking(move || {
        run_transfer_with_rules(kind, &sources, &destination, &rules, &cancel, &progress)
    })
    .await
    .unwrap_or_else(|_| worker_failure(kind.into(), fallback_sources, fallback_destination))
}

/// Copy entries beside themselves under conflict-renamed names.
pub async fn duplicate(
    sources: Vec<PathBuf>,
    cancel: CancelFlag,
    progress: mpsc::SyncSender<JobProgress>,
) -> JobReport {
    let fallback_sources = sources.clone();
    gio::spawn_blocking(move || {
        let _lease = match OperationLease::acquire(&sources) {
            Ok(lease) => lease,
            Err(error) => {
                return JobReport {
                    kind: OperationKind::Duplicate,
                    items: sources
                        .iter()
                        .map(|source| {
                            let mut item = ItemResult::new(source.clone(), source.clone());
                            item.error = Some(error.clone());
                            item
                        })
                        .collect(),
                    cancelled: false,
                };
            }
        };
        let mut report = JobReport::new(OperationKind::Duplicate);
        let total_bytes = measure_sources(&sources, &cancel).unwrap_or(0);
        let mut completed_bytes = 0;
        let rules = ConflictRules::uniform(ConflictPolicy::RenameIncoming);

        for (index, source) in sources.iter().enumerate() {
            if cancel.is_cancelled() {
                report.cancelled = true;
                append_cancelled(&mut report, &sources[index..], None);
                break;
            }
            let Some(parent) = source.parent() else {
                let mut item = ItemResult::new(source.clone(), source.clone());
                item.error = Some("it has no parent folder".to_owned());
                report.items.push(item);
                continue;
            };
            let result = transfer_one(
                TransferKind::Copy,
                source,
                parent,
                &rules,
                &cancel,
                &progress,
                &mut completed_bytes,
                total_bytes,
                index,
                sources.len(),
            );
            report.cancelled |= result.state == ItemState::Cancelled;
            report.items.push(result);
        }
        report
    })
    .await
    .unwrap_or_else(|_| worker_failure(OperationKind::Duplicate, fallback_sources, PathBuf::new()))
}

fn worker_failure(kind: OperationKind, sources: Vec<PathBuf>, destination: PathBuf) -> JobReport {
    JobReport {
        kind,
        items: sources
            .into_iter()
            .map(|source| {
                let name = source
                    .file_name()
                    .unwrap_or(source.as_os_str())
                    .to_os_string();
                let mut item = ItemResult::new(source, destination.join(name));
                item.error = Some("the operation worker stopped unexpectedly".to_owned());
                item
            })
            .collect(),
        cancelled: false,
    }
}

#[cfg(test)]
fn run_transfer(
    kind: TransferKind,
    sources: &[PathBuf],
    destination: &Path,
    policy: ConflictPolicy,
    cancel: &CancelFlag,
    progress: &mpsc::SyncSender<JobProgress>,
) -> JobReport {
    let rules = ConflictRules::uniform(policy);
    run_transfer_with_rules(kind, sources, destination, &rules, cancel, progress)
}

fn run_transfer_with_rules(
    kind: TransferKind,
    sources: &[PathBuf],
    destination: &Path,
    rules: &ConflictRules,
    cancel: &CancelFlag,
    progress: &mpsc::SyncSender<JobProgress>,
) -> JobReport {
    let requested_targets: Vec<PathBuf> = sources
        .iter()
        .filter_map(|source| source.file_name().map(|name| destination.join(name)))
        .collect();
    let mut lease_paths = sources.to_vec();
    lease_paths.extend(requested_targets);
    let lease = match OperationLease::acquire(&lease_paths) {
        Ok(lease) => lease,
        Err(error) => {
            return JobReport {
                kind: kind.into(),
                items: sources
                    .iter()
                    .map(|source| {
                        let name = source.file_name().unwrap_or(source.as_os_str());
                        let mut item = ItemResult::new(source.clone(), destination.join(name));
                        item.error = Some(error.clone());
                        item
                    })
                    .collect(),
                cancelled: false,
            };
        }
    };

    let total_bytes = measure_sources(sources, cancel).unwrap_or(0);
    let mut completed_bytes = 0;
    let mut report = JobReport::new(kind.into());

    for (index, source) in sources.iter().enumerate() {
        if cancel.is_cancelled() {
            report.cancelled = true;
            append_cancelled(&mut report, &sources[index..], Some(destination));
            break;
        }

        let result = transfer_one(
            kind,
            source,
            destination,
            rules,
            cancel,
            progress,
            &mut completed_bytes,
            total_bytes,
            index,
            sources.len(),
        );
        if result.state == ItemState::Cancelled {
            report.cancelled = true;
        }
        report.items.push(result);
    }

    drop(lease);
    report
}

#[allow(clippy::too_many_arguments)]
fn transfer_one(
    kind: TransferKind,
    source: &Path,
    destination: &Path,
    rules: &ConflictRules,
    cancel: &CancelFlag,
    progress: &mpsc::SyncSender<JobProgress>,
    completed_bytes: &mut u64,
    total_bytes: u64,
    completed_items: usize,
    total_items: usize,
) -> ItemResult {
    let Some(name) = source.file_name() else {
        let mut result = ItemResult::new(source.to_path_buf(), destination.to_path_buf());
        result.error = Some("the source has no file name".to_owned());
        return result;
    };
    let requested = destination.join(name);
    let mut result = ItemResult::new(source.to_path_buf(), requested.clone());

    if kind == TransferKind::Move && same_entry(source, &requested) {
        result.actual_destination = Some(source.to_path_buf());
        result.state = ItemState::Skipped;
        return result;
    }

    if let Err(error) = reject_recursive_destination(source, destination) {
        result.error = Some(error.to_string());
        return result;
    }

    let before = *completed_bytes;
    let outcome = execute_with_rules(
        kind,
        source,
        destination,
        name,
        rules,
        cancel,
        progress,
        completed_bytes,
        total_bytes,
        completed_items,
        total_items,
    );

    match outcome {
        Ok(Execution::Completed(path)) => {
            result.actual_destination = Some(path);
            result.bytes = completed_bytes.saturating_sub(before);
            result.state = ItemState::Completed;
        }
        Ok(Execution::Partial(path, error)) => {
            result.actual_destination = Some(path);
            result.bytes = completed_bytes.saturating_sub(before);
            result.state = ItemState::Partial;
            result.error = Some(error);
        }
        Ok(Execution::Skipped(path)) => {
            result.actual_destination = Some(path);
            result.state = ItemState::Skipped;
        }
        Err(error) if is_cancelled_error(&error) || cancel.is_cancelled() => {
            result.state = ItemState::Cancelled;
            result.error = Some("cancelled".to_owned());
        }
        Err(error) => result.error = Some(error.to_string()),
    }

    let _ = progress.try_send(JobProgress {
        processed_items: completed_items + 1,
        total_items,
        completed_bytes: *completed_bytes,
        total_bytes,
        current: Some(source.to_path_buf()),
    });
    result
}

pub(super) enum Execution {
    Completed(PathBuf),
    Partial(PathBuf, String),
    Skipped(PathBuf),
}

/// Place `source` inside `destination` under `name`, honouring `policy`.
///
/// Restore reuses this directly: a trashed item's name inside `files/` is not
/// necessarily the name it must be restored under, so the target name is passed in
/// rather than derived from the source.
#[allow(clippy::too_many_arguments)]
pub(super) fn execute_with_policy(
    kind: TransferKind,
    source: &Path,
    destination: &Path,
    name: &OsStr,
    policy: ConflictPolicy,
    cancel: &CancelFlag,
    progress: &mpsc::SyncSender<JobProgress>,
    completed_bytes: &mut u64,
    total_bytes: u64,
    completed_items: usize,
    total_items: usize,
) -> io::Result<Execution> {
    let rules = ConflictRules::uniform(policy);
    execute_with_rules(
        kind,
        source,
        destination,
        name,
        &rules,
        cancel,
        progress,
        completed_bytes,
        total_bytes,
        completed_items,
        total_items,
    )
}

#[allow(clippy::too_many_arguments)]
fn execute_with_rules(
    kind: TransferKind,
    source: &Path,
    destination: &Path,
    name: &OsStr,
    rules: &ConflictRules,
    cancel: &CancelFlag,
    progress: &mpsc::SyncSender<JobProgress>,
    completed_bytes: &mut u64,
    total_bytes: u64,
    completed_items: usize,
    total_items: usize,
) -> io::Result<Execution> {
    let requested = destination.join(name);
    let policy = rules.policy_for(source, &requested);
    if policy != ConflictPolicy::RenameIncoming {
        match path_exists(&requested)? {
            true => {
                return match policy {
                    ConflictPolicy::Skip => Ok(Execution::Skipped(requested)),
                    ConflictPolicy::Cancel => Err(cancelled_error()),
                    ConflictPolicy::Merge => merge_entry(
                        kind,
                        source,
                        &requested,
                        rules,
                        cancel,
                        progress,
                        completed_bytes,
                        total_bytes,
                        completed_items,
                        total_items,
                    ),
                    ConflictPolicy::Replace => replace_entry(
                        kind,
                        source,
                        &requested,
                        cancel,
                        progress,
                        completed_bytes,
                        total_bytes,
                        completed_items,
                        total_items,
                    ),
                    ConflictPolicy::RenameIncoming => {
                        Err(io::Error::other("invalid conflict policy state"))
                    }
                };
            }
            false => match transfer_to_new(
                kind,
                source,
                &requested,
                cancel,
                progress,
                completed_bytes,
                total_bytes,
                completed_items,
                total_items,
            ) {
                Ok(execution) => return Ok(execution),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    return match policy {
                        ConflictPolicy::Skip => Ok(Execution::Skipped(requested)),
                        ConflictPolicy::Cancel => Err(cancelled_error()),
                        ConflictPolicy::Merge => merge_entry(
                            kind,
                            source,
                            &requested,
                            rules,
                            cancel,
                            progress,
                            completed_bytes,
                            total_bytes,
                            completed_items,
                            total_items,
                        ),
                        ConflictPolicy::Replace => replace_entry(
                            kind,
                            source,
                            &requested,
                            cancel,
                            progress,
                            completed_bytes,
                            total_bytes,
                            completed_items,
                            total_items,
                        ),
                        ConflictPolicy::RenameIncoming => {
                            Err(io::Error::other("invalid conflict policy state"))
                        }
                    };
                }
                Err(error) => return Err(error),
            },
        }
    }

    for candidate_name in DestinationNames::new(name) {
        let candidate = destination.join(candidate_name);
        match transfer_to_new(
            kind,
            source,
            &candidate,
            cancel,
            progress,
            completed_bytes,
            total_bytes,
            completed_items,
            total_items,
        ) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Ok(execution) => return Ok(execution),
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(
        "could not reserve an unused name in the destination folder",
    ))
}

/// Combine one real directory with another, applying the decisions collected for each
/// child collision. Existing destination directories stay in place; only their
/// contents are changed.
#[allow(clippy::too_many_arguments)]
fn merge_entry(
    kind: TransferKind,
    source: &Path,
    target: &Path,
    rules: &ConflictRules,
    cancel: &CancelFlag,
    progress: &mpsc::SyncSender<JobProgress>,
    completed_bytes: &mut u64,
    total_bytes: u64,
    completed_items: usize,
    total_items: usize,
) -> io::Result<Execution> {
    let source_metadata = fs::symlink_metadata(source)?;
    let target_metadata = fs::symlink_metadata(target)?;
    if !source_metadata.is_dir()
        || source_metadata.file_type().is_symlink()
        || !target_metadata.is_dir()
        || target_metadata.file_type().is_symlink()
    {
        return Err(io::Error::other("only two folders can be merged"));
    }

    let mut changed = false;
    let mut incomplete = Vec::new();
    let mut children = fs::read_dir(source)?.collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(fs::DirEntry::file_name);
    for child in children {
        if cancel.is_cancelled() {
            return Err(cancelled_error());
        }
        let child_source = child.path();
        let child_name = child.file_name();
        match execute_with_rules(
            kind,
            &child_source,
            target,
            &child_name,
            rules,
            cancel,
            progress,
            completed_bytes,
            total_bytes,
            completed_items,
            total_items,
        ) {
            Ok(Execution::Completed(_)) => changed = true,
            Ok(Execution::Skipped(_)) => {
                incomplete.push(format!("{} was skipped", child_name.to_string_lossy()))
            }
            Ok(Execution::Partial(_, error)) => {
                changed = true;
                incomplete.push(error);
            }
            Err(error) if !changed => return Err(error),
            Err(error) => incomplete.push(error.to_string()),
        }
    }

    if kind == TransferKind::Move {
        match fs::remove_dir(source) {
            Ok(()) => {}
            Err(error)
                if !incomplete.is_empty() && error.kind() == io::ErrorKind::DirectoryNotEmpty => {}
            Err(error) if changed => incomplete.push(format!(
                "the source folder remains because it could not be removed: {error}"
            )),
            Err(error) => return Err(error),
        }
    }

    if incomplete.is_empty() {
        Ok(Execution::Completed(target.to_path_buf()))
    } else {
        Ok(Execution::Partial(
            target.to_path_buf(),
            incomplete.join("; "),
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn transfer_to_new(
    kind: TransferKind,
    source: &Path,
    target: &Path,
    cancel: &CancelFlag,
    progress: &mpsc::SyncSender<JobProgress>,
    completed_bytes: &mut u64,
    total_bytes: u64,
    completed_items: usize,
    total_items: usize,
) -> io::Result<Execution> {
    if cancel.is_cancelled() {
        return Err(cancelled_error());
    }

    if kind == TransferKind::Link {
        std::os::unix::fs::symlink(source, target)?;
        return Ok(Execution::Completed(target.to_path_buf()));
    }

    if kind == TransferKind::Move && same_filesystem(source, target.parent().unwrap_or(target)) {
        move_no_replace(source, target)?;
        return Ok(Execution::Completed(target.to_path_buf()));
    }

    let copied = copy_new(
        source,
        target,
        cancel,
        progress,
        completed_bytes,
        total_bytes,
        completed_items,
        total_items,
    )?;

    if kind == TransferKind::Move {
        if cancel.is_cancelled() {
            copied.cleanup();
            return Err(cancelled_error());
        }
        if let Err(error) = remove_source(source) {
            return Ok(Execution::Partial(
                target.to_path_buf(),
                format!(
                    "the copy completed at {}, but the source could not be removed: {error}",
                    target.display()
                ),
            ));
        }
    }
    Ok(Execution::Completed(target.to_path_buf()))
}

#[allow(clippy::too_many_arguments)]
fn replace_entry(
    kind: TransferKind,
    source: &Path,
    target: &Path,
    cancel: &CancelFlag,
    progress: &mpsc::SyncSender<JobProgress>,
    completed_bytes: &mut u64,
    total_bytes: u64,
    completed_items: usize,
    total_items: usize,
) -> io::Result<Execution> {
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::other("the destination has no parent folder"))?;
    let staging = private_path(parent, "incoming")?;
    let copied = if kind == TransferKind::Link {
        std::os::unix::fs::symlink(source, &staging)?;
        CreatedTree {
            paths: vec![staging.clone()],
        }
    } else {
        copy_new(
            source,
            &staging,
            cancel,
            progress,
            completed_bytes,
            total_bytes,
            completed_items,
            total_items,
        )?
    };

    if cancel.is_cancelled() {
        copied.cleanup();
        return Err(cancelled_error());
    }

    let backup_dir = private_directory(parent, "backup")?;
    let backup = backup_dir.join("original");
    if let Err(error) = fs::rename(target, &backup) {
        copied.cleanup();
        let _ = fs::remove_dir(&backup_dir);
        return Err(error);
    }

    if let Err(error) = move_no_replace(&staging, target) {
        let restore_error = move_no_replace(&backup, target).err();
        copied.cleanup();
        if restore_error.is_none() {
            let _ = fs::remove_dir(&backup_dir);
            return Err(error);
        }
        return Ok(Execution::Partial(
            backup,
            format!(
                "replacement failed ({error}); the original remains in the reported backup path"
            ),
        ));
    }

    if let Err(error) = remove_source_path(&backup) {
        return Ok(Execution::Partial(
            target.to_path_buf(),
            format!(
                "replacement completed, but the old destination remains at {}: {error}",
                backup.display()
            ),
        ));
    }
    let _ = fs::remove_dir(&backup_dir);

    if kind == TransferKind::Move
        && let Err(error) = remove_source(source)
    {
        return Ok(Execution::Partial(
            target.to_path_buf(),
            format!("replacement completed, but the source could not be removed: {error}"),
        ));
    }
    Ok(Execution::Completed(target.to_path_buf()))
}

struct CreatedTree {
    paths: Vec<PathBuf>,
}

impl CreatedTree {
    fn cleanup(&self) {
        cleanup_created(&self.paths);
    }
}

#[allow(clippy::too_many_arguments)]
fn copy_new(
    source: &Path,
    target: &Path,
    cancel: &CancelFlag,
    progress: &mpsc::SyncSender<JobProgress>,
    completed_bytes: &mut u64,
    total_bytes: u64,
    completed_items: usize,
    total_items: usize,
) -> io::Result<CreatedTree> {
    let mut created = Vec::new();
    let result = copy_entry(
        source,
        target,
        cancel,
        progress,
        completed_bytes,
        total_bytes,
        completed_items,
        total_items,
        &mut created,
    );
    if let Err(error) = result {
        cleanup_created(&created);
        return Err(error);
    }
    Ok(CreatedTree { paths: created })
}

#[allow(clippy::too_many_arguments)]
fn copy_entry(
    source: &Path,
    target: &Path,
    cancel: &CancelFlag,
    progress: &mpsc::SyncSender<JobProgress>,
    completed_bytes: &mut u64,
    total_bytes: u64,
    completed_items: usize,
    total_items: usize,
    created: &mut Vec<PathBuf>,
) -> io::Result<()> {
    if cancel.is_cancelled() {
        return Err(cancelled_error());
    }

    let metadata = fs::symlink_metadata(source)?;
    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        let link = fs::read_link(source)?;
        std::os::unix::fs::symlink(link, target)?;
        created.push(target.to_path_buf());
        copy_metadata(source, target);
        return Ok(());
    }

    if metadata.is_dir() {
        fs::create_dir(target)?;
        created.push(target.to_path_buf());
        for child in fs::read_dir(source)? {
            let child = child?;
            copy_entry(
                &child.path(),
                &target.join(child.file_name()),
                cancel,
                progress,
                completed_bytes,
                total_bytes,
                completed_items,
                total_items,
                created,
            )?;
        }
        copy_metadata(source, target);
        return Ok(());
    }

    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "special filesystem entries are not copied automatically",
        ));
    }

    let mut input = File::open(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    created.push(target.to_path_buf());

    let mut buffer = vec![0u8; COPY_BUFFER_SIZE];
    let mut logical_length = 0u64;
    loop {
        if cancel.is_cancelled() {
            return Err(cancelled_error());
        }
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        if chunk.iter().all(|byte| *byte == 0) {
            output.seek(SeekFrom::Current(read as i64))?;
        } else {
            output.write_all(chunk)?;
        }
        logical_length += read as u64;
        *completed_bytes = completed_bytes.saturating_add(read as u64);
        cancel.observe_progress(*completed_bytes);
        let _ = progress.try_send(JobProgress {
            processed_items: completed_items,
            total_items,
            completed_bytes: *completed_bytes,
            total_bytes,
            current: Some(source.to_path_buf()),
        });
    }
    output.set_len(logical_length)?;
    output.sync_all()?;
    // The handle is closed before metadata is applied, so a copied modification time is
    // not immediately overwritten by the final write.
    drop(output);
    copy_metadata(source, target);
    Ok(())
}

/// Metadata worth carrying from a source to its copy.
///
/// Ownership is deliberately absent: only root can change it, and a copy made by an
/// ordinary user belongs to that user, which is what every other file manager does.
const COPIED_ATTRIBUTES: &str = "unix::mode,time::modified,time::modified-usec,\
time::access,time::access-usec,xattr::*";

/// Carry a source's metadata onto its copy, as far as the destination allows.
///
/// `g_file_copy_attributes` asks the destination for attributes it never advertised as
/// settable — `standard::size` among them — and fails outright when one is refused,
/// which would abort an otherwise complete copy. Asking for a known set instead keeps
/// the request to things a filesystem can actually accept.
///
/// Failure here is never fatal. A FAT or NTFS destination has no Unix mode, and a
/// network mount may refuse extended attributes; the file's contents still copied, and
/// reporting that as a failed transfer would be untrue.
fn copy_metadata(source: &Path, target: &Path) {
    let source = gio::File::for_path(source);
    let target = gio::File::for_path(target);

    let Ok(info) = source.query_info(
        COPIED_ATTRIBUTES,
        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
        gio::Cancellable::NONE,
    ) else {
        return;
    };

    let _ = target.set_attributes_from_info(
        &info,
        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
        gio::Cancellable::NONE,
    );
}

fn cleanup_created(paths: &[PathBuf]) {
    for path in paths.iter().rev() {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                let _ = fs::remove_dir(path);
            }
            Ok(_) => {
                let _ = fs::remove_file(path);
            }
            Err(_) => {}
        }
    }
}

fn remove_source(path: &Path) -> io::Result<()> {
    remove_source_path(path)
}

fn remove_source_path(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn move_no_replace(source: &Path, target: &Path) -> io::Result<()> {
    gio::File::for_path(source)
        .move_(
            &gio::File::for_path(target),
            gio::FileCopyFlags::NOFOLLOW_SYMLINKS,
            gio::Cancellable::NONE,
            None::<&mut dyn FnMut(i64, i64)>,
        )
        .map_err(|error| {
            if error.matches(gio::IOErrorEnum::Exists) {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "the destination already exists",
                )
            } else {
                io::Error::other(error.message().trim().to_owned())
            }
        })
}

fn reject_recursive_destination(source: &Path, destination: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Ok(());
    }
    let source = fs::canonicalize(source)?;
    let destination = fs::canonicalize(destination)?;
    if destination == source || destination.starts_with(&source) {
        Err(io::Error::other("a folder cannot be copied into itself"))
    } else {
        Ok(())
    }
}

fn path_exists(path: &Path) -> io::Result<bool> {
    match path.symlink_metadata() {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// True when two paths name the same directory entry, including through a symlinked
/// parent directory. Metadata is not followed when the entry itself is a symlink.
fn same_entry(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let Ok(left) = fs::symlink_metadata(left) else {
        return false;
    };
    let Ok(right) = fs::symlink_metadata(right) else {
        return false;
    };
    left.dev() == right.dev() && left.ino() == right.ino()
}

/// True when both paths live on the same filesystem.
pub fn same_filesystem(source: &Path, destination: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let source = fs::symlink_metadata(source)
        .ok()
        .map(|metadata| metadata.dev());
    let destination = fs::metadata(destination)
        .ok()
        .map(|metadata| metadata.dev());
    matches!((source, destination), (Some(a), Some(b)) if a == b)
}

fn measure_sources(sources: &[PathBuf], cancel: &CancelFlag) -> io::Result<u64> {
    sources.iter().try_fold(0u64, |total, source| {
        measure(source, cancel).map(|bytes| total.saturating_add(bytes))
    })
}

fn measure(path: &Path, cancel: &CancelFlag) -> io::Result<u64> {
    if cancel.is_cancelled() {
        return Err(cancelled_error());
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        return fs::read_dir(path)?.try_fold(0u64, |total, child| {
            measure(&child?.path(), cancel).map(|bytes| total.saturating_add(bytes))
        });
    }
    Ok(0)
}

pub(super) fn append_cancelled(
    report: &mut JobReport,
    sources: &[PathBuf],
    destination: Option<&Path>,
) {
    for source in sources {
        let requested = destination
            .and_then(|directory| source.file_name().map(|name| directory.join(name)))
            .unwrap_or_else(|| source.clone());
        let mut item = ItemResult::new(source.clone(), requested);
        item.state = ItemState::Cancelled;
        item.error = Some("cancelled".to_owned());
        report.items.push(item);
    }
}

fn cancelled_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "cancelled")
}

fn is_cancelled_error(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::Interrupted
}

fn private_path(parent: &Path, purpose: &str) -> io::Result<PathBuf> {
    for _ in 0..10_000 {
        let path = parent.join(private_name(purpose));
        if !path_exists(&path)? {
            return Ok(path);
        }
    }
    Err(io::Error::other(
        "could not reserve a private operation path",
    ))
}

fn private_directory(parent: &Path, purpose: &str) -> io::Result<PathBuf> {
    for _ in 0..10_000 {
        let path = parent.join(private_name(purpose));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(
        "could not reserve a private operation directory",
    ))
}

fn private_name(purpose: &str) -> OsString {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    OsString::from(format!(
        ".teral-{purpose}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

struct DestinationNames {
    stem: OsString,
    extension: Option<OsString>,
    attempt: u32,
}

impl DestinationNames {
    fn new(name: &OsStr) -> Self {
        let path = Path::new(name);
        Self {
            stem: path.file_stem().unwrap_or(name).to_os_string(),
            extension: path.extension().map(OsStr::to_os_string),
            attempt: 0,
        }
    }
}

impl Iterator for DestinationNames {
    type Item = OsString;

    fn next(&mut self) -> Option<Self::Item> {
        if self.attempt >= 10_000 {
            return None;
        }
        let attempt = self.attempt;
        self.attempt += 1;
        if attempt == 0 {
            let mut original = self.stem.clone();
            if let Some(extension) = &self.extension {
                original.push(".");
                original.push(extension);
            }
            return Some(original);
        }

        let mut name = self.stem.clone();
        name.push(if attempt == 1 {
            " (copy)".to_owned()
        } else {
            format!(" (copy {attempt})")
        });
        if let Some(extension) = &self.extension {
            name.push(".");
            name.push(extension);
        }
        Some(name)
    }
}

static ACTIVE_TARGETS: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();

pub(super) struct OperationLease {
    targets: Vec<PathBuf>,
}

impl OperationLease {
    pub(super) fn acquire(targets: &[PathBuf]) -> Result<Self, String> {
        let registry = ACTIVE_TARGETS.get_or_init(|| Mutex::new(Vec::new()));
        let mut active = registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if targets.iter().any(|target| {
            active
                .iter()
                .any(|other| paths_overlap(target.as_path(), other.as_path()))
        }) {
            return Err("another operation is already changing the same destination".to_owned());
        }
        active.extend(targets.iter().cloned());
        Ok(Self {
            targets: targets.to_vec(),
        })
    }
}

impl Drop for OperationLease {
    fn drop(&mut self) {
        let registry = ACTIVE_TARGETS.get_or_init(|| Mutex::new(Vec::new()));
        let mut active = registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        active.retain(|path| !self.targets.contains(path));
    }
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
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
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let dir = std::env::temp_dir().join(format!(
            "teral-transfer-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("scratch directory");
        dir
    }

    fn run(
        kind: TransferKind,
        sources: &[PathBuf],
        destination: &Path,
        policy: ConflictPolicy,
    ) -> JobReport {
        let (progress, _) = mpsc::sync_channel(1);
        run_transfer(
            kind,
            sources,
            destination,
            policy,
            &CancelFlag::new(),
            &progress,
        )
    }

    #[test]
    fn destination_names_preserve_extensions_and_non_utf8() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let raw = OsString::from_vec(vec![b'n', 0xff, b'.', b't', b'x', b't']);
        let names: Vec<OsString> = DestinationNames::new(&raw).take(2).collect();
        assert_eq!(names[0].as_bytes(), raw.as_bytes());
        assert_eq!(
            names[1].as_bytes(),
            &[
                b'n', 0xff, b' ', b'(', b'c', b'o', b'p', b'y', b')', b'.', b't', b'x', b't'
            ]
        );
    }

    #[test]
    fn moving_an_item_to_its_own_folder_is_reported_before_the_job() {
        let root = scratch("self-move-conflict");
        let source = root.join("notes.txt");
        fs::write(&source, b"notes").unwrap();

        let conflicts =
            inspect_conflicts(TransferKind::Move, std::slice::from_ref(&source), &root).unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].kind, ConflictKind::SelfMove);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn copying_an_item_onto_itself_only_offers_a_new_name() {
        let root = scratch("same-copy-conflict");
        let source = root.join("notes.txt");
        fs::write(&source, b"notes").unwrap();

        let conflicts =
            inspect_conflicts(TransferKind::Copy, std::slice::from_ref(&source), &root).unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].kind, ConflictKind::SameEntry);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn folder_merge_uses_individual_file_decisions() {
        let root = scratch("merge-decisions");
        let source_parent = root.join("source");
        let destination = root.join("destination");
        let source = source_parent.join("project");
        let target = destination.join("project");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(source.join("replace.txt"), b"new").unwrap();
        fs::write(source.join("skip.txt"), b"new").unwrap();
        fs::write(source.join("added.txt"), b"added").unwrap();
        fs::write(target.join("replace.txt"), b"old").unwrap();
        fs::write(target.join("skip.txt"), b"old").unwrap();

        let conflicts = inspect_conflicts(
            TransferKind::Copy,
            std::slice::from_ref(&source),
            &destination,
        )
        .unwrap();
        let mut rules = ConflictRules::new(ConflictPolicy::RenameIncoming);
        for conflict in &conflicts {
            let policy = if conflict.kind == ConflictKind::Folder {
                ConflictPolicy::Merge
            } else if conflict.source.ends_with("replace.txt") {
                ConflictPolicy::Replace
            } else {
                ConflictPolicy::Skip
            };
            rules.set(conflict, policy);
        }

        let (progress, _) = mpsc::sync_channel(1);
        let report = run_transfer_with_rules(
            TransferKind::Copy,
            &[source],
            &destination,
            &rules,
            &CancelFlag::new(),
            &progress,
        );
        assert_eq!(fs::read(target.join("replace.txt")).unwrap(), b"new");
        assert_eq!(fs::read(target.join("skip.txt")).unwrap(), b"old");
        assert_eq!(fs::read(target.join("added.txt")).unwrap(), b"added");
        assert_eq!(report.items[0].state, ItemState::Partial);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn copy_never_overwrites_an_existing_entry() {
        let root = scratch("no-overwrite");
        let source_dir = root.join("source");
        let destination = root.join("destination");
        fs::create_dir(&source_dir).unwrap();
        fs::create_dir(&destination).unwrap();
        let source = source_dir.join("notes.txt");
        fs::write(&source, b"incoming").unwrap();
        fs::write(destination.join("notes.txt"), b"existing").unwrap();

        let report = run(
            TransferKind::Copy,
            &[source],
            &destination,
            ConflictPolicy::RenameIncoming,
        );
        assert!(report.is_complete(), "{:?}", report.problems());
        assert_eq!(
            fs::read(destination.join("notes.txt")).unwrap(),
            b"existing"
        );
        assert_eq!(
            fs::read(destination.join("notes (copy).txt")).unwrap(),
            b"incoming"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recursive_copy_preserves_symlinks_and_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let root = scratch("metadata");
        let source = root.join("source");
        let destination = root.join("destination");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        let file = source.join("private.txt");
        fs::write(&file, b"payload").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
        std::os::unix::fs::symlink("private.txt", source.join("link")).unwrap();

        let report = run(
            TransferKind::Copy,
            std::slice::from_ref(&source),
            &destination,
            ConflictPolicy::RenameIncoming,
        );
        assert!(report.is_complete(), "{:?}", report.problems());
        let copied = destination.join("source");
        assert_eq!(fs::read(copied.join("private.txt")).unwrap(), b"payload");
        assert_eq!(
            fs::symlink_metadata(copied.join("private.txt"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
        assert_eq!(
            fs::read_link(copied.join("link")).unwrap(),
            PathBuf::from("private.txt")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn move_reports_the_actual_conflict_renamed_destination() {
        let root = scratch("actual-destination");
        let source_dir = root.join("source");
        let destination = root.join("destination");
        fs::create_dir(&source_dir).unwrap();
        fs::create_dir(&destination).unwrap();
        let source = source_dir.join("notes.txt");
        fs::write(&source, b"incoming").unwrap();
        fs::write(destination.join("notes.txt"), b"existing").unwrap();

        let report = run(
            TransferKind::Move,
            std::slice::from_ref(&source),
            &destination,
            ConflictPolicy::RenameIncoming,
        );
        assert!(report.is_complete(), "{:?}", report.problems());
        assert!(!source.exists());
        assert_eq!(
            report.items[0].actual_destination.as_deref(),
            Some(destination.join("notes (copy).txt").as_path())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn skipped_conflicts_leave_both_entries_untouched() {
        let root = scratch("skip");
        let source_dir = root.join("source");
        let destination = root.join("destination");
        fs::create_dir(&source_dir).unwrap();
        fs::create_dir(&destination).unwrap();
        let source = source_dir.join("notes.txt");
        fs::write(&source, b"incoming").unwrap();
        fs::write(destination.join("notes.txt"), b"existing").unwrap();

        let report = run(
            TransferKind::Move,
            std::slice::from_ref(&source),
            &destination,
            ConflictPolicy::Skip,
        );
        assert_eq!(report.items[0].state, ItemState::Skipped);
        assert!(source.exists());
        assert_eq!(
            fs::read(destination.join("notes.txt")).unwrap(),
            b"existing"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replacement_keeps_the_old_entry_until_the_copy_is_complete() {
        let root = scratch("replace");
        let source_dir = root.join("source");
        let destination = root.join("destination");
        fs::create_dir(&source_dir).unwrap();
        fs::create_dir(&destination).unwrap();
        let source = source_dir.join("notes.txt");
        fs::write(&source, b"incoming").unwrap();
        fs::write(destination.join("notes.txt"), b"existing").unwrap();

        let report = run(
            TransferKind::Copy,
            &[source],
            &destination,
            ConflictPolicy::Replace,
        );
        assert!(report.is_complete(), "{:?}", report.problems());
        assert_eq!(
            fs::read(destination.join("notes.txt")).unwrap(),
            b"incoming"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_replacement_does_not_destroy_the_existing_entry() {
        let root = scratch("failed-replace");
        let destination = root.join("destination");
        fs::create_dir(&destination).unwrap();
        let missing = root.join("missing").join("notes.txt");
        fs::write(destination.join("notes.txt"), b"existing").unwrap();

        let report = run(
            TransferKind::Copy,
            &[missing],
            &destination,
            ConflictPolicy::Replace,
        );
        assert_eq!(report.succeeded(), 0);
        assert_eq!(
            fs::read(destination.join("notes.txt")).unwrap(),
            b"existing"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_directory_cannot_be_copied_through_a_symlink_into_itself() {
        let root = scratch("self");
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        std::os::unix::fs::symlink(&source, root.join("alias")).unwrap();
        let report = run(
            TransferKind::Copy,
            std::slice::from_ref(&source),
            &root.join("alias"),
            ConflictPolicy::RenameIncoming,
        );
        assert_eq!(report.succeeded(), 0);
        assert!(report.problems()[0].contains("cannot be copied into itself"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cancellation_during_a_file_copy_removes_teral_partial_output() {
        let root = scratch("cancel");
        let source = root.join("large.bin");
        let destination = root.join("destination");
        fs::create_dir(&destination).unwrap();
        fs::write(&source, vec![7u8; COPY_BUFFER_SIZE * 3]).unwrap();
        let cancel = CancelFlag::new();
        cancel.cancel_after_bytes(COPY_BUFFER_SIZE as u64);
        let (progress, _) = mpsc::sync_channel(1);
        let report = run_transfer(
            TransferKind::Copy,
            std::slice::from_ref(&source),
            &destination,
            ConflictPolicy::RenameIncoming,
            &cancel,
            &progress,
        );
        assert!(report.cancelled);
        assert!(!destination.join("large.bin").exists());
        assert!(source.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn copy_handles_zero_byte_deep_trees_and_broken_symlinks() {
        let root = scratch("tree-shapes");
        let source = root.join("source");
        let destination = root.join("destination");
        let deepest = source.join("one").join("two").join("three");
        fs::create_dir_all(&deepest).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::write(deepest.join("empty"), []).unwrap();
        std::os::unix::fs::symlink("missing-target", deepest.join("broken-link")).unwrap();

        let report = run(
            TransferKind::Copy,
            std::slice::from_ref(&source),
            &destination,
            ConflictPolicy::RenameIncoming,
        );
        assert!(report.is_complete(), "{:?}", report.problems());
        let copied = destination.join("source/one/two/three");
        assert_eq!(fs::metadata(copied.join("empty")).unwrap().len(), 0);
        assert_eq!(
            fs::read_link(copied.join("broken-link")).unwrap(),
            PathBuf::from("missing-target")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_destination_does_not_remove_a_move_source() {
        let root = scratch("invalid-destination");
        let source = root.join("source.txt");
        let destination_file = root.join("not-a-directory");
        fs::write(&source, b"keep me").unwrap();
        fs::write(&destination_file, b"occupied").unwrap();

        let report = run(
            TransferKind::Move,
            std::slice::from_ref(&source),
            &destination_file,
            ConflictPolicy::RenameIncoming,
        );
        assert_eq!(report.succeeded(), 0);
        assert_eq!(fs::read(&source).unwrap(), b"keep me");
        assert_eq!(fs::read(&destination_file).unwrap(), b"occupied");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn link_action_creates_a_symlink_without_touching_the_source() {
        let root = scratch("link");
        let source = root.join("source.txt");
        let destination = root.join("destination");
        fs::write(&source, b"payload").unwrap();
        fs::create_dir(&destination).unwrap();

        let report = run(
            TransferKind::Link,
            std::slice::from_ref(&source),
            &destination,
            ConflictPolicy::RenameIncoming,
        );
        assert!(report.is_complete(), "{:?}", report.problems());
        assert!(source.exists());
        assert_eq!(
            fs::read_link(destination.join("source.txt")).unwrap(),
            source
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_gnome_clipboard_payload_has_no_trailing_newline() {
        // Consumers split this payload on "\n" and treat every field after the first as
        // a URI. A trailing newline produces an empty field, which Thunar and several
        // other file managers turn into an invalid entry when pasting from Teral.
        let uris = ["file:///tmp/one", "file:///tmp/two"];
        let payload = format!("cut\n{}", uris.join("\n"));
        assert_eq!(payload, "cut\nfile:///tmp/one\nfile:///tmp/two");
        assert!(!payload.ends_with('\n'));

        // It must still survive a round trip through Teral's own reader.
        let clipboard = parse_gnome_clipboard(payload.as_bytes()).unwrap();
        assert_eq!(clipboard.kind, TransferKind::Move);
        assert_eq!(
            clipboard.sources,
            [PathBuf::from("/tmp/one"), PathBuf::from("/tmp/two")]
        );
    }

    #[test]
    fn a_trailing_newline_from_another_application_is_still_accepted() {
        // Teral is strict about what it writes and lenient about what it reads.
        let clipboard = parse_gnome_clipboard(b"copy\nfile:///tmp/one\n\n").unwrap();
        assert_eq!(clipboard.kind, TransferKind::Copy);
        assert_eq!(clipboard.sources, [PathBuf::from("/tmp/one")]);
    }

    /// Place `source` into `destination` under `name`, the way a restore does.
    fn place(
        source: &Path,
        destination: &Path,
        name: &OsStr,
        policy: ConflictPolicy,
    ) -> io::Result<Execution> {
        let (progress, _) = mpsc::sync_channel(1);
        let mut completed_bytes = 0;
        execute_with_policy(
            TransferKind::Move,
            source,
            destination,
            name,
            policy,
            &CancelFlag::new(),
            &progress,
            &mut completed_bytes,
            0,
            0,
            1,
        )
    }

    #[test]
    fn a_restore_uses_the_recorded_name_not_the_name_in_the_trash() {
        let root = scratch("restore-name");
        let trash = root.join("files");
        let home = root.join("home");
        fs::create_dir(&trash).unwrap();
        fs::create_dir(&home).unwrap();
        // The desktop de-duplicated the name when it was trashed.
        let trashed = trash.join("notes.2.txt");
        fs::write(&trashed, b"payload").unwrap();

        let placed = place(
            &trashed,
            &home,
            OsStr::new("notes.txt"),
            ConflictPolicy::RenameIncoming,
        )
        .unwrap();
        match placed {
            Execution::Completed(path) => assert_eq!(path, home.join("notes.txt")),
            _ => panic!("the restore should have completed"),
        }
        assert_eq!(fs::read(home.join("notes.txt")).unwrap(), b"payload");
        assert!(!trashed.exists(), "the item must leave the trash");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_occupied_restore_destination_is_renamed_rather_than_replaced() {
        let root = scratch("restore-occupied");
        let trash = root.join("files");
        let home = root.join("home");
        fs::create_dir(&trash).unwrap();
        fs::create_dir(&home).unwrap();
        let trashed = trash.join("notes.txt");
        fs::write(&trashed, b"restored").unwrap();
        fs::write(home.join("notes.txt"), b"newer file").unwrap();

        let placed = place(
            &trashed,
            &home,
            OsStr::new("notes.txt"),
            ConflictPolicy::RenameIncoming,
        )
        .unwrap();
        match placed {
            Execution::Completed(path) => assert_eq!(path, home.join("notes (copy).txt")),
            _ => panic!("the restore should have completed"),
        }
        // The file that was already there is untouched.
        assert_eq!(fs::read(home.join("notes.txt")).unwrap(), b"newer file");
        assert_eq!(
            fs::read(home.join("notes (copy).txt")).unwrap(),
            b"restored"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_skipped_restore_leaves_the_item_in_the_trash() {
        let root = scratch("restore-skip");
        let trash = root.join("files");
        let home = root.join("home");
        fs::create_dir(&trash).unwrap();
        fs::create_dir(&home).unwrap();
        let trashed = trash.join("notes.txt");
        fs::write(&trashed, b"restored").unwrap();
        fs::write(home.join("notes.txt"), b"newer file").unwrap();

        let placed = place(
            &trashed,
            &home,
            OsStr::new("notes.txt"),
            ConflictPolicy::Skip,
        )
        .unwrap();
        assert!(matches!(placed, Execution::Skipped(_)));
        // Skipping must never lose the trashed copy, or its restore record would be
        // removed for an item that is still sitting in the trash.
        assert_eq!(fs::read(&trashed).unwrap(), b"restored");
        assert_eq!(fs::read(home.join("notes.txt")).unwrap(), b"newer file");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_restored_non_utf8_name_keeps_its_exact_bytes() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let root = scratch("restore-non-utf8");
        let trash = root.join("files");
        let home = root.join("home");
        fs::create_dir(&trash).unwrap();
        fs::create_dir(&home).unwrap();
        let trashed = trash.join("plain-trash-name");
        fs::write(&trashed, b"payload").unwrap();

        let original = OsString::from_vec(b"na\xffme.txt".to_vec());
        let placed = place(&trashed, &home, &original, ConflictPolicy::RenameIncoming).unwrap();
        let Execution::Completed(path) = placed else {
            panic!("the restore should have completed");
        };
        assert_eq!(
            path.file_name().unwrap().as_bytes(),
            b"na\xffme.txt".as_slice()
        );
        assert_eq!(fs::read(&path).unwrap(), b"payload");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_restored_directory_keeps_its_contents() {
        let root = scratch("restore-directory");
        let trash = root.join("files");
        let home = root.join("home");
        fs::create_dir(&trash).unwrap();
        fs::create_dir(&home).unwrap();
        let trashed = trash.join("project");
        fs::create_dir_all(trashed.join("src")).unwrap();
        fs::write(trashed.join("src/main.rs"), b"fn main() {}").unwrap();
        std::os::unix::fs::symlink("src/main.rs", trashed.join("link")).unwrap();

        let placed = place(
            &trashed,
            &home,
            OsStr::new("project"),
            ConflictPolicy::RenameIncoming,
        )
        .unwrap();
        let Execution::Completed(path) = placed else {
            panic!("the restore should have completed");
        };
        assert_eq!(fs::read(path.join("src/main.rs")).unwrap(), b"fn main() {}");
        assert_eq!(
            fs::read_link(path.join("link")).unwrap(),
            PathBuf::from("src/main.rs")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn gnome_clipboard_preserves_cut_and_local_uris() {
        let clipboard = parse_gnome_clipboard(b"cut\nfile:///tmp/one\nfile:///tmp/two\n").unwrap();
        assert_eq!(clipboard.kind, TransferKind::Move);
        assert_eq!(
            clipboard.sources,
            [PathBuf::from("/tmp/one"), PathBuf::from("/tmp/two")]
        );
    }

    #[test]
    fn operation_leases_reject_overlapping_targets() {
        let root = PathBuf::from("/tmp/teral-destination");
        let first = OperationLease::acquire(std::slice::from_ref(&root)).unwrap();
        assert!(OperationLease::acquire(&[root.join("child")]).is_err());
        drop(first);
        assert!(OperationLease::acquire(&[root.join("child")]).is_ok());
    }
    #[test]
    fn tag_updates_follow_only_completed_items() {
        let mut report = JobReport::new(OperationKind::Restore);

        let mut completed = ItemResult::new(PathBuf::from("/trash/a"), PathBuf::from("/home/a"));
        completed.actual_destination = Some(PathBuf::from("/home/a (copy)"));
        completed.state = ItemState::Completed;

        let mut destroyed = ItemResult::new(PathBuf::from("/home/b"), PathBuf::from("/home/b"));
        destroyed.state = ItemState::Completed;

        let mut partial = ItemResult::new(PathBuf::from("/trash/c"), PathBuf::from("/home/c"));
        partial.actual_destination = Some(PathBuf::from("/home/c"));
        partial.state = ItemState::Partial;

        let mut skipped = ItemResult::new(PathBuf::from("/trash/d"), PathBuf::from("/home/d"));
        skipped.actual_destination = Some(PathBuf::from("/home/d"));
        skipped.state = ItemState::Skipped;

        let mut cancelled = ItemResult::new(PathBuf::from("/trash/e"), PathBuf::from("/home/e"));
        cancelled.state = ItemState::Cancelled;

        let mut failed = ItemResult::new(PathBuf::from("/trash/f"), PathBuf::from("/home/f"));
        failed.error = Some("nope".to_owned());

        report.items = vec![completed, destroyed, partial, skipped, cancelled, failed];

        let updates = report.tag_updates();
        assert_eq!(updates.len(), 2, "only completed items may move tags");
        // A completed item moves its tags to the destination the job really used.
        assert_eq!(updates[0].0, Path::new("/trash/a"));
        assert_eq!(updates[0].1, Some(Path::new("/home/a (copy)")));
        // A completed item that landed nowhere was destroyed, so its tags go.
        assert_eq!(updates[1].0, Path::new("/home/b"));
        assert_eq!(updates[1].1, None);
    }

    #[test]
    fn an_item_that_did_not_move_is_not_treated_as_a_relocation() {
        let mut report = JobReport::new(OperationKind::PermanentDelete);
        let mut item = ItemResult::new(PathBuf::from("/home/a"), PathBuf::from("/home/a"));
        item.actual_destination = Some(PathBuf::from("/home/a"));
        item.state = ItemState::Completed;
        report.items = vec![item];

        assert_eq!(report.tag_updates(), vec![(Path::new("/home/a"), None)]);
    }
}
