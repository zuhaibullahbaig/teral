# Teral

Teral is a modern native Linux file manager written in Rust with GTK4.

The goal is a fast, information-rich file manager that feels at home on a normal Linux desktop while also integrating cleanly with theme-driven environments such as Omarchy.

Teral is one application and one codebase. Desktop-specific behavior is handled through integration layers and configuration rather than separate editions.

## Current state

Teral is in early local development. The initial foundation provides:

- a native GTK4 application shell
- basic filesystem browsing
- a three-pane layout with navigation, files, and details
- system icon-theme usage through GTK
- a TOML-driven theme foundation
- automatic Omarchy palette discovery from the active theme when available
- user theme overrides from `~/.config/teral/teral.toml`

The full file-management feature set is intentionally being built incrementally on top of this foundation.

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

## Development checks

```bash
./scripts/check.sh
```

That runs formatting checks, Clippy with warnings treated as errors, and tests.

## Theming

Teral ships with `themes/default/teral.toml`.

The default theme deliberately leaves colors to GTK so Teral follows the desktop's normal appearance. Layout values remain Teral-specific.

On Omarchy, Teral looks for the active theme at the XDG state location used by Omarchy. If the active theme contains `teral.toml`, Teral applies it. Otherwise Teral can derive its core palette from Omarchy's `colors.toml`.

A user-level override can be placed at:

```text
~/.config/teral/teral.toml
```

User values override environment-derived values. Missing values continue to inherit from lower layers.

## License

MIT License. Copyright (c) 2026 Zuhaib Ullah Baig.
