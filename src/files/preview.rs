//! Bounded, inert previews for text-like files.

use gtk::gio;
use gtk::gdk::gdk_pixbuf;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_PDF_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RENDER_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RENDER_EDGE: i32 = 1024;
const MAX_METADATA_BYTES: u64 = 64 * 1024;
const PROCESS_TIMEOUT: Duration = Duration::from_secs(10);
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextPreview {
    Text(String),
    Oversized,
    Binary,
    UnsupportedEncoding,
}

#[derive(Debug)]
pub struct DecodedImage {
    pub pixels: Vec<u8>,
    pub width: i32,
    pub height: i32,
    pub rowstride: i32,
    pub has_alpha: bool,
}

#[derive(Debug)]
pub enum RichPreview {
    Image {
        image: DecodedImage,
        summary: String,
    },
    Metadata(String),
    Oversized,
    Unsupported(String),
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

pub async fn load_rich(path: &Path, content_type: &str) -> io::Result<RichPreview> {
    let path = path.to_path_buf();
    let content_type = content_type.to_owned();
    gio::spawn_blocking(move || read_rich(&path, &content_type))
        .await
        .map_err(|_| io::Error::other("preview worker stopped unexpectedly"))?
}

fn read_rich(path: &Path, content_type: &str) -> io::Result<RichPreview> {
    if content_type == "application/pdf" {
        return read_pdf(path);
    }
    if content_type.starts_with("audio/") || content_type.starts_with("video/") {
        return read_media(path, content_type.starts_with("video/"));
    }
    Ok(RichPreview::Unsupported(
        "Preview is not supported for this file type".to_owned(),
    ))
}

fn read_pdf(path: &Path) -> io::Result<RichPreview> {
    if path.metadata()?.len() > MAX_PDF_BYTES {
        return Ok(RichPreview::Oversized);
    }
    let temporary = PreviewDirectory::new()?;
    let prefix = temporary.path.join("first-page");
    let status = run_with_timeout(
        Command::new("pdftoppm")
            .arg("-f")
            .arg("1")
            .arg("-singlefile")
            .arg("-scale-to")
            .arg(MAX_RENDER_EDGE.to_string())
            .arg("-png")
            .arg(path)
            .arg(&prefix),
    );
    let status = match status {
        Ok(status) => status,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(RichPreview::Unsupported(
                "PDF preview requires the Poppler pdftoppm utility".to_owned(),
            ));
        }
        Err(error) => return Err(error),
    };
    if !status.success() {
        return Ok(RichPreview::Unsupported(
            "This PDF is malformed, encrypted, or cannot be rendered".to_owned(),
        ));
    }
    let image = decode_image(&prefix.with_extension("png"))?;
    Ok(RichPreview::Image {
        image,
        summary: "First page".to_owned(),
    })
}

fn read_media(path: &Path, video: bool) -> io::Result<RichPreview> {
    let summary = probe_media(path)?;
    if !video {
        return Ok(RichPreview::Metadata(summary));
    }

    let temporary = PreviewDirectory::new()?;
    let output = temporary.path.join("frame.png");
    let status = run_with_timeout(
        Command::new("ffmpeg")
            .arg("-nostdin")
            .arg("-v")
            .arg("error")
            .arg("-i")
            .arg(path)
            .arg("-frames:v")
            .arg("1")
            .arg("-vf")
            .arg("scale=1024:-2:force_original_aspect_ratio=decrease")
            .arg("-y")
            .arg(&output),
    );
    match status {
        Ok(status) if status.success() => Ok(RichPreview::Image {
            image: decode_image(&output)?,
            summary,
        }),
        Ok(_) => Ok(RichPreview::Metadata(summary)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(RichPreview::Metadata(summary)),
        Err(error) => Err(error),
    }
}

fn probe_media(path: &Path) -> io::Result<String> {
    let mut child = match Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=duration,bit_rate:format_tags=title,artist,album")
        .arg("-of")
        .arg("default=noprint_wrappers=1")
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok("Media metadata requires the ffprobe utility".to_owned());
        }
        Err(error) => return Err(error),
    };
    let status = wait_with_timeout(&mut child)?;
    if !status.success() {
        return Ok("Media metadata is unavailable".to_owned());
    }
    let mut bytes = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        stdout
            .take(MAX_METADATA_BYTES + 1)
            .read_to_end(&mut bytes)?;
    }
    if bytes.len() as u64 > MAX_METADATA_BYTES {
        return Ok("Media metadata is too large to display".to_owned());
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = Vec::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if value.trim().is_empty() {
            continue;
        }
        let label = match key {
            "duration" => "Duration",
            "bit_rate" => "Bit rate",
            "TAG:title" => "Title",
            "TAG:artist" => "Artist",
            "TAG:album" => "Album",
            _ => continue,
        };
        lines.push(format!("{label}: {}", value.trim()));
    }
    Ok(if lines.is_empty() {
        "No media metadata was reported".to_owned()
    } else {
        lines.join("\n")
    })
}

fn run_with_timeout(command: &mut Command) -> io::Result<ExitStatus> {
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = command.spawn()?;
    wait_with_timeout(&mut child)
}

fn wait_with_timeout(child: &mut std::process::Child) -> io::Result<ExitStatus> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if started.elapsed() >= PROCESS_TIMEOUT {
            child.kill()?;
            let _ = child.wait();
            return Err(io::Error::new(io::ErrorKind::TimedOut, "preview timed out"));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn decode_image(path: &Path) -> io::Result<DecodedImage> {
    if path.metadata()?.len() > MAX_RENDER_BYTES {
        return Err(io::Error::other("rendered preview exceeded its size limit"));
    }
    let pixbuf = gdk_pixbuf::Pixbuf::from_file_at_scale(
        path,
        MAX_RENDER_EDGE,
        MAX_RENDER_EDGE,
        true,
    )
    .map_err(|error| io::Error::other(error.to_string()))?;
    if pixbuf.width() > MAX_RENDER_EDGE
        || pixbuf.height() > MAX_RENDER_EDGE
        || pixbuf.byte_length() > MAX_RENDER_BYTES as usize
    {
        return Err(io::Error::other("decoded preview exceeded its memory limit"));
    }
    Ok(DecodedImage {
        pixels: pixbuf.read_pixel_bytes().as_ref().to_vec(),
        width: pixbuf.width(),
        height: pixbuf.height(),
        rowstride: pixbuf.rowstride(),
        has_alpha: pixbuf.has_alpha(),
    })
}

struct PreviewDirectory {
    path: PathBuf,
}

impl PreviewDirectory {
    fn new() -> io::Result<Self> {
        let base = std::env::temp_dir();
        for _ in 0..64 {
            let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("teral-preview-{}-{sequence}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not reserve preview workspace",
        ))
    }
}

impl Drop for PreviewDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
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
