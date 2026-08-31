//! The FreeDesktop trash model.
//!
//! This module is deliberately free of GTK, GIO and GLib. Everything here is plain
//! `std`, which keeps the trash specification — directory discovery, `.trashinfo`
//! records, raw filename bytes, and recursive removal — testable without a display
//! server, and keeps the desktop integration in [`super::ops`] thin.
//!
//! Filenames never pass through a lossy string. `Path=` is decoded to raw bytes and
//! rebuilt as an `OsString`, so a file whose name is not valid UTF-8 is restored under
//! the name it actually had.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// The suffix every restore record carries, per the trash specification.
pub const INFO_SUFFIX: &str = ".trashinfo";

/// One trash directory: the home trash, or a trash on another filesystem.
///
/// `top_dir` is the mount point the trash serves. The specification stores `Path=`
/// relative to it for anything other than the home trash, so it is required to resolve
/// a record back to a real location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashDir {
    pub root: PathBuf,
    pub top_dir: PathBuf,
}

impl TrashDir {
    pub fn new(root: PathBuf, top_dir: PathBuf) -> Self {
        Self { root, top_dir }
    }

    /// Where trashed data lives.
    pub fn files(&self) -> PathBuf {
        self.root.join("files")
    }

    /// Where restore records live.
    pub fn info(&self) -> PathBuf {
        self.root.join("info")
    }

    /// True once the desktop has actually created this trash.
    pub fn is_present(&self) -> bool {
        self.files().is_dir()
    }

    /// The record belonging to a name inside `files/`.
    pub fn info_path(&self, name: &OsStr) -> PathBuf {
        let mut info_name = OsString::from(name);
        info_name.push(INFO_SUFFIX);
        self.info().join(info_name)
    }
}

/// The user id this process runs as.
///
/// The trash specification names per-filesystem trash directories after the user id.
/// `/proc/self` is owned by the running process, which gives the id without a libc
/// binding; `$HOME` is the fallback for a system that does not mount `/proc`.
pub fn current_uid() -> Option<u32> {
    fs::metadata("/proc/self")
        .ok()
        .map(|metadata| metadata.uid())
        .or_else(|| {
            std::env::var_os("HOME")
                .and_then(|home| fs::metadata(home).ok())
                .map(|metadata| metadata.uid())
        })
}

/// The home trash, which serves the filesystem `$XDG_DATA_HOME` lives on.
///
/// Records written here are absolute, so `/` is the only sensible base for the
/// malformed case where one is stored relative.
pub fn home_trash(data_home: &Path) -> TrashDir {
    TrashDir::new(data_home.join("Trash"), PathBuf::from("/"))
}

/// The trash directory a filesystem uses, following the specification's order.
///
/// `$topdir/.Trash` is only trusted when it is a real directory, is not a symlink, and
/// carries the sticky bit — an administrator-created shared trash. Anything else falls
/// back to the unshared `$topdir/.Trash-$uid`, which is what desktops create in
/// practice. Both are returned so a caller can find whichever exists.
pub fn top_dir_trashes(top_dir: &Path, uid: u32) -> Vec<TrashDir> {
    let mut candidates = Vec::with_capacity(2);

    let shared = top_dir.join(".Trash");
    if let Ok(metadata) = fs::symlink_metadata(&shared)
        && metadata.is_dir()
        && !metadata.file_type().is_symlink()
        && metadata.mode() & 0o1000 != 0
    {
        candidates.push(TrashDir::new(
            shared.join(uid.to_string()),
            top_dir.to_path_buf(),
        ));
    }

    candidates.push(TrashDir::new(
        top_dir.join(format!(".Trash-{uid}")),
        top_dir.to_path_buf(),
    ));
    candidates
}

/// Every trash directory that currently exists, home first.
///
/// `mount_points` comes from the desktop's volume monitor. A device that has been
/// unplugged since something was trashed on it simply stops appearing here; its records
/// stay on the device and reappear when it is mounted again.
pub fn discover(data_home: &Path, mount_points: &[PathBuf], uid: u32) -> Vec<TrashDir> {
    let mut found = Vec::new();

    let home = home_trash(data_home);
    if home.is_present() {
        found.push(home);
    }

    for mount_point in mount_points {
        for candidate in top_dir_trashes(mount_point, uid) {
            if candidate.is_present() && !found.contains(&candidate) {
                found.push(candidate);
            }
        }
    }
    found
}

/// The trash directory whose `files/` contains `path`, if any.
pub fn containing<'a>(path: &Path, dirs: &'a [TrashDir]) -> Option<&'a TrashDir> {
    dirs.iter().find(|dir| path.starts_with(dir.files()))
}

