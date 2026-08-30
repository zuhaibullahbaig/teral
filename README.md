# Teral

Teral is a modern native Linux file manager written in Rust with GTK4.

The goal is a fast, information-rich file manager that feels at home on a normal Linux desktop while also integrating cleanly with theme-driven environments such as Omarchy.

Teral is one application and one codebase. Desktop-specific behavior is handled through integration layers and configuration rather than separate editions.

## Current state

Teral is usable as a day-to-day file manager. It provides:

- a dark, dense three-pane shell: sidebar, file view, details/actions panel
- restrained `TERAL` branding, clickable breadcrumbs and a `Ctrl+L` path entry
- back / forward / parent navigation with real history
- a sidebar with XDG user locations, the trash folder, GIO-discovered mounts with capacity meters, and pinned folders that persist
- a polished grid view and a dense list view (Name, Size, Type, Modified) sharing one selection
- real image thumbnails, and system/MIME icons through GIO for everything else
- a details panel with type, size, path, modified/created/accessed times, owner, permissions and symlink target
- actions: Open, Open With, Copy Path, Open Terminal Here, Rename, Move, Copy and Trash
- copy / cut / paste with recursive folder transfers that run off the GTK main thread and never overwrite an existing file
- filename search, sorting, folders-first and hidden-file toggles
- Quick Command: run a shell command with the browsed folder as its working directory, asynchronously, with its output in a collapsible console
- a status bar with selection count and size, free space, and a grid zoom control
- tabs, each with its own location and history
- drag and drop: between Teral folders, and to and from other Linux applications
- live directory monitoring, so changes made elsewhere appear on their own
- trash browsing, restore to the original location, and Empty Trash
- a Settings window with three theme modes, an accent colour, density and command settings
- a layered TOML theme system that live-reloads when its files change

Richer previews (PDF, video, text), a command palette and network locations are not implemented yet.

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
Ctrl+D              Duplicate
Ctrl+T / Ctrl+W     New tab / close tab
Ctrl+Tab            Next tab (add Shift for previous)
Ctrl+,              Settings
Ctrl+0              Reset the grid zoom
F2                  Rename
Delete              Move to trash
F5 / Ctrl+R         Refresh
Escape              Cancel a transfer, or hide the command console
```

## Ubuntu development setup

Install the native GTK4 development dependencies:

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libgtk-4-dev
```

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

Pinned sidebar folders are stored separately, in `~/.local/share/teral/places.toml`.

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

`mode = "system"` asks the FreeDesktop appearance portal for the desktop's light/dark
preference and accent colour, falling back to GTK's own settings on desktops without a
portal, and picks Teral's matching palette.

`mode = "omarchy"` looks for the active theme under the XDG state location Omarchy uses.
If the active theme contains `teral.toml`, Teral applies it. Otherwise Teral derives its
palette from that theme's `colors.toml`, so switching Omarchy themes restyles Teral even
when the theme author has never heard of Teral.

Teral watches both its own configuration file and the active Omarchy theme, so changing
either restyles a running Teral without a restart.

Invalid colors and out-of-range layout values are discarded or clamped, so a broken theme can never make Teral unusable.

## License

MIT License. Copyright (c) 2026 Zuhaib Ullah Baig.
