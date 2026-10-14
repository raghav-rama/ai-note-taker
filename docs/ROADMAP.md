# AI Note Taker -- Product Roadmap

## Platform Priority

**Build order: Linux first, then macOS, Windows, Android, iOS last.**

| Priority | Platform | Rationale |
|----------|----------|-----------|
| 1st | Linux (GTK4) | Only app that compiles and links today (~90s CI). Direct Rust crate dependency on `notetaker-core` -- no C FFI bridge needed. Three small fixes get it producing real transcripts. |
| 2nd | macOS | User is on Darwin. Highest-value desktop target. Requires creating an Xcode project from scratch (medium effort), but Swift source files are complete. |
| 3rd | Windows | `.sln` and `.csproj` exist and are structurally valid. One path fix unblocks CI. P/Invoke signatures already match the C header. |
| 4th | Android | Complete Compose UI exists but requires a JNI bridge layer (~500 lines Rust) that does not exist anywhere in the codebase. Gradle wrapper is trivially generated. |
| 5th | iOS | Cannot distribute downloadable artifacts without Apple Developer Program ($99/year) + TestFlight. Compile-only CI validation until macOS ships and signing is set up. |

**Note:** Phase 1 produces a Linux binary. That will not run on the user's Darwin machine. The purpose of Phase 1 is to prove the shared core pipeline (audio capture, whisper transcription, SQLite storage, UI display) end-to-end in CI. macOS is the first thing Phase 3 ships.

---

## Quick Wins (Do Immediately, All Size S)

These items can each be done by a single agent with no cross-file coordination. Every one of them fixes a real bug or unblocks a real build.

| # | Type | File | Change |
|---|------|------|--------|
| QW-1 | ci | `.github/workflows/build.yml:218` | Fix Linux upload path: `target/release/notetaker-linux` -> `target/release/dbus-monitor-session` |
| QW-2 | ci | `.github/workflows/build.yml:163` | Fix Windows `.sln` path: `windows/NoteHelper.sln` -> `windows/NoteHelper/NoteHelper.sln`; fix dotnet build command to match |
| QW-3 | ci | `.github/workflows/build.yml:244` | Fix Windows upload path: `windows/bin/x64/Release/` -> `windows/NoteHelper/bin/x64/Release/net8.0-windows10.0.22621.0/**/*` |
| QW-4 | platform:android | `android/` | Run `gradle wrapper --gradle-version 8.7` and commit `gradlew`, `gradlew.bat`, `gradle/wrapper/*` |
| QW-5 | core | `core/src/lib.rs:214-226` | Fix state leak: clear `current_meeting_id` and delete orphan meeting row on every error path after line 214 |
| QW-6 | core | `core/src/lib.rs:295-302` | Fix stop order: call `transcriber.stop()` BEFORE dropping `AudioCapture` so the transcriber can drain the mpsc channel |
| QW-7 | core | `core/src/lib.rs:220` | Wire audio level callback: pass `Some(level_callback)` instead of `None` to `AudioCapture::new`; store last level in `NoteHandle`; add `note_get_audio_level` FFI function |
| QW-8 | core | `core/Cargo.toml` | Fix reqwest for cross-compilation: `reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"], optional = true }` |
| QW-9 | platform:android | `android/.../SettingsScreen.kt` | Fix API key mismatch: change `claude_api_key` to `anthropic_api_key` to match `core/src/lib.rs:420` |
| QW-10 | platform:android | `android/.../CoreBridge.kt` | Fix timestamp units: `onTranscript` receives seconds, multiply by 1000 for `timestampMs` field |
| QW-11 | platform:ios | `apple/Shared/Models/Meeting.swift` | Fix field name: Summary struct `overview` -> `summary` to match `core/src/models.rs` |
| QW-12 | core | `core/src/storage.rs` | Fix FTS5 injection: wrap user query in double quotes before passing to MATCH |
| QW-13 | docs | `CLAUDE.md`, `README.md` | Remove false claims: "Cloud (OpenAI Whisper API) is opt-in fallback" (does not exist); "SQLCipher encryption" (plain SQLite) |
| QW-14 | platform:macos | `apple/NoteTaker/Models/MeetingViewModel.swift` | Fix settings key: `model_name` -> `whisper_model` to match other platforms |

---

## Phase 1: Foundation -- One Platform End-to-End

**Goal:** Download the `app-linux` artifact from a `main` CI run, run the binary on Ubuntu 24.04, speak into the mic, and see live transcript segments appear in the UI. CI also runs a headless WAV-to-text test that asserts whisper.cpp produces real output.

