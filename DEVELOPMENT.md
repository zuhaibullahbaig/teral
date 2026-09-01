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

Install `poppler-utils ffmpeg` on Ubuntu or `poppler ffmpeg` on Arch when exercising PDF
and media previews. They are runtime preview providers, not build dependencies.

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
the commit or test notes. This smoke test is required on Ubuntu; repeat it on
Omarchy for changes that affect desktop integration, themes, packaging, or file operations.

## Copy, move and clipboard test matrix

Run this matrix with disposable test data after `./scripts/check.sh` succeeds. Passing
unit tests is not sufficient; the desktop and cross-filesystem behaviour has to be
exercised by hand.

- [ ] Copy, Cut/Paste, Move, Duplicate, and Link work on files, folders, empty files,
  deep trees, relative symlinks, absolute symlinks, and broken symlinks.
- [ ] Permissions, timestamps, xattrs/ACLs where supported, and sparse allocation are
  retained to the extent supported by the source and destination filesystems.
- [ ] Replace, Rename, Skip, and Cancel preserve the old destination correctly;
  repeat while another process creates the requested destination during the operation.
- [ ] Paste same-name folders and verify each folder can be merged, renamed or skipped.
  Inside a merge, verify same-name files are asked about individually and that applying
  Replace or Skip to all similar conflicts does not affect folder decisions.
- [ ] Cut an item and paste it back into its current folder. Verify Teral refuses the
  self-move without changing or duplicating the item.
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
Omarchy/Arch before treating this matrix as passed.

## Trash and deletion test matrix

Run this with disposable data on a scratch filesystem after `./scripts/check.sh`
succeeds. Never point it at a trash that holds anything you want back. Passing unit tests
is not sufficient; the device and multi-filesystem behaviour has to be exercised by hand.

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
- [ ] Restore into an occupied original location. Verify Replace, Rename, Skip
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
Omarchy/Arch before treating this matrix as passed.

## Navigation, opening and filenames test matrix

Run with disposable data after `./scripts/check.sh` succeeds.

- [ ] `teral` with no arguments opens the home directory.
- [ ] `teral ~/Documents` opens that folder; `teral ~/Documents/report.pdf` opens the
  folder with the file selected; `teral /path/that/does/not/exist` still opens something
  usable rather than failing to start.
- [ ] With Teral already running, launch it again with a folder. The running window is
  raised and the folder arrives as a new tab; no second window appears.
- [ ] Set Teral as the default file manager
  (`xdg-mime default dev.zuhaibullahbaig.Teral.desktop inode/directory`), then run
  `xdg-open ~/Documents` and open a folder from a browser's download list.
- [ ] Create and rename with names containing spaces, a leading dot, quotes, `*`, `?`,
  a newline, and emoji. Each is accepted and the file is created under exactly that name.
- [ ] Create a name with leading and trailing spaces. It is not trimmed, and the file
  appears under the name that was typed.
- [ ] Try `/`, an empty name, `.`, `..`, and a name over 255 bytes. Each is refused with
  a reason, and the dialog stays open so the name can be corrected.
- [ ] Rename a file whose name is not valid UTF-8. Confirming it unchanged does not
  replace the name; editing it renames it correctly.
- [ ] Open a symlink to a directory. Teral navigates into it. Open one to a file and it
  opens the file.
- [ ] Select a broken symlink. It is listed, the details panel names its target and says
  it is missing, Open is unavailable, and it can still be trashed and deleted.
- [ ] Select a FIFO (`mkfifo`), a socket and a device node under `/dev`. Each is
  described, none is opened, and Teral does not hang.
- [ ] Delete and recreate files in a folder from a terminal while browsing it. The view
  updates on its own. Do the same while clicking into a subfolder; the navigation
  completes and is not thrown back to the previous folder.
- [ ] Open a tag view in one tab and a folder in another. Switch between them repeatedly;
  each tab returns to its own view, with its own back and forward history.
- [ ] In a tag view and in the trash, confirm New Folder, Paste, Open Terminal Here and
  Quick Command are not offered, and that dropping files there is refused.
- [ ] Mount a network share, disconnect it without unmounting, then browse an unrelated
  folder and click through several files. The window stays responsive, and the trash
  entries for reachable disks still work.
- [ ] Put a symlink in a folder pointing into a network share, then disconnect the share
  without unmounting it. Opening that folder still draws the window and stays
  responsive; the link resolves, or shows as broken, without blocking the view.
- [ ] Start Teral, close the window, and confirm the process exits. Repeat ten times and
  confirm no `teral` processes are left behind (`pgrep -f teral`).

