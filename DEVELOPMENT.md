# Building Teral

## Dependencies

Ubuntu and Debian:

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libgtk-4-dev libvte-2.91-gtk4-dev
```

Arch:

```bash
sudo pacman -S --needed base-devel gtk4 vte4 pkgconf
```

`libvte-2.91-gtk4-dev` is what gives Quick Command a real pseudo-terminal, so
interactive programs can be used from inside Teral.

Rust comes from [rustup](https://rustup.rs); the toolchain is pinned in
`rust-toolchain.toml`.

Teral's supported system-library floor is GTK 4.12 and VTE for GTK4 0.66. These match
the API floors selected by the Rust bindings. Check what you have:

```bash
pkg-config --modversion gtk4
pkg-config --modversion vte-2.91-gtk4
```

## Running

```bash
cargo run --locked
```

Teral is a single-instance application, so a second `cargo run --locked` hands over to the copy
already running instead of starting the new build. Stop the old one first:

```bash
pkill -f target/debug/teral && cargo run --locked
```

## Required checks

```bash
./scripts/check.sh
```

System-library and version consistency checks, formatting, Clippy with warnings as errors,
tests, and debug and release builds. The script uses the committed lockfile and is the same
entry point CI runs.

## Manual smoke test

After `./scripts/check.sh` succeeds, launch the release build with
`./target/release/teral` and complete this checklist on a supported Linux desktop:

- [ ] The window launches without warnings that prevent normal use.
- [ ] Browse the home directory and another readable directory.
- [ ] Navigate backward, forward, and to the parent directory.
- [ ] Switch between list and grid views; the same directory remains visible.
- [ ] Select a file and confirm its details update.
- [ ] Open a regular file with its default application.
- [ ] Open a folder and confirm Teral navigates into it.
- [ ] Close the window and confirm the process exits cleanly.

Record the distribution, desktop session, GTK version, VTE version, and any failed step in
the commit or release-candidate notes. This smoke test is required on Ubuntu; repeat it on
Omarchy for changes that affect desktop integration, themes, packaging, or file operations.

## Packaging

```bash
./scripts/package.sh
```

Builds the release binary with the committed lockfile and writes
`dist/teral-<version>-<arch>-linux.tar.gz` and `dist/teral_<version>_<arch>.deb`.
`scripts/install.sh` installs a built binary, its desktop entry and its icon under
`PREFIX` (default `/usr/local`, `DESTDIR` honoured).

System-wide installation requires root:

```bash
sudo ./scripts/install.sh
```

For a user-only installation:

```bash
PREFIX="$HOME/.local" ./scripts/install.sh
```

## House rules

- Rust, GTK4, GIO/GLib. No web view, no Electron, no Tauri.
- `unsafe` is forbidden by the crate, not by convention.
- Filesystem behavior must retain names as `OsStr`/`Path`; UTF-8 conversion is display-only.
- Anything that can be slow, including recursive operations and media decoding, must run
  off the GTK main thread.
- No placeholder UI. A button that cannot do its job is absent, not disabled-forever.
- Dependencies are added reluctantly.
- A documented guarantee must have implementation and failure tests behind it.

## Layout

```text
src/
  app.rs        application startup
  config.rs     ~/.config/teral/teral.toml
  theme.rs      palette resolution, desktop and Omarchy integration
  style.rs      generated CSS, applied through one live provider
  files/        entries, sorting, and file operations (files/ops.rs)
  icons.rs      icon and thumbnail resolution
  places.rs     XDG locations, mounts, bookmarks
  tags.rs       the tag store
  command.rs    Quick Command
  ui.rs         application state, and the window's building blocks
  ui/           window, header, sidebar, details, statusbar, dialogs, settings, help
themes/default/ the shipped palette and stylesheet
packaging/      desktop entry, icon, PKGBUILD
scripts/        check.sh, package.sh, install.sh
```
