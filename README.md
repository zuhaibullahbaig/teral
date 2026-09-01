<h1 align="center">Teral</h1>

<p align="center">
  A native Linux file manager, written in Rust with GTK4.
</p>

<p align="center">
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue"></a>
  <img alt="Platform" src="https://img.shields.io/badge/platform-Linux-informational">
</p>

Teral is a three-pane file manager: locations on the left, files in the middle, details
and actions for the selection on the right, and a terminal rooted in the folder you are
browsing. It is one application for Ubuntu, Arch, Omarchy and other Linux desktops.

> **Teral is early software under active development.** There has been no release, and it
> is not ready to be your only file manager. Use it with files you can afford to lose.

## Features

- Grid and list views with multi-selection, sorting, filtering and hidden-file toggling.
- Tabs, breadcrumbs, and back / forward history.
- Copy, cut, paste, move, duplicate, rename and link, with conflict prompts and
  cancellable progress.
- Drag and drop inside Teral and to and from other applications.
- A shared clipboard with Nautilus, Dolphin and Thunar.
- Trash that follows the FreeDesktop specification, including external drives, and
  restores files to where they came from.
- Lossless tags and reorderable bookmarks with optional sidebar labels.
- A built-in terminal that follows the folder you are in, with bounded command history
  and explicit interrupt and force-stop behavior.
- System icon themes, image thumbnails, Open With, and archive extraction.
- Appearance that follows your desktop, with first-class support for Omarchy themes.
- Atomic settings and metadata writes, and restoration of tabs and navigation history.
- Atomic Create File, a local permissions editor, and inert bounded text/source previews.

## Install

There are no packaged releases yet, so build from source. You will need
[Rust](https://rustup.rs) and GTK 4.12 or newer:

```bash
# Ubuntu / Debian
sudo apt install -y build-essential pkg-config libgtk-4-dev libvte-2.91-gtk4-dev

# Arch / Omarchy
sudo pacman -S --needed base-devel gtk4 vte4 pkgconf
```

Then:

```bash
git clone https://github.com/zuhaibullahbaig/teral
cd teral
cargo run --release
```

To install the binary, its desktop entry and its icon system-wide:

```bash
sudo ./scripts/install.sh
```

## Shortcuts

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
Escape              Close search, cancel an operation, or hide the console
```

## Configuration

Settings live in `~/.config/teral/teral.toml`, and can also be changed from inside Teral
with `Ctrl+,`. Bookmarks are in `~/.local/share/teral/places.toml` and tags in
`~/.local/share/teral/tags.toml`.

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
details_visible = true

[commands]
shell = ""            # empty: $SHELL, then /bin/sh
terminal = ""         # empty: detected from PATH
```

## Themes

Teral ships dark and light palettes. In system mode it takes its colors from GTK. Under
Omarchy it reads the active theme's `teral.toml`, and derives a palette from `colors.toml`
when there is no Teral-specific file. A theme with missing keys falls back rather than
breaking the window.

## Current boundaries

- Sidebar devices include mountable volumes and mounted locations with local paths.
  GIO locations that provide no local path are detected but cannot yet be browsed.
- Search filters the folder being viewed; it is not a recursive or indexed search.
- Preview supports bounded images and UTF-8 text/source files. PDF rendering and
  audio/video probing are intentionally absent because no safe renderer or media probe
  is currently part of the dependency set.
- Hardware, Wayland interoperability, accessibility, high-contrast, large-directory and
  Ubuntu/Omarchy visual checks remain manual development checks.

## Contributing

Read [DEVELOPMENT.md](DEVELOPMENT.md) first. Filesystem changes need failure tests, not
only happy-path tests.

## License

MIT. Copyright © 2026 Zuhaib Ullah Baig.