**Platforms covered:** Linux only (core benefits all platforms).

### Work Items

| # | Type | Effort | Files | Description |
|---|------|--------|-------|-------------|
| P1-1 | core | M | `core/src/bin/transcribe_wav.rs` (new), `core/Cargo.toml` | **Headless WAV CLI.** New binary target that takes a WAV file path and a model path, reads the WAV, runs through the resampler and `LocalTranscriber`, prints transcript to stdout. This is the only way to verify transcription in CI (runners have no microphone). |
| P1-2 | ci | S | `.github/workflows/build.yml` | **Model caching + WAV test job.** Download `ggml-tiny.bin` via `scripts/download-model.sh tiny` with `actions/cache` keyed on `whisper-model-tiny`. Download `jfk.wav` from whisper.cpp test fixtures. Run `transcribe-wav jfk.wav models/ggml-tiny.bin` and assert output contains "country". |
| P1-3 | core | S | `core/src/lib.rs`, `core/src/transcribe.rs` | **Model path contract.** Change `note_init`'s `model_path` parameter to accept a models directory. `note_start_recording` reads the `whisper_model` setting from the DB (default: `tiny`) and composes `{model_dir}/ggml-{name}.bin`. This matches what Linux `main.rs`, Android, and iOS already pass. macOS passes a file path and must be updated. |
| P1-4 | platform:linux | S | `linux/src/core_bridge.rs:72-83` | **Wire real transcription.** Replace the drain loop with `LocalTranscriber::new(model_path, audio_rx, db, meeting_id, callback)`. Create a `TranscriptCallback` that converts `(&str, f64)` into `TranscriptSegment` and sends on `transcript_tx`. Store `Arc<Mutex<Option<LocalTranscriber>>>` in `CoreBridge` so `stop_recording` can call `.stop()`. |
| P1-5 | platform:linux | S | `linux/src/main.rs:25` | **Fix model path.** Currently passes a directory. After P1-3, this is correct. Ensure the path resolves to `~/.local/share/session-monitor/models/` and falls back to `./models/` if the XDG path does not exist. |
| P1-6 | ci | S | `.github/workflows/build.yml` | **Bundle model in Linux artifact.** After building the Linux app, copy `models/ggml-tiny.bin` alongside the binary. Upload both in the `app-linux` artifact. Add a `README-run.txt` saying `chmod +x dbus-monitor-session && mkdir -p models && cp ggml-tiny.bin models/ && ./dbus-monitor-session`. |
| P1-7 | ci | S | `.github/workflows/build.yml` | **Flip Linux CI to real failures.** For `app-linux` only: remove `\|\| echo ::warning` from the build step. Change `if-no-files-found: warn` to `if-no-files-found: error` on upload. Fix the upload path per QW-1. Leave other platforms as `warn` until they are real. |
| P1-8 | core | S | `core/src/lib.rs:214-226` | **State leak fix** (QW-5). On `AudioCapture::new` failure, `LocalTranscriber::new` failure, or `capture.start()` failure: clear `current_meeting_id`, delete the orphan meeting row via `h.db.delete_meeting(meeting_id)`, then return error. |
| P1-9 | core | S | `core/src/lib.rs:295-302` | **Stop order fix** (QW-6). Call `transcriber.stop()` first (which joins its thread and flushes the internal buffer), then drop `AudioCapture` (which closes `audio_tx`). Current order drops capture first, closing the sender, then the transcriber drains its internal buffer but never sees queued mpsc chunks. |
| P1-10 | platform:linux | S | `linux/src/views/history.rs` | **Wire history-to-notes navigation.** Add `connect_row_activated` handler on the history list box. Set `current_meeting_id` in the notes view. Make the ViewSwitcher navigate to the Notes tab (or push a detail page). Without this, the notes view is unreachable. |

### Dependencies and Ordering

```
QW-1 ──────────────────────────────────────────────> P1-7 (flip CI)
P1-3 (model path contract) ──> P1-4 (wire transcription) ──> P1-5 (fix linux path)
P1-1 (WAV CLI) ──> P1-2 (CI test job)
P1-8, P1-9 (core fixes) -- independent, can be parallel
P1-6 (bundle model) -- depends on model cache from P1-2
P1-10 (history navigation) -- independent
```

### Definition of Done

