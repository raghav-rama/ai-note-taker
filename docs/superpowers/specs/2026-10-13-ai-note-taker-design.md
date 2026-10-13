# AI Note Taker — Design Specification

## Overview

A privacy-focused, bot-less meeting note-taker that runs entirely on the user's device. Captures audio from the local microphone/system audio, transcribes in real-time using local AI (whisper.cpp), summarizes meetings, and stores everything locally. The app is invisible to screen sharing and uses a generic process name. Fully native apps for each platform.

## Architecture

### Platform-Native Apps with Shared Rust Core

The core logic (audio processing, transcription, summarization, storage) lives in a shared Rust library exposed via C FFI. Each platform wraps it with a native UI.

| Platform | UI Framework | Language | Native APIs |
|----------|-------------|----------|-------------|
| macOS | SwiftUI | Swift | CoreAudio, ScreenCaptureKit, NSWindow |
| iOS | SwiftUI | Swift | AVAudioEngine, AVAudioSession |
| Windows | WinUI 3 | C# | WASAPI, SetWindowDisplayAffinity |
| Linux | GTK4 | Rust | PulseAudio/PipeWire, X11/Wayland |
| Android | Jetpack Compose | Kotlin | AudioRecord, MediaProjection |

### Shared Rust Core Library (`notetaker-core`)

```
notetaker-core/
├── src/
│   ├── lib.rs          # C FFI exports
│   ├── audio.rs        # Audio capture abstraction
│   ├── transcribe.rs   # whisper.cpp integration
│   ├── summarize.rs    # AI summarization
│   ├── storage.rs      # SQLite operations
│   └── models.rs       # Data types
├── Cargo.toml
└── cbindgen.toml       # C header generation
```

The core compiles to:
- `libnotetaker.dylib` (macOS), `libnotetaker.a` (iOS)
- `notetaker.dll` (Windows)
- `libnotetaker.so` (Linux, Android)

### Component Architecture

```
┌─────────────────────────────────┐
│        Platform Native UI        │
│  (SwiftUI / WinUI / GTK / JC)   │
├─────────────────────────────────┤
│            C FFI Bridge          │
├─────────────────────────────────┤
│        notetaker-core (Rust)     │
│                                  │
│  ┌───────────┐  ┌────────────┐  │
│  │  Audio     │  │ Transcribe │  │
│  │  Capture   │──│ (whisper)  │  │
│  └───────────┘  └────────────┘  │
│  ┌───────────┐  ┌────────────┐  │
│  │ Summarize  │  │  Storage   │  │
│  │ (LLM API)  │  │  (SQLite)  │  │
│  └───────────┘  └────────────┘  │
└─────────────────────────────────┘
```

## Monorepo Structure

```
ai-note-taker/
├── core/                    # Shared Rust library
│   ├── Cargo.toml
│   ├── cbindgen.toml
│   └── src/
├── apple/                   # macOS + iOS (Xcode project)
│   ├── Shared/              # Shared Swift code
│   ├── macOS/               # macOS-specific
│   ├── iOS/                 # iOS-specific
│   └── Package.swift
├── windows/                 # WinUI 3 / C#
│   └── NoteHelper.sln
├── linux/                   # GTK4 Rust app
│   ├── Cargo.toml
│   └── src/
├── android/                 # Kotlin + Compose
│   └── app/
├── models/                  # Whisper model files (gitignored, downloaded)
├── docs/
└── scripts/                 # Build scripts
```

## Core Library API (C FFI)

```c
// Lifecycle
NoteHandle* note_init(const char* db_path, const char* model_path);
void note_destroy(NoteHandle* handle);

// Recording
int32_t note_start_recording(NoteHandle* handle, const char* device_id);
int32_t note_stop_recording(NoteHandle* handle);
int32_t note_pause_recording(NoteHandle* handle);

// Transcription callbacks
typedef void (*TranscriptCallback)(const char* text, double timestamp, void* ctx);
void note_set_transcript_callback(NoteHandle* handle, TranscriptCallback cb, void* ctx);

// Summarization
char* note_summarize(NoteHandle* handle, int64_t meeting_id);
void note_free_string(char* s);

// Storage
char* note_list_meetings(NoteHandle* handle, int32_t limit, int32_t offset);
char* note_get_meeting(NoteHandle* handle, int64_t meeting_id);
int32_t note_delete_meeting(NoteHandle* handle, int64_t meeting_id);
char* note_export_meeting(NoteHandle* handle, int64_t meeting_id, const char* format);

// Settings
int32_t note_set_setting(NoteHandle* handle, const char* key, const char* value);
char* note_get_setting(NoteHandle* handle, const char* key);

// Audio devices
char* note_list_audio_devices(NoteHandle* handle);
```

## Core Features

### 1. Audio Capture

