<h1 align="center">Teral</h1>

<p align="center">
  A fast, native file manager for Linux — written in Rust with GTK4.
</p>

<p align="center">
  <a href="https://github.com/zuhaibullahbaig/teral/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/zuhaibullahbaig/teral?label=release"></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue"></a>
  <img alt="Platform" src="https://img.shields.io/badge/platform-Linux-informational">
</p>

Teral is a file manager for people who live in their files: a dense three-pane window
that shows you what you are looking at, a command line built into the folder you are
browsing, and colours that follow the desktop you already set up.

It is one application on every Linux desktop — no separate editions, no web view, no
Electron. Just GTK4.

---

## Install

### Ubuntu and Debian

Download the `.deb` from the [latest release](https://github.com/zuhaibullahbaig/teral/releases/latest), then:

```bash
sudo apt install ./teral_1.0.0_amd64.deb
```

Teral appears in your launcher straight away. To make it the default file manager:

```bash
xdg-mime default dev.zuhaibullahbaig.Teral.desktop inode/directory
```

### Any other distribution

Download `teral-1.0.0-x86_64-linux.tar.gz` from the same page:

```bash
tar -xzf teral-1.0.0-x86_64-linux.tar.gz
cd teral-1.0.0
./scripts/install.sh                  # system-wide, asks for sudo
PREFIX=~/.local ./scripts/install.sh  # just for you, no root
```

`./scripts/install.sh --uninstall` removes it again.

### Arch and Omarchy

```bash
git clone https://github.com/zuhaibullahbaig/teral
cd teral/packaging && makepkg -si
```

### What it needs

GTK 4.12 or newer and VTE's GTK4 build — `libgtk-4-1` and `libvte-2.91-gtk4-0` on
Ubuntu, `gtk4` and `vte4` on Arch. The packages declare these, so a normal install pulls
them in.

---

## What Teral does

**Browsing.** Sidebar with your XDG locations, mounted drives with capacity meters,
bookmarks you drag folders onto, and your own tags. Grid and list views over one
selection, live icon sizing, image thumbnails, tabs, breadcrumbs, real back/forward
history, and a details panel with type, size, owner, permissions, timestamps and symlink
target.

**File work.** Copy, cut, paste, duplicate, rename, new folder, and drag and drop —
between Teral's own folders and to and from other applications. Recursive transfers run
off the UI thread, so a big copy never freezes the window, and they never overwrite
anything: a name collision becomes a new name, not a lost file.

**Trash that works.** Browse it, restore an item to where it came from, empty it, or
delete permanently.

**Archives.** Extract here or into a folder — zip, tar, 7z, rar — and compress a
selection into a zip.

**Tags.** Give one a name and a colour, attach it to anything, and click it in the
sidebar to see everything carrying it. Tags follow their files when Teral renames or
moves them.

**Quick Command.** Type a command and it runs in the folder you are browsing, in a real
terminal — `vim`, `git rebase -i`, anything interactive. Drag the console's title bar to
resize it. Commands run only when you type them; Teral never escalates privileges on its
own.

**Theming.** Teral's own dark palette, or the desktop's. Following the system means the
GTK theme's real colours and the desktop's accent — and, on Omarchy, the active theme
itself: its `teral.toml` if it ships one, otherwise colours derived from its
`colors.toml`. Switching your desktop theme restyles a running Teral, no restart.

---

## Keyboard

```text
Enter               Open
Backspace           Parent folder
Alt+Left/Right      Back / Forward
Ctrl+L              Edit the location
Ctrl+F              Search this folder
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
Ctrl+= / Ctrl+-     Larger / smaller icons
Ctrl+0              Reset the icon size
F2                  Rename
Delete              Move to trash
Shift+Delete        Delete permanently
F5 / Ctrl+R         Refresh
F1                  Keyboard shortcuts
Escape              Close search, cancel a transfer, or hide the console
```

---

## Configuration

Everything lives in one file, `~/.config/teral/teral.toml`. Teral's Settings window
(`Ctrl+,`) writes exactly that file, so the GUI and hand-editing never disagree, and
Teral restyles itself as soon as the file changes.

```toml
version = 1

[appearance]
mode = "teral"        # "teral" or "system"
accent = "#e0a63c"    # optional, overrides the palette's accent

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

A `[colors]` table can override any semantic colour individually. Invalid colours and
out-of-range layout values are discarded or clamped, so a broken file can never make
Teral unusable.

Bookmarks live in `~/.local/share/teral/places.toml`, tags in
`~/.local/share/teral/tags.toml`.

### Themes

Teral ships `themes/default/teral.toml`, `teral-light.toml` and `teral.css`. The TOML
files carry the semantic palette; the CSS is written entirely against those values and
stable `.teral-*` classes, so a theme only supplies colours — never GTK widget
selectors.

Themes resolve in layers, each overriding only what it sets:

```text
built-in Teral palette (dark, or light when the desktop asks for light)
        ↓
the environment, when the mode is "system"
        ↓
~/.config/teral/teral.toml
```

On Omarchy, Teral looks for the active theme link under `~/.config/omarchy/current/theme`,
then the XDG state and data locations. `TERAL_OMARCHY_THEME` overrides that search.

---

## Not there yet

Teral is honest about what it does not do, rather than shipping buttons that do nothing:

- richer previews (PDF, video, text) — files show their MIME icon instead
- a paste conflict dialog: a collision always becomes a new name, never a prompt
- trash on secondary drives — Teral's trash view only covers your home filesystem
- network locations, and a command palette

---

## Building and contributing

See [DEVELOPMENT.md](DEVELOPMENT.md) to build from source, and
[RELEASING.md](RELEASING.md) for how a release is cut.

## License

MIT. Copyright © 2026 Zuhaib Ullah Baig.