- [ ] `cargo build --release -p notetaker-linux` succeeds in CI
- [ ] `app-linux` artifact contains `dbus-monitor-session` binary + `ggml-tiny.bin`
- [ ] CI `transcribe-wav` job passes (output from jfk.wav contains expected text)
- [ ] Downloaded binary on Ubuntu 24.04 with GTK4 4.12+ and libadwaita 1.4+: launch, grant mic permission, press Record, speak, see timestamped transcript segments appear in the recording view
- [ ] Press Stop, navigate to History, see the meeting listed, click through to Notes and see the full transcript
- [ ] `note_start_recording` failure does not leave handle in broken state (no orphan meeting, no "already active" on retry)

---

## Phase 2: Core Completeness

**Goal:** All core library features work, are tested, and have a clean FFI surface. Cloud summarization works. Error reporting exists. The core is ready for all five platform bridges to consume.

**Platforms covered:** Core (all platforms benefit). Linux app continues as the integration testbed.

### Work Items

| # | Type | Effort | Files | Description |
|---|------|--------|-------|-------------|
| P2-1 | core | M | `core/src/lib.rs` | **Complete FFI surface.** Add missing functions: `note_resume_recording`, `note_search_meetings` (expose FTS5), `note_get_segments` (return transcript segments for a meeting), `note_update_meeting_title`, `note_get_audio_level`. Update `core/include/notetaker.h` via cbindgen. |
| P2-2 | core | S | `core/src/lib.rs` | **Error reporting.** Add `note_last_error() -> *mut c_char` that returns the most recent error message (stored in thread-local or in `NoteHandle`). Add `note_set_log_callback` for structured logging. Without this, every FFI consumer is flying blind. |
| P2-3 | core | M | `core/tests/` (new) | **Test suite.** Tests for: storage (schema creation, CRUD, FTS5 queries, cascade delete, export formats), models (serde round-trip), FFI lifecycle (init/destroy, null pointer handling, double-stop), transcriber (chunk processing with a mock audio source). Use an in-memory SQLite DB for storage tests. |
| P2-4 | core | S | `core/Cargo.toml`, `core/src/summarize.rs` | **Enable and fix cloud summarization.** Enable `cloud` feature in CI builds. Fix reqwest: `default-features = false, features = ["json", "rustls-tls"]` (QW-8). Add 30-second request timeout. Fix the markdown fence stripping on API response. Store structured summary fields (key_points, action_items, decisions) in the database, not just the flat summary string. |
| P2-5 | core | S | `core/src/lib.rs` | **Push-audio FFI entry point.** Add `note_push_audio(handle, samples: *const f32, count: usize) -> i32` so native layers can feed system audio (ScreenCaptureKit, WASAPI loopback) into the transcription pipeline. This is the architectural prerequisite for system audio on every platform. |
| P2-6 | core | M | `core/src/transcribe.rs` | **Improve streaming quality.** Add 1-second overlap between 4-second chunks (sliding window). Enable `set_no_context(false)` to carry Whisper state between chunks. Add a simple energy-based VAD to skip silence. Create `WhisperState` once per session instead of per chunk. Cap the internal buffer at 30 seconds with backpressure. |
| P2-7 | core | S | `core/src/transcribe.rs` | **Cache whisper model across recordings.** Move `WhisperContext` into `NoteHandle` (loaded lazily on first recording start or eagerly in a background thread after `note_init`). Eliminates multi-second blocking on every `note_start_recording`. |
| P2-8 | core | S | `core/src/storage.rs` | **Schema versioning.** Add `PRAGMA user_version` check. Create a `migrations` array. On `Database::open`, compare version and run pending migrations. Prevents data loss on future schema changes. |
| P2-9 | core | S | `core/src/transcribe.rs` | **Configurable transcription parameters.** Read `whisper_language` (default: `en`), `whisper_threads` (default: 4) from settings table. Allow `auto` for language to enable Whisper's auto-detection. |
| P2-10 | platform:linux | S | `linux/src/views/settings.rs` | **Model download handler.** Wire the "Download Selected Model" button to an HTTP download from `huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{model}.bin`. Show a progress bar via `gio::OutputStream` or a simple polling loop. Write to the models directory. |
| P2-11 | platform:linux | M | `linux/src/` (multiple files) | **Linux app polish.** Wire summarize to real `CloudSummarizer` (enable `cloud` feature in `linux/Cargo.toml`). Add `GtkFileDialog` for export. Add `AdwToast` for error feedback. Wire FTS5 search in history. Add pause/resume button. Fix `set_hide_on_close` by adding `GtkApplication::activate` re-present logic. |

### Dependencies and Ordering

