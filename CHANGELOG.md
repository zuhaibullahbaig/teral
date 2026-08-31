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

### Added

- One transfer engine behind Copy, Move, Link, Paste, drag-and-drop and Duplicate, with
  per-item requested and actual destinations and explicit completion states.
- Atomic no-overwrite destination creation, raw-filename conflict names, and explicit
  Replace, Rename Incoming, Skip and Cancel handling with backup-and-restore behaviour
  for replacements.
- Bounded byte and item progress, cancellation during file copies, tracked partial
  cleanup, and an explicit partial state when a cross-filesystem move cannot remove its
  source.
- Recursive file, directory and symlink handling, metadata copying where GIO supports it,
  sparse zero-block preservation, and filesystem-aware recursive-copy rejection.
- GNOME, URI-list, KDE Cut and GTK file-list clipboard interoperability, and negotiated
  Copy/Move/Link drop handling for folder tiles, folder backgrounds and sidebar locations.
- FreeDesktop trash discovery across the home filesystem and mounted secondary
  filesystems, covering both the sticky shared `$topdir/.Trash/$uid` form and the
  unshared `$topdir/.Trash-$uid` form. A symlinked or non-sticky `.Trash` is never used.
- A GTK-free trash module that reads `.trashinfo` records as bytes, so original paths
  keep filenames that are not valid UTF-8 or that contain spaces or newlines.
- Trash locations on secondary filesystems in the sidebar, labelled by the disk they
  belong to. A device that is unplugged stops appearing; its records stay on it.
- A prompt before recreating an original folder that no longer exists during a restore.

- Command-line and desktop file arguments. `teral ~/Documents` opens that folder, and
  `teral report.pdf` opens the folder containing it with the file selected. The desktop
  entry's `%U` and `inode/directory` handler now work, so Teral can be set as the
  system's file manager.
- Filename validation on New Folder and Rename, refusing only what the filesystem
  refuses — a slash, a null byte, `.`, `..`, an empty name, or one over 255 bytes.
- Symlink resolution in directory listings, so a link to a folder can be opened, a
  broken link is identified as broken, and the details panel says what a link points at
  and whether that target is still there.
- A location model that distinguishes an ordinary folder, the trash, and a tag view, so
  every action is scoped to what the view can actually support.

### Changed

- Restore reads trash records off the GTK thread, restores under the recorded original
  name rather than the de-duplicated name inside the trash, and resolves an occupied
  original location through the same conflict handling as Paste.
- Trash, Restore, Empty Trash and permanent deletion share the transfer engine's
  single-job lock, progress reporting, `Esc` cancellation and structured reporting, and
  return a result for every item.
- Tags follow a job's authoritative per-item results: a completed item that landed
  somewhere carries its tags to the destination the job reported, a completed item that
  landed nowhere loses them, and failed, partial, skipped and cancelled items are left
  alone.
- The Empty Trash confirmation names the real number of items and trash locations, and
  the permanent-delete confirmation says when deleting also gives up the ability to
  restore.
- A second `teral` launch reaches the instance already running: a folder opens as a new
  tab in the existing window and the window is raised, instead of starting a second
  window.
- New Folder, Paste, Open Terminal Here, drops and Quick Command are offered only where
  they have a real folder to act in, rather than silently acting on whichever directory
  was visited last.
- Each tab keeps its own tag view alongside its location and history, so switching tabs
  returns to exactly the view that tab was showing.
- Metadata is copied onto a new file explicitly — mode, timestamps and extended
  attributes — and a destination that cannot accept one of them no longer fails the
  whole transfer.
- Names entered in New Folder and Rename are no longer trimmed. Leading and trailing
  spaces are legal on Linux, and removing them created files under a name that was never
  typed.

### Fixed

- Restoring a file whose name is not valid UTF-8 no longer corrupts the name. Trash
  records were previously decoded through a lossy string conversion.
- Emptying the trash no longer strands an item permanently when its data cannot be
  removed. Records and data were previously deleted in separate passes, so a failed
  deletion still lost the record needed to restore it.
- A restore record is now removed only after its item has actually been placed. A restore
  that lands but cannot clear its record is reported as partial rather than as success.
- Recursive deletion no longer risks following a directory symlink. A symlink is
  unlinked and whatever it points at is untouched.
- Copying a file no longer fails after the data has been written. `g_file_copy_attributes`
  requests attributes such as `standard::size` that a local filesystem refuses, which
  aborted the copy and removed the result.
- A symlink to a directory can be opened. Listings are read without following symlinks,
  so every link reported the content type `inode/symlink` and none was treated as a
  folder.
- A broken symlink is no longer followed, thumbnailed, or handed to another application.
- FIFOs, sockets and device nodes are described rather than opened; opening a FIFO can
  block the process that opens it indefinitely.
- A filesystem event arriving from the folder being left no longer cancels a navigation
  already in flight.
- Renaming a file whose name is not valid UTF-8 no longer writes replacement characters
  over the real name when the field is confirmed unchanged.
- Secondary trash locations are labelled by the disk they belong to in both forms the
  specification allows; the shared `.Trash/<uid>` form was previously labelled after the
  wrong path component.
- The `x-special/gnome-copied-files` clipboard payload no longer carries a trailing
  newline. The empty final field it created was turned into an invalid entry by some
  other file managers when pasting from Teral. Teral's own reader still accepts a
  trailing newline from applications that write one.
