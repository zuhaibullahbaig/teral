# Releasing Teral

Read this before cutting a release. It is short on purpose, and it is binding: the
release workflow enforces the parts that can be enforced, and refuses to publish
anything that skips them.

## The rule

**Every release gets a new version number. `version` in `Cargo.toml` is canonical;
`Cargo.lock` and `packaging/PKGBUILD` mirror it.** The git tag is `v` plus that number.
The version check stops before building when those files disagree.

Teral uses semantic versioning:

- While Teral is pre-1.0, **patch** (`0.1.0` → `0.1.1`) is a compatible bug-fix release.
- While Teral is pre-1.0, **minor** (`0.1.0` → `0.2.0`) is the next tested product
  milestone and may include documented compatibility changes.
- `1.0.0` is reserved for a later stable milestone with mature compatibility guarantees.

Configuration and theme files people already have must keep loading across a minor
release. When a key is replaced, keep reading the old one as an alias — the way
`mode = "omarchy"` is still read as `mode = "system"` — rather than breaking a file
somebody hand-edited.

## Before the first release

Teral is currently an unreleased `0.1.0` application. Keep `CHANGELOG.md` under
`Unreleased`; do not create release badges, download links, tags, or GitHub releases until
the public 0.1 release gate in the local roadmap has passed.

## Cutting a release

1. `./scripts/check.sh` — version consistency, formatting, Clippy, tests, and a build.
2. Bump `version` in `Cargo.toml` and `pkgver` in `packaging/PKGBUILD`.
3. Run `cargo check` once to update the root package version in `Cargo.lock`, then run
   `./scripts/check.sh` again. Commit all three version files together.
4. Replace `Unreleased` with a `## <version>` section in `CHANGELOG.md`, written for people who use
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

`scripts/package.sh` produces the candidate artifacts below. CI retains artifacts for
development verification; they are not public releases unless the tagged release workflow
publishes them.

| Artifact | For |
| --- | --- |
| `teral-<version>-x86_64-linux.tar.gz` | any distribution — binary, desktop entry, icon and `scripts/install.sh` |
| `teral_<version>_amd64.deb` | Debian and Ubuntu |
| `packaging/PKGBUILD` (in the repo) | Arch and Omarchy, via `makepkg -si` |

`scripts/check-version.sh` verifies the three version files agree before checks or CI
continue.

## For anyone — human or agent — continuing this work

- Never publish, push tags, or create releases unless you were asked to.
- A release is not "the code is finished", it is "this version number is now taken".
  Once a tag is pushed it is not reused: a mistake becomes the next patch release.
- Do not hand-edit a published artifact. Fix the source, bump the patch version, retag.
- Keep `CHANGELOG.md` honest: what changed for the person using Teral, including
  anything removed. An empty section is a sign the release should not be cut.