```
P2-1 (FFI surface) -- unblocks Phase 3 platform bridges
P2-2 (error reporting) -- should land before Phase 3 bring-up
P2-4 (cloud feature) -- must fix reqwest BEFORE any mobile CI enables cloud
P2-5 (push-audio) -- prerequisite for system audio in Phase 3/4
P2-6, P2-7, P2-9 -- transcription quality, independent of each other
P2-3 (tests) -- can run in parallel with everything
P2-8 (schema versioning) -- should land before any public release
```

### Definition of Done

- [ ] `cargo test -p notetaker-core` runs 20+ tests covering storage, FFI lifecycle, model serde
- [ ] `note_last_error` returns meaningful messages for: missing model file, no microphone, DB locked, no API key
- [ ] `note_summarize` with a valid API key returns structured JSON with `summary`, `key_points`, `action_items`, `decisions`
- [ ] `note_search_meetings("meeting")` returns FTS5 results across transcript text
- [ ] `note_push_audio` feeds audio into the transcription pipeline (verified by headless CLI test with raw PCM input)
- [ ] Transcription quality: chunk boundaries do not produce garbled text (overlap + context)
- [ ] Linux app: summarize button produces AI summary, export writes a file, model download works

---

## Phase 3: Platform Expansion

**Goal:** All five platforms produce real, downloadable artifacts in CI. Each app launches, records audio, displays transcripts, and shows meeting history.

**Platforms covered:** macOS, Windows, Android, iOS (compile-only).

### Work Items

| # | Type | Effort | Files | Description |
|---|------|--------|-------|-------------|
| P3-1 | platform:macos | M | `apple/project.yml` (new) | **Create Xcode project via XcodeGen.** Define two targets: macOS (`NoteTaker/SystemHelper`) and iOS (`iOS/Utility`). Configure `SWIFT_INCLUDE_PATHS` for `Shared/Core/include`, `LIBRARY_SEARCH_PATHS` for `dist/core/macos` and `dist/core/ios`, `OTHER_LDFLAGS` for `-lnotetaker -lc++ -framework Foundation -framework Accelerate -framework Security`. Add `brew install xcodegen` and `xcodegen generate` to CI before `xcodebuild`. |
| P3-2 | platform:macos | S | `apple/NoteTaker/Core/CoreBridge.swift` | **Fix C FFI import.** Add `import NoteCore` (using the shared modulemap at `apple/Shared/Core/include/module.modulemap`) instead of bare symbol references. Alternatively, define a bridging header. |
| P3-3 | platform:macos | M | `apple/NoteTaker/`, `apple/Shared/` | **Unify model types.** Migrate macOS to use `Shared/Models/Meeting.swift` Codable types. Remove duplicate `Meeting`/`TranscriptSegment` definitions from `MeetingViewModel.swift`. Fix JSON contract: `note_get_meeting` returns flat Meeting, use `note_export_meeting(format=json)` for the detail view with segments. Unify settings keys to `whisper_model`. |
| P3-4 | platform:macos | S | `apple/project.yml`, `scripts/build-core.sh` | **macOS: static lib + Metal.** Switch macOS core from `.dylib` to `.a` (simpler -- no embed framework phase, no rpath). Drop x86_64 target (Apple Silicon only since 2020 -- avoids the metal-on-x86_64 problem). Pass `--features metal,cloud` for macOS core builds. Update CI scheme, derive data path, and artifact upload path. |
| P3-5 | ci | S | `.github/workflows/build.yml` | **Fix macOS/iOS CI jobs.** After P3-1: install xcodegen, run `xcodegen generate`, fix scheme names and output paths, fix artifact upload to use `-derivedDataPath` output. Add `CODE_SIGN_IDENTITY=-` for ad-hoc signing. Flip to `if-no-files-found: error`. |
| P3-6 | platform:windows | S | `windows/NoteHelper/NoteHelper.csproj` | **Fix DLL copy for CI.** Add a second `ItemGroup` that copies `dist/core/windows/notetaker.dll` to output when building in CI (detect via environment variable or MSBuild property). Fix the existing local-build path too. |
| P3-7 | ci | S | `.github/workflows/build.yml` | **Fix Windows CI.** Apply QW-2 and QW-3. After P3-6: add step to copy `dist/core/windows/notetaker.dll` to the location `.csproj` expects. Flip to `if-no-files-found: error`. |
| P3-8 | platform:windows | S | `windows/NoteHelper/` | **Singleton CoreBridge.** Move CoreBridge instantiation from per-page to `App.xaml.cs`. Pass shared instance to pages via DI or a static property. Prevents multiple `note_init` calls and SQLite locking. |
| P3-9 | core | L | `core/src/android_jni.rs` (new), `core/Cargo.toml` | **Android JNI bridge.** Add `jni = "0.21"` dependency gated on `#[cfg(target_os = "android")]`. Implement all 14 JNI entry points (`Java_com_system_service_helper_native_CoreBridge_nativeInit`, etc.) that delegate to existing `note_*` C functions. Implement `onTranscript` callback via stored `JavaVM` global reference + `AttachCurrentThread`. Approximately 500 lines of Rust. |
| P3-10 | platform:android | S | `android/` | **Generate Gradle wrapper and fix CI.** Commit `gradlew`, `gradlew.bat`, `gradle/wrapper/`. Fix CI to use correct path. Add step to copy jniLibs from `dist/core/android/jniLibs/`. Fix `nativeStartRecording` to pass null (not empty string) for default device. Fix timestamp units (QW-10). Fix API key name (QW-9). |
| P3-11 | ci | S | `.github/workflows/build.yml` | **Split Android core into its own CI job.** Currently `core-linux` builds both Linux and Android targets. An Android cross-compile regression kills the Linux core artifact. Create `core-android` job on `ubuntu-latest` with NDK setup. `app-android` depends on `core-android` instead of `core-linux`. |
| P3-12 | platform:ios | S | `apple/project.yml`, CI | **iOS compile-only CI.** Add iOS target to the XcodeGen project. Build for simulator (`-destination 'platform=iOS Simulator,name=iPhone 15'`). Upload no artifact (compile-only validation). Fix `overview` -> `summary` (QW-11). Fix iOS deployment target to 17.0. |
| P3-13 | platform:macos | S | `apple/NoteTaker/Views/SettingsView.swift` | **macOS model download.** Replace fake progress loop with real `URLSession.download` task. Download from HuggingFace URL. Show progress via `Progress` + `ProgressView`. Write to app support models directory. |

