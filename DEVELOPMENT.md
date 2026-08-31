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

## Filesystem transfer gate

Run this matrix with disposable test data after `./scripts/check.sh` succeeds. A Stage 1–2
candidate does not pass merely because its unit tests pass.

- [ ] Copy, Cut/Paste, Move, Duplicate, and Link work on files, folders, empty files,
  deep trees, relative symlinks, absolute symlinks, and broken symlinks.
- [ ] Permissions, timestamps, xattrs/ACLs where supported, and sparse allocation are
  retained to the extent supported by the source and destination filesystems.
- [ ] Replace, Rename Incoming, Skip, and Cancel preserve the old destination correctly;
  repeat while another process creates the requested destination during the operation.
- [ ] Cancel a multi-gigabyte copy while bytes are moving. The source and any unrelated
  destination remain intact, and no hidden `.teral-*` partial is left behind.
- [ ] Copy and Move on one filesystem, then Move between two filesystems. Unmount or
  disconnect disposable removable media during a copy and confirm the failure is explicit.
- [ ] Repeat failure cases with an unreadable source child, a read-only destination, and a
  nearly full disposable filesystem. No completed item may be reported as failed or vice versa.
- [ ] Copy and Cut in Teral and paste into Nautilus, Dolphin, and Thunar; then copy and Cut
  in each of those file managers and paste into Teral.
- [ ] Drag into a folder tile, empty background, sidebar location, and bookmark. Verify
  Copy, Move, and Link modifiers show the chosen action before release and perform it.
- [ ] Attempt same-folder, self, descendant, and symlink-mediated recursive drops. No source
  is deleted and no recursive tree is created.
- [ ] Confirm tags follow only completed Moves, including conflict-renamed destinations,
  and an incomplete Cut remains available with only its uncompleted sources.

Record filesystem types, desktop/file-manager versions, available free space, and the
exact failed row. Ubuntu coverage is mandatory; repeat the desktop-integration rows on
Omarchy/Arch before treating the stage as accepted.

## Trash gate

Run this with disposable data on a scratch filesystem after `./scripts/check.sh`
succeeds. Never point it at a trash that holds anything you want back. A Stage 3
candidate does not pass merely because its unit tests pass.

- [ ] Trash a file, an empty folder, a deep folder, a relative symlink, an absolute
  symlink, and a broken symlink. Each appears in the trash and restores to its original
  path.
- [ ] Trash a file whose name is not valid UTF-8, one containing spaces, and one
  containing a newline. Each restores under exactly its original name.
- [ ] Trash something on a second filesystem — a disposable USB stick or loopback mount.
  Confirm `.Trash-$uid` is created there, that the location appears in the sidebar
  labelled by that disk, and that the item restores to that filesystem.
- [ ] Where an administrator has created a sticky `$topdir/.Trash`, confirm the per-user
  subdirectory inside it is used. Replace it with a symlink and confirm Teral falls back
  to `.Trash-$uid` instead.
- [ ] Unmount the second filesystem with items still in its trash. Its sidebar entry
  disappears, Teral stays usable, and remounting brings the items and their records back.
- [ ] Restore into an occupied original location. Verify Replace, Rename Incoming, Skip
  and Cancel each behave as they do for Paste, and that Skip leaves the item and its
  record in the trash.
- [ ] Delete an original folder, then restore an item from it. Verify the choice between
  recreating the folder, leaving that item in the trash, and cancelling, and that
  closing the dialog changes nothing.
- [ ] Corrupt one `.trashinfo` record and delete another. Both items stay listed, both
  say why they cannot be restored, and everything else in the same selection restores.
- [ ] Empty Trash with items in two trash locations. The confirmation names the real
  count and the number of locations before anything is deleted.
- [ ] Make one trashed item undeletable — a read-only parent, or an unreadable child.
  Empty Trash removes everything else, reports that item by name, and leaves its
  `.trashinfo` record in place so it is still restorable.
- [ ] Trash a folder containing a symlink to a directory outside the trash. Delete it
  permanently and confirm the directory it pointed at is untouched.
- [ ] Cancel Empty Trash with `Esc` part-way through a large trash. Items not yet reached
  and all of their records survive, and the message says how many were deleted.
- [ ] Tag a file, trash it, and confirm the tag is not left pointing at a path that no
  longer exists. Repeat with a restore into a conflict-renamed destination and confirm
  the tag follows the actual destination.
- [ ] Repeat the failure rows with a read-only filesystem and with permission denied.
  No item may be reported as deleted when it still exists, or as failed when it is gone.

Record filesystem types, desktop versions, the exact failed row, and whether the second
filesystem was removable. Ubuntu coverage is mandatory; repeat the device rows on
Omarchy/Arch before treating the stage as accepted.

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
                transfer.rs is the one copy/move/placement engine; trash.rs is the
                GTK-free FreeDesktop trash model
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