/// True when `path` is a trashed item, or a directory inside one.
pub fn is_in_trash(path: &Path, dirs: &[TrashDir]) -> bool {
    containing(path, dirs).is_some()
}

// ------------------------------------------------------------------- records ----

/// What a `.trashinfo` record says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashInfo {
    /// The absolute path the item came from, with its raw bytes intact.
    pub original: PathBuf,
    /// The specification's `DeletionDate`, kept verbatim because it is display-only.
    pub deleted_at: Option<String>,
}

/// Why a restore record could not be used.
#[derive(Debug)]
pub enum InfoError {
    Unreadable(io::Error),
    Missing,
    Malformed,
    NoOriginalPath,
}

impl fmt::Display for InfoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(error) => write!(formatter, "its trash record is unreadable: {error}"),
            Self::Missing => {
                formatter.write_str("it has no trash record, so its original location is unknown")
            }
            Self::Malformed => {
                formatter.write_str("its trash record is not a valid [Trash Info] record")
            }
            Self::NoOriginalPath => {
                formatter.write_str("its trash record does not record an original location")
            }
        }
    }
}

impl std::error::Error for InfoError {}

/// Read and parse the record for a trashed item.
pub fn read_info(info_path: &Path, top_dir: &Path) -> Result<TrashInfo, InfoError> {
    let contents = match fs::read(info_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(InfoError::Missing),
        Err(error) => return Err(InfoError::Unreadable(error)),
    };
    parse_info(&contents, top_dir)
}

/// Parse a `.trashinfo` record.
///
/// The file is handled as bytes throughout. `Path=` may legally contain percent escapes
/// for bytes that are not valid UTF-8, and some implementations write those bytes
/// unescaped, so neither form may be pushed through a `String`.
pub fn parse_info(contents: &[u8], top_dir: &Path) -> Result<TrashInfo, InfoError> {
    let mut in_section = false;
    let mut original: Option<PathBuf> = None;
    let mut deleted_at: Option<String> = None;

    for line in contents.split(|byte| *byte == b'\n') {
        let line = trim_ascii(line);
        if line.is_empty() {
            continue;
        }
        if line.starts_with(b"[") {
            in_section = line.eq_ignore_ascii_case(b"[Trash Info]");
            continue;
        }
        if !in_section {
            continue;
        }

        // The specification says the first occurrence of a key wins, so a second
        // Path= cannot redirect a restore somewhere else.
        if let Some(value) = line.strip_prefix(b"Path=") {
            if original.is_none() {
                original = Some(resolve_original(value, top_dir));
            }
        } else if let Some(value) = line.strip_prefix(b"DeletionDate=")
            && deleted_at.is_none()
        {
            deleted_at = Some(String::from_utf8_lossy(value).into_owned());
        }
    }

    if !in_section && original.is_none() {
        return Err(InfoError::Malformed);
    }
    match original {
        Some(original) => Ok(TrashInfo {
            original,
            deleted_at,
        }),
        None => Err(InfoError::NoOriginalPath),
    }
}

/// Turn a record's `Path=` value into an absolute path with its raw bytes intact.
fn resolve_original(value: &[u8], top_dir: &Path) -> PathBuf {
    let decoded = PathBuf::from(OsString::from_vec(percent_decode(value)));
    if decoded.is_absolute() {
        decoded
    } else {
        top_dir.join(decoded)
    }
}