### Dependencies and Ordering

```
P3-1 (XcodeGen project) ──> P3-2 (FFI import) ──> P3-3 (unify models)
P3-1 ──> P3-4 (static lib + Metal) ──> P3-5 (fix macOS CI)
P3-1 ──> P3-12 (iOS compile-only)
P3-6 (DLL copy) ──> P3-7 (fix Windows CI)
P3-8 (singleton CoreBridge) -- independent
P3-9 (JNI bridge) ──> P3-10 (Gradle + fixes) ──> P3-11 (split CI)
P3-13 -- after P3-5
```

### Definition of Done

- [ ] All five platform CI jobs produce real artifacts (iOS: compile-only pass)
- [ ] macOS: download `.app`, `xattr -cr`, launch, record, see transcript, summarize
- [ ] Windows: download folder, run `.exe`, record, see transcript
- [ ] Android: download `.apk`, sideload, grant mic permission, record, see transcript
- [ ] Linux: continues to work from Phase 1
- [ ] iOS: CI compile passes with zero warnings from FFI type mismatches

---

## Phase 4: Polish and Packaging

**Goal:** Each platform has a proper installer/package, the UX is polished, stealth features are hardened, and the product feels like a real app rather than a development build.

**Platforms covered:** All five.

### Work Items

| # | Type | Effort | Files | Description |
|---|------|--------|-------|-------------|
| P4-1 | ci | M | `.github/workflows/release.yml` (new) | **Release workflow.** Trigger on `v*` tags. Build all platforms. Create GitHub Release. Attach artifacts with platform-appropriate names (`SystemHelper-macos-arm64.zip`, `SystemHelper-windows-x64.zip`, `dbus-monitor-session-linux-x64.tar.gz`, `SystemService-android.apk`). Include `ggml-tiny.bin` model in each artifact. |
| P4-2 | platform:macos | M | CI, scripts | **macOS .dmg packaging.** Use `create-dmg` in CI to produce a `.dmg` with drag-to-Applications layout. Ad-hoc sign with `codesign -s -`. Include first-run instructions for Gatekeeper (`xattr -cr` or right-click > Open). |
| P4-3 | platform:windows | M | CI, scripts | **Windows portable zip.** Package the build output + `notetaker.dll` + model into a self-contained zip. Add a `README-run.txt`. Skip MSIX for now (requires signing cert). Add `Assets/` directory with a placeholder icon to fix the build warning. |
| P4-4 | platform:linux | M | CI, scripts | **Linux AppImage.** Use `linuxdeploy` + `linuxdeploy-plugin-gtk` to create an AppImage that bundles GTK4/libadwaita. Add a `.desktop` file and icon. The AppImage runs on any glibc 2.35+ distro without installing GTK4 dev packages. |
| P4-5 | platform:android | S | `android/app/build.gradle.kts` | **Android release signing.** Add `signingConfigs` block reading keystore from CI secrets. Switch `assembleDebug` to `assembleRelease`. The release APK can be sideloaded without debug mode. |
| P4-6 | core | M | `core/src/lib.rs`, platform bridges | **System audio capture.** macOS: add `ScreenCaptureKit` audio capture in `apple/NoteTaker/Core/AudioManager.swift`, feed PCM via `note_push_audio`. Windows: add WASAPI loopback capture in a new `AudioLoopback.cs`, feed via P/Invoke `note_push_audio`. Linux: check if PipeWire `.monitor` devices appear in cpal device enumeration; if so, expose them in the device picker (no new code needed). Android: no system audio API available. |
| P4-7 | platform:all | S | All settings views | **Secure API key storage.** macOS: `KeychainHelper` (iOS already has one -- share it). Windows: `CredWrite`/`CredRead` via P/Invoke. Linux: `libsecret` via `secret-service` crate. Android: `EncryptedSharedPreferences`. Remove plaintext storage from SQLite settings table. |
| P4-8 | platform:all | S | All recording views | **Meeting title from transcript.** After the first 3 transcript segments, auto-generate a title from the first 8 words. Add `note_update_meeting_title` FFI call (from P2-1). Allow manual editing in all UIs. |
| P4-9 | platform:all | S | Multiple | **Model download UX.** Each platform: settings view shows available models (tiny/base/small) with size info. Download button fetches from HuggingFace with progress. Store in platform-appropriate app data directory. Fall back to bundled tiny model if selected model is missing. |
| P4-10 | core | S | `core/src/transcribe.rs` | **Hardware acceleration in CI builds.** macOS: `--features metal,cloud`. Windows: CPU-only (CUDA requires runtime detection). Linux: CPU-only (Vulkan optional). Android: CPU-only. Update `scripts/build-core.sh` to pass platform-appropriate features. |
| P4-11 | platform:all | S | All views | **Error UX.** Wire `note_last_error` (from P2-2) into platform-native error displays: `AdwToast` (Linux), `NSAlert`/SwiftUI `.alert` (macOS/iOS), `ContentDialog` (Windows), `Snackbar` (Android). Replace silent failures with visible feedback. |

