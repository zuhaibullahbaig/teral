//! Bounded recursive search for local directories.

use std::collections::VecDeque;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;

const RESULT_BATCH: usize = 48;

#[derive(Debug)]
pub enum SearchEvent {
    Batch(Vec<PathBuf>),
    Finished { unreadable: usize },
}

/// Search `root` without following symlink directories or crossing filesystem mounts.
///
/// Results retain their original `PathBuf`s. The bounded sender provides backpressure
/// when GTK is still turning an earlier batch into model entries.
pub fn run(
    root: PathBuf,
    query: String,
    show_hidden: bool,
    cancelled: Arc<AtomicBool>,
    sender: SyncSender<SearchEvent>,
) {
    let root_device = match fs::metadata(&root) {
        Ok(metadata) => metadata.dev(),
        Err(_) => {
            let _ = sender.send(SearchEvent::Finished { unreadable: 1 });
            return;
        }
    };
    let query = query.to_lowercase();
    let mut pending = VecDeque::from([root]);
    let mut batch = Vec::with_capacity(RESULT_BATCH);
    let mut unreadable = 0usize;

    while let Some(directory) = pending.pop_front() {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }
        let children = match fs::read_dir(&directory) {
            Ok(children) => children,
            Err(_) => {
                unreadable = unreadable.saturating_add(1);
                continue;
            }
        };

        for child in children {
            if cancelled.load(Ordering::Relaxed) {
                return;
            }
            let child = match child {
                Ok(child) => child,
                Err(_) => {
                    unreadable = unreadable.saturating_add(1);
                    continue;
                }
            };
            let name = child.file_name();
            if !show_hidden && name.as_encoded_bytes().starts_with(b".") {
                continue;
            }
            let path = child.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    unreadable = unreadable.saturating_add(1);
                    continue;
                }
            };

            if name.to_string_lossy().to_lowercase().contains(&query) {
                batch.push(path.clone());
                if batch.len() == RESULT_BATCH
                    && sender
                        .send(SearchEvent::Batch(std::mem::take(&mut batch)))
                        .is_err()
                {
                    return;
                }
            }

            if metadata.file_type().is_dir() && metadata.dev() == root_device {
                pending.push_back(path);
            }
        }
    }

    if !batch.is_empty() && sender.send(SearchEvent::Batch(batch)).is_err() {
        return;
    }
    let _ = sender.send(SearchEvent::Finished { unreadable });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn batches_are_bounded_and_symlinks_are_not_followed() {
        let root = std::env::temp_dir().join(format!("teral-search-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).expect("root");
        for index in 0..300 {
            fs::write(root.join(format!("match-{index}")), b"").expect("file");
        }
        std::os::unix::fs::symlink(&root, root.join("match-loop")).expect("symlink");
        let (sender, receiver) = mpsc::sync_channel(8);
        run(
            root.clone(),
            "match".to_owned(),
            true,
            Arc::new(AtomicBool::new(false)),
            sender,
        );
        let mut found = 0usize;
        while let Ok(event) = receiver.recv() {
            match event {
                SearchEvent::Batch(paths) => {
                    assert!(paths.len() <= RESULT_BATCH);
                    found += paths.len();
                }
                SearchEvent::Finished { .. } => break,
            }
        }
        assert_eq!(found, 301);
        fs::remove_dir_all(root).expect("cleanup");
    }
}