/// Percent-decode a record value into raw bytes.
///
/// An escape that is not two hex digits is kept literally, because a stray `%` in a
/// filename is far more likely than a truncated record, and guessing would corrupt the
/// name being restored.
fn percent_decode(value: &[u8]) -> Vec<u8> {
    let mut decoded = Vec::with_capacity(value.len());
    let mut index = 0;

    while index < value.len() {
        if value[index] == b'%' && index + 2 < value.len() {
            let high = (value[index + 1] as char).to_digit(16);
            let low = (value[index + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                decoded.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        decoded.push(value[index]);
        index += 1;
    }
    decoded
}

fn trim_ascii(mut line: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = line {
        if first.is_ascii_whitespace() {
            line = rest;
        } else {
            break;
        }
    }
    while let [rest @ .., last] = line {
        if last.is_ascii_whitespace() {
            line = rest;
        } else {
            break;
        }
    }
    line
}

// ------------------------------------------------------------------- listing ----

/// One entry in a trash directory, paired with whatever its record says.
#[derive(Debug)]
pub struct TrashedItem {
    /// The item as it exists now, inside `files/`.
    pub file: PathBuf,
    /// Where its record lives, whether or not the record is readable.
    pub info: PathBuf,
    /// The record, or the reason it cannot be used.
    pub info_result: Result<TrashInfo, InfoError>,
}

impl TrashedItem {
    /// The name the item should be restored under, taken from its record.
    pub fn original_name(&self) -> Option<&OsStr> {
        self.info_result
            .as_ref()
            .ok()
            .and_then(|info| info.original.file_name())
    }

    /// The directory the item should be restored into.
    pub fn original_parent(&self) -> Option<&Path> {
        self.info_result
            .as_ref()
            .ok()
            .and_then(|info| info.original.parent())
    }

    /// Where the item came from and when it was deleted, for messages about it.
    ///
    /// The record's two useful facts are worth repeating whenever Teral has to explain
    /// why an item could not be restored, because the name alone rarely identifies
    /// which of several same-named files is meant.
    pub fn origin_summary(&self) -> Option<String> {
        let info = self.info_result.as_ref().ok()?;
        let parent = info.original.parent()?;
        Some(match info.deleted_at.as_deref() {
            Some(deleted_at) => {
                format!("from {}, deleted {deleted_at}", parent.to_string_lossy())
            }
            None => format!("from {}", parent.to_string_lossy()),
        })
    }
}

/// Describe one trashed path, reading its record.
pub fn item_at(path: &Path, dir: &TrashDir) -> TrashedItem {
    let name = path.file_name().unwrap_or(path.as_os_str());
    let info = dir.info_path(name);
    let info_result = read_info(&info, &dir.top_dir);
    TrashedItem {
        file: path.to_path_buf(),
        info,
        info_result,
    }
}

/// Everything currently in one trash directory.
///
/// Entries whose record is missing or malformed are still listed. They cannot be
/// restored, but they are real data and must never be hidden from Empty Trash.
pub fn list(dir: &TrashDir) -> io::Result<Vec<TrashedItem>> {
    let mut items = Vec::new();
    for entry in fs::read_dir(dir.files())? {
        let entry = entry?;
        items.push(item_at(&entry.path(), dir));
    }
    items.sort_by(|left, right| left.file.cmp(&right.file));
    Ok(items)
}

/// Records in `info/` whose data is already gone.
///
/// These strand nothing on their own, but they keep an emptied trash from ever looking
/// empty, so Empty Trash clears them once every item it names has been removed.
pub fn orphan_records(dir: &TrashDir) -> io::Result<Vec<PathBuf>> {
    let files = dir.files();
    let mut orphans = Vec::new();

    for entry in fs::read_dir(dir.info())? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(stem) = strip_info_suffix(&name) else {
            continue;
        };
        if fs::symlink_metadata(files.join(stem)).is_err() {
            orphans.push(entry.path());
        }
    }
    orphans.sort();
    Ok(orphans)
}

fn strip_info_suffix(name: &OsStr) -> Option<OsString> {
    let bytes = name.as_bytes();
    bytes
        .len()
        .checked_sub(INFO_SUFFIX.len())
        .filter(|split| &bytes[*split..] == INFO_SUFFIX.as_bytes())
        .map(|split| OsString::from_vec(bytes[..split].to_vec()))
}

/// Count everything Empty Trash would remove, so the confirmation can name real numbers.
pub fn count(dirs: &[TrashDir]) -> usize {
    dirs.iter()
        .map(|dir| {
            fs::read_dir(dir.files())
                .map(|entries| entries.flatten().count())
                .unwrap_or(0)
        })
        .sum()
}

// ------------------------------------------------------------------ deletion ----

/// Remove an entry and everything under it, never following a symlink.
///
/// A symlink — including one pointing at a directory — is unlinked, so the thing it
/// points at is untouched. Directory contents go through `std::fs::remove_dir_all`,
/// which walks the tree with directory handles rather than paths, so an entry that is
/// swapped for a symlink while the walk is running cannot redirect the deletion outside
/// the tree. That safety is why a single directory is removed in one uninterruptible
/// step; cancellation is offered between items instead, where stopping is safe.
pub fn remove_tree(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Why a permanent deletion did not fully succeed.
#[derive(Debug)]
pub enum PurgeError {
    /// Nothing was removed. The item and its record are both intact, so it can still
    /// be restored.
    Failed(io::Error),
    /// The data is gone, but its record could not be removed. Nothing is stranded;
    /// the leftover record is reported so it is never mistaken for recoverable data.
    RecordRemains(io::Error),
}

impl fmt::Display for PurgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed(error) => write!(formatter, "{error}"),
            Self::RecordRemains(error) => write!(
                formatter,
                "it was deleted, but its trash record could not be removed: {error}"
            ),
        }
    }
}

impl std::error::Error for PurgeError {}

