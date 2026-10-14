# Meeting Assistant

A privacy-focused, on-device meeting assistant that captures audio, transcribes in real-time using local AI, and generates summaries — all without sending data off your device.

## Features

- **Local transcription** powered by [whisper.cpp](https://github.com/ggerganov/whisper.cpp) — no cloud required
- **Real-time transcript** as you speak
- **AI-powered summaries** with key points, action items, and decisions
- **Cross-platform** native apps for macOS, iOS, Windows, Linux, and Android
- **Privacy-first** — audio and transcripts stay on your device
- **Invisible to screen sharing** — window is excluded from screen capture
- **Local storage** via local SQLite (unencrypted)

## Architecture

A shared Rust core library handles audio processing, transcription, summarization, and storage. Each platform wraps it with a native UI:

| Platform | UI Framework     | Language |
|----------|-----------------|----------|
| macOS    | SwiftUI          | Swift    |
| iOS      | SwiftUI          | Swift    |
| Windows  | WinUI 3          | C#       |
| Linux    | GTK4             | Rust     |
| Android  | Jetpack Compose  | Kotlin   |

```
monorepo/
├── core/        Shared Rust library (audio, transcription, storage)
├── apple/       macOS + iOS (Xcode project, SwiftUI)
├── windows/     WinUI 3 / C# application
├── linux/       GTK4 Rust application
├── android/     Kotlin + Jetpack Compose
├── models/      Whisper model files (downloaded separately)
└── scripts/     Build and utility scripts
```

## Prerequisites

- **Rust** 1.75+ — install via [rustup](https://rustup.rs)
- **cmake** — required for building whisper.cpp
- Platform-specific tools (see below)

## Quick Start

### 1. Download a Whisper Model

```bash
# Download the tiny model (~75 MB) for quick start
./scripts/download-model.sh tiny

# Other options: base (~142 MB), small (~466 MB), medium (~1.5 GB)
./scripts/download-model.sh base
```

### 2. Build the Core Library

```bash
# Build for your current platform
./scripts/build-core.sh host

# Build for a specific target
./scripts/build-core.sh macos
./scripts/build-core.sh linux

# Build all targets possible on this host
./scripts/build-core.sh all
```

### 3. Build a Platform App

#### macOS / iOS (requires macOS + Xcode)

```bash
# Build core for Apple platforms
./scripts/build-core.sh macos ios

# Open in Xcode
open apple/*.xcodeproj
```

#### Windows (requires Visual Studio + .NET 8)

```bash
# Build core
scripts\build-core.sh windows

# Build app
dotnet build windows\NoteHelper.sln -c Release
```

#### Linux (requires GTK4 development libraries)

```bash
# Install dependencies (Ubuntu/Debian)
sudo apt install libgtk-4-dev libadwaita-1-dev cmake

# Build everything
./scripts/build-core.sh linux
cargo build --release -p notetaker-linux
```

#### Android (requires Android SDK + NDK)

```bash
# Set NDK path
export ANDROID_NDK_HOME=/path/to/ndk

# Install cargo-ndk
cargo install cargo-ndk

# Build core
./scripts/build-core.sh android

# Build app
cd android && ./gradlew assembleDebug
```

## Model Download

The app uses [whisper.cpp](https://github.com/ggerganov/whisper.cpp) GGML models for local transcription.

| Model  | Size     | Quality                          |
|--------|----------|----------------------------------|
| tiny   | ~75 MB   | Fastest, basic accuracy          |
| base   | ~142 MB  | Good balance for everyday use    |
| small  | ~466 MB  | Better accuracy                  |
| medium | ~1.5 GB  | Best quality, higher CPU usage   |

```bash
# List downloaded models
./scripts/download-model.sh --list

# Download a model
./scripts/download-model.sh small

# Force re-download
./scripts/download-model.sh small --force
```

Models are stored in the `models/` directory and are gitignored.

## Development

### Project Structure

- **`core/`** — Shared Rust library exposing a C FFI. All business logic lives here: audio capture abstraction, whisper.cpp transcription, AI summarization, and SQLite storage.
- **`apple/`** — Xcode project with shared SwiftUI code and platform targets for macOS and iOS.
- **`windows/`** — WinUI 3 application in C#, consuming the core library via P/Invoke.
- **`linux/`** — GTK4 application in Rust, using the core library as a direct Rust dependency.
- **`android/`** — Kotlin application with Jetpack Compose, loading the core library via JNI.
- **`scripts/`** — Build automation and utility scripts.

### CI

GitHub Actions builds all platforms on every push to `main` and on pull requests. The workflow:

1. Builds the core library for each platform matrix (macOS, Windows, Linux + Android)
2. Builds each native app using the core library artifact
3. Uploads build artifacts

## License

MIT — see [LICENSE](LICENSE) for details.