### Definition of Done

- [ ] `v0.1.0` tag produces a GitHub Release with downloadable artifacts for macOS, Windows, Linux, Android
- [ ] macOS `.dmg` opens Finder, user drags to Applications, first launch works after Gatekeeper bypass
- [ ] Windows `.zip` extracts and runs without .NET or SDK installation
- [ ] Linux AppImage runs on Ubuntu 22.04+ without installing GTK4
- [ ] Android signed APK installs via `adb install` or file manager
- [ ] System audio capture works on macOS and Windows (both sides of a call are transcribed)
- [ ] API keys stored in platform keychain, not plaintext SQLite
- [ ] Model download works from settings on all platforms

---

## Phase 5: Distribution

**Goal:** Non-technical users can find, download, install, and run the app. Code signing eliminates OS security warnings. The download page explains what the app does and how to install it.

**Platforms covered:** macOS, Windows, Linux, Android. iOS via TestFlight if Apple Developer account is available.

### Work Items

| # | Type | Effort | Files | Description |
|---|------|--------|-------|-------------|
| P5-1 | ci | L | `.github/workflows/release.yml` | **macOS code signing + notarization.** Store Apple Developer certificate and provisioning profile as GitHub secrets. Use `apple-actions/import-codesign-certs` and `xcrun notarytool`. Sign the `.app`, create a notarized `.dmg`. This eliminates the Gatekeeper warning entirely. Requires Apple Developer Program ($99/year). |
| P5-2 | ci | M | `.github/workflows/release.yml` | **Windows code signing.** Use `signtool` with an EV or OV code signing certificate stored as a GitHub secret. Sign the `.exe` and `.dll`. This reduces SmartScreen warnings. Certificate cost: $200-400/year from a CA. |
| P5-3 | ci | S | `.github/workflows/release.yml` | **Android Play Store preparation.** Generate a release signing key. Build AAB (Android App Bundle) instead of APK. The AAB can be uploaded to Play Store or sideloaded via `bundletool`. Google Play listing requires screenshots and description. |
| P5-4 | ci | M | `docs/`, GitHub Pages | **Download page.** GitHub Pages site with: product description, per-platform download buttons linking to latest GitHub Release assets, installation instructions per platform, screenshots. Use the generic "System Helper" / "Session Monitor" naming. |
| P5-5 | ci | L | Platform-specific | **iOS TestFlight distribution.** Requires Apple Developer Program. Set up App Store Connect, configure CI to build and upload to TestFlight. Invite testers via public link. This is the only path to iOS distribution without enterprise certificates. |
| P5-6 | platform:all | L | Multiple | **Auto-update mechanism.** macOS: Sparkle framework. Windows: check GitHub Releases API on launch, prompt to download. Linux: AppImageUpdate integration. Android: in-app version check against GitHub Releases. All: opt-in, no silent updates (privacy-first). |
| P5-7 | core | S | `core/src/lib.rs` | **Version reporting.** Add `note_version() -> *const c_char` returning the crate version. Display in all settings views. Include in GitHub Release notes for support. |

