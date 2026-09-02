<h1 align="center">Teral</h1>

<p align="center">
  A native Linux file manager, written in Rust with GTK4.
</p>

<p align="center">
  <a href="https://github.com/zuhaibullahbaig/teral/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/zuhaibullahbaig/teral"></a>
  <a href="https://github.com/zuhaibullahbaig/teral/actions/workflows/ci.yml"><img alt="Build status" src="https://github.com/zuhaibullahbaig/teral/actions/workflows/ci.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue"></a>
  <img alt="Platform" src="https://img.shields.io/badge/platform-Linux-informational">
</p>

Teral is a three-pane file manager: locations on the left, files in the middle, details
and actions for the selection on the right, and a terminal rooted in the folder you are
browsing. It is one application for Ubuntu, Arch, Omarchy and other Linux desktops.

> **Teral 0.1.3 is an early public release under active development.** Keep another file
> manager installed, back up important files, and report anything that behaves incorrectly.

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
- Recursive, cancellable Home search with streamed results, bounded first-page PDF
  previews, and bounded audio/video metadata and video-frame previews.
- Live mount, unmount and eject controls for removable devices.
- Overlay Navigation and Details drawers for narrow tiling layouts, while Files and
  Quick Command remain available underneath.
- A checksum-verified `teral-update` command for installing future releases.

## Install

Prebuilt packages for x86_64 Linux are available from the
[latest GitHub release](https://github.com/zuhaibullahbaig/teral/releases/latest).

### Ubuntu and Debian

Download `teral_0.1.3_amd64.deb`, then install it with the system package manager so its
GTK and VTE dependencies are resolved automatically:

```bash
sudo apt install ./teral_0.1.3_amd64.deb
```

### Other x86_64 Linux distributions

Download `teral-0.1.3-x86_64-linux.tar.gz`, extract it, and run the included installer:

```bash
tar -xzf teral-0.1.3-x86_64-linux.tar.gz
cd teral-0.1.3
sudo ./scripts/install.sh
```

The tarball expects GTK 4.12 or newer and VTE for GTK4 to already be installed. For a
user-only installation that needs no root privileges, use:

```bash
PREFIX="$HOME/.local" ./scripts/install.sh
```

### Build from source

Install [Rust](https://rustup.rs) and the development packages for GTK and VTE:

```bash
# Ubuntu 24.04+ / Debian with GTK 4.12+
sudo apt install -y build-essential pkg-config libgtk-4-dev libvte-2.91-gtk4-dev

# Arch / Omarchy
sudo pacman -S --needed base-devel gtk4 vte4 pkgconf
```

PDF and media previews use the standard `pdftoppm`, `ffprobe`, and `ffmpeg` utilities
when installed (`poppler-utils ffmpeg` on Ubuntu; `poppler ffmpeg` on Arch). Teral stays
usable without them and explains which preview provider is unavailable.

Then:

```bash
git clone https://github.com/zuhaibullahbaig/teral
cd teral
cargo run --release
```

Build as your normal user, then install the binary, desktop entry, icon and application
metadata system-wide:

```bash
cargo build --release --locked
sudo TERAL_BINARY="$PWD/target/release/teral" ./scripts/install.sh
```

## Update

Every Teral package from 0.1.1 onward installs the updater. After installing Teral once,
future updates only require closing the application and running:

```bash
teral-update
```

`teral-update` finds the newest release, verifies its checksum, uses `apt` or an available
Arch AUR helper when Teral is managed by one of them, and removes its temporary download
when it exits. Users of 0.1.0 must install 0.1.1 manually once to receive the updater.
For a source checkout that you want to keep building locally, update it directly instead:

```bash
git pull --ff-only
cargo build --release --locked
sudo TERAL_BINARY="$PWD/target/release/teral" ./scripts/install.sh
```

## Shortcuts

```text
Enter               Open
Backspace           Parent folder
Alt+Left/Right      Back / Forward
Ctrl+L              Edit the location
Ctrl+F              Filter this folder
Ctrl+Shift+F        Search Home and subfolders
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
Ctrl+1 / 2 / 3      Navigation drawer / Files / Details drawer
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
mode = "system"       # "system" (default) or "teral"
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

- Sidebar devices appear and disappear live and support mount, unmount and eject.
  Mounted GIO/GVfs locations are browsable when the desktop exposes a local FUSE path;
  GIO locations with no local path remain outside the local-file operation boundary.
- The rounded left-header search recursively searches Home without following directory
  symlinks or crossing filesystem boundaries. Results enter the view in bounded pages as
  you scroll, and closing search restores the previous directory. Ctrl+F remains the
  fast current-folder filter.
- Preview supports bounded images, UTF-8 text/source, first-page PDF rendering, media
  metadata, and a bounded first-frame video preview. External preview providers are
  optional and content is never executed or autoplayed.
- Hardware, Wayland interoperability, accessibility, high-contrast, large-directory and
  Ubuntu/Omarchy visual checks remain manual development checks.

## Contributing

Read [DEVELOPMENT.md](DEVELOPMENT.md) first. Filesystem changes need failure tests, not
only happy-path tests.

Bug reports and tested distribution results belong in
[GitHub Issues](https://github.com/zuhaibullahbaig/teral/issues).

## License

MIT. Copyright © 2026 Zuhaib Ullah Baig.
