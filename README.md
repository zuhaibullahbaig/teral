# Teral

Teral is a modern native Linux file manager written in Rust with GTK4.

The goal is a fast, information-rich file manager that feels at home on a normal Linux desktop while also integrating cleanly with theme-driven environments such as Omarchy.

Teral is one application and one codebase. Desktop-specific behavior is handled through integration layers and configuration rather than separate editions.

## Current state

Teral is a working visual prototype. It browses real directories and provides:

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
- a layered TOML theme system with automatic Omarchy palette discovery

Tabs, drag and drop, trash restore, live directory monitoring and richer previews are deliberately not implemented yet.

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

Two environment variables tune the shell integration without a settings file:

```text
TERAL_SHELL       shell used by Quick Command (defaults to $SHELL, then /bin/sh)
TERAL_TERMINAL    terminal used by Open Terminal Here (otherwise auto-detected)
```

Pinned sidebar folders are stored in `~/.local/share/teral/places.toml`.

## Development checks

```bash
./scripts/check.sh
```

That runs formatting checks, Clippy with warnings treated as errors, and tests.

## Theming

Teral ships with `themes/default/teral.toml` and `themes/default/teral.css`.

The TOML file carries Teral's semantic palette and layout numbers. The CSS file is written entirely against those semantic values and against stable `.teral-*` classes, so a theme only has to supply colors — never GTK widget-tree selectors.

Themes are resolved in layers, each one overriding only the keys it sets:

```text
built-in Teral defaults
        ↓
Omarchy active theme (teral.toml, otherwise derived from colors.toml)
        ↓
~/.config/teral/teral.toml
```

On Omarchy, Teral looks for the active theme under the XDG state location Omarchy uses. If the active theme contains `teral.toml`, Teral applies it. Otherwise Teral derives its palette from that theme's `colors.toml`, so switching Omarchy themes restyles Teral even when the theme author has never heard of Teral.

Invalid colors and out-of-range layout values are discarded or clamped, so a broken theme can never make Teral unusable.

## License

MIT License. Copyright (c) 2026 Zuhaib Ullah Baig.
