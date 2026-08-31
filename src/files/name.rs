//! Filename rules.
//!
//! Linux allows almost anything in a filename: spaces, newlines, quotes, leading and
//! trailing whitespace, emoji, and bytes that are not valid UTF-8. Teral only refuses
//! what the kernel itself refuses, plus the two names that would silently mean a
//! different directory. Everything else is the user's business.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// The longest a single filename component may be on the filesystems Teral targets.
const MAX_NAME_BYTES: usize = 255;

/// Why a name cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    Empty,
    Separator,
    Nul,
    Dot,
    DotDot,
    TooLong,
}

impl NameError {
    /// A message that says what to change, in the words a person would use.
    pub const fn message(self) -> &'static str {
        match self {
            Self::Empty => "A name is required",
            Self::Separator => "A name cannot contain a slash",
            Self::Nul => "A name cannot contain a null character",
            Self::Dot => "“.” already means this folder",
            Self::DotDot => "“..” already means the parent folder",
            Self::TooLong => "That name is too long for the filesystem",
        }
    }
}

impl std::fmt::Display for NameError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for NameError {}

/// Check that `name` is exactly one usable filename component.
///
/// Nothing is trimmed or rewritten. A name with leading or trailing spaces is legal on
/// Linux, and quietly turning `" draft "` into `"draft"` would create a file the user
/// did not ask for and cannot find by the name they typed.
pub fn validate(name: &OsStr) -> Result<(), NameError> {
    let bytes = name.as_bytes();

    if bytes.is_empty() {
        return Err(NameError::Empty);
    }
    if bytes.contains(&b'/') {
        return Err(NameError::Separator);
    }
    if bytes.contains(&0) {
        return Err(NameError::Nul);
    }
    if bytes == b"." {
        return Err(NameError::Dot);
    }
    if bytes == b".." {
        return Err(NameError::DotDot);
    }
    if bytes.len() > MAX_NAME_BYTES {
        return Err(NameError::TooLong);
    }
    Ok(())
}

/// True when `path`'s own file name is not valid UTF-8.
///
/// Such a name can only be shown through a lossy conversion, so the text on screen is
/// not the name on disk. Anything that would write that text back to the filesystem has
/// to know the difference.
pub fn is_lossy_on_screen(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| std::str::from_utf8(name.as_bytes()).is_err())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    fn check(name: &str) -> Result<(), NameError> {
        validate(OsStr::new(name))
    }

    #[test]
    fn ordinary_names_are_accepted() {
        assert_eq!(check("notes.txt"), Ok(()));
        assert_eq!(check("Some Folder"), Ok(()));
        assert_eq!(check(".hidden"), Ok(()));
        assert_eq!(check("..hidden"), Ok(()));
        assert_eq!(check("...."), Ok(()));
    }

    #[test]
    fn names_linux_allows_are_not_second_guessed() {
        // Every one of these is a legal filename, however unusual it looks.
        assert_eq!(check("  leading spaces"), Ok(()));
        assert_eq!(check("trailing spaces  "), Ok(()));
        assert_eq!(check("   "), Ok(()));
        assert_eq!(check("two words\nand a line"), Ok(()));
        assert_eq!(check("quote\"and'apostrophe"), Ok(()));
        assert_eq!(check("star*question?brace{}"), Ok(()));
        assert_eq!(check("back\\slash"), Ok(()));
        assert_eq!(check("emoji 🎉 name"), Ok(()));
        assert_eq!(check("-leading-dash"), Ok(()));
        assert_eq!(check("~tilde"), Ok(()));
    }

    #[test]
    fn a_name_that_is_not_valid_utf8_is_still_a_name() {
        let raw = OsString::from_vec(b"bad\xffname.txt".to_vec());
        assert_eq!(validate(&raw), Ok(()));
    }

    #[test]
    fn a_name_must_be_exactly_one_component() {
        assert_eq!(check("a/b"), Err(NameError::Separator));
        assert_eq!(check("/absolute"), Err(NameError::Separator));
        assert_eq!(check("trailing/"), Err(NameError::Separator));
        assert_eq!(check("../escape"), Err(NameError::Separator));
        assert_eq!(check("/"), Err(NameError::Separator));
    }

    #[test]
    fn the_two_navigation_names_are_refused() {
        assert_eq!(check("."), Err(NameError::Dot));
        assert_eq!(check(".."), Err(NameError::DotDot));
    }

    #[test]
    fn empty_and_null_names_are_refused() {
        assert_eq!(check(""), Err(NameError::Empty));
        let with_nul = OsString::from_vec(b"a\0b".to_vec());
        assert_eq!(validate(&with_nul), Err(NameError::Nul));
    }

    #[test]
    fn names_are_refused_once_they_exceed_the_filesystem_limit() {
        assert_eq!(check(&"a".repeat(255)), Ok(()));
        assert_eq!(check(&"a".repeat(256)), Err(NameError::TooLong));
        // The limit is in bytes, not characters, which is what the kernel enforces.
        assert_eq!(check(&"é".repeat(127)), Ok(()));
        assert_eq!(check(&"é".repeat(128)), Err(NameError::TooLong));
    }

    #[test]
    fn every_refusal_explains_itself() {
        for error in [
            NameError::Empty,
            NameError::Separator,
            NameError::Nul,
            NameError::Dot,
            NameError::DotDot,
            NameError::TooLong,
        ] {
            assert!(!error.message().is_empty());
        }
    }

    #[test]
    fn a_lossy_name_is_recognised_before_it_is_written_back() {
        let raw = OsString::from_vec(b"/tmp/bad\xffname.txt".to_vec());
        assert!(is_lossy_on_screen(Path::new(&raw)));
        assert!(!is_lossy_on_screen(Path::new("/tmp/ordinary.txt")));
        assert!(!is_lossy_on_screen(Path::new("/tmp/emoji 🎉.txt")));
        assert!(!is_lossy_on_screen(Path::new("/")));
    }
}
