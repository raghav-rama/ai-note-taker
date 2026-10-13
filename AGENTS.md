# AI Note Taker

A bot-less, privacy-first meeting note taker that runs entirely on-device. Invisible to screen sharing, with a generic process name that doesn't reveal its purpose.

## Architecture

Shared **Rust core library** (`core/`) exposed via C FFI, wrapped by native UI per platform:

| Platform | Directory | UI Framework | Language |
|----------|-----------|-------------|----------|
| macOS | `apple/NoteTaker/` | SwiftUI | Swift |
| iOS | `apple/iOS/` | SwiftUI | Swift |
| Windows | `windows/NoteHelper/` | WinUI 3 | C# |
| Linux | `linux/` | GTK4 + libadwaita | Rust |
| Android | `android/` | Jetpack Compose | Kotlin |

Shared Swift code lives in `apple/Shared/`.

## Core library (`core/`)

- `audio.rs` — Audio capture via cpal, resamples to 16kHz mono f32
- `transcribe.rs` — Local transcription via whisper-rs (whisper.cpp bindings)
- `summarize.rs` — AI summarization via Claude API (behind `cloud` feature flag)
- `storage.rs` — SQLite with FTS5 full-text search
- `models.rs` — Shared data types (Meeting, TranscriptSegment, Note, etc.)
- `lib.rs` — C FFI surface (`extern "C"` functions with `#[no_mangle]`)
- `build.rs` — Platform link flags + cbindgen header generation → `core/include/notetaker.h`

## Stealth features

Each platform uses its native API to exclude the window from screen capture:

- **macOS**: `NSWindow.sharingType = .none`
- **iOS**: `UITextField.isSecureTextEntry` container trick + `UIScreen.isCaptured` detection
- **Windows**: `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`
- **Linux**: Best-effort compositor hints (no reliable cross-compositor API)
- **Android**: `FLAG_SECURE` on the window

Process names are generic per platform — see the design spec for details.

## Building

```bash
# Download a Whisper model first
./scripts/download-model.sh tiny

# Build core for current platform
make build-core

# Build a specific platform
make build-macos    # Requires macOS + Xcode
make build-linux    # Requires GTK4 dev libs
make build-windows  # Requires Windows + .NET 8
make build-android  # Requires Android NDK
make build-ios      # Requires macOS + Xcode

# Run tests
make test
```

## Key decisions

- **Native apps, not cross-platform framework** — Each platform gets its own UI in the idiomatic toolkit. The Rust core is the shared layer, exposed via C FFI.
- **Local-first transcription** — whisper.cpp runs on-device. Cloud (OpenAI Whisper API) is opt-in fallback behind a feature flag.
- **SQLite for storage** — All data local. FTS5 for full-text search across transcripts.
- **No telemetry, no analytics** — Nothing phones home unless the user explicitly enables cloud transcription or summarization.

## Design spec

Full specification: `docs/superpowers/specs/2026-10-13-ai-note-taker-design.md`
