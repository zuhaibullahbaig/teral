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

Teral targets GTK 4.12 and newer. Check what you have:

```bash
pkg-config --modversion gtk4
```

## Running

```bash
cargo run
```

Teral is a single-instance application, so a second `cargo run` hands over to the copy
already running instead of starting the new build. Stop the old one first:

```bash
pkill -f target/debug/teral && cargo run
```

## Checks

```bash
./scripts/check.sh
```

Formatting, Clippy with warnings as errors, and tests — the same script CI runs.

## Packaging

```bash
./scripts/package.sh
```

Builds the release binary and writes `dist/teral-<version>-<arch>-linux.tar.gz` and
`dist/teral_<version>_<arch>.deb`. `scripts/install.sh` installs a built binary, its
desktop entry and its icon under `PREFIX` (default `/usr/local`, `DESTDIR` honoured).

## House rules

- Rust, GTK4, GIO/GLib. No web view, no Electron, no Tauri.
- `unsafe` is forbidden by the crate, not by convention.
- Filenames are `OsStr`/`Path`, never assumed to be UTF-8.
- Anything that can be slow — recursive copies, deletes, archive work — runs off the GTK
  main thread.
- No placeholder UI. A button that cannot do its job is absent, not disabled-forever.
- Dependencies are added reluctantly.

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