/// Delete a trashed item permanently.
///
/// The record is removed only after the data is gone. If the data cannot be removed the
/// record is left exactly as it was, so the item stays restorable.
pub fn purge(file: &Path, info: Option<&Path>) -> Result<(), PurgeError> {
    match remove_tree(file) {
        Ok(()) => {}
        // Something else already removed it. The item is gone either way, so the
        // record should follow rather than being stranded.
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let _ = error;
        }
        Err(error) => return Err(PurgeError::Failed(error)),
    }

    if let Some(info) = info {
        match fs::remove_file(info) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(PurgeError::RecordRemains(error)),
        }
    }
    Ok(())
}

/// What happened to one item in a batch deletion.
#[derive(Debug, PartialEq, Eq)]
pub enum Removal {
    /// Gone, along with its restore record.
    Removed,
    /// The data is gone but its record could not be removed.
    RecordRemains(String),
    /// Nothing was removed; the item is still exactly where it was.
    Failed(String),
    /// The batch was cancelled before reaching this item.
    Cancelled,
}

/// Permanently delete a batch, one item at a time.
///
/// Cancellation is checked between items, which is the only point at which stopping
/// leaves nothing half-removed. Every item after the cancellation is reported as
/// `Cancelled` rather than being quietly dropped from the result, and one item's failure
/// never stops the rest of the batch: the caller gets an outcome for every input, in
/// order, so a partial run can be reported honestly.
pub fn purge_batch(
    items: &[(PathBuf, Option<PathBuf>)],
    cancelled: &dyn Fn() -> bool,
    mut observed: impl FnMut(usize, &Path),
) -> Vec<Removal> {
    let mut outcomes = Vec::with_capacity(items.len());

    for (index, (file, info)) in items.iter().enumerate() {
        if cancelled() {
            outcomes.extend((index..items.len()).map(|_| Removal::Cancelled));
            return outcomes;
        }
        outcomes.push(match purge(file, info.as_deref()) {
            Ok(()) => Removal::Removed,
            Err(error @ PurgeError::RecordRemains(_)) => Removal::RecordRemains(error.to_string()),
            Err(error) => Removal::Failed(error.to_string()),
        });
        observed(index, file);
    }
    outcomes
}