### Definition of Done

- [ ] macOS `.dmg` opens without any Gatekeeper warning (notarized)
- [ ] Windows `.exe` runs without SmartScreen warning (signed)
- [ ] Download page at `username.github.io/ai-note-taker` has working download links
- [ ] Non-technical user can follow platform-specific instructions to install and run
- [ ] iOS available via TestFlight public link (if Apple Developer account exists)

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| **Stealth process names trigger antivirus** | `svchost-user.exe` on Windows mimics a system process; SmartScreen will flag it. `dbus-monitor-session` on Linux may confuse security tools. | Rename to something generic but not a system-process impersonation: `syshelper.exe` (Windows), `session-helper` (Linux). Consider a `--stealth-names` compile flag that applies the more aggressive names only when explicitly opted in. |
| **CPU-only Whisper inference too slow for real-time** | Without Metal/CUDA, the `base` model takes 2-8 seconds per 4-second chunk. Latency grows unboundedly. | Phase 1 ships with `tiny` model only (fast enough on CPU). Phase 4 enables Metal on macOS. Add backpressure in Phase 2 (cap internal buffer at 30s). Document that `tiny` is the only recommended model without GPU acceleration. |
| **Unsigned macOS .app blocked by Gatekeeper** | Users cannot open the app without `xattr -cr` or right-click > Open workaround. | Phase 4: ad-hoc signing + README instructions. Phase 5: full notarization with Apple Developer account. |
| **iOS distribution impossible without Apple Developer Program** | No `.ipa` sideloading for arbitrary users. TestFlight requires $99/year. | Defer iOS to compile-only CI validation until Phase 5. macOS is the primary Apple platform. Document this constraint in the roadmap. |
| **Whisper model distribution inflates artifact size** | `ggml-tiny.bin` is 75MB. `base` is 142MB. GitHub artifact storage has quotas. | Bundle only `tiny` in CI artifacts. In-app download for larger models (Phase 4). Use `actions/cache` for the model in CI to avoid re-downloading. |
| **whisper.cpp cmake build is slow and fragile in CI** | 5-10 minutes per target. Can fail on runner environment changes. | Use `Swatinem/rust-cache@v2` (already configured). Cache the whisper.cpp build artifacts. Pin the runner OS version if builds become flaky. |
| **Android 12+ mic privacy indicator** | Persistent status bar dot when mic is active. Cannot be suppressed. Partially undermines stealth. | Document as a known limitation. The generic app name and icon still provide meaningful obfuscation. |
| **Five platform bridges must stay in sync with C header** | Every FFI change requires updating Swift (x2), C#, Kotlin, and Rust (GTK) wrappers. | Phase 2 completes the FFI surface before Phase 3 starts platform work. Linux GTK bridge uses Rust API directly (no C header sync needed). Consider generating platform bridge code from the cbindgen header in the future. |
| **No crash/error telemetry** | Bugs in shipped artifacts are invisible. | Phase 2 adds `note_last_error` and `note_set_log_callback`. Phase 4 wires these into platform-native error UX. An opt-in crash reporter is a future consideration, not in this roadmap. |
| **Multiple `NoteHandle` instances (Windows)** | Each WinUI page creates its own `CoreBridge` -> `note_init`, opening the same SQLite DB multiple times -> `SQLITE_BUSY`. | Phase 3 (P3-8): singleton CoreBridge in `App.xaml.cs`. |

---

## What NOT to Build (YAGNI)

These items are explicitly deferred. Do not spend time on them in Phases 1-5.