- Capture from default input device (microphone) via platform audio APIs
- Optional system audio capture (loopback) where supported
- Real-time audio streaming to transcription engine
- Audio format: 16kHz mono f32 PCM (Whisper's native format)
- Configurable input device selection
- Audio level metering for UI feedback

### 2. Transcription

**Local mode (default):**
- whisper.cpp via whisper-rs Rust bindings in core library
- Bundled tiny model (~75MB), downloadable base/small/medium models
- Streaming transcription with ~3-5 second chunks
- Language auto-detection with manual override

**Cloud mode (optional):**
- OpenAI Whisper API as fallback
- User provides their own API key
- Used when local transcription quality is insufficient

### 3. Meeting Notes & Summarization

- Live transcript view during recording
- Post-meeting summary via Claude API (user provides key)
- Extracts: key points, action items, decisions
- Editable notes after generation
- Export to Markdown, plain text, JSON

### 4. Storage

SQLite database (via rusqlite in core):

```sql
CREATE TABLE meetings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL DEFAULT 'Untitled Meeting',
    started_at TEXT NOT NULL,
    ended_at TEXT,
    duration_secs INTEGER,
    summary TEXT
);

CREATE TABLE transcript_segments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    meeting_id INTEGER NOT NULL REFERENCES meetings(id),
    timestamp_ms INTEGER NOT NULL,
    text TEXT NOT NULL,
    confidence REAL
);

CREATE TABLE notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    meeting_id INTEGER NOT NULL REFERENCES meetings(id),
    content TEXT NOT NULL,
    note_type TEXT NOT NULL DEFAULT 'summary',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

### 5. Stealth Features

**Window exclusion from screen capture:**
- macOS: `window.sharingType = .none` on NSWindow
- Windows: `SetWindowDisplayAffinity(hWnd, WDA_EXCLUDEFROMCAPTURE)`
- Linux: Best-effort via compositor hints (limited support)
- Android: `FLAG_SECURE` on window
- iOS: Content exclusion via `UIScreen` monitoring

**Process name obfuscation:**
- macOS: Bundle as `com.system.helper`, process name `sysproc`
- Windows: Executable named `svchost-user.exe`
- Linux: Binary named `dbus-monitor-session`
- Android: Package name `com.system.service.helper`
- iOS: App name `System Utility` with generic icon

## Platform-Specific Implementation

### macOS (SwiftUI)
- Audio: AVAudioEngine + CoreAudio for mic, ScreenCaptureKit for system audio
- Stealth: `NSWindow.sharingType = .none`
- Menu bar app with popover UI
- Notarization-ready
- Core lib: linked as `.dylib` via Swift package

### iOS (SwiftUI)
- Audio: AVAudioEngine with AVAudioSession `.record` category
- Background audio recording via background mode
- Core lib: linked as `.a` (static)
- Share extension for quick access

### Windows (WinUI 3 / C#)
- Audio: NAudio + WASAPI (mic + loopback)
- Stealth: `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`
- System tray app
- Core lib: P/Invoke to `notetaker.dll`
- MSIX installer

### Linux (GTK4 + Rust)
- Audio: PipeWire (preferred) or PulseAudio via cpal
- Stealth: Compositor-dependent, best-effort
- System tray via libappindicator
- Core lib: direct Rust dependency (no FFI needed)
- AppImage + Flatpak

### Android (Jetpack Compose + Kotlin)
- Audio: AudioRecord API with VOICE_COMMUNICATION source
- Foreground service for recording
- Stealth: `FLAG_SECURE`
- Core lib: JNI via `libnotetaker.so`
- Play Store + APK

## UI Design

### Screens (consistent across platforms, native look & feel)

1. **Main / Recording View**
   - Large record/stop button (platform-native styling)
   - Live transcript scrolling below
   - Meeting title (editable, auto-generated from first words)
   - Timer showing duration
   - Audio level indicator (waveform or VU meter)

2. **Notes View**
   - AI-generated summary at top
   - Collapsible sections: Key Points, Action Items, Decisions
   - Full transcript below
   - Edit mode toggle
   - Share/export button

3. **History View**
   - List of past meetings with date, title, duration
   - Search bar (full-text search via SQLite FTS5)
   - Swipe to delete (mobile), right-click menu (desktop)

4. **Settings View**
   - Audio input device picker
   - Transcription: local vs cloud toggle
   - API keys (stored in platform keychain)
   - Whisper model selector with download UI
   - App appearance (name, theme)
   - Data management (export all, clear)

## Error Handling

- Audio device unavailable: guide user to grant permissions, show device selector
- Transcription failure: buffer audio, retry, offer cloud fallback
- API key invalid/missing: clear error, link to settings
- Storage full: warn with usage stats, suggest export + cleanup
- Model not downloaded: prompt download with size info and progress

## Security

- All data local by default, SQLite with SQLCipher encryption
- API keys in platform keychain (Keychain/Credential Manager/Secret Service/Keystore)
- No telemetry, no analytics, no network unless user enables cloud features
- Audio never leaves device unless cloud transcription explicitly enabled by user

## Build & Distribution

- Rust core: `cargo build` with cross-compilation targets
- macOS/iOS: Xcode build, archive for distribution
- Windows: MSBuild / dotnet publish
- Linux: cargo build (direct Rust app)
- Android: Gradle build with NDK for native lib
- CI: GitHub Actions matrix for all platforms
