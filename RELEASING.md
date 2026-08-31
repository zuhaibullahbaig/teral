# Releasing Teral

Read this before cutting a release. It is short on purpose, and it is binding: the
release workflow enforces the parts that can be enforced, and refuses to publish
anything that skips them.

## The rule

**Every release gets a new version number, and that number lives in exactly one place:
`version` in `Cargo.toml`.** The git tag is `v` plus that number. If they disagree, the
release job stops before building anything.

Teral uses semantic versioning:

- **patch** (`1.0.0` → `1.0.1`) — fixes only, nothing new to learn
- **minor** (`1.0.0` → `1.1.0`) — new features, existing configuration keeps working
- **major** (`1.0.0` → `2.0.0`) — something people relied on changed: a configuration key
  removed or renamed, a theme format break, a default that flips

Configuration and theme files people already have must keep loading across a minor
release. When a key is replaced, keep reading the old one as an alias — the way
`mode = "omarchy"` is still read as `mode = "system"` — rather than breaking a file
somebody hand-edited.

## Cutting one

1. `./scripts/check.sh` — formatting, Clippy with warnings as errors, tests.
2. Bump `version` in `Cargo.toml`.
3. Run `cargo build` once so `Cargo.lock` picks up the new version, and commit both.
4. Add a `## <version>` section at the top of `CHANGELOG.md`, written for people who use
   Teral rather than for people who wrote it.
5. Commit: `Release <version>`.
6. Tag and push:

   ```bash
   git tag -a v<version> -m "Teral <version>"
   git push origin main --follow-tags
   ```

The Release workflow then verifies the tag against `Cargo.toml`, verifies `CHANGELOG.md`
has a section for it, runs the same checks again, builds, packages and publishes the
GitHub release with the tarball, the `.deb` and `SHA256SUMS`.

## What ships

`scripts/package.sh` produces everything, and CI runs it on every push too, so any
commit has downloadable artifacts:

| Artifact | For |
| --- | --- |
| `teral-<version>-x86_64-linux.tar.gz` | any distribution — binary, desktop entry, icon and `scripts/install.sh` |
| `teral_<version>_amd64.deb` | Debian and Ubuntu |
| `packaging/PKGBUILD` (in the repo) | Arch and Omarchy, via `makepkg -si` |

The `PKGBUILD`'s `pkgver` is the one version number that lives outside `Cargo.toml`.
Update it in the same commit as the bump; it is part of step 2.

## For anyone — human or agent — continuing this work

- Never publish, push tags, or create releases unless you were asked to.
- A release is not "the code is finished", it is "this version number is now taken".
  Once a tag is pushed it is not reused: a mistake becomes the next patch release.
- Do not hand-edit a published artifact. Fix the source, bump the patch version, retag.
- Keep `CHANGELOG.md` honest: what changed for the person using Teral, including
  anything removed. An empty section is a sign the release should not be cut.
