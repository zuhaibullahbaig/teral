//! File operations.
//!
//! Metadata-only operations use GIO's asynchronous APIs. Copy, move and duplicate jobs
//! live in [`super::transfer`], which is the one authoritative transfer implementation.

use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use super::transfer::{
    Execution, ItemResult, ItemState, OperationKind, OperationLease, append_cancelled,
    execute_with_policy,
};
use super::trash;

pub use super::transfer::{
    CancelFlag, Clipboard, Conflict, ConflictKind, ConflictPolicy, ConflictRules, JobProgress,
    JobReport, TransferKind, clipboard_has_files, conflicts, duplicate, read_clipboard,
    transfer_resolved, write_clipboard,
};

/// Create a directory, reporting the GIO error if it already exists or is refused.
pub async fn create_directory(parent: &Path, name: &OsStr) -> Result<PathBuf, glib::Error> {
    let path = parent.join(name);
    gio::File::for_path(&path)
        .make_directory_future(glib::Priority::DEFAULT)
        .await?;
    Ok(path)
}

/// Atomically reserve a new empty file. `create_new` closes the check/create race and
/// never truncates an entry another process created first.
pub async fn create_file(parent: &Path, name: &OsStr) -> io::Result<PathBuf> {
    crate::files::name::validate(name)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let path = parent.join(name);
    let worker_path = path.clone();
    gio::spawn_blocking(move || {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&worker_path)?;
        Ok::<(), io::Error>(())
    })
    .await
    .map_err(|_| io::Error::other("file worker stopped unexpectedly"))??;
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

// -------------------------------------------------------------------- trash ----

/// Mount points whose trash directories Teral should consider.
///
/// The volume monitor lives on the GTK thread, so this is gathered before any worker
/// starts and the resulting list is handed to the worker. A device unplugged after
/// something was trashed on it simply stops appearing, which is what makes its items
/// disappear from Teral until it is mounted again — the data and its records stay on
/// the device.
fn mount_points() -> Vec<PathBuf> {
    let mut points = vec![PathBuf::from("/")];
    if let Some(home) = crate::theme::home_dir() {
        points.push(home);
    }
    for mount in gio::VolumeMonitor::get().mounts() {
        if mount.is_shadowed() {
            continue;
        }
        if let Some(path) = mount.root().path()
            && !points.contains(&path)
        {
            points.push(path);
        }
    }
    points
}