/// Remove a restore record once its item has been restored.
///
/// Called only after a restore has actually placed the data somewhere, so a failure
/// here leaves a record pointing at a location that is now occupied by the restored
/// item — untidy, but never destructive.
pub fn discard_record(info: &Path) -> io::Result<()> {
    match fs::remove_file(info) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch(name: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let dir = std::env::temp_dir().join(format!(
            "teral-trash-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch directory");
        dir
    }

    /// Build an empty trash directory pair under `root`.
    fn make_trash(root: &Path, top_dir: &Path) -> TrashDir {
        let dir = TrashDir::new(root.to_path_buf(), top_dir.to_path_buf());
        fs::create_dir_all(dir.files()).expect("files");
        fs::create_dir_all(dir.info()).expect("info");
        dir
    }

    /// Percent-encode the way the specification does, for building test records.
    fn encode(path: &Path) -> Vec<u8> {
        let mut encoded = Vec::new();
        for byte in path.as_os_str().as_bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
                encoded.push(*byte);
            } else {
                encoded.extend_from_slice(format!("%{byte:02X}").as_bytes());
            }
        }
        encoded
    }

    /// Put `name` into `dir`'s trash with a record pointing at `original`.
    fn stage(dir: &TrashDir, name: &OsStr, original: &Path, contents: &[u8]) -> PathBuf {
        let file = dir.files().join(name);
        fs::write(&file, contents).expect("trashed file");
        let mut record = b"[Trash Info]\nPath=".to_vec();
        record.extend_from_slice(&encode(original));
        record.extend_from_slice(b"\nDeletionDate=2026-08-31T10:00:00\n");
        fs::write(dir.info_path(name), record).expect("record");
        file
    }

    fn raw(bytes: &[u8]) -> OsString {
        OsString::from_vec(bytes.to_vec())
    }

    // ------------------------------------------------------------- records ----

    #[test]
    fn a_record_decodes_an_absolute_original_path() {
        let info = parse_info(
            b"[Trash Info]\nPath=/home/zub/My%20Notes.txt\nDeletionDate=2026-08-31T10:00:00\n",
            Path::new("/"),
        )
        .expect("record");
        assert_eq!(info.original, PathBuf::from("/home/zub/My Notes.txt"));
        assert_eq!(info.deleted_at.as_deref(), Some("2026-08-31T10:00:00"));
    }

    #[test]
    fn a_relative_record_resolves_against_its_filesystem() {
        let info = parse_info(
            b"[Trash Info]\nPath=photos/holiday.jpg\n",
            Path::new("/media/usb"),
        )
        .expect("record");
        assert_eq!(
            info.original,
            PathBuf::from("/media/usb/photos/holiday.jpg")
        );
        assert_eq!(info.deleted_at, None);
    }

    #[test]
    fn a_record_keeps_non_utf8_bytes_exactly() {
        // %FF is not valid UTF-8, and must survive as one raw byte.
        let info =
            parse_info(b"[Trash Info]\nPath=/tmp/na%FFme.txt\n", Path::new("/")).expect("record");
        assert_eq!(
            info.original.as_os_str().as_bytes(),
            b"/tmp/na\xffme.txt".as_slice()
        );
        // The byte must still be there, not repaired into a replacement character.
        assert!(info.original.as_os_str().as_bytes().contains(&0xff));
    }

    #[test]
    fn a_record_keeps_spaces_and_newlines_in_names() {
        let info = parse_info(
            b"[Trash Info]\nPath=/tmp/two%20words%0Aand%20a%20line.txt\n",
            Path::new("/"),
        )
        .expect("record");
        assert_eq!(
            info.original.file_name().expect("name").as_bytes(),
            b"two words\nand a line.txt".as_slice()
        );
    }

    #[test]
    fn a_lone_percent_in_a_name_is_kept_literally() {
        let info =
            parse_info(b"[Trash Info]\nPath=/tmp/100%.txt\n", Path::new("/")).expect("record");
        assert_eq!(info.original, PathBuf::from("/tmp/100%.txt"));
    }

    #[test]
    fn the_first_path_in_a_record_wins() {
        let info = parse_info(
            b"[Trash Info]\nPath=/tmp/real.txt\nPath=/etc/passwd\n",
            Path::new("/"),
        )
        .expect("record");
        assert_eq!(info.original, PathBuf::from("/tmp/real.txt"));
    }

    #[test]
    fn a_record_without_a_section_header_is_malformed() {
        assert!(matches!(
            parse_info(b"Path=/tmp/x\n", Path::new("/")),
            Err(InfoError::Malformed)
        ));
    }

    #[test]
    fn a_record_without_a_path_is_rejected() {
        assert!(matches!(
            parse_info(
                b"[Trash Info]\nDeletionDate=2026-08-31T10:00:00\n",
                Path::new("/")
            ),
            Err(InfoError::NoOriginalPath)
        ));
        assert!(matches!(
            parse_info(b"", Path::new("/")),
            Err(InfoError::Malformed)
        ));
    }

    #[test]
    fn a_missing_record_is_reported_as_missing_not_as_an_io_error() {
        let root = scratch("missing-record");
        assert!(matches!(
            read_info(&root.join("nothing.trashinfo"), Path::new("/")),
            Err(InfoError::Missing)
        ));
        fs::remove_dir_all(root).expect("cleanup");
    }

    // ------------------------------------------------------------ listing ----

    #[test]
    fn listing_includes_entries_whose_record_is_broken() {
        let root = scratch("listing");
        let dir = make_trash(&root.join("Trash"), &root);

        stage(
            &dir,
            OsStr::new("good.txt"),
            &root.join("good.txt"),
            b"payload",
        );
        // An entry with no record at all.
        fs::write(dir.files().join("orphan.txt"), b"payload").expect("orphan");
        // An entry whose record is unusable.
        fs::write(dir.files().join("broken.txt"), b"payload").expect("broken");
        fs::write(dir.info_path(OsStr::new("broken.txt")), b"nonsense").expect("record");

        let items = list(&dir).expect("listing");
        assert_eq!(items.len(), 3);

        let broken = items
            .iter()
            .find(|item| item.file.file_name() == Some(OsStr::new("broken.txt")))
            .expect("broken entry");
        assert!(matches!(broken.info_result, Err(InfoError::Malformed)));

        let orphan = items
            .iter()
            .find(|item| item.file.file_name() == Some(OsStr::new("orphan.txt")))
            .expect("orphan entry");
        assert!(matches!(orphan.info_result, Err(InfoError::Missing)));

        let good = items
            .iter()
            .find(|item| item.file.file_name() == Some(OsStr::new("good.txt")))
            .expect("good entry");
        assert_eq!(good.original_name(), Some(OsStr::new("good.txt")));
        assert_eq!(good.original_parent(), Some(root.as_path()));

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_trashed_name_may_differ_from_the_name_being_restored() {
        let root = scratch("renamed-in-trash");
        let dir = make_trash(&root.join("Trash"), &root);
        // The desktop de-duplicates inside the trash; the record still names the original.
        stage(
            &dir,
            OsStr::new("notes.2.txt"),
            &root.join("notes.txt"),
            b"payload",
        );

        let items = list(&dir).expect("listing");
        assert_eq!(items[0].file.file_name(), Some(OsStr::new("notes.2.txt")));
        assert_eq!(items[0].original_name(), Some(OsStr::new("notes.txt")));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_non_utf8_trashed_name_round_trips_through_a_listing() {
        let root = scratch("non-utf8-listing");
        let dir = make_trash(&root.join("Trash"), &root);
        let name = raw(b"bad\xffname.txt");
        stage(&dir, &name, &root.join(&name), b"payload");

        let items = list(&dir).expect("listing");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].file.file_name(), Some(name.as_os_str()));
        assert_eq!(
            items[0].original_name().expect("name").as_bytes(),
            b"bad\xffname.txt".as_slice()
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn an_origin_summary_names_the_folder_and_the_deletion_time() {
        let root = scratch("origin-summary");
        let dir = make_trash(&root.join("Trash"), &root);
        stage(
            &dir,
            OsStr::new("a.txt"),
            &root.join("papers/a.txt"),
            b"payload",
        );

        let item = item_at(&dir.files().join("a.txt"), &dir);
        let summary = item.origin_summary().expect("summary");
        assert!(summary.contains("papers"), "{summary}");
        assert!(summary.contains("2026-08-31T10:00:00"), "{summary}");

        // An item with no usable record has nothing truthful to say about its origin.
        fs::write(dir.files().join("b.txt"), b"payload").expect("orphan");
        let orphan = item_at(&dir.files().join("b.txt"), &dir);
        assert_eq!(orphan.origin_summary(), None);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn orphan_records_are_the_ones_whose_data_is_gone() {
        let root = scratch("orphans");
        let dir = make_trash(&root.join("Trash"), &root);
        stage(&dir, OsStr::new("kept.txt"), &root.join("kept.txt"), b"x");
        stage(&dir, OsStr::new("gone.txt"), &root.join("gone.txt"), b"x");
        fs::remove_file(dir.files().join("gone.txt")).expect("remove data");

        let orphans = orphan_records(&dir).expect("orphans");
        assert_eq!(orphans, vec![dir.info_path(OsStr::new("gone.txt"))]);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn counting_names_the_real_number_of_items() {
        let root = scratch("count");
        let first = make_trash(&root.join("home/Trash"), Path::new("/"));
        let second = make_trash(&root.join("usb/.Trash-1000"), &root.join("usb"));
        stage(&first, OsStr::new("a"), &root.join("a"), b"x");
        stage(&first, OsStr::new("b"), &root.join("b"), b"x");
        stage(&second, OsStr::new("c"), &root.join("usb/c"), b"x");

        assert_eq!(count(&[first, second]), 3);
        fs::remove_dir_all(root).expect("cleanup");
    }

    // -------------------------------------------------------- discovery ----

    #[test]
    fn discovery_finds_the_home_trash_and_secondary_filesystems() {
        let root = scratch("discovery");
        let data_home = root.join("data");
        let usb = root.join("usb");
        make_trash(&data_home.join("Trash"), Path::new("/"));
        make_trash(&usb.join(".Trash-1000"), &usb);
        // A mount with no trash yet must not be invented.
        fs::create_dir_all(root.join("empty-mount")).expect("mount");

        let found = discover(&data_home, &[usb.clone(), root.join("empty-mount")], 1000);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].root, data_home.join("Trash"));
        assert_eq!(found[1].root, usb.join(".Trash-1000"));
        assert_eq!(found[1].top_dir, usb);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_shared_trash_is_only_trusted_when_it_is_sticky_and_not_a_symlink() {
        use std::os::unix::fs::PermissionsExt;

        let root = scratch("shared-trash");
        let usb = root.join("usb");
        fs::create_dir_all(&usb).expect("mount");

        // A plain, non-sticky .Trash must be ignored.
        fs::create_dir(usb.join(".Trash")).expect(".Trash");
        let candidates = top_dir_trashes(&usb, 1000);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].root, usb.join(".Trash-1000"));

        // With the sticky bit it becomes the preferred location.
        fs::set_permissions(usb.join(".Trash"), fs::Permissions::from_mode(0o1777))
            .expect("sticky");
        let candidates = top_dir_trashes(&usb, 1000);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].root, usb.join(".Trash/1000"));

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_symlinked_shared_trash_is_never_used() {
        let root = scratch("symlinked-trash");
        let usb = root.join("usb");
        let elsewhere = root.join("elsewhere");
        fs::create_dir_all(&usb).expect("mount");
        fs::create_dir_all(&elsewhere).expect("elsewhere");
        std::os::unix::fs::symlink(&elsewhere, usb.join(".Trash")).expect("symlink");

        let candidates = top_dir_trashes(&usb, 1000);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].root, usb.join(".Trash-1000"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn the_home_trash_is_located_without_asking_the_filesystem() {
        // Deciding whether a path is in the trash happens on every selection change, so
        // it must not depend on a directory existing, on a disk being reachable, or on
        // any `stat` at all. A path that has never been created still resolves.
        let never_created = PathBuf::from("/nonexistent-for-this-test/share");
        let home = home_trash(&never_created);
        assert_eq!(
            home.files(),
            never_created.join("Trash/files"),
            "the home trash location is derived from the path alone"
        );
        assert!(!home.is_present(), "and it is not there");
        assert!(is_in_trash(
            &never_created.join("Trash/files/deleted.txt"),
            std::slice::from_ref(&home)
        ));
        assert!(!is_in_trash(
            &never_created.join("Documents/kept.txt"),
            &[home]
        ));
    }

    #[test]
    fn trash_membership_covers_every_discovered_location() {
        let root = scratch("membership");
        let data_home = root.join("data");
        let usb = root.join("usb");
        make_trash(&data_home.join("Trash"), Path::new("/"));
        make_trash(&usb.join(".Trash-1000"), &usb);
        let dirs = discover(&data_home, std::slice::from_ref(&usb), 1000);

        assert!(is_in_trash(&data_home.join("Trash/files/a.txt"), &dirs));
        assert!(is_in_trash(
            &usb.join(".Trash-1000/files/deep/inside.txt"),
            &dirs
        ));
        assert!(!is_in_trash(&root.join("documents/a.txt"), &dirs));
        // The info directory is not the data directory.
        assert!(!is_in_trash(
            &data_home.join("Trash/info/a.trashinfo"),
            &dirs
        ));
        fs::remove_dir_all(root).expect("cleanup");
    }

    // ------------------------------------------------------------ removal ----

    #[test]
    fn recursive_removal_unlinks_a_directory_symlink_without_following_it() {
        let root = scratch("symlink-safety");
        let outside = root.join("outside");
        let keep = outside.join("precious.txt");
        fs::create_dir_all(&outside).expect("outside");
        fs::write(&keep, b"do not delete").expect("precious");

        let doomed = root.join("doomed");
        fs::create_dir_all(doomed.join("nested")).expect("doomed");
        fs::write(doomed.join("nested/inner.txt"), b"gone").expect("inner");
        std::os::unix::fs::symlink(&outside, doomed.join("link-to-outside")).expect("symlink");

        remove_tree(&doomed).expect("removal");

        assert!(!doomed.exists());
        assert_eq!(fs::read(&keep).expect("survivor"), b"do not delete");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn removing_a_symlink_never_touches_its_target() {
        let root = scratch("symlink-target");
        let target = root.join("target.txt");
        fs::write(&target, b"payload").expect("target");
        let link = root.join("link");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");

        remove_tree(&link).expect("removal");
        assert!(fs::symlink_metadata(&link).is_err());
        assert_eq!(fs::read(&target).expect("target"), b"payload");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_broken_symlink_is_removed_rather_than_reported_as_missing() {
        let root = scratch("broken-symlink");
        let link = root.join("broken");
        std::os::unix::fs::symlink(root.join("nowhere"), &link).expect("symlink");

        remove_tree(&link).expect("removal");
        assert!(fs::symlink_metadata(&link).is_err());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn purging_removes_the_record_only_after_the_data_is_gone() {
        let root = scratch("purge");
        let dir = make_trash(&root.join("Trash"), &root);
        let file = stage(&dir, OsStr::new("a.txt"), &root.join("a.txt"), b"payload");
        let info = dir.info_path(OsStr::new("a.txt"));

        purge(&file, Some(&info)).expect("purge");
        assert!(fs::symlink_metadata(&file).is_err());
        assert!(fs::symlink_metadata(&info).is_err());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_failed_purge_keeps_the_record_so_the_item_stays_restorable() {
        use std::os::unix::fs::PermissionsExt;

        let root = scratch("purge-denied");
        let dir = make_trash(&root.join("Trash"), &root);
        // A directory inside the trash whose parent is read-only cannot be unlinked.
        let locked = dir.files().join("locked");
        fs::create_dir(&locked).expect("locked");
        fs::write(locked.join("inner.txt"), b"payload").expect("inner");
        let mut record = b"[Trash Info]\nPath=".to_vec();
        record.extend_from_slice(&encode(&root.join("locked")));
        record.push(b'\n');
        let info = dir.info_path(OsStr::new("locked"));
        fs::write(&info, record).expect("record");

        fs::set_permissions(dir.files(), fs::Permissions::from_mode(0o500)).expect("read-only");
        let outcome = purge(&locked, Some(&info));
        fs::set_permissions(dir.files(), fs::Permissions::from_mode(0o700)).expect("restore mode");

        // Root can delete regardless of mode, so only assert the invariant that matters:
        // the record survives exactly when the data does.
        match outcome {
            Err(PurgeError::Failed(_)) => {
                assert!(locked.exists(), "the data must survive a failed purge");
                assert!(
                    info.exists(),
                    "the record must survive so the item stays restorable"
                );
            }
            Ok(()) => {
                assert!(!locked.exists());
                assert!(!info.exists());
            }
            Err(other) => panic!("unexpected outcome: {other}"),
        }
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn purging_an_item_that_already_vanished_still_clears_its_record() {
        let root = scratch("purge-vanished");
        let dir = make_trash(&root.join("Trash"), &root);
        let file = stage(&dir, OsStr::new("a.txt"), &root.join("a.txt"), b"payload");
        let info = dir.info_path(OsStr::new("a.txt"));
        fs::remove_file(&file).expect("something else removed it");

        purge(&file, Some(&info)).expect("purge");
        assert!(fs::symlink_metadata(&info).is_err());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn purging_an_item_with_no_record_is_not_an_error() {
        let root = scratch("purge-no-record");
        let dir = make_trash(&root.join("Trash"), &root);
        let file = dir.files().join("orphan.txt");
        fs::write(&file, b"payload").expect("orphan");

        purge(&file, None).expect("purge");
        assert!(fs::symlink_metadata(&file).is_err());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn discarding_an_already_missing_record_succeeds() {
        let root = scratch("discard");
        discard_record(&root.join("nothing.trashinfo")).expect("discard");
        fs::remove_dir_all(root).expect("cleanup");
    }
    #[test]
    fn a_batch_reports_every_item_even_when_one_fails() {
        let root = scratch("batch-partial");
        let dir = make_trash(&root.join("Trash"), &root);
        let first = stage(&dir, OsStr::new("a.txt"), &root.join("a.txt"), b"x");
        let second = stage(&dir, OsStr::new("b.txt"), &root.join("b.txt"), b"x");

        // The middle entry does not exist at all, which must not stop the batch.
        let missing = dir.files().join("never-existed");
        let items = vec![
            (first.clone(), Some(dir.info_path(OsStr::new("a.txt")))),
            (missing, None),
            (second.clone(), Some(dir.info_path(OsStr::new("b.txt")))),
        ];

        let outcomes = purge_batch(&items, &|| false, |_, _| {});
        assert_eq!(outcomes.len(), 3);
        assert_eq!(outcomes[0], Removal::Removed);
        assert_eq!(outcomes[2], Removal::Removed);
        assert!(fs::symlink_metadata(&first).is_err());
        assert!(fs::symlink_metadata(&second).is_err());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn cancelling_a_batch_leaves_the_rest_of_the_trash_intact() {
        let root = scratch("batch-cancel");
        let dir = make_trash(&root.join("Trash"), &root);
        let names = ["a.txt", "b.txt", "c.txt"];
        let items: Vec<(PathBuf, Option<PathBuf>)> = names
            .iter()
            .map(|name| {
                let name = OsStr::new(name);
                let file = stage(&dir, name, &root.join(name), b"payload");
                (file, Some(dir.info_path(name)))
            })
            .collect();

        // Cancel as soon as the first item has been removed.
        let stop = std::cell::Cell::new(false);
        let outcomes = purge_batch(&items, &|| stop.get(), |_, _| stop.set(true));

        assert_eq!(outcomes[0], Removal::Removed);
        assert_eq!(outcomes[1], Removal::Cancelled);
        assert_eq!(outcomes[2], Removal::Cancelled);

        // Everything not reached is still present, and still restorable.
        assert!(fs::symlink_metadata(&items[0].0).is_err());
        for (file, info) in &items[1..] {
            assert!(file.exists(), "cancelled items must survive");
            assert!(
                info.as_ref().expect("record").exists(),
                "a cancelled item must keep the record that makes it restorable"
            );
        }
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn an_empty_batch_is_not_an_error() {
        assert!(purge_batch(&[], &|| false, |_, _| {}).is_empty());
    }

    #[test]
    fn a_batch_cancelled_before_it_starts_removes_nothing() {
        let root = scratch("batch-cancel-first");
        let dir = make_trash(&root.join("Trash"), &root);
        let file = stage(&dir, OsStr::new("a.txt"), &root.join("a.txt"), b"payload");
        let info = dir.info_path(OsStr::new("a.txt"));

        let outcomes = purge_batch(&[(file.clone(), Some(info.clone()))], &|| true, |_, _| {
            panic!("nothing should have been touched")
        });
        assert_eq!(outcomes, vec![Removal::Cancelled]);
        assert!(file.exists());
        assert!(info.exists());
        fs::remove_dir_all(root).expect("cleanup");
    }
}
