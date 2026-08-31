<h1 align="center">Teral</h1>

<p align="center">
  A native Linux file manager written in Rust with GTK4.
</p>

<p align="center">
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue"></a>
  <img alt="Development status" src="https://img.shields.io/badge/status-pre--release%200.1-orange">
  <img alt="Platform" src="https://img.shields.io/badge/platform-Linux-informational">
</p>

Teral is an information-rich three-pane file manager with native Linux integration, a
details panel, and a command console rooted in the folder being browsed. It is one GTK4
application for Ubuntu, Omarchy, Arch, and other Linux desktops.

## Development status

Teral is currently an unreleased `0.1.0` application. It is suitable for development and
testing with non-critical files; it is not yet recommended as a primary file manager for
important data.

No public binaries or GitHub release have been published. The first public `0.1.0` release
will happen only after the core filesystem-safety and Linux-integration test gates pass.

## Verified development foundation

The current source implements:

- asynchronous directory enumeration through GIO;
- Back, Forward, Parent, clickable breadcrumbs, and `Ctrl+L` location editing;
- grid and list views sharing multi-selection;
- current-folder filename filtering, sorting, and hidden-file visibility;
- tabs with separate location and navigation history;
- existing XDG user locations and currently mounted local filesystems;
- bookmarks, tags, and a selected-item details panel;
- system MIME icons, image thumbnails, Open, and Open With;
- create folder, rename, copy, cut, paste, move, duplicate, Trash, restore, permanent
  deletion, archive extraction, and archive creation code paths;
- a VTE terminal for interactive Quick Commands;
- built-in, system-derived, and Omarchy-aware appearance layers;
- a Settings window backed by `~/.config/teral/teral.toml`.

This list means the behavior exists in the source. It does not claim that every failure,
race, cancellation, device, or cross-filesystem case is finished.

## Stage 1–2 candidate awaiting its gate

The current source now contains one transfer coordinator for Paste and drag-and-drop,
structured per-item results, atomic no-overwrite destination reservation, safe conflict
choices, byte-level cancellation, partial-output cleanup, metadata-aware recursive copy,
same- and cross-filesystem move paths, desktop file clipboard formats, and negotiated
Copy/Move/Link drops. Duplicate uses the same transfer engine.

This is an implementation candidate, not a verified guarantee. It moves into the
verified list only after the automated checks and the Ubuntu/Omarchy interoperability
matrix in `DEVELOPMENT.md` pass.

## Stage 3 candidate awaiting its gate

Trash now follows the FreeDesktop trash model rather than assuming a single home trash.
Teral discovers the home trash and the trash directories of mounted secondary
filesystems, shows each of them in the sidebar, and reads `.trashinfo` records as raw
bytes so a filename that is not valid UTF-8 is restored exactly as it was.

Restore puts an item back under the name its record holds, resolves an occupied original
location through the same conflict handling as Paste, and asks before recreating an
original folder that has since been removed. A restore record is discarded only once its
item has actually been placed.

Empty Trash and permanent deletion report a result for every item, keep the restore
record of anything they failed to remove, can be cancelled between items, and never
follow a directory symlink while deleting.

This is also an implementation candidate. It moves into the verified list only after the
automated checks and the trash matrix in `DEVELOPMENT.md` pass.

## Core work still in progress

Before the first public release, Teral is hardening:

- one aggregated Trash view instead of one sidebar entry per trash location;
- deletion time and original location shown in the details panel for trashed items;
- exact non-UTF-8 filename handling throughout the remaining operations;
- directory symlinks and special filesystem entries;
- desktop `%U`/`xdg-open` directory handling;
- mounted-volume actions, removable devices, MTP/GVfs, and disconnected-media behavior;
- large-directory responsiveness and off-main-thread thumbnail decoding;
- consistent persistence for tags, bookmarks, and view preferences.

Richer document/media previews, advanced search, and other expansion work come after the
core file manager is trustworthy.

## Build from source

Teral currently targets GTK 4.12 or newer and VTE's GTK4 build.

Ubuntu and Debian development dependencies:

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libgtk-4-dev libvte-2.91-gtk4-dev
```

Arch and Omarchy development dependencies:

```bash
sudo pacman -S --needed base-devel gtk4 vte4 pkgconf
```

Install Rust through [rustup](https://rustup.rs), then run:

```bash
cargo run --locked
```

Run the complete baseline checks with:

```bash
./scripts/check.sh
```

See [DEVELOPMENT.md](DEVELOPMENT.md) for build, check, packaging, and local-install details.

## Keyboard

```text
Enter               Open
Backspace           Parent folder
Alt+Left/Right      Back / Forward
Ctrl+L              Edit the location
Ctrl+F              Filter this folder
Ctrl+H              Show hidden files
Ctrl+A              Select all
Ctrl+C / Ctrl+X     Copy / Cut
Ctrl+V              Paste
Ctrl+Shift+N        New folder
Ctrl+Shift+T        Open a terminal here
Ctrl+K              Focus Quick Command
Ctrl+`              Show or hide the command console
Ctrl+D              Duplicate
Ctrl+T / Ctrl+W     New tab / close tab
Ctrl+Tab            Next tab (add Shift for previous)
Ctrl+,              Settings
Ctrl+I              Show or hide the details panel
Ctrl+= / Ctrl+-     Larger / smaller grid icons
Ctrl+0              Reset the grid icon size
F2                  Rename
Delete              Move to Trash
Shift+Delete        Delete permanently
F5 / Ctrl+R         Refresh
F1                  Keyboard shortcuts
Escape              Close search, request transfer cancellation, or hide the console
```

## Configuration

User configuration lives in `~/.config/teral/teral.toml`. Bookmarks live in
`~/.local/share/teral/places.toml`, and tags live in `~/.local/share/teral/tags.toml`.

```toml
version = 1

[appearance]
mode = "teral"        # "teral" or "system"
accent = "#e0a63c"    # optional palette override

[layout]
grid_icon_size = 64
row_height = 30

[files]
show_hidden = false
folders_first = true
sort = "name"         # name, size, type, modified
descending = false
view = "grid"         # grid or list

[commands]
shell = ""            # empty: $SHELL, then /bin/sh
terminal = ""         # empty: detected from PATH
```

Invalid colors and out-of-range layout values fall back or are clamped when the
configuration is loaded.

## Themes and Omarchy

Teral includes dark and light semantic palettes. In system mode it derives colors from
GTK where available. Under Omarchy it looks for the active theme and reads `teral.toml`,
falling back to colors derived from `colors.toml`.

Omarchy theme and Teral configuration files are monitored while the application runs.
Complete live desktop-theme monitoring outside Omarchy remains part of the pre-release
work.

## Contributing

Read [DEVELOPMENT.md](DEVELOPMENT.md) before changing the project. Filesystem changes
must include failure-oriented tests, not only happy-path tests.

## License

MIT. Copyright © 2026 Zuhaib Ullah Baig.