thread_local! {
    /// Trash directories found by the last scan.
    ///
    /// Finding them means asking every mounted filesystem whether it has one, and a
    /// `stat` on a disconnected network share or a removed USB stick can block for a
    /// long time — or, on a hard NFS mount, indefinitely. That is intolerable here,
    /// because the answer is wanted on every selection change and every context menu,
    /// on the GTK thread. The scan runs on a worker and everything on screen reads this.
    static TRASH_DIRS: std::cell::RefCell<Vec<trash::TrashDir>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The trash directories the last scan found. Never touches the filesystem.
pub fn trash_dirs() -> Vec<trash::TrashDir> {
    TRASH_DIRS.with_borrow(Clone::clone)
}

/// Rescan for trash directories on a worker thread, then run `finished` on the GTK
/// thread so what is on screen can catch up.
///
/// Worth doing at start-up, whenever a filesystem is mounted or unmounted, and after
/// anything that could have created a trash directory on another disk.
pub fn refresh_trash_dirs(finished: impl FnOnce() + 'static) {
    let data_home = crate::theme::data_home();
    // The volume monitor belongs to the GTK thread, but only reports what is already
    // mounted, so this part is cheap. The per-mount probing is what moves off it.
    let mounts = mount_points();

    glib::spawn_future_local(async move {
        let found = gio::spawn_blocking(move || {
            let home = trash::ensure_home_trash(&data_home).ok();
            let mut found = match trash::current_uid() {
                Some(uid) => trash::discover(&data_home, &mounts, uid),
                // Without a user id the per-filesystem naming scheme cannot be applied,
                // and guessing an id would point at another user's trash.
                None => Vec::new(),
            };
            if let Some(home) = home
                && !found.contains(&home)
            {
                found.insert(0, home);
            }
            found
        })
        .await
        .unwrap_or_default();

        TRASH_DIRS.with_borrow_mut(|dirs| *dirs = found);
        finished();
    });
}

/// True when `path` is inside any trash Teral knows about, so restore and permanent
/// deletion are the meaningful actions for it.
///
/// Answered from the path alone. The home trash sits at a location that can be derived
/// without asking the filesystem anything, and every other trash comes from the cached
/// scan, so this stays instant no matter what is mounted or how badly it is behaving.
pub fn is_in_trash(path: &Path) -> bool {
    if path.starts_with(trash::home_trash(&crate::theme::data_home()).files()) {
        return true;
    }
    trash::is_in_trash(path, &trash_dirs())
}

/// What Empty Trash would actually remove, so the confirmation can name real numbers.
#[derive(Debug, Default, Clone, Copy)]
pub struct TrashScope {
    pub items: usize,
    pub records: usize,
    pub locations: usize,
}

impl TrashScope {
    pub fn is_empty(&self) -> bool {
        self.items == 0 && self.records == 0
    }

    /// One sentence naming exactly what will be deleted and from where.
    pub fn describe(&self) -> String {
        let mut description = crate::files::item_count_label(self.items);
        if self.locations > 1 {
            description.push_str(&format!(" across {} trash locations", self.locations));
        }
        if self.records > 0 {
            description.push_str(&format!(
                ", plus {} leftover trash {}",
                self.records,
                if self.records == 1 {
                    "record"
                } else {
                    "records"
                }
            ));
        }
        description
    }
}

/// Measure the trash, off the GTK thread.
///
/// One directory read per trash location, and one of those locations may be a removable
/// disk that is slow or already gone, so the confirmation waits for a worker rather
/// than freezing the window while it counts.
pub async fn trash_scope(dirs: Vec<trash::TrashDir>) -> TrashScope {
    gio::spawn_blocking(move || TrashScope {
        items: trash::count(&dirs),
        records: dirs
            .iter()
            .map(|dir| {
                trash::orphan_records(dir)
                    .map(|records| records.len())
                    .unwrap_or(0)
            })
            .sum(),
        locations: dirs.len(),
    })
    .await
    .unwrap_or_default()
}

/// Move entries to the trash through GIO, which implements the FreeDesktop model for
/// the home filesystem and for secondary filesystems alike.
///
/// GIO does not report the name an item receives inside the trash, so no destination is
/// claimed for a trashed item. Callers get a completion state per item and nothing more,
/// which is exactly what they are entitled to act on.
pub async fn trash(
    paths: Vec<PathBuf>,
    cancel: CancelFlag,
    progress: mpsc::SyncSender<JobProgress>,
) -> JobReport {
    let mut report = JobReport::new(OperationKind::Trash);
    let _lease = match OperationLease::acquire(&paths) {
        Ok(lease) => lease,
        Err(error) => return blocked_job(OperationKind::Trash, &paths, &error),
    };

    let total = paths.len();
    for (index, path) in paths.iter().enumerate() {
        if cancel.is_cancelled() {
            report.cancelled = true;
            append_cancelled(&mut report, &paths[index..], None);
            break;
        }

        let mut item = ItemResult::new(path.clone(), path.clone());
        match gio::File::for_path(path)
            .trash_future(glib::Priority::DEFAULT)
            .await
        {
            Ok(()) => item.state = ItemState::Completed,
            Err(error) => item.error = Some(describe_gio(&error)),
        }
        report.items.push(item);

        let _ = progress.try_send(JobProgress {
            processed_items: index + 1,
            total_items: total,
            completed_bytes: 0,
            total_bytes: 0,
            current: Some(path.clone()),
        });
    }
    report
}

/// One trashed item that a restore is prepared to put back.
#[derive(Debug, Clone)]
pub struct RestoreTarget {
    /// The item as it sits in the trash right now.
    pub file: PathBuf,
    /// Its restore record, removed only once the item is back in place.
    pub info: PathBuf,
    /// The folder it came from.
    pub parent: PathBuf,
    /// The name it must be restored under, with its raw bytes intact.
    pub name: OsString,
    /// False when the original folder has been removed since the item was trashed.
    pub parent_exists: bool,
    /// True when something already occupies the original location.
    pub occupied: bool,
    /// Where the record says it came from and when it was deleted. Repeated in failure
    /// messages, because a filename on its own rarely says which item is meant.
    pub origin: Option<String>,
}

/// What a restore will run into before it starts.
#[derive(Debug, Default)]
pub struct RestorePlan {
    pub targets: Vec<RestoreTarget>,
    /// Items whose record cannot tell Teral where they came from, with the reason.
    pub unrestorable: Vec<(PathBuf, String)>,
}

impl RestorePlan {
    pub fn conflicts(&self) -> usize {
        self.targets.iter().filter(|target| target.occupied).count()
    }

    pub fn missing_parents(&self) -> usize {
        self.targets
            .iter()
            .filter(|target| !target.parent_exists)
            .count()
    }
}

/// Read the restore records for a selection, off the GTK thread.
pub async fn plan_restore(paths: Vec<PathBuf>, dirs: Vec<trash::TrashDir>) -> RestorePlan {
    gio::spawn_blocking(move || build_restore_plan(&paths, &dirs))
        .await
        .unwrap_or_else(|_| RestorePlan {
            targets: Vec::new(),
            unrestorable: vec![(
                PathBuf::new(),
                "the restore worker stopped unexpectedly".to_owned(),
            )],
        })
}

fn build_restore_plan(paths: &[PathBuf], dirs: &[trash::TrashDir]) -> RestorePlan {
    let mut plan = RestorePlan::default();

    for path in paths {
        let Some(dir) = trash::containing(path, dirs) else {
            plan.unrestorable.push((
                path.clone(),
                "it is not in a trash directory Teral can restore from".to_owned(),
            ));
            continue;
        };
        // Only a top-level entry in files/ has a record. Something selected further
        // inside a trashed folder is restored by restoring the folder it lives in.
        if path.parent() != Some(dir.files().as_path()) {
            plan.unrestorable.push((
                path.clone(),
                "only whole trashed items can be restored; restore the item it is inside"
                    .to_owned(),
            ));
            continue;
        }

        let item = trash::item_at(path, dir);
        let (Some(parent), Some(name)) = (item.original_parent(), item.original_name()) else {
            let reason = match &item.info_result {
                Err(error) => error.to_string(),
                Ok(_) => "its trash record does not name a folder to restore into".to_owned(),
            };
            plan.unrestorable.push((path.clone(), reason));
            continue;
        };

        let requested = parent.join(name);
        plan.targets.push(RestoreTarget {
            file: path.clone(),
            info: item.info.clone(),
            parent: parent.to_path_buf(),
            name: name.to_os_string(),
            parent_exists: parent.is_dir(),
            occupied: requested.symlink_metadata().is_ok(),
            origin: item.origin_summary(),
        });
    }
    plan
}

/// What to do about a restore whose original folder no longer exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingParent {
    /// Recreate the folder chain and restore into it.
    Recreate,
    /// Leave the item in the trash and say so.
    Fail,
}

