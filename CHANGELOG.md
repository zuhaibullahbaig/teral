# Changelog

Teral follows semantic versioning. See `RELEASING.md` for the release process.

## Unreleased

No changes yet.

## 0.1.3 - 2026-09-02

### Changed

- Narrow windows keep Files and Quick Command as the permanent workspace. Navigation
  slides over them from the left, while Details and its footer controls slide in from
  the right on hover or through the existing keyboard shortcuts. Wide layouts retain
  the complete three-pane presentation.

### Fixed

- Responsive sizing now follows compositor surface changes instead of running a
  permanent frame callback, preventing needless rendering work while Teral is idle.
- File drags advertise typed file lists and standard URI formats, and the Bookmarks drop
  region recognizes folder drags consistently under Wayland and Omarchy.
- Compact Navigation and Details drawers paint their footer controls opaquely, so the
  permanent Quick Command layer does not show through an open drawer.
- The Details hover handle sits just inside the upper file-canvas edge, remains visible
  as a close arrow over the open drawer, and the title-bar close control rejects
  rectangular focus and active styling from desktop themes.

## 0.1.2 - 2026-09-02

### Changed

- Narrow tiled windows switch to an adaptive one-pane workspace with Navigation, Files
  and Details controls. Paths collapse to the current folder, secondary toolbar and
  footer controls yield space, and search, Quick Command and console controls shrink
  without hiding their close buttons. Wide windows retain the full three-pane layout.

### Fixed

- The title-bar close button now keeps a compact circular hover plate instead of
  inheriting an oversized or square hover background from the active desktop theme.
- Footer columns once again track the Navigation, Files and Details pane widths exactly;
  the full zoom slider, current pixel value and selection summary remain available in
  the Details footer without widening Quick Command beyond the file pane.

## 0.1.1 - 2026-09-02

### Added

- `teral-update` installs the newest published release through the package manager when
  possible, verifies its checksum, and always removes its temporary download.
- `teral --version` and `teral -V` report the installed build version.

### Changed

- New configurations follow the system appearance by default. Existing saved appearance
  choices are preserved.
- Details-panel action rows wrap as the window narrows, and the footer zoom control can
  contract without overflowing its pane.

### Fixed

- Closing the last window now cancels active work, stops an attached Quick Command,
  releases the application instance, and exits the process on the first close.
- The Terminal action keeps its place for selected files and is disabled instead of
  disappearing and leaving a hole in the action row.
- System-theme resolution now falls back safely when GTK is unavailable, and partial
  file metadata no longer produces GLib critical warnings.

## 0.1.0

Teral's first public release.

### Highlights

- Native Rust and GTK4 application with grid and list browsing.
- Back, Forward, Parent, breadcrumbs, tabs, filtering, sorting, and hidden-file controls.
- XDG locations, currently mounted local filesystems, bookmarks, tags, and a details panel.
- GIO MIME icons, image thumbnails, Open, Open With, and a VTE-based Quick Command console.
- Built-in, system, and Omarchy-aware appearance layers.

### Added

- Crash-safe atomic persistence shared by settings, tags, bookmarks, command history and
  window sessions, with lossless raw Linux path encoding and last-valid-file preservation.
- Reorderable bookmarks with optional display labels that never rename their folders.
- Session restoration for tabs, tag views and per-tab navigation history, with a safe
  fallback when the active location is unavailable.
- Atomic Create File, a local owner/group/other permissions editor, and bounded inert
  UTF-8 text/source previews.
- Bounded Quick Command history with Up/Down navigation, validated shell arguments,
  graceful interrupt, explicit force stop and signal-aware outcome reporting.
- Recursive Home search with a rounded header takeover, scroll-driven bounded result
  pages, cancellation, previous-directory restoration, stale-generation protection and
  per-tab session restoration.
- Bounded first-page PDF previews, audio/video metadata, and first-frame video previews
  through optional desktop utilities invoked directly without a shell.
- Asynchronous mount, unmount and eject controls, with live device removal handling.

- One transfer engine behind Copy, Move, Link, Paste, drag-and-drop and Duplicate, with
  per-item requested and actual destinations and explicit completion states.
- Atomic no-overwrite destination creation, raw-filename conflict names, and explicit
  per-item Merge, Replace, Rename, Skip and Cancel handling, an apply-to-all option,
  self-move rejection, and backup-and-restore behaviour for replacements.
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

- The source installer now rebuilds on every install or update, while release tarballs
  continue to install their included checked binary.
- Packages now install AppStream metadata alongside the desktop entry and application
  icon, and advertised Arch support is limited to the tested release architecture.
- Directory enumeration publishes bounded batches and discards stale generations;
  decorative child counts are no longer fetched for every folder tile.
- Image decoding runs in a bounded worker queue with generation cancellation and bounded
  decoded dimensions and cache size.
- Toolbar, shortcut and Settings changes now converge on the same persisted sorting,
  hidden-file, view, icon-size and details-panel preferences.
- GTK appearance and icon-theme changes clear relevant caches and re-resolve the active
  theme without accumulating handlers across closed windows.
- Mountable volumes and mounted devices now appear and disappear live, and GVfs devices
  use readable volume names instead of backend identifiers such as `mtp`.
- Theme, icon-size, search, desktop-association and configuration monitor updates are
  coalesced and no longer feed back into directory rescans; settings, bookmark, tag and
  command-history writes no longer block the interface.
- Folder-only actions are hidden in Trash and tag views, and long names can no longer
  change the width of the Details panel.

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

- Force Stop now terminates the Quick Command process group instead of only its shell,
  preventing foreground pipelines from being orphaned when the console or window closes.
- Restoring a file whose name is not valid UTF-8 no longer corrupts the name. Trash
  records were previously decoded through a lossy string conversion.
- Emptying the trash no longer strands an item permanently when its data cannot be
  removed. Records and data were previously deleted in separate passes, so a failed
  deletion still lost the record needed to restore it.
- A restore record is now removed only after its item has actually been placed. A restore
  that lands but cannot clear its record is reported as partial rather than as success.
- Recursive deletion no longer risks following a directory symlink. A symlink is
  unlinked and whatever it points at is untouched.
- Teral no longer freezes on start-up, or when opening any folder containing a symlink,
  if the link's target is on a filesystem that is slow or has stopped answering.
  Resolving a link means a `stat` on its target, and that was happening on the thread
  that draws the window, once per link in the folder. Links are now followed in one
  batch on a worker thread.
- Teral no longer freezes while browsing when a mounted filesystem is slow or gone.
  Deciding whether a location is in the trash probed every mount on the UI thread, and
  ran on every selection change; a `stat` on a disconnected network share or an unplugged
  drive blocked the whole window. Trash directories are now found on a worker thread and
  cached, refreshed when a disk is mounted or unmounted. A frozen window also stopped
  answering the session bus, which is why a later `teral` could fail to start with
  "Failed to register: Timeout was reached".
- Emptying the trash counts its contents on a worker thread instead of blocking the
  window while it measures a possibly-slow disk.
- A failure to register on the session bus now says what happened and what to do about
  it, instead of GLib's bare one-line message.
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
