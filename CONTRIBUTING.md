# Contributing to AI Note Taker

Thank you for your interest in contributing. This guide covers setting up a development environment, code style conventions, and the pull request process.

## Development Environment Setup

### Prerequisites (all platforms)

- **Rust** 1.78+ via [rustup](https://rustup.rs/)
- **cbindgen** for C header generation: `cargo install cbindgen`

Install the core build dependencies first, then add platform-specific toolchains below.

### macOS / iOS

```bash
# Xcode 16+ (from the Mac App Store)
xcode-select --install

# Rust targets
rustup target add aarch64-apple-darwin x86_64-apple-darwin
rustup target add aarch64-apple-ios aarch64-apple-ios-sim

# Build
make build-core
make build-macos
```

### Windows

- **Visual Studio 2022** with the C++ and .NET Desktop workloads
- **.NET 8 SDK**

```powershell
rustup target add x86_64-pc-windows-msvc
make build-core
make build-windows
```

### Linux

```bash
# Debian / Ubuntu
sudo apt install build-essential libgtk-4-dev libpipewire-0.3-dev \
    libappindicator3-dev pkg-config libsqlite3-dev

# Build (Linux app links core directly, no FFI layer)
make build-linux
```

### Android

- **Android Studio** with NDK 26+ and CMake
- **cargo-ndk**: `cargo install cargo-ndk`

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi \
    x86_64-linux-android i686-linux-android

make build-android
```

### Downloading Whisper models

```bash
make download-models              # downloads the tiny model (~75 MB)
make download-models MODEL=base   # or base / small / medium
```

## Code Style

### Rust (core and linux)

- Format with `rustfmt` (default config).
- Run `cargo clippy -- -D warnings` before committing.
- Keep `unsafe` blocks minimal and well-documented.

### Swift (apple)

- [SwiftFormat](https://github.com/nicklockwood/SwiftFormat) with default rules.
- [SwiftLint](https://github.com/realm/SwiftLint) with the project `.swiftlint.yml`.

### C# (windows)

- `dotnet format` on the solution.

### Kotlin (android)

- [ktlint](https://pinterest.github.io/ktlint/) with the project config.

### General

- Commit messages: imperative mood, 72-character summary line, blank line, then body.
- One logical change per commit.

## Stealth Naming Convention

The app intentionally uses generic process names and bundle identifiers to remain invisible in screen-sharing and task managers. This is by design. Do **not** rename executables, bundle IDs, or process names to something more descriptive in pull requests. See the design specification in `docs/` for details.

## Pull Request Process

1. **Fork and branch.** Create a feature branch from `main` (e.g., `feat/speaker-diarization`).
2. **Keep changes focused.** One feature or fix per PR.
3. **Test.**
   - Run `make test` for core library checks.
   - If your change touches a platform app, build and run it on that platform.
4. **Describe the change.** Fill in the PR template with:
   - What changed and why.
   - How to test.
   - Screenshots or screen recordings for UI changes.
5. **Review.** At least one maintainer approval is required before merging.
6. **Rebase on main** if the branch falls behind; we prefer a linear history.

## Reporting Issues

Open an issue with:
- Platform and OS version.
- Steps to reproduce.
- Expected vs. actual behavior.
- Logs from the app's console output (if applicable).