/// Put trashed items back where their records say they came from.
///
/// Placement goes through the same conflict handling, atomic no-overwrite reservation,
/// replacement backup, cross-filesystem fallback and cancellation as Paste. A record is
/// discarded only after its item has actually landed somewhere.
pub async fn restore_from_trash(
    targets: Vec<RestoreTarget>,
    policy: ConflictPolicy,
    missing_parent: MissingParent,
    cancel: CancelFlag,
    progress: mpsc::SyncSender<JobProgress>,
) -> JobReport {
    let fallback: Vec<PathBuf> = targets.iter().map(|target| target.file.clone()).collect();
    gio::spawn_blocking(move || run_restore(&targets, policy, missing_parent, &cancel, &progress))
        .await
        .unwrap_or_else(|_| {
            blocked_job(
                OperationKind::Restore,
                &fallback,
                "the restore worker stopped unexpectedly",
            )
        })
}

fn run_restore(
    targets: &[RestoreTarget],
    policy: ConflictPolicy,
    missing_parent: MissingParent,
    cancel: &CancelFlag,
    progress: &mpsc::SyncSender<JobProgress>,
) -> JobReport {
    let mut lease_paths: Vec<PathBuf> = targets.iter().map(|target| target.file.clone()).collect();
    lease_paths.extend(
        targets
            .iter()
            .map(|target| target.parent.join(&target.name)),
    );
    let sources: Vec<PathBuf> = targets.iter().map(|target| target.file.clone()).collect();

    let _lease = match OperationLease::acquire(&lease_paths) {
        Ok(lease) => lease,
        Err(error) => return blocked_job(OperationKind::Restore, &sources, &error),
    };

    let mut report = JobReport::new(OperationKind::Restore);
    let mut completed_bytes = 0u64;
    let total = targets.len();

    for (index, target) in targets.iter().enumerate() {
        if cancel.is_cancelled() {
            report.cancelled = true;
            append_cancelled(&mut report, &sources[index..], None);
            break;
        }

        let requested = target.parent.join(&target.name);
        let mut item = ItemResult::new(target.file.clone(), requested);

        if !target.parent.is_dir() {
            match missing_parent {
                MissingParent::Fail => {
                    item.error = Some(format!(
                        "its original folder {} no longer exists{}",
                        target.parent.display(),
                        describe_origin(target.origin.as_deref())
                    ));
                    report.items.push(item);
                    continue;
                }
                MissingParent::Recreate => {
                    if let Err(error) = fs::create_dir_all(&target.parent) {
                        item.error = Some(format!(
                            "its original folder {} could not be recreated: {error}",
                            target.parent.display()
                        ));
                        report.items.push(item);
                        continue;
                    }
                }
            }
        }

        let placement = execute_with_policy(
            TransferKind::Move,
            &target.file,
            &target.parent,
            &target.name,
            policy,
            cancel,
            progress,
            &mut completed_bytes,
            0,
            index,
            total,
        );

        match placement {
            Ok(Execution::Completed(path)) => {
                // The record goes only now, once the data is demonstrably back.
                match trash::discard_record(&target.info) {
                    Ok(()) => {
                        item.actual_destination = Some(path);
                        item.state = ItemState::Completed;
                    }
                    Err(error) => {
                        item.actual_destination = Some(path);
                        item.state = ItemState::Partial;
                        item.error = Some(format!(
                            "it was restored, but its trash record could not be removed: {error}"
                        ));
                    }
                }
            }
            Ok(Execution::Skipped(path)) => {
                item.actual_destination = Some(path);
                item.state = ItemState::Skipped;
            }
            Ok(Execution::Partial(path, error)) => {
                item.actual_destination = Some(path);
                item.state = ItemState::Partial;
                item.error = Some(error);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted || cancel.is_cancelled() => {
                report.cancelled = true;
                item.state = ItemState::Cancelled;
                item.error = Some("cancelled".to_owned());
            }
            Err(error) => {
                item.error = Some(format!(
                    "{error}{}",
                    describe_origin(target.origin.as_deref())
                ));
            }
        }

        report.items.push(item);
        let _ = progress.try_send(JobProgress {
            processed_items: index + 1,
            total_items: total,
            completed_bytes,
            total_bytes: 0,
            current: Some(target.file.clone()),
        });
    }
    report
}

/// Delete entries permanently, without going through the trash.
///
/// An entry that is itself in the trash keeps its restore record until its data is
/// actually gone, so a refusal part-way through leaves it restorable.
pub async fn delete_permanently(
    paths: Vec<PathBuf>,
    dirs: Vec<trash::TrashDir>,
    cancel: CancelFlag,
    progress: mpsc::SyncSender<JobProgress>,
) -> JobReport {
    let fallback = paths.clone();
    gio::spawn_blocking(move || run_delete(&paths, &dirs, &cancel, &progress))
        .await
        .unwrap_or_else(|_| {
            blocked_job(
                OperationKind::PermanentDelete,
                &fallback,
                "the delete worker stopped unexpectedly",
            )
        })
}

fn run_delete(
    paths: &[PathBuf],
    dirs: &[trash::TrashDir],
    cancel: &CancelFlag,
    progress: &mpsc::SyncSender<JobProgress>,
) -> JobReport {
    let _lease = match OperationLease::acquire(paths) {
        Ok(lease) => lease,
        Err(error) => return blocked_job(OperationKind::PermanentDelete, paths, &error),
    };

    let batch: Vec<(PathBuf, Option<PathBuf>)> = paths
        .iter()
        .map(|path| (path.clone(), record_for(path, dirs)))
        .collect();
    let outcomes = purge_batch(&batch, cancel, progress, paths.len());
    collect_removals(OperationKind::PermanentDelete, paths, outcomes)
}

/// Run a batch removal, reporting progress as it goes.
fn purge_batch(
    batch: &[(PathBuf, Option<PathBuf>)],
    cancel: &CancelFlag,
    progress: &mpsc::SyncSender<JobProgress>,
    total: usize,
) -> Vec<trash::Removal> {
    trash::purge_batch(batch, &|| cancel.is_cancelled(), |index, path| {
        let _ = progress.try_send(JobProgress {
            processed_items: index + 1,
            total_items: total,
            completed_bytes: 0,
            total_bytes: 0,
            current: Some(path.to_path_buf()),
        });
    })
}

/// Turn per-item removal outcomes into the structured report the UI reads.
fn collect_removals(
    kind: OperationKind,
    paths: &[PathBuf],
    outcomes: Vec<trash::Removal>,
) -> JobReport {
    let mut report = JobReport::new(kind);
    for (path, outcome) in paths.iter().zip(outcomes) {
        let mut item = ItemResult::new(path.clone(), path.clone());
        match outcome {
            trash::Removal::Removed => item.state = ItemState::Completed,
            trash::Removal::RecordRemains(error) => {
                item.state = ItemState::Partial;
                item.error = Some(error);
            }
            trash::Removal::Failed(error) => item.error = Some(error),
            trash::Removal::Cancelled => {
                report.cancelled = true;
                item.state = ItemState::Cancelled;
                item.error = Some("cancelled".to_owned());
            }
        }
        report.items.push(item);
    }
    report
}

/// The restore record belonging to `path`, when `path` is a whole trashed item.
///
/// Anything deeper inside a trashed folder has no record of its own, and the folder's
/// record must not be removed just because one child was deleted.
fn record_for(path: &Path, dirs: &[trash::TrashDir]) -> Option<PathBuf> {
    let dir = trash::containing(path, dirs)?;
    if path.parent() != Some(dir.files().as_path()) {
        return None;
    }
    Some(dir.info_path(path.file_name()?))
}

/// Permanently delete everything in every trash Teral can see.
///
/// Items are removed one at a time so a refusal on one leaves the rest of the trash —
/// and every remaining restore record — exactly as it was.
pub async fn empty_trash(
    dirs: Vec<trash::TrashDir>,
    cancel: CancelFlag,
    progress: mpsc::SyncSender<JobProgress>,
) -> JobReport {
    gio::spawn_blocking(move || run_empty_trash(&dirs, &cancel, &progress))
        .await
        .unwrap_or_else(|_| {
            blocked_job(
                OperationKind::PermanentDelete,
                &[],
                "the delete worker stopped unexpectedly",
            )
        })
}

fn run_empty_trash(
    dirs: &[trash::TrashDir],
    cancel: &CancelFlag,
    progress: &mpsc::SyncSender<JobProgress>,
) -> JobReport {
    let roots: Vec<PathBuf> = dirs.iter().map(|dir| dir.root.clone()).collect();
    let _lease = match OperationLease::acquire(&roots) {
        Ok(lease) => lease,
        Err(error) => return blocked_job(OperationKind::PermanentDelete, &roots, &error),
    };

    let mut report = JobReport::new(OperationKind::PermanentDelete);

    // Records whose data was already gone before this run started. Anything that becomes
    // a record without data during the run is a reported partial failure, and clearing it
    // here would quietly contradict that report.
    let mut stale_records = Vec::new();
    let mut batch: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    for dir in dirs {
        stale_records.extend(trash::orphan_records(dir).unwrap_or_default());
        match trash::list(dir) {
            Ok(items) => batch.extend(items.into_iter().map(|item| (item.file, Some(item.info)))),
            Err(error) => {
                let mut failed = ItemResult::new(dir.files(), dir.files());
                failed.error = Some(format!("its contents could not be read: {error}"));
                report.items.push(failed);
            }
        }
    }

    let total = batch.len() + stale_records.len();
    let paths: Vec<PathBuf> = batch.iter().map(|(file, _)| file.clone()).collect();
    let outcomes = purge_batch(&batch, cancel, progress, total);
    let removals = collect_removals(OperationKind::PermanentDelete, &paths, outcomes);
    report.cancelled |= removals.cancelled;
    report.items.extend(removals.items);

    for record in &stale_records {
        if cancel.is_cancelled() {
            report.cancelled = true;
            break;
        }
        let mut item = ItemResult::new(record.clone(), record.clone());
        match trash::discard_record(record) {
            Ok(()) => item.state = ItemState::Completed,
            Err(error) => item.error = Some(format!("a leftover trash record remains: {error}")),
        }
        report.items.push(item);
    }
    report
}

/// A job that never started because another operation owns the same paths.
fn blocked_job(kind: OperationKind, paths: &[PathBuf], error: &str) -> JobReport {
    let mut report = JobReport::new(kind);
    report.items = paths
        .iter()
        .map(|path| {
            let mut item = ItemResult::new(path.clone(), path.clone());
            item.error = Some(error.to_owned());
            item
        })
        .collect();
    report
}

/// Append what a trash record knows about an item, when it knows anything.
fn describe_origin(origin: Option<&str>) -> String {
    match origin {
        Some(origin) => format!(" ({origin})"),
        None => String::new(),
    }
}

/// Turn a GIO failure into something that says what actually went wrong.
fn describe_gio(error: &glib::Error) -> String {
    let message = error.message().trim().to_owned();
    if error.matches(gio::IOErrorEnum::NotFound) {
        format!("it no longer exists ({message})")
    } else if error.matches(gio::IOErrorEnum::PermissionDenied) {
        format!("permission was denied ({message})")
    } else if error.matches(gio::IOErrorEnum::NotSupported) {
        format!("this filesystem has no trash Teral can use ({message})")
    } else {
        message
    }
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
    let _lease = OperationLease::acquire(&[archive.clone(), destination.clone()])?;
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
    let mut lease_paths = paths.clone();
    lease_paths.push(archive.clone());
    let _lease = OperationLease::acquire(&lease_paths)?;

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

/// Launch an entry with the desktop's default application without making GTK wait for
/// the desktop MIME database or a portal.
pub async fn open(path: PathBuf) -> Result<(), String> {
    gio::spawn_blocking(move || {
        let uri = gio::File::for_path(path).uri();
        gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>)
            .map_err(|error| error.message().trim().to_owned())
    })
    .await
    .map_err(|_| "the application launcher stopped unexpectedly".to_owned())?
}

thread_local! {
    /// Querying the desktop's application database costs real time, and the details
    /// panel asks about the same handful of content types over and over.
    static APPLICATIONS: std::cell::RefCell<
        std::collections::HashMap<String, Vec<ApplicationChoice>>,
    > = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Send-safe description of an application returned by the desktop MIME database.
///
/// GIO objects are bound to their creating thread and therefore cannot be returned
/// from `spawn_blocking`. Keeping only owned text here lets the query stay off GTK's
/// main thread; the selected application is looked up and launched on that same kind
/// of worker later.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationChoice {
    pub id: String,
    pub display_name: String,
    content_type: String,
}

/// Applications resolved for an entry's content type.
pub fn applications_for(content_type: Option<&str>) -> Vec<ApplicationChoice> {
    let Some(content_type) = content_type else {
        return Vec::new();
    };

    APPLICATIONS.with_borrow(|cache| cache.get(content_type).cloned().unwrap_or_default())
}

/// Resolve desktop MIME applications and populate the shared cache.
///
/// `gio::AppInfo` cannot cross a worker-thread boundary, so the worker returns plain
/// owned descriptions. This keeps a slow MIME database query out of selection and
/// context-menu callbacks without depending on optional desktop-entry bindings.
pub async fn load_applications(content_type: String) -> Vec<ApplicationChoice> {
    if let Some(cached) = APPLICATIONS.with_borrow(|cache| cache.get(&content_type).cloned()) {
        return cached;
    }

    let query = content_type.clone();
    let Ok(applications) = gio::spawn_blocking(move || {
        let mut applications = gio::AppInfo::recommended_for_type(&query);
        if applications.is_empty() {
            applications = gio::AppInfo::all_for_type(&query);
        }
        applications
            .into_iter()
            .filter_map(|application| {
                application.id().map(|id| ApplicationChoice {
                    id: id.to_string(),
                    display_name: application.display_name().to_string(),
                    content_type: query.clone(),
                })
            })
            .collect::<Vec<_>>()
    })
    .await
    else {
        return Vec::new();
    };

    APPLICATIONS.with_borrow_mut(|cache| {
        cache.insert(content_type, applications.clone());
    });
    applications
}

pub fn clear_application_cache() {
    APPLICATIONS.with_borrow_mut(std::collections::HashMap::clear);
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
pub async fn set_executable(paths: Vec<PathBuf>, executable: bool) -> JobReport {
    let fallback = paths.clone();
    gio::spawn_blocking(move || {
        let _lease = match OperationLease::acquire(&paths) {
            Ok(lease) => lease,
            Err(error) => return blocked_job(OperationKind::SetExecutable, &paths, &error),
        };
        use std::os::unix::fs::PermissionsExt;

        let mut report = JobReport::new(OperationKind::SetExecutable);
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

            let mut item = ItemResult::new(path.clone(), path);
            match result {
                Ok(()) => item.state = ItemState::Completed,
                Err(error) => item.error = Some(error.to_string()),
            }
            report.items.push(item);
        }
        report
    })
    .await
    .unwrap_or_else(|_| {
        blocked_job(
            OperationKind::SetExecutable,
            &fallback,
            "the permission worker stopped unexpectedly",
        )
    })
}

/// Replace only owner/group/other rwx bits on one local entry. Symlinks are refused so
/// the target is never changed unexpectedly; special mode bits remain untouched.
pub async fn set_permissions(path: PathBuf, permissions: u32) -> io::Result<()> {
    if permissions > 0o777 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "permissions must contain exactly owner/group/other rwx bits",
        ));
    }
    gio::spawn_blocking(move || {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        // Linux O_NOFOLLOW: open the entry itself before inspecting its mode, so a
        // rename-to-symlink race cannot redirect chmod onto a target.
        const O_NOFOLLOW: i32 = 0o400000;
        const O_NONBLOCK: i32 = 0o4000;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW | O_NONBLOCK)
            .open(&path)?;
        let metadata = file.metadata()?;
        let current = metadata.permissions().mode();
        let updated = (current & !0o777) | permissions;
        file.set_permissions(fs::Permissions::from_mode(updated))
    })
    .await
    .map_err(|_| io::Error::other("permission worker stopped unexpectedly"))?
}

