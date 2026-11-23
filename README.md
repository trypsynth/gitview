# Gitview

Gitview is a native, keyboard-first GitHub client for Windows, made for screen reader users.

## Documentation

The [user manual](doc/readme.md) covers every feature, setting, and key.

## Building

You need Rust, and CMake and Ninja to build wxWidgets for wxDragon.

```batch
cargo build --release
```

This makes `target/release/gitview.exe`.

- Stable Rust `1.91.1` is the minimum supported version. CI builds, tests, and lints on the latest stable.
- Nightly Rust is only for formatting, with `cargo +nightly fmt`.
- Clippy runs with `clippy::all`, `clippy::pedantic`, and `clippy::nursery`, and CI fails on any warning.

### Release Builds

`cargo xtask release` builds Gitview and makes `gitview-x64.zip` and, with Inno Setup installed, `gitview_setup-x64.exe`, in `target/release`. On an Arm computer the names end in `arm64`. With a minisign key in `MINISIGN_KEY` and its password in `MINISIGN_PASSWORD`, it also signs them, and the updater installs only signed builds.

## Before Committing

Format your changes:

```batch
cargo +nightly fmt
```

The repository's pre-commit hooks run the same check:

```batch
cargo install prek
prek install
```
