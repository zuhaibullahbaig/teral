# Changelog

Teral follows semantic versioning. See `RELEASING.md` for how a release is cut.

## 1.0.0

The first release.

Teral is a native Linux file manager written in Rust with GTK4: one application and one
codebase, at home on any desktop.

- A dark, dense three-pane window: sidebar, file view, details and actions panel, with
  tabs, breadcrumbs, a `Ctrl+L` location entry and real back/forward history.
- Grid and list views over one selection, with live icon sizing, image thumbnails and
  system MIME icons through GIO.
- The file work: copy, cut, paste, duplicate, rename, new folder, drag and drop between
  Teral folders and other applications, and recursive transfers that run off the GTK
  main thread and never overwrite an existing file.
- Trash you can browse, restore from, empty, or delete from permanently.
- Archives: extract here or into a folder, and compress a selection into a zip.
- User tags with a name and colour that follow their files when Teral moves or renames
  them, and a sidebar entry per tag.
- Quick Command: run a command in the browsed folder in a real terminal, interactive
  programs included, in a console you can drag to resize.
- Bookmarks you can drag folders onto, mounted devices with capacity meters, and XDG
  user locations.
- Theming: Teral's own palette, or the desktop's — under Omarchy that means the active
  theme's `teral.toml`, or colours derived from its `colors.toml`, live-reloaded when the
  theme changes; elsewhere the GTK theme's own colours and the desktop's accent.
- A Settings window that writes the same `~/.config/teral/teral.toml` you can hand-edit,
  plus Shortcuts and About windows.
