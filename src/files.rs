//! Filesystem domain layer.
//!
//! Everything in here is deliberately free of widget code so the presentation layer can
//! be rebuilt without touching filesystem behaviour.

pub mod entry;
pub mod ops;
pub mod scan;

pub use entry::{EntryData, FileEntry};
pub use scan::{SortKey, Sorting};

/// Format a byte count the way a Linux desktop normally does.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    const STEP: f64 = 1024.0;

    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= STEP && unit + 1 < UNITS.len() {
        value /= STEP;
        unit += 1;
    }

    if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Format a timestamp relative to today, matching how file managers usually read.
pub fn format_time(time: &gtk::glib::DateTime) -> String {
    let now = gtk::glib::DateTime::now_local().ok();
    let is_today = now
        .as_ref()
        .is_some_and(|now| now.year() == time.year() && now.day_of_year() == time.day_of_year());

    let pattern = if is_today {
        "Today %H:%M"
    } else {
        "%b %-d, %Y %H:%M"
    };
    time.format(pattern)
        .map(|formatted| formatted.to_string())
        .unwrap_or_default()
}

/// Render a Unix mode as the familiar `rwxr-xr-x` string.
pub fn format_permissions(mode: u32) -> String {
    const FLAGS: [(u32, char); 9] = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];

    let mut rendered = String::with_capacity(9);
    for (bit, symbol) in FLAGS {
        rendered.push(if mode & bit == 0 { '-' } else { symbol });
    }
    rendered
}

/// Pluralise an item count without repeating the same `if` everywhere.
pub fn item_count_label(count: usize) -> String {
    if count == 1 {
        "1 item".to_owned()
    } else {
        format!("{count} items")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_human_readable() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(8_909), "8.7 KB");
        assert_eq!(format_size(1_572_864), "1.5 MB");
        assert_eq!(format_size(1_099_511_627_776), "1.0 TB");
    }

    #[test]
    fn permissions_render_like_ls() {
        assert_eq!(format_permissions(0o755), "rwxr-xr-x");
        assert_eq!(format_permissions(0o640), "rw-r-----");
        assert_eq!(format_permissions(0o000), "---------");
    }

    #[test]
    fn item_counts_are_pluralised() {
        assert_eq!(item_count_label(0), "0 items");
        assert_eq!(item_count_label(1), "1 item");
        assert_eq!(item_count_label(12), "12 items");
    }
}
