# Changelog

Teral follows semantic versioning. See `RELEASING.md` for the release process.

## Unreleased

Teral is in active `0.1.0` development. No public release has been published.

Current development work is focused on making core filesystem operations, Trash,
cross-application clipboard behavior, drag and drop, Linux device integration, and
failure handling trustworthy before the first public test release.

### Existing foundation

- Native Rust and GTK4 application with grid and list browsing.
- Back, Forward, Parent, breadcrumbs, tabs, filtering, sorting, and hidden-file controls.
- XDG locations, currently mounted local filesystems, bookmarks, tags, and a details panel.
- GIO MIME icons, image thumbnails, Open, Open With, and a VTE-based Quick Command console.
- Built-in, system, and Omarchy-aware appearance layers.

These items describe the current development foundation, not a stable-release guarantee.

### Stage 1–2 implementation candidate

- Added one structured transfer job engine for Copy, Move, Link, Paste, drag-and-drop,
  and Duplicate, with per-item requested/actual destinations and completion states.
- Added atomic no-overwrite destination creation, raw-filename conflict names, explicit
  Replace/Rename/Skip/Cancel handling, and replacement backup/restore behavior.
- Added bounded byte/item progress, cancellation during file copies, tracked partial
  cleanup, and explicit partial state when a cross-filesystem Move cannot remove its source.
- Added recursive file/directory/symlink handling, metadata copying where GIO supports it,
  sparse zero-block preservation, and filesystem-aware recursive-copy rejection.
- Added GNOME, URI-list, KDE Cut, and GTK file-list clipboard interoperability paths.
- Added negotiated Copy/Move/Link drop handling for folder tiles, folder backgrounds, and
  sidebar locations without guessing a destructive Move.

This candidate remains unreleased and is not promoted to verified behavior until the
automated and desktop interoperability gates in `DEVELOPMENT.md` pass.

### Stage 2 clipboard repair

- Removed the trailing newline from the `x-special/gnome-copied-files` payload Teral
  publishes. GNOME's own producer writes no trailing newline, and the empty final field
  it creates is turned into an invalid entry by some other file managers when pasting
  from Teral. Teral's reader continues to accept the trailing newline from applications
  that write one.

### Stage 3 implementation candidate

- Replaced the assumption that the trash is only `$XDG_DATA_HOME/Trash` with FreeDesktop
  trash discovery across the home filesystem and mounted secondary filesystems, including
  the sticky shared `$topdir/.Trash/$uid` form and the unshared `$topdir/.Trash-$uid`
  form. A symlinked or non-sticky `.Trash` is never used.
- Added a GTK-free trash module that reads `.trashinfo` records as bytes, so original
  paths keep filenames that are not valid UTF-8, contain spaces, or contain newlines.
  Lossy conversion is now display-only.
- Trash locations on secondary filesystems appear in the sidebar, labelled by the disk
  they belong to. A device that is unplugged stops appearing; its records stay on it.
- Restore now reads records off the GTK thread, restores under the recorded original
  name rather than the de-duplicated name inside the trash, resolves occupied
  destinations through the Stage 1–2 conflict system, and asks before recreating an
  original folder that no longer exists.
- A restore record is discarded only after its item has actually been placed. A restore
  that lands but cannot clear its record is reported as partial, not as success.
- Empty Trash and permanent deletion return per-item results, remove each item's record
  only after its data is gone, keep the record for every item whose deletion failed, and
  can be cancelled between items.
- Recursive deletion never follows a directory symlink; a symlink is unlinked and what it
  points at is untouched.
- Trash, Restore, Empty Trash and permanent deletion share the transfer coordinator's
  single-job lock, progress reporting, `Esc` cancellation and structured reporting.
- Tags now follow a job's authoritative per-item results: a completed item that landed
  somewhere carries its tags to the destination the job reported, a completed item that
  landed nowhere loses them, and failed, partial, skipped and cancelled items are left
  alone.
- The Empty Trash confirmation names the real number of items and trash locations, and
  the permanent-delete confirmation says when deleting also gives up the ability to
  restore.

This candidate remains unreleased. Stage 3's acceptance gate stays open until the
repository checks and the trash matrix in `DEVELOPMENT.md` pass on Ubuntu and Omarchy.