## Persistence, responsiveness and desktop integration checks

Use isolated `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `XDG_STATE_HOME` directories for
these checks. Never use a real profile containing valuable tags, bookmarks or sessions.

- [ ] Save settings, tags, bookmarks, command history and a multi-tab session; interrupt
  each write before rename and confirm the previous complete file still loads.
- [ ] Make each state directory read-only, occupy several plausible temporary names,
  replace a valid file with malformed TOML, and repeat rapid saves. Every error is shown
  and no valid file is truncated.
- [ ] Bookmark and tag a raw non-UTF-8 path on removable storage. Unmount and restart;
  the metadata remains, then resolves to the exact bytes after remounting.
- [ ] Reorder bookmarks and apply, replace and clear display labels. Restart and confirm
  order, paths and labels are unchanged.
- [ ] Browse synthetic directories containing 1,000, 10,000 and 100,000 entries while
  navigating rapidly, filtering, sorting and generating monitor bursts. Record hardware,
  filesystem, cold/warm cache state, first-batch time, completion time and peak memory.
- [ ] Use malformed and oversized images, an image-heavy folder, disappearing files and
  rapid navigation. Old thumbnails never land in the new folder and memory stays bounded.
- [ ] Test a USB filesystem, a separate filesystem, a read-only mount, unmount while
  browsing, and removal during Copy and Move. Record every partial result precisely.
- [ ] Verify clipboard and drag-and-drop both directions with GNOME Files under Wayland,
  then repeat on Omarchy/Arch. Test Copy, Move and Link modifiers.
- [ ] Where available, connect an MTP device and a GVfs network location. Record that
  a GVfs location with a local FUSE path is browsable, and that a location with no local
  path is refused explicitly rather than converted into an invented path.
- [ ] Mount a removable volume from its unmounted sidebar row, then use its context menu
  to unmount and eject it. Exercise authentication cancellation, refusal, a busy mount,
  and physical removal while its directory is open.
- [ ] Switch GTK light/dark and icon themes while running. On Omarchy, replace the active
  theme symlink and edit `teral.toml` and `colors.toml`, including malformed and partial
  files. Confirm the window stays readable and cache changes appear live.
- [ ] Drag and repeatedly click the bottom-right icon-size controls in a 10,000-entry
  folder, then leave the window idle for one minute. Repeat while changing between Teral
  and system appearance. The pointer, window and directory view must remain responsive,
  and each settled preference must survive one restart.
- [ ] Select files with single-component names near the filesystem limit, paths with
  long components, and long MIME descriptions. The Details panel must keep one width;
  names wrap or ellipsize inside it rather than resizing the file view.

## Commands, previews, sessions and accessibility checks

- [ ] Run a successful command, a non-zero command, a signal-terminated command and an
  invalid executable. Interrupt a foreground pipeline, then exercise the separate force
  stop. Confirm no child remains after exit or window close.
- [ ] Run Quick Command in paths containing spaces and newlines. Select a non-UTF-8
  directory and confirm execution is refused visibly rather than run elsewhere.
- [ ] Restart with ordinary, missing, removed-media and tag tabs. Confirm recoverable tabs
  remain recorded, one usable location appears, and repeated starts do not duplicate
  windows.
- [ ] Preview UTF-8 text, Markdown, source, binary, malformed UTF-8, oversized and
  disappearing files. Confirm content is inert and no HTML, script or resource executes.
- [ ] Search Home from the far-left header control. Confirm the normal header is hidden
  while the centered rounded search field is active, the first bounded page appears,
  scrolling near the end loads another page, and Close or Escape cancels the search and
  restores the previous directory. Confirm Ctrl+F still filters one folder, symlink
  directories are not followed, and leaving the result view cancels stale work.
- [ ] Preview a normal, encrypted, malformed, oversized and disappearing PDF, plus audio
  and video with missing or malformed metadata. Remove each provider from `PATH`, test a
  slow input, and confirm errors are bounded and the window remains responsive.
- [ ] Create files with leading/trailing spaces and race another process for the same
  name. Existing data is never replaced and the actual created file is selected.
- [ ] Edit permissions on files and folders, including permission-denied and read-only
  cases. Confirm special bits remain and symlink targets are never changed.
- [ ] Traverse every visible action with only the keyboard. Check focus visibility,
  screen-reader names, high contrast, empty/loading/error/disconnected states and the
  absence of keyboard traps on Ubuntu and Omarchy.

Non-local GIO locations that expose no local path remain outside Teral's local file
operation boundary. Do not mark that case as supported merely because a desktop mount
with a local GVfs FUSE path works.

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
