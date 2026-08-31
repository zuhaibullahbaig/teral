# Teral

Teral is a modern native Linux file manager written in Rust with GTK4.

The goal is a fast, information-rich file manager that feels at home on a normal Linux desktop while also integrating cleanly with theme-driven environments such as Omarchy.

Teral is one application and one codebase. Desktop-specific behavior is handled through integration layers and configuration rather than separate editions.

## Current state

Teral is usable as a day-to-day file manager. It provides:

- a dark, dense three-pane shell: sidebar, file view, details/actions panel
- restrained `TERAL` branding, clickable breadcrumbs and a `Ctrl+L` path entry
- back / forward / parent navigation with real history
- a sidebar with XDG user locations, the trash folder, GIO-discovered mounts with capacity meters, and bookmarks you can drag folders onto
- a polished grid view and a dense list view (Name, Size, Type, Modified) sharing one selection
- real image thumbnails, and system/MIME icons through GIO for everything else
- a details panel with type, size, path, modified/created/accessed times, owner, permissions and symlink target
- actions: Open, Open With, Copy Path, Open Terminal Here, Rename, Move, Copy and Trash
- copy / cut / paste with recursive folder transfers that run off the GTK main thread and never overwrite an existing file
- type-ahead filename search in the toolbar: start typing in the file list and the field opens with a live match count
- Quick Command: run a shell command with the browsed folder as its working directory, in a real terminal — `vim`, `git rebase -i` and anything else interactive work. Drag the console's title bar to resize it, double-click to expand it, or use the expand button
- a footer split into the same three columns as the window: Teral's own controls under the sidebar, Quick Command at exactly the width of the file list, and the selection and storage readout under the details panel
- tabs, each with its own location and history
- drag and drop: between Teral folders, and to and from other Linux applications
- live directory monitoring, so changes made elsewhere appear on their own
- trash browsing, restore to the original location, Empty Trash and permanent delete
- archive extraction (Extract Here / Extract to Folder) for zip, tar, 7z and rar
- Open in New Tab and Open in New Window, Select All, Select by Type, and an executable toggle offered only for scripts and programs
- user tags: create them with a name and colour, attach them to files, and click one in the sidebar to see everything carrying it
- a Settings window with three theme modes, an accent colour, density and command settings, plus Shortcuts and About windows
- a layered TOML theme system that live-reloads when its files change

Richer previews (PDF, video, text), a command palette and network locations are not implemented yet.

Tags live in `~/.local/share/teral/tags.toml` and follow their files when Teral renames or moves them.

## Keyboard

```text
Enter               Open
Backspace           Parent folder
Alt+Left/Right      Back / Forward
Ctrl+L              Edit the location
Ctrl+F              Search this folder
Ctrl+H              Show hidden files
Ctrl+A              Select all
Ctrl+C / Ctrl+X     Copy / Move
Ctrl+V              Paste
Ctrl+Shift+N        New folder
Ctrl+Shift+T        Open a terminal here
Ctrl+K              Focus Quick Command
Ctrl+`              Show or hide the command console
F1                  Keyboard shortcuts
Ctrl+D              Duplicate
Ctrl+T / Ctrl+W     New tab / close tab
Ctrl+Tab            Next tab (add Shift for previous)
Ctrl+,              Settings
Ctrl+I              Show or hide the details panel
Ctrl+0              Reset the grid zoom
F2                  Rename
Delete              Move to trash
F5 / Ctrl+R         Refresh
Shift+Delete        Delete permanently
Ctrl+= / Ctrl+-     Larger / smaller icons
Escape              Close search, cancel a transfer, or hide the console
```

## Ubuntu development setup

Install the native GTK4 development dependencies:

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libgtk-4-dev libvte-2.91-gtk4-dev
```

`libvte-2.91-gtk4-dev` is what gives Quick Command a real pseudo-terminal, so
interactive programs can be used from inside Teral.

Install Rust with rustup if Rust is not already installed, then restart your shell or source Cargo's environment.

Run Teral from the project root:

```bash
cargo run
```

The project currently enables GTK 4.12 APIs, so the installed GTK version must be 4.12 or newer. Check it with:

```bash
pkg-config --modversion gtk4
```

## Configuration

Everything lives in one file, `~/.config/teral/teral.toml`. Teral's Settings window
(Ctrl+,) writes exactly that file, so the GUI and hand-editing never disagree, and
Teral restyles itself as soon as the file changes.

```toml
version = 1

[appearance]
mode = "teral"        # "teral", "system" or "omarchy"
accent = "#e0a63c"    # optional, overrides the palette's accent

[layout]
grid_icon_size = 64
row_height = 30

[files]
show_hidden = false
folders_first = true
sort = "name"
descending = false
view = "grid"

[commands]
shell = ""            # empty: $SHELL, then /bin/sh
terminal = ""         # empty: detected from PATH
```

A `[colors]` table can override any semantic colour individually.

Two environment variables still work as a fallback when the settings are empty:
`TERAL_SHELL` and `TERAL_TERMINAL`.

Bookmarked folders are stored separately, in `~/.local/share/teral/places.toml`.

## Development checks

```bash
./scripts/check.sh
```

That runs formatting checks, Clippy with warnings treated as errors, and tests.

## Theming

Teral ships with `themes/default/teral.toml`, `themes/default/teral-light.toml` and
`themes/default/teral.css`.

The TOML files carry Teral's semantic palettes and layout numbers. The CSS file is written entirely against those semantic values and against stable `.teral-*` classes, so a theme only has to supply colors — never GTK widget-tree selectors.

Themes are resolved in layers, each one overriding only the keys it sets:

```text
built-in Teral palette (dark, or light when the desktop asks for light)
        ↓
the environment, according to the chosen mode
        ↓
~/.config/teral/teral.toml
```

`mode = "teral"` uses Teral's own dark palette everywhere.

`mode = "system"` adopts the desktop's real appearance. Teral asks the FreeDesktop
appearance portal for the light/dark preference and accent colour, falling back to GTK's
own settings on desktops without a portal, then reads the named colours the active GTK
theme publishes (`theme_bg_color`, `theme_fg_color`, `accent_bg_color` and friends) and
derives its surfaces, borders and muted text from them. Teral therefore takes on the
desktop's own colours, not just its light/dark preference.

`mode = "omarchy"` looks for the active theme under the XDG state location Omarchy uses.
If the active theme contains `teral.toml`, Teral applies it. Otherwise Teral derives its
palette from that theme's `colors.toml`, so switching Omarchy themes restyles Teral even
when the theme author has never heard of Teral.

Teral watches both its own configuration file and the active Omarchy theme, so changing
either restyles a running Teral without a restart.

Invalid colors and out-of-range layout values are discarded or clamped, so a broken theme can never make Teral unusable.

## License

MIT License. Copyright (c) 2026 Zuhaib Ullah Baig.
