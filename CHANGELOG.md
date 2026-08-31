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
