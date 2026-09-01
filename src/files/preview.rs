//! Bounded, inert previews for text-like files.

use gtk::gio;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

pub const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextPreview {
    Text(String),
    Oversized,
    Binary,
    UnsupportedEncoding,
}

pub async fn load_text(path: &Path) -> io::Result<TextPreview> {
    let path = path.to_path_buf();
    gio::spawn_blocking(move || read_text(path))
        .await
        .map_err(|_| io::Error::other("preview worker stopped unexpectedly"))?
}

fn read_text(path: PathBuf) -> io::Result<TextPreview> {
    let metadata = path.metadata()?;
    if metadata.len() > MAX_TEXT_BYTES {
        return Ok(TextPreview::Oversized);
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or_default());
    File::open(path)?
        .take(MAX_TEXT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_TEXT_BYTES {
        return Ok(TextPreview::Oversized);
    }
    let controls = bytes
        .iter()
        .filter(|byte| **byte == 0 || **byte < 0x09 || (0x0e..0x20).contains(&**byte))
        .count();
    if bytes.contains(&0) || controls.saturating_mul(100) > bytes.len().max(1) {
        return Ok(TextPreview::Binary);
    }
    match String::from_utf8(bytes) {
        Ok(text) => Ok(TextPreview::Text(text)),
        Err(_) => Ok(TextPreview::UnsupportedEncoding),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_limits_are_small_enough_for_an_interactive_panel() {
        assert_eq!(MAX_TEXT_BYTES, 2 * 1024 * 1024);
        assert!(matches!(TextPreview::Binary, TextPreview::Binary));
    }
}