| Item | Why Not |
|------|---------|
| **OpenAI Whisper cloud transcription** | Zero implementation exists. The CLAUDE.md claim is false. Fix the documentation instead. Local whisper.cpp is the product's value proposition ("local-first, privacy-first"). |
| **SQLCipher database encryption** | README claims it but the code uses plain `rusqlite` with `bundled` feature. Switching to `bundled-sqlcipher` requires key management UX on every platform. Fix the README to say "local SQLite" and defer encryption to post-v1. |
| **iOS distribution (Phases 1-4)** | Requires $99/year Apple Developer Program, provisioning profiles, TestFlight setup. Compile-only CI is sufficient until macOS is shipped and an Apple account is available. |
| **Windows MSIX packaging** | Requires a code signing certificate. Portable `.zip` is sufficient for sideloading. MSIX is only needed for Microsoft Store distribution. |
| **Flatpak / .deb packaging for Linux** | AppImage is the most portable single-file distribution. Flatpak requires a Flathub submission process. `.deb` only covers Debian/Ubuntu. Defer to post-v1 community demand. |
| **Auto-update (Phases 1-4)** | Complex per-platform implementation. GitHub Releases with manual download is acceptable for early users. Add in Phase 5 or later. |
| **armeabi-v7a / i686 Android targets** | 32-bit Android devices are <5% market share. `arm64-v8a` and `x86_64` cover >95%. Remove from `build-core.sh` to save CI time. |
| **Telemetry / analytics** | Contradicts the privacy-first design. Not even opt-in. User feedback is via GitHub Issues. |
| **Linux screen-capture exclusion** | No reliable cross-compositor API exists (GTK4/Wayland/X11). The documented no-op is the correct implementation. Do not waste time on compositor-specific hacks. |
| **Speaker diarization (who said what)** | Whisper.cpp does not natively support diarization. Would require a separate model (pyannote, etc.) and significant architecture changes. Post-v1 feature. |
| **Real-time translation** | Out of scope for a note-taker MVP. The language setting in Phase 2 supports transcription in non-English languages. |
| **Electron / cross-platform UI rewrite** | The native-per-platform approach is a deliberate design decision. The Rust core is the shared layer. Do not consolidate UIs. |

---

## Agent Allocation (Max 20)

Recommended parallel agent assignments per phase. Items within a phase that touch different files can run simultaneously.

### Phase 1 (8 agents)
1. **Agent 1:** QW-1 through QW-4 (CI path fixes + Gradle wrapper)
2. **Agent 2:** QW-5, QW-6, QW-7 (core/src/lib.rs fixes: state leak, stop order, audio level)
3. **Agent 3:** QW-8 through QW-14 (dependency fixes, field name fixes, doc fixes)
4. **Agent 4:** P1-1 (headless WAV CLI binary)
5. **Agent 5:** P1-2, P1-6, P1-7 (CI: model caching, bundle model, flip Linux to real)
6. **Agent 6:** P1-3 (model path contract change in core)
7. **Agent 7:** P1-4, P1-5 (Linux transcription wiring + model path fix) -- after Agent 6
8. **Agent 8:** P1-8, P1-9, P1-10 (remaining core fixes + Linux history navigation)

### Phase 2 (6 agents)
9. **Agent 9:** P2-1 (complete FFI surface)
10. **Agent 10:** P2-2, P2-8 (error reporting + schema versioning)
11. **Agent 11:** P2-3 (test suite)
12. **Agent 12:** P2-4 (cloud summarization)
13. **Agent 13:** P2-5, P2-6, P2-7, P2-9 (transcription improvements: push-audio, streaming, caching, config)
14. **Agent 14:** P2-10, P2-11 (Linux app polish)

### Phase 3 (6 agents)
15. **Agent 15:** P3-1, P3-2, P3-3, P3-4, P3-5, P3-12, P3-13 (entire Apple platform: XcodeGen, FFI, models, CI, iOS)
16. **Agent 16:** P3-6, P3-7, P3-8 (Windows: DLL copy, CI, singleton)
17. **Agent 17:** P3-9 (Android JNI bridge -- large, standalone Rust file)
18. **Agent 18:** P3-10, P3-11 (Android: Gradle, CI fixes, split core job)

### Phase 4-5 (up to 6 agents as needed)
19. **Agent 19:** P4-1, P4-2, P4-3, P4-4, P4-5 (release workflow + packaging)
20. **Agent 20:** P4-6 through P4-11 (feature polish across platforms)

Total: 20 agents maximum, with natural serialization points between phases.