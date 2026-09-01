//! Crash-safe storage for Teral-owned state.
//!
//! Every mutable metadata file is written beside its destination, flushed, and then
//! atomically renamed. A failed write therefore leaves the last valid file untouched.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Replace `path` atomically after completely writing and syncing `contents`.
pub fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    atomic_write_with(path, |file| file.write_all(contents))
}

/// Injectable form used by callers that serialize directly and by failure tests.
pub fn atomic_write_with(
    path: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "state file has no parent directory")
    })?;
    fs::create_dir_all(parent)?;

    let previous_mode = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions().mode());
    let (temporary_path, mut temporary) = create_temporary(parent, path.file_name())?;

    let result = (|| {
        write(&mut temporary)?;
        temporary.flush()?;
        temporary.sync_all()?;
        if let Some(mode) = previous_mode {
            temporary.set_permissions(fs::Permissions::from_mode(mode))?;
        }
        drop(temporary);
        fs::rename(&temporary_path, path)?;
        sync_directory(parent)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

fn create_temporary(parent: &Path, destination: Option<&OsStr>) -> io::Result<(PathBuf, File)> {
    let destination = destination.unwrap_or_else(|| OsStr::new("state"));
    for _ in 0..128 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let mut name = OsString::from(".");
        name.push(destination);
        name.push(format!(".teral-{}-{sequence}.tmp", std::process::id()));
        let path = parent.join(name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a temporary state file",
    ))
}

fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

/// Lossless TOML-friendly representation of a Linux path.
pub fn encode_path(path: &Path) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = path.as_os_str().as_bytes();
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

/// Decode a path previously produced by [`encode_path`].
pub fn decode_path(encoded: &str) -> Result<PathBuf, String> {
    if encoded.len() % 2 != 0 {
        return Err("encoded path has an odd number of digits".to_owned());
    }
    let mut bytes = Vec::with_capacity(encoded.len() / 2);
    for pair in encoded.as_bytes().chunks_exact(2) {
        let high = hex_digit(pair[0]).ok_or_else(|| "encoded path is not hexadecimal".to_owned())?;
        let low = hex_digit(pair[1]).ok_or_else(|| "encoded path is not hexadecimal".to_owned())?;
        bytes.push((high << 4) | low);
    }
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::os::unix::ffi::OsStringExt;

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "teral-persistence-{}-{name}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).expect("scratch directory");
        directory
    }

    #[test]
    fn a_failed_write_preserves_the_last_valid_file() {
        let directory = scratch("failure");
        let path = directory.join("state.toml");
        fs::write(&path, b"valid").expect("seed");
        let error = atomic_write_with(&path, |file| {
            file.write_all(b"broken")?;
            Err(io::Error::other("injected write failure"))
        })
        .expect_err("failure is surfaced");
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(fs::read(&path).expect("old file"), b"valid");
        assert_eq!(fs::read_dir(&directory).expect("listing").count(), 1);
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn repeated_saves_replace_the_complete_file() {
        let directory = scratch("repeated");
        let path = directory.join("state.toml");
        for value in [b"one".as_slice(), b"two", b"three"] {
            atomic_write(&path, value).expect("save");
        }
        assert_eq!(fs::read(&path).expect("state"), b"three");
        assert_eq!(fs::read_dir(&directory).expect("listing").count(), 1);
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn raw_linux_paths_round_trip() {
        let raw = PathBuf::from(OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff]));
        assert_eq!(decode_path(&encode_path(&raw)).expect("decode"), raw);
        assert!(decode_path("xyz").is_err());
    }
}