/// Launch `path` with a specific desktop application away from GTK's main loop.
pub async fn open_with(application: ApplicationChoice, path: PathBuf) -> Result<(), String> {
    gio::spawn_blocking(move || {
        let mut applications = gio::AppInfo::recommended_for_type(&application.content_type);
        applications.extend(gio::AppInfo::all_for_type(&application.content_type));
        let application = applications
            .into_iter()
            .find(|candidate| {
                candidate
                    .id()
                    .is_some_and(|id| id.as_str() == application.id.as_str())
            })
            .ok_or_else(|| "that application is no longer installed".to_owned())?;
        let file = gio::File::for_path(path);
        application
            .launch(&[file], None::<&gio::AppLaunchContext>)
            .map_err(|error| error.message().trim().to_owned())
    })
    .await
    .map_err(|_| "the application launcher stopped unexpectedly".to_owned())?
}

/// Open the user's terminal emulator in `directory`.
///
/// Teral's own setting wins, then `TERAL_TERMINAL`, then the first terminal found on
/// `PATH`, so the behaviour stays configurable without a Teral-only registry.
pub fn open_terminal(directory: &Path, setting: &str) -> Result<(), String> {
    let configured = Some(setting.trim().to_owned()).filter(|value| !value.is_empty());

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

    let arguments = if let Some(configured) = configured {
        let arguments = crate::command::parse_program_spec(&configured)?;
        crate::command::validate_program(&arguments)?;
        arguments
    } else if let Some(environment) = std::env::var("TERAL_TERMINAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        let arguments = crate::command::parse_program_spec(&environment)?;
        crate::command::validate_program(&arguments)?;
        arguments
    } else {
        let program = CANDIDATES
            .iter()
            .find_map(glib::find_program_in_path)
            .ok_or_else(|| "no terminal emulator was found on PATH".to_owned())?;
        vec![program.to_string_lossy().into_owned()]
    };
    let (program, extra) = arguments.split_first().expect("validated arguments");

    std::process::Command::new(program)
        .args(extra)
        .current_dir(directory)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("{program}: {error}"))
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
}
