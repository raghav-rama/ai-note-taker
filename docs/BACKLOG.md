Now I have full orientation. Let me produce the comprehensive backlog.

## Task 1: Fix CI path errors in build.yml (QW-1, QW-2, QW-3)
- **Phase:** Quick Wins
- **Area:** ci
- **Effort:** S
- **Depends on:** none
- **Description:** Fix three path mismatches in `.github/workflows/build.yml`. Line 217: change upload path from `target/release/notetaker-linux` to `target/release/dbus-monitor-session`. Line 163: change `.sln` check from `windows/NoteHelper.sln` to `windows/NoteHelper/NoteHelper.sln` and update the `dotnet build` command to match; add `|| echo "::warning::..."` guard so an uncompilable .csproj does not break main before Phase 3. Line 174: change Windows upload path from `windows/bin/x64/Release/` to `windows/NoteHelper/bin/x64/Release/net8.0-windows10.0.22621.0/**/*`.
- **Acceptance criteria:** CI: Linux `app-linux` artifact contains the `dbus-monitor-session` binary. Windows job finds the .sln and attempts `dotnet build` (may warn-skip if compile fails). Windows upload path matches actual output directory.

## Task 2: Generate Android Gradle wrapper (QW-4)
- **Phase:** Quick Wins
- **Area:** android
- **Effort:** S
- **Depends on:** none
- **Description:** Hand-author `android/gradle/wrapper/gradle-wrapper.properties` (pointing to Gradle 8.7 distribution URL), commit the `android/gradlew` and `android/gradlew.bat` scripts, and the `android/gradle/wrapper/gradle-wrapper.jar`. This is needed because the user builds nothing locally; the files must be committed so CI can run `./gradlew assembleDebug`. Alternatively, switch CI to use `gradle/actions/setup-gradle@v4` which provisions Gradle without a committed wrapper. Either approach unblocks the Android CI gate at build.yml line 251.
- **Acceptance criteria:** CI: `app-android` job no longer prints "android/gradlew not found" warning. The `./gradlew assembleDebug` command is attempted (will still fail due to missing JNI -- that is expected at this phase).

## Task 3: Fix core lib.rs recording lifecycle bugs (QW-5, QW-6, QW-7)
- **Phase:** Quick Wins
- **Area:** core
- **Effort:** S
- **Depends on:** none
- **Description:** Three fixes in `core/src/lib.rs`. (1) Lines 214-226: on failure of `AudioCapture::new`, `LocalTranscriber::new`, or `capture.start()`, clear `current_meeting_id` and call `h.db.delete_meeting(meeting_id)` before returning error, preventing state leak and orphan rows. (2) Lines 295-302: swap stop order -- call `transcriber.stop()` BEFORE dropping `AudioCapture` so the transcriber can drain queued mpsc audio chunks. (3) Line 220: pass `Some(level_callback)` instead of `None` to `AudioCapture::new`; store last audio level in a new `audio_level: Mutex<f32>` field on `NoteHandle`; add a new `note_get_audio_level` FFI function that returns the stored value.
- **Acceptance criteria:** After a failed `note_start_recording` (e.g., bad model path), calling `note_start_recording` again does not return "already active". `note_get_audio_level` appears in the generated `core/include/notetaker.h`. Stop-recording does not silently drop trailing audio chunks.

## Task 4: Fix reqwest for cross-compilation (QW-8)
- **Phase:** Quick Wins
- **Area:** core
- **Effort:** S
- **Depends on:** none
- **Description:** In `core/Cargo.toml`, change the reqwest dependency to `reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"], optional = true }`. This prevents native-tls from being pulled in, which breaks Android/iOS cross-compilation with the NDK/iOS SDK. The `rustls-tls` feature uses pure-Rust TLS.
- **Acceptance criteria:** `cargo check -p notetaker-core --features cloud --target aarch64-linux-android` does not error on OpenSSL headers (verifiable once Android NDK is present in CI).

## Task 5: Fix Android and iOS field name mismatches (QW-9, QW-10, QW-11)
- **Phase:** Quick Wins
- **Area:** android
- **Effort:** S
- **Depends on:** none
- **Description:** (1) In `android/.../SettingsScreen.kt`, change the settings key `claude_api_key` to `anthropic_api_key` to match `core/src/lib.rs:420`. (2) In `android/.../CoreBridge.kt`, the `onTranscript` callback receives seconds from the C FFI; multiply by 1000 for the `timestampMs` field used by `RecordingScreen.formatTimestamp`. (3) In `apple/Shared/Models/Meeting.swift`, change the Summary struct field `overview` to `summary` to match `core/src/models.rs`.
- **Acceptance criteria:** Grep confirms no occurrence of `claude_api_key` in android/. `Meeting.swift` Summary struct field is `summary`. `onTranscript` multiplies the timestamp parameter by 1000.

## Task 6: Fix documentation inaccuracies (QW-13)
- **Phase:** Quick Wins
- **Area:** docs
- **Effort:** S
- **Depends on:** none
- **Description:** In `CLAUDE.md` and `README.md`: remove the claim "Cloud (OpenAI Whisper API) is opt-in fallback" (no cloud transcription exists; only local whisper-rs). Remove "SQLCipher encryption" claims (the code uses plain `rusqlite` with `bundled` feature, not `bundled-sqlcipher`). Fix Makefile paths: `windows/NoteHelper.sln` to `windows/NoteHelper/NoteHelper.sln`, `apple/NoteHelper.xcodeproj` to reference XcodeGen-generated project. Update `CLAUDE.md` transcription description to say "local whisper.cpp only; cloud summarization via Anthropic API behind `cloud` feature flag."
- **Acceptance criteria:** No false claims about OpenAI Whisper API or SQLCipher remain in any documentation file.

## Task 7: Fix macOS settings key mismatch (QW-14)
- **Phase:** Quick Wins
- **Area:** macos
- **Effort:** S
- **Depends on:** none
- **Description:** In `apple/NoteTaker/Models/MeetingViewModel.swift`, change the settings key `model_name` to `whisper_model` to match the key used by the Linux app, Android app, and the model-path contract established in Phase 1.
- **Acceptance criteria:** Grep for `model_name` in `apple/NoteTaker/` returns zero results. The key `whisper_model` is used consistently.

## Task 8: Fix FTS5 injection vulnerability (QW-12)
- **Phase:** Quick Wins
- **Area:** core
- **Effort:** S
- **Depends on:** none
- **Description:** In `core/src/storage.rs`, in the `search_meetings` method, wrap the user query string in double quotes before passing it to the FTS5 `MATCH` operator. Characters like `*`, `"`, `:`, and `()` are FTS5 syntax operators and cause SQLite errors if passed raw. Example: `format!("\"{}\"", query.replace('"', "\"\""))`.
- **Acceptance criteria:** Searching for a string containing special characters (e.g., `test: "hello"`) does not cause a SQLite syntax error. A unit test in P2-3 covers this.

## Task 9: Add cargo test step to CI (new)
- **Phase:** Quick Wins
- **Area:** ci
- **Effort:** S
- **Depends on:** none
- **Description:** Add a `cargo test -p notetaker-core` step to the `core` job in `.github/workflows/build.yml`, in the Linux matrix entry (after the build step). This runs the existing stub test today and becomes meaningful when P2-3 adds the test suite. Also add `cargo test -p notetaker-linux` in the `app-linux` job after the build step.
- **Acceptance criteria:** CI core-linux job runs `cargo test` and reports results. Currently passes with 1 test (test_stub_summarizer).

---

## Task 10: Create headless WAV-to-text CLI binary (P1-1)
- **Phase:** Phase 1
- **Area:** core
- **Effort:** M
- **Depends on:** none
- **Description:** Create `core/src/bin/transcribe_wav.rs` and add a `[[bin]]` target in `core/Cargo.toml`. The binary takes two arguments: a WAV file path and a Whisper GGML model path. It reads the WAV, resamples to 16kHz mono f32 using the existing `audio` module resampler, feeds chunks through `LocalTranscriber`, and prints transcript text to stdout. This is the only way to verify transcription in CI (runners have no microphone).
- **Acceptance criteria:** `cargo build -p notetaker-core --bin transcribe-wav` produces a binary. Running it with a valid WAV and model file prints transcript text to stdout. Running with a missing model prints an error and exits non-zero.

## Task 11: Model path contract change (P1-3)
- **Phase:** Phase 1
- **Area:** core
- **Effort:** S
- **Depends on:** none
- **Description:** Change `note_init`'s `model_path` parameter semantics to accept a models *directory* rather than a model file path. In `note_start_recording`, read the `whisper_model` setting from the DB (default: `tiny`) and compose `{model_dir}/ggml-{name}.bin`. Update the doc comment on `note_init` in `core/src/lib.rs:105`. This matches what Linux `main.rs`, Android, and iOS already pass. macOS passes a file path and must be updated in Phase 3 (P3-3).
- **Acceptance criteria:** `note_init("/data", "/models")` followed by `note_start_recording` looks for `/models/ggml-tiny.bin` by default. Setting `whisper_model=base` makes it look for `/models/ggml-base.bin`. Doc comment updated.

## Task 12: CI model caching and WAV transcription test (P1-2)
- **Phase:** Phase 1
- **Area:** ci
- **Effort:** S
- **Depends on:** 10
- **Description:** In `.github/workflows/build.yml`, add a new job `test-transcription` on `ubuntu-latest`. Steps: (1) checkout, (2) install Rust + cmake + libasound2-dev, (3) download `ggml-tiny.bin` via `scripts/download-model.sh tiny` with `actions/cache` keyed on `whisper-model-tiny`, (4) download `jfk.wav` from the whisper.cpp test fixtures URL, (5) `cargo run --bin transcribe-wav -- jfk.wav models/ggml-tiny.bin`, (6) assert output contains "country" (grep).
- **Acceptance criteria:** CI: `test-transcription` job passes. Model is cached across runs. Output of jfk.wav transcription contains recognizable English text.

## Task 13: Wire real transcription into Linux core_bridge (P1-4)
- **Phase:** Phase 1
- **Area:** linux
- **Effort:** S
- **Depends on:** 11
- **Description:** In `linux/src/core_bridge.rs`, replace the placeholder drain loop (lines 72-83) with real transcription. Import `notetaker::transcribe::LocalTranscriber`. Compose the model file path from `self.model_path` + the `whisper_model` setting (default `tiny`) + `ggml-{name}.bin`. Create a `TranscriptCallback` that converts `(&str, f64)` into a `TranscriptSegment` and sends on `transcript_tx`. Store `Arc<Mutex<Option<LocalTranscriber>>>` in `CoreBridge` so `stop_recording` can call `.take().stop()`. Update `stop_recording` to stop the transcriber before dropping audio capture.
- **Acceptance criteria:** When recording, real transcript segments appear on the `transcript_tx` channel. Stop-recording calls `transcriber.stop()` before dropping capture. The placeholder TODO comment is removed.

## Task 14: Fix Linux model path resolution (P1-5)
- **Phase:** Phase 1
- **Area:** linux
- **Effort:** S
- **Depends on:** 11, 13
- **Description:** In `linux/src/main.rs:25`, ensure the model path passed to `CoreBridge::new` resolves to `~/.local/share/session-monitor/models/` (XDG data dir) with a fallback to `./models/` if the XDG path does not exist. After Task 11, the path is a directory, which is correct. Verify the directory exists before passing it; create it if missing.
- **Acceptance criteria:** On a fresh system, the app creates `~/.local/share/session-monitor/models/` if absent. Passing a non-existent model directory produces a clear error in the transcript channel, not a panic.

## Task 15: Bundle model in Linux CI artifact (P1-6)
- **Phase:** Phase 1
- **Area:** ci
- **Effort:** S
- **Depends on:** 1, 12
- **Description:** In the `app-linux` job in `.github/workflows/build.yml`, after building the Linux app, copy `models/ggml-tiny.bin` (from the cache established in Task 12, or re-download) alongside the binary. Upload both in the `app-linux` artifact. Add a `README-run.txt` file with instructions: `chmod +x dbus-monitor-session && mkdir -p models && cp ggml-tiny.bin models/ && ./dbus-monitor-session`.
- **Acceptance criteria:** CI: `app-linux` artifact contains both `dbus-monitor-session` and `ggml-tiny.bin` and `README-run.txt`.

## Task 16: Flip Linux CI to real failures (P1-7)
- **Phase:** Phase 1
- **Area:** ci
- **Effort:** S
- **Depends on:** 1, 15
- **Description:** In `.github/workflows/build.yml`, for the `app-linux` job only: remove the `|| echo "::warning::..."` fallback from the build step (the Linux build already succeeds). Change `if-no-files-found: warn` to `if-no-files-found: error` on the upload step. Leave other platform jobs as `warn` until their builds are real in Phase 3.
- **Acceptance criteria:** CI: if the Linux binary is not produced, the `app-linux` job fails red instead of warning green.

## Task 17: Fix state leak on recording failure (P1-8)
- **Phase:** Phase 1
- **Area:** core
- **Effort:** S
- **Depends on:** 3
- **Description:** This is the same fix as QW-5 in Task 3. If Task 3 is implemented correctly, this is already done. Verify that on `AudioCapture::new` failure, `LocalTranscriber::new` failure, or `capture.start()` failure: `current_meeting_id` is cleared and the orphan meeting row is deleted via `h.db.delete_meeting(meeting_id)`.
- **Acceptance criteria:** Verified by Task 3's acceptance criteria. No separate work needed if Task 3 is complete.

## Task 18: Fix stop order for transcriber drain (P1-9)
- **Phase:** Phase 1
- **Area:** core
- **Effort:** S
- **Depends on:** 3
- **Description:** This is the same fix as QW-6 in Task 3. If Task 3 is implemented correctly, this is already done. Verify that `transcriber.stop()` is called BEFORE dropping `AudioCapture` in `note_stop_recording`.
- **Acceptance criteria:** Verified by Task 3's acceptance criteria. No separate work needed if Task 3 is complete.

## Task 19: Wire Linux history-to-notes navigation (P1-10)
- **Phase:** Phase 1
- **Area:** linux
- **Effort:** S
- **Depends on:** none
- **Description:** In `linux/src/views/history.rs`, add a `connect_row_activated` handler on the history list box. When a row is activated, set `current_meeting_id` in the notes view and navigate to the Notes tab (via the `AdwViewSwitcher` or by pushing a detail page). Without this, the notes view is unreachable from the UI -- users can record but never review past meetings.
- **Acceptance criteria:** Clicking a meeting in the History list navigates to the Notes view with that meeting's content displayed. The Generate Summary and Export buttons have a valid `current_meeting_id`.

---

## Task 20: Complete FFI surface (P2-1)
- **Phase:** Phase 2
- **Area:** core
- **Effort:** M
- **Depends on:** 3
- **Description:** Add missing FFI functions to `core/src/lib.rs`: `note_resume_recording` (expose the existing `AudioCapture::resume`), `note_search_meetings` (expose FTS5 `search_meetings` from storage.rs), `note_get_segments` (return transcript segments for a meeting as JSON), `note_update_meeting_title` (add a `Database::update_meeting_title` method and FFI wrapper), `note_get_meeting_detail` (return meeting + segments + notes as nested JSON, replacing the need for export-as-json for UI display). Regenerate `core/include/notetaker.h` via cbindgen.
- **Acceptance criteria:** `core/include/notetaker.h` contains declarations for all new functions. `note_resume_recording` resumes a paused recording. `note_search_meetings("test")` returns matching results. `note_get_segments(meeting_id)` returns a JSON array of segments.

## Task 21: Add error reporting FFI (P2-2)
- **Phase:** Phase 2
- **Area:** core
- **Effort:** S
- **Depends on:** none
- **Description:** Add `note_last_error() -> *mut c_char` to `core/src/lib.rs` that returns the most recent error message stored in a thread-local `RefCell<Option<String>>`. Every error path that currently only calls `error!()` should also store the message via a helper `set_last_error(msg)`. Add `note_set_log_callback` for structured logging. Without this, every FFI consumer is blind to error causes.
- **Acceptance criteria:** After a failed `note_start_recording` (e.g., missing model), `note_last_error()` returns a descriptive message like "Failed to start transcriber: model file not found". The returned string must be freed with `note_free_string`.

## Task 22: Add database schema versioning (P2-8)
- **Phase:** Phase 2
- **Area:** core
- **Effort:** S
- **Depends on:** none
- **Description:** In `core/src/storage.rs`, add `PRAGMA user_version` check in `Database::open`. Create a `migrations` array. On open, compare the stored version with the code version and run pending migrations sequentially. Set `user_version` after successful migration. This prevents data loss on future schema changes.
- **Acceptance criteria:** Opening a database created by the current schema sets `user_version = 1`. A future migration (e.g., adding a column) can be added to the array and runs automatically on next open. Opening a newer-than-code database returns an error rather than silently corrupting data.

## Task 23: Core test suite (P2-3)
- **Phase:** Phase 2
- **Area:** core
- **Effort:** M
- **Depends on:** 8, 20, 21, 22
- **Description:** Create `core/tests/` directory with integration tests. Test coverage: storage (schema creation, CRUD for meetings/segments/notes/settings, FTS5 queries including special characters per Task 8, cascade delete, export formats markdown/text/json), models (serde round-trip for all structs), FFI lifecycle (init/destroy, null pointer handling, double-stop, start-after-failed-start), error reporting (note_last_error returns correct messages). Use in-memory SQLite DB (`:memory:`) for storage tests.
- **Acceptance criteria:** `cargo test -p notetaker-core` runs 20+ tests. All pass. Tests cover: schema creation, meeting CRUD, segment insertion + retrieval, FTS5 search with special chars, cascade delete, JSON export, FFI null-pointer safety.

## Task 24: Enable and fix cloud summarization (P2-4)
- **Phase:** Phase 2
- **Area:** core
- **Effort:** S
- **Depends on:** 4
- **Description:** In `core/src/summarize.rs`: add a 30-second request timeout to the reqwest client. Fix the markdown fence stripping on the API response (the response JSON may be wrapped in code fences). Store structured summary fields (`key_points`, `action_items`, `decisions`) in the database, not just the flat summary string -- add new columns or a `summary_json` column to the meetings table. In `scripts/build-core.sh`, add `--features cloud` to the Linux build function (line 235) and macOS build function (line 134). In `linux/Cargo.toml`, add `features = ["cloud"]` to the notetaker-core dependency.
- **Acceptance criteria:** `cargo build -p notetaker-core --features cloud` succeeds with reqwest using rustls-tls. The summarizer includes a 30-second timeout. `scripts/build-core.sh linux` passes `--features cloud`. Linux app's `Cargo.toml` enables `cloud` feature.

## Task 25: Push-audio FFI entry point (P2-5)
- **Phase:** Phase 2
- **Area:** core
- **Effort:** S
- **Depends on:** 20
- **Description:** Add `note_push_audio(handle: *mut NoteHandle, samples: *const f32, count: usize) -> i32` to `core/src/lib.rs`. This allows native platform layers to feed system audio (ScreenCaptureKit, WASAPI loopback) into the transcription pipeline. The function writes the samples into the same `audio_tx` mpsc channel that `AudioCapture` uses. This is the architectural prerequisite for system audio capture on every platform.
- **Acceptance criteria:** `note_push_audio` appears in `notetaker.h`. Calling it while a recording is active feeds audio into the transcriber. Calling it with no active recording returns an error code. Add a headless test: feed raw PCM via `note_push_audio` and verify transcript output (extend the CLI from Task 10 with a `--pcm` mode).

## Task 26: Improve streaming transcription quality (P2-6)
- **Phase:** Phase 2
- **Area:** core
- **Effort:** M
- **Depends on:** none
- **Description:** In `core/src/transcribe.rs`: add 1-second overlap between 4-second chunks (sliding window). Enable `set_no_context(false)` to carry Whisper state between chunks. Add a simple energy-based VAD to skip silence (avoid processing quiet chunks). Create `WhisperState` once per session instead of per chunk. Cap the internal buffer at 30 seconds with backpressure (drop oldest chunks if buffer exceeds limit, log a warning).
- **Acceptance criteria:** Chunk boundaries do not produce garbled text (verify with the WAV CLI from Task 10 on a multi-sentence recording). Silent portions are skipped. Buffer does not grow unboundedly under slow inference.

## Task 27: Cache whisper model across recordings (P2-7)
- **Phase:** Phase 2
- **Area:** core
- **Effort:** S
- **Depends on:** 11
- **Description:** Move `WhisperContext` loading from `LocalTranscriber::new` into `NoteHandle` (loaded lazily on first `note_start_recording` or eagerly in a background thread after `note_init`). Store it as `Mutex<Option<Arc<WhisperContext>>>` in `NoteHandle`. `LocalTranscriber::new` receives the pre-loaded context instead of a model path. This eliminates multi-second blocking on every recording start for `base` or `small` models.
- **Acceptance criteria:** Second call to `note_start_recording` (after stop) does not reload the model. The model is loaded once and reused. Changing the `whisper_model` setting invalidates the cached context on next recording start.

## Task 28: Configurable transcription parameters (P2-9)
- **Phase:** Phase 2
- **Area:** core
- **Effort:** S
- **Depends on:** none
- **Description:** In `core/src/transcribe.rs`, read `whisper_language` (default: `en`), `whisper_threads` (default: 4) from the settings table via `Database`. Allow `auto` for language to enable Whisper's auto-detection. Pass these to `FullParams` when configuring whisper inference.
- **Acceptance criteria:** Setting `whisper_language=auto` enables language auto-detection. Setting `whisper_threads=2` uses 2 threads. Default behavior unchanged (English, 4 threads).

## Task 29: Linux model download handler (P2-10)
- **Phase:** Phase 2
- **Area:** linux
- **Effort:** S
- **Depends on:** 14
- **Description:** In `linux/src/views/settings.rs`, wire the "Download Selected Model" button to an HTTP download from `huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{model}.bin`. Show a progress bar (AdwProgressBar or similar). Write the downloaded file to the models directory. The download runs on a background thread via `spawn_blocking` to avoid blocking the GTK main loop.
- **Acceptance criteria:** Clicking "Download Selected Model" with "tiny" selected downloads `ggml-tiny.bin` to the models directory. Progress is visible in the UI. Completion shows a toast/notification.

## Task 30: Linux app polish (P2-11)
- **Phase:** Phase 2
- **Area:** linux
- **Effort:** M
- **Depends on:** 13, 19, 24, 29
- **Description:** Multiple improvements to the Linux app: (1) Wire summarize button to real `CloudSummarizer` (the `cloud` feature is now enabled via Task 24). (2) Add `GtkFileDialog` for export instead of `eprintln!`. (3) Add `AdwToast` for error feedback replacing `eprintln!` on all error paths. (4) Wire FTS5 search in history view via `Database::search_meetings` instead of client-side title filter. (5) Add pause/resume button in recording view. (6) Fix `set_hide_on_close` by adding `GtkApplication::activate` re-present logic so the window can be re-shown.
- **Acceptance criteria:** Summarize button produces an AI summary (with valid API key). Export writes a file via native dialog. Errors show toast notifications. History search searches transcript content via FTS5. Pause button pauses/resumes recording.

---

## Task 31: Create Xcode project via XcodeGen (P3-1)
- **Phase:** Phase 3
- **Area:** macos
- **Effort:** M
- **Depends on:** none
- **Description:** Create `apple/project.yml` for XcodeGen defining two targets: macOS app (`SystemHelper`, sources from `NoteTaker/` and `Shared/`) and iOS app (`Utility`, sources from `iOS/` and `Shared/`). Configure `SWIFT_INCLUDE_PATHS` for `Shared/Core/include` (the NoteCore modulemap). Configure `LIBRARY_SEARCH_PATHS` for `$(SRCROOT)/../dist/core/macos` and `$(SRCROOT)/../dist/core/ios`. Set `OTHER_LDFLAGS` for `-lnotetaker -lc++ -framework Foundation -framework Accelerate -framework Security`. Set macOS deployment target to 14.0, iOS to 17.0. Enable microphone entitlement. Set `CODE_SIGN_IDENTITY` to `-` for ad-hoc signing.
- **Acceptance criteria:** Running `xcodegen generate` in the `apple/` directory produces a valid `.xcodeproj`. `xcodebuild -list` shows both macOS and iOS schemes.

## Task 32: Fix macOS CoreBridge C FFI import (P3-2)
- **Phase:** Phase 3
- **Area:** macos
- **Effort:** S
- **Depends on:** 31
- **Description:** In `apple/NoteTaker/Core/CoreBridge.swift`, add `import NoteCore` (using the shared modulemap at `apple/Shared/Core/include/module.modulemap`). The macOS app currently calls C FFI functions (note_init, note_destroy, etc.) without any import statement, which fails to compile. Ensure the modulemap path is included in `SWIFT_INCLUDE_PATHS` in `project.yml`. The CoreBridge must wrap all FFI functions from `notetaker.h` including `note_last_error` (from Task 21) and `note_resume_recording` (from Task 20).
- **Acceptance criteria:** macOS app compiles with `import NoteCore` resolving all C FFI symbols. No undefined symbol errors at link time.

## Task 33: Unify macOS and Shared model types (P3-3)
- **Phase:** Phase 3
- **Area:** macos
- **Effort:** M
- **Depends on:** 7, 11, 31, 32
- **Description:** Migrate macOS to use `Shared/Models/Meeting.swift` Codable types. Remove duplicate `Meeting`/`TranscriptSegment` definitions from `MeetingViewModel.swift`. Fix JSON contract: `note_get_meeting` returns a flat Meeting JSON; use `note_get_meeting_detail` (from Task 20) for the detail view with segments. Unify settings keys to `whisper_model` (Task 7 already fixed `model_name`). Update macOS `CoreBridge` to pass a directory (not file path) to `note_init` per the contract from Task 11.
- **Acceptance criteria:** macOS app has no duplicate Meeting/TranscriptSegment types. All model types come from `Shared/Models/Meeting.swift`. Settings key is `whisper_model` everywhere. `note_init` receives a models directory.

## Task 34: macOS static lib + Metal acceleration (P3-4)
- **Phase:** Phase 3
- **Area:** macos
- **Effort:** S
- **Depends on:** 31
- **Description:** Switch macOS core from `.dylib` to `.a` (static library) to simplify distribution (no embed framework phase, no rpath issues). Drop x86_64 target (Apple Silicon only since 2020; avoids the Metal-on-x86_64 incompatibility). In `scripts/build-core.sh` `build_macos` function, pass `--features metal,cloud` for macOS builds. Build only `aarch64-apple-darwin`, skip lipo. Update `project.yml` `LIBRARY_SEARCH_PATHS` to point to the static lib.
- **Acceptance criteria:** `scripts/build-core.sh macos` produces `dist/core/macos/libnotetaker.a` (not .dylib). Metal feature is enabled. Only arm64 is built. The XcodeGen project links the static library.

## Task 35: Fix macOS and iOS CI jobs (P3-5)
- **Phase:** Phase 3
- **Area:** ci
- **Effort:** S
- **Depends on:** 31, 34
- **Description:** In `.github/workflows/build.yml`, update the `app-macos` job: install xcodegen (`brew install xcodegen`), run `cd apple && xcodegen generate` before `xcodebuild`. Fix scheme name to match what `project.yml` generates. Add `-derivedDataPath build` to `xcodebuild` so output path is predictable. Use `CODE_SIGN_IDENTITY=-` for ad-hoc signing. Fix artifact upload path to `apple/build/Build/Products/Release/*.app`. Flip to `if-no-files-found: error`. Bundle `ggml-tiny.bin` in the macOS artifact alongside the `.app`.
- **Acceptance criteria:** CI: `app-macos` job builds and uploads a real `.app` bundle + model file. Job fails red on build failure (not warn-skip).

## Task 36: Fix Windows DLL copy for CI (P3-6)
- **Phase:** Phase 3
- **Area:** windows
- **Effort:** S
- **Depends on:** 1
- **Description:** In `windows/NoteHelper/NoteHelper.csproj`, add a second `<ItemGroup>` conditional on a CI environment variable (e.g., `Condition="'$(CI)'=='true'"`) that copies `notetaker.dll` from `$(SolutionDir)../../dist/core/windows/notetaker.dll` to the output directory. In the CI `app-windows` job, add a step to copy `dist/core/windows/notetaker.dll` to the location the .csproj expects. Create an empty `windows/NoteHelper/Assets/` directory with a placeholder icon to fix the Content Include build warning.
- **Acceptance criteria:** CI: `dotnet build` finds and copies `notetaker.dll` into the build output. The Assets/ directory exists and the build warning is resolved.

## Task 37: Fix Windows CI and flip to real failures (P3-7)
- **Phase:** Phase 3
- **Area:** ci
- **Effort:** S
- **Depends on:** 1, 36
- **Description:** In `.github/workflows/build.yml`, the Windows CI path fixes from Task 1 make `dotnet build` actually run. Now: remove the `|| echo ::warning` guard from the build step (or add one if the build is not yet stable). Fix the upload artifact path (already done in Task 1). Flip `if-no-files-found` to `error`. Bundle `ggml-tiny.bin` in the Windows artifact zip. Add a `README-run.txt` with run instructions.
- **Acceptance criteria:** CI: `app-windows` job builds the WinUI 3 app and uploads real artifacts including the DLL and model file. Job fails red on build failure.

## Task 38: Windows singleton CoreBridge (P3-8)
- **Phase:** Phase 3
- **Area:** windows
- **Effort:** S
- **Depends on:** none
- **Description:** Move CoreBridge instantiation from per-page to `App.xaml.cs`. Pass the shared instance to pages via a static property or DI container. Prevents multiple `note_init` calls opening the same SQLite DB, which causes `SQLITE_BUSY` errors. Update `note_init` call to pass a models directory (not file path) per Task 11's contract. Wrap all P2-1 FFI functions including `note_last_error` and `note_resume_recording`.
- **Acceptance criteria:** Only one `CoreBridge` / `NoteHandle` instance exists at runtime. All pages share it. No `SQLITE_BUSY` errors under normal navigation.

## Task 39: Android JNI bridge in Rust (P3-9)
- **Phase:** Phase 3
- **Area:** android
- **Effort:** L
- **Depends on:** 20, 21
- **Description:** Add `jni = "0.21"` dependency to `core/Cargo.toml`, gated on `#[cfg(target_os = "android")]`. Create `core/src/android_jni.rs` implementing all 14 JNI entry points that `android/.../CoreBridge.kt` declares (`Java_com_system_service_helper_native_CoreBridge_nativeInit`, etc.). Each delegates to the existing `note_*` C functions. Implement the `onTranscript` callback via stored `JavaVM` global reference + `AttachCurrentThread`. Implement `nativeGetAudioLevel` using the new `note_get_audio_level` from Task 3. Approximately 500 lines of Rust.
- **Acceptance criteria:** `cargo build -p notetaker-core --target aarch64-linux-android` produces `libnotetaker.so` containing all 14 `Java_com_system_service_helper_native_*` symbols. Verified via `nm -D` or `readelf`. No `UnsatisfiedLinkError` when loaded by the Android app.

## Task 40: Fix Android Gradle, CI, and field mismatches (P3-10)
- **Phase:** Phase 3
- **Area:** android
- **Effort:** S
- **Depends on:** 2, 5, 39
- **Description:** With the Gradle wrapper from Task 2 and field fixes from Task 5 already applied: fix CI to copy jniLibs from `dist/core/android/jniLibs/` correctly. Fix `nativeStartRecording` to pass null (not empty string) for default device by updating CoreBridge.kt. Bundle `ggml-tiny.bin` in the APK assets or in the CI artifact alongside the APK. Remove `armeabi-v7a` from `scripts/build-core.sh` line 264 and line 282 (YAGNI -- not in abiFilters).
- **Acceptance criteria:** CI: `app-android` job produces a real `.apk` artifact. The APK or CI artifact includes the tiny Whisper model. `armeabi-v7a` is no longer built.

## Task 41: Split Android core into its own CI job (P3-11)
- **Phase:** Phase 3
- **Area:** ci
- **Effort:** S
- **Depends on:** 40
- **Description:** Currently, `core-linux` builds both Linux and Android targets. An Android cross-compile regression kills the Linux core artifact. Create a separate `core-android` job on `ubuntu-latest` with NDK setup. The `app-android` job depends on `core-android` instead of `core-linux`. Remove Android targets from the Linux core matrix entry.
- **Acceptance criteria:** CI: `core-android` job builds Android .so files independently. `core-linux` no longer builds Android targets. `app-android` depends on `core-android`.

## Task 42: iOS compile-only CI (P3-12)
- **Phase:** Phase 3
- **Area:** ios
- **Effort:** S
- **Depends on:** 5, 31
- **Description:** The iOS target is already defined in `project.yml` from Task 31. Update the `app-ios` CI job to: install xcodegen, generate the project, build for simulator (`-destination 'platform=iOS Simulator,name=iPhone 15'`). Upload no artifact (compile-only validation). Set iOS deployment target to 17.0 in `project.yml` (matching the iOS 17 APIs used). Update `scripts/build-core.sh` to set `IPHONEOS_DEPLOYMENT_TARGET=17.0` instead of 16.0.
- **Acceptance criteria:** CI: `app-ios` job compiles the iOS app for simulator without errors. No artifact uploaded (compile-only). No warnings from FFI type mismatches or deployment target.

## Task 43: macOS model download (P3-13)
- **Phase:** Phase 3
- **Area:** macos
- **Effort:** S
- **Depends on:** 35
- **Description:** In `apple/NoteTaker/Views/SettingsView.swift`, replace the fake progress loop (5-second sleep) with a real `URLSession.download` task. Download from the HuggingFace `ggml-{model}.bin` URL. Show progress via `Progress` + `ProgressView`. Write to the app support models directory (`~/Library/Application Support/SystemHelper/models/`).
- **Acceptance criteria:** Clicking "Download Model" with "tiny" selected downloads the real 75MB model file. Progress bar updates during download. File is written to the correct app support directory.

---

## Task 44: Release workflow (P4-1)
- **Phase:** Phase 4
- **Area:** ci
- **Effort:** M
- **Depends on:** 16, 35, 37, 40
- **Description:** Create `.github/workflows/release.yml`. Trigger on `v*` tags. Build all platforms (reuse the build jobs from build.yml or duplicate them). Create a GitHub Release. Attach artifacts with platform-appropriate names: `SystemHelper-macos-arm64.zip`, `SystemHelper-windows-x64.zip`, `dbus-monitor-session-linux-x64.tar.gz`, `SystemService-android.apk`. Include `ggml-tiny.bin` model in each artifact. Use `softprops/action-gh-release` or `gh release create`.
- **Acceptance criteria:** Pushing a `v0.1.0` tag triggers the workflow. A GitHub Release is created with downloadable assets for all four shipping platforms (iOS excluded). Each asset includes the bundled tiny model.

## Task 45: macOS .dmg packaging (P4-2)
- **Phase:** Phase 4
- **Area:** macos
- **Effort:** M
- **Depends on:** 44
- **Description:** In the release workflow, use `create-dmg` (installable via brew) to produce a `.dmg` with drag-to-Applications layout. Ad-hoc sign with `codesign -s -`. Include first-run instructions for Gatekeeper bypass (`xattr -cr SystemHelper.app` or right-click > Open) in a `README.txt` inside the DMG.
- **Acceptance criteria:** Release workflow produces a `.dmg` file. Opening the DMG shows the app icon and an Applications folder alias. After `xattr -cr`, the app launches.

## Task 46: Windows portable zip packaging (P4-3)
- **Phase:** Phase 4
- **Area:** windows
- **Effort:** M
- **Depends on:** 44
- **Description:** In the release workflow, package the Windows build output + `notetaker.dll` + `ggml-tiny.bin` model into a self-contained zip. Add a `README-run.txt` with instructions. Skip MSIX for now (requires signing cert). Ensure the zip structure has a single top-level folder. Add an `Assets/` directory with a placeholder icon.
- **Acceptance criteria:** Release workflow produces a `.zip`. Extracting and running the `.exe` works without .NET SDK installation (WinAppSDK is self-contained). README explains how to run.

## Task 47: Linux AppImage packaging (P4-4)
- **Phase:** Phase 4
- **Area:** linux
- **Effort:** M
- **Depends on:** 44
- **Description:** In the release workflow, use `linuxdeploy` + `linuxdeploy-plugin-gtk` to create an AppImage that bundles GTK4/libadwaita shared libraries. Add a `.desktop` file (Name=Session Monitor, Exec=dbus-monitor-session, Icon=session-monitor) and a placeholder icon. The AppImage runs on any glibc 2.35+ distro without installing GTK4 dev packages.
- **Acceptance criteria:** Release workflow produces an `.AppImage`. The AppImage runs on Ubuntu 22.04+ without installing GTK4 packages. `--appimage-extract` shows bundled GTK4 libs.

## Task 48: Android release signing (P4-5)
- **Phase:** Phase 4
- **Area:** android
- **Effort:** S
- **Depends on:** 44
- **Description:** In `android/app/build.gradle.kts`, add a `signingConfigs` block reading keystore path, password, key alias, and key password from CI environment variables (GitHub secrets). Switch `assembleDebug` to `assembleRelease` in the release workflow. The release APK can be sideloaded without debug mode enabled.
- **Acceptance criteria:** Release workflow produces a signed release APK. **Blocked on: human adds `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD` secrets to the GitHub repo.**

## Task 49: macOS system audio capture via ScreenCaptureKit (P4-6a)
- **Phase:** Phase 4
- **Area:** macos
- **Effort:** M
- **Depends on:** 25, 33
- **Description:** In `apple/NoteTaker/Core/AudioManager.swift`, add ScreenCaptureKit audio capture. Use `SCStreamConfiguration` with audio-only settings. Feed captured PCM samples via the FFI `note_push_audio` function (from Task 25). Add a toggle in RecordingView for "Capture system audio" that requests screen recording permission and starts the SCStream.
- **Acceptance criteria:** With screen recording permission granted, system audio (e.g., from a video call) is captured and transcribed alongside microphone audio.

## Task 50: Windows system audio capture via WASAPI loopback (P4-6b)
- **Phase:** Phase 4
- **Area:** windows
- **Effort:** M
- **Depends on:** 25, 38
- **Description:** Create a new `windows/NoteHelper/Services/AudioLoopback.cs` that uses WASAPI loopback capture via `NAudio` NuGet package (or raw COM interop). Feed captured PCM samples to the core via P/Invoke `note_push_audio`. Add a "Capture system audio" toggle in RecordingPage that starts/stops the loopback capture.
- **Acceptance criteria:** With the toggle enabled, system audio is captured and transcribed. Both microphone and loopback audio are fed to the transcription pipeline simultaneously.

## Task 51: macOS secure API key storage (P4-7a)
- **Phase:** Phase 4
- **Area:** macos
- **Effort:** S
- **Depends on:** 33
- **Description:** Create a `KeychainHelper` class for macOS (iOS already has one in Shared -- share it or adapt it). Store the Anthropic API key in the macOS Keychain instead of the plaintext SQLite settings table. Update SettingsView to read/write from Keychain. Remove the `note_set_setting("anthropic_api_key", ...)` call for the API key.
- **Acceptance criteria:** API key is stored in macOS Keychain, not SQLite. `security find-generic-password` shows the stored key. Clearing settings does not delete the Keychain entry.

## Task 52: Windows secure API key storage (P4-7b)
- **Phase:** Phase 4
- **Area:** windows
- **Effort:** S
- **Depends on:** 38
- **Description:** Use `CredWrite`/`CredRead` via P/Invoke (or the `Windows.Security.Credentials.PasswordVault` API) to store API keys in Windows Credential Manager instead of plaintext SQLite.
- **Acceptance criteria:** API key is stored in Windows Credential Manager. `cmdkey /list` shows the credential. Plaintext key no longer in SQLite.

## Task 53: Linux secure API key storage (P4-7c)
- **Phase:** Phase 4
- **Area:** linux
- **Effort:** S
- **Depends on:** 30
- **Description:** Add the `secret-service` crate to `linux/Cargo.toml`. Use the `libsecret` / Secret Service D-Bus API to store API keys instead of plaintext SQLite. Fall back to the SQLite settings table if no secret service is available (headless/minimal systems).
- **Acceptance criteria:** With GNOME Keyring or KDE Wallet running, API key is stored in the secret service. Without a secret service, falls back to SQLite with a warning.

## Task 54: Android secure API key storage (P4-7d)
- **Phase:** Phase 4
- **Area:** android
- **Effort:** S
- **Depends on:** 40
- **Description:** Use `EncryptedSharedPreferences` from `androidx.security.crypto` to store API keys instead of sending them to the SQLite settings table via JNI. Add the dependency to `app/build.gradle.kts`.
- **Acceptance criteria:** API key is stored in EncryptedSharedPreferences. Key is not visible in the SQLite database file.

## Task 55: Auto-title meetings from transcript (P4-8)
- **Phase:** Phase 4
- **Area:** core
- **Effort:** S
- **Depends on:** 20
- **Description:** In the transcription callback path (either in `LocalTranscriber` or in the FFI layer), after the first 3 transcript segments are produced, auto-generate a meeting title from the first 8 words of the combined text. Call `note_update_meeting_title` (from Task 20) to update the database. All platform UIs should display the updated title on next refresh. Allow manual editing via the existing title fields in each platform's recording view.
- **Acceptance criteria:** A new recording starts as "Untitled Meeting" and is automatically renamed after 3 segments. The title appears in the history view. Manual title editing overrides the auto-title.

## Task 56: macOS model download UX (P4-9a)
- **Phase:** Phase 4
- **Area:** macos
- **Effort:** S
- **Depends on:** 43
- **Description:** This was already implemented in Task 43. Enhance to show available models (tiny/base/small) with file sizes in the settings view. Fall back to bundled tiny model if the selected model file is missing. Show a warning if the selected model is not downloaded.
- **Acceptance criteria:** Settings shows model sizes. Selecting an un-downloaded model shows a download prompt. Missing selected model falls back to tiny.

## Task 57: Windows model download UX (P4-9b)
- **Phase:** Phase 4
- **Area:** windows
- **Effort:** S
- **Depends on:** 38
- **Description:** In `windows/NoteHelper/Views/SettingsPage.xaml.cs`, replace the placeholder download dialog with a real `HttpClient` download from HuggingFace. Show a `ProgressBar` during download. Write to the app data models directory (`%LOCALAPPDATA%/SystemHelper/models/`).
- **Acceptance criteria:** Clicking "Download Model" downloads the selected model. Progress bar updates. File is written to the correct directory.

## Task 58: Android model download UX (P4-9c)
- **Phase:** Phase 4
- **Area:** android
- **Effort:** S
- **Depends on:** 40
- **Description:** Add a model download screen or dialog to the Android settings. Use `OkHttp` or `HttpURLConnection` to download from HuggingFace. Show progress via a `LinearProgressIndicator`. Write to `filesDir.resolve("models")`.
- **Acceptance criteria:** Model download works from Settings. Progress is shown. File is saved to the app's internal storage models directory.

## Task 59: Linux model download UX (P4-9d)
- **Phase:** Phase 4
- **Area:** linux
- **Effort:** S
- **Depends on:** 29
- **Description:** This was already implemented in Task 29. Enhance to show model sizes and support multiple models. Add fallback to bundled tiny model.
- **Acceptance criteria:** Enhanced model picker shows sizes. Fallback to tiny works.

## Task 60: Hardware acceleration in CI builds (P4-10)
- **Phase:** Phase 4
- **Area:** ci
- **Effort:** S
- **Depends on:** 34
- **Description:** Update `scripts/build-core.sh` to pass platform-appropriate feature flags: macOS: `--features metal,cloud`. Windows: `--features cloud` (CPU-only; CUDA requires runtime detection). Linux: `--features cloud` (CPU-only; Vulkan optional). Android: `--features cloud`. Update the CI workflow to pass these feature flags to each platform's build.
- **Acceptance criteria:** `scripts/build-core.sh macos` passes `--features metal,cloud`. All platform builds include the `cloud` feature. macOS binary includes Metal acceleration code.

## Task 61: macOS error UX (P4-11a)
- **Phase:** Phase 4
- **Area:** macos
- **Effort:** S
- **Depends on:** 21, 33
- **Description:** Wire `note_last_error` into SwiftUI `.alert` modifiers in macOS views. Replace silent failures with visible error dialogs. Show errors for: missing model file, no microphone permission, no API key, summarization failure.
- **Acceptance criteria:** A failed recording start shows an alert with the specific error message from `note_last_error`. Silent failures are eliminated from all macOS views.

## Task 62: Windows error UX (P4-11b)
- **Phase:** Phase 4
- **Area:** windows
- **Effort:** S
- **Depends on:** 21, 38
- **Description:** Wire `note_last_error` P/Invoke into `ContentDialog` error displays in all WinUI 3 pages. Replace silent failures with visible error feedback.
- **Acceptance criteria:** Failed operations show a `ContentDialog` with the error message. No more silent null returns.

## Task 63: Linux error UX (P4-11c)
- **Phase:** Phase 4
- **Area:** linux
- **Effort:** S
- **Depends on:** 21, 30
- **Description:** This was partially done in Task 30 (AdwToast). Ensure all error paths use `AdwToast` or `AdwMessageDialog` and that `note_last_error` is called to retrieve specific error messages from the core. Replace remaining `eprintln!` calls.
- **Acceptance criteria:** No `eprintln!` error output remains. All errors shown via AdwToast with descriptive messages.

## Task 64: Android error UX (P4-11d)
- **Phase:** Phase 4
- **Area:** android
- **Effort:** S
- **Depends on:** 21, 39
- **Description:** Wire `nativeLastError` JNI call into `Snackbar` or `AlertDialog` displays in all Compose screens. Show errors for: missing model, no mic permission, no API key, summarization failure.
- **Acceptance criteria:** Failed operations show a Snackbar with the specific error message. No more silent null returns from JNI.

---

## Task 65: macOS code signing and notarization (P5-1)
- **Phase:** Phase 5
- **Area:** ci
- **Effort:** L
- **Depends on:** 45
- **Description:** In `.github/workflows/release.yml`, add steps to import an Apple Developer certificate from GitHub secrets using `apple-actions/import-codesign-certs`. Sign the `.app` bundle with `codesign`. Create a notarized `.dmg` using `xcrun notarytool submit` + `xcrun stapler staple`. This eliminates the Gatekeeper "unidentified developer" warning entirely.
- **Acceptance criteria:** Release `.dmg` opens without any Gatekeeper warning on a fresh macOS install. **Blocked on: human adds Apple Developer Program ($99/year) certificate, provisioning profile, and notarization credentials as GitHub secrets.**

## Task 66: Windows code signing (P5-2)
- **Phase:** Phase 5
- **Area:** ci
- **Effort:** M
- **Depends on:** 46
- **Description:** In the release workflow, use `signtool` with an EV or OV code signing certificate stored as a GitHub secret to sign the `.exe` and `.dll`. This reduces SmartScreen warnings.
- **Acceptance criteria:** Signed `.exe` shows publisher name in properties. SmartScreen warnings reduced. **Blocked on: human purchases and adds a code signing certificate ($200-400/year) as GitHub secrets.**

## Task 67: Android Play Store preparation (P5-3)
- **Phase:** Phase 5
- **Area:** android
- **Effort:** S
- **Depends on:** 48
- **Description:** In the release workflow, build AAB (Android App Bundle) instead of APK using `./gradlew bundleRelease`. The AAB can be uploaded to Play Store. Generate screenshots and prepare store listing metadata in `android/fastlane/` or `android/metadata/`.
- **Acceptance criteria:** Release workflow produces a signed `.aab` file. **Blocked on: human creates Google Play Developer account ($25 one-time) and uploads the AAB.**

## Task 68: GitHub Pages download page (P5-4)
- **Phase:** Phase 5
- **Area:** ci
- **Effort:** M
- **Depends on:** 44
- **Description:** Create a `docs/` directory with an `index.html` GitHub Pages site. Include: product description (using the generic "System Helper" / "Session Monitor" naming), per-platform download buttons linking to latest GitHub Release assets, installation instructions per platform (including macOS `xattr -cr` workaround), and screenshots. Add a `gh-pages` deployment workflow or configure Pages from `docs/` on main.
- **Acceptance criteria:** `username.github.io/ai-note-taker` serves a landing page with working download links to the latest release. Each platform has clear installation instructions.

## Task 69: iOS TestFlight distribution (P5-5)
- **Phase:** Phase 5
- **Area:** ios
- **Effort:** L
- **Depends on:** 42, 65
- **Description:** Requires Apple Developer Program (same account as Task 65). Set up App Store Connect for the iOS app. Configure CI to build and upload to TestFlight using `xcodebuild -exportArchive` + `xcrun altool --upload-app` or `xcodebuild -exportArchive -exportOptionsPlist`. Invite testers via public TestFlight link.
- **Acceptance criteria:** TestFlight build is uploadable from CI. Public link allows testers to install the iOS app. **Blocked on: human has Apple Developer Program account and configures App Store Connect.**

## Task 70: macOS auto-update via Sparkle (P5-6a)
- **Phase:** Phase 5
- **Area:** macos
- **Effort:** L
- **Depends on:** 65
- **Description:** Integrate the Sparkle framework for macOS auto-updates. Add an `appcast.xml` hosted on GitHub Pages. On app launch, check for updates via the appcast feed. Prompt to download and install. Opt-in only (no silent updates, consistent with privacy-first design).
- **Acceptance criteria:** App checks for updates on launch (if opted in). A newer release triggers a download prompt. Update installs and restarts the app.

## Task 71: Windows auto-update check (P5-6b)
- **Phase:** Phase 5
- **Area:** windows
- **Effort:** M
- **Depends on:** 46
- **Description:** On app launch, check the GitHub Releases API for a newer version. If found, show a notification with a download link. No automatic installation -- the user downloads and replaces manually. Opt-in via Settings toggle.
- **Acceptance criteria:** App detects a newer GitHub Release and shows a notification. The notification links to the download page.

## Task 72: Linux auto-update via AppImageUpdate (P5-6c)
- **Phase:** Phase 5
- **Area:** linux
- **Effort:** M
- **Depends on:** 47
- **Description:** Embed AppImageUpdate metadata in the AppImage. On launch, check for updates against the GitHub Releases API. If a newer version exists, prompt the user to update in-place. Opt-in via Settings.
- **Acceptance criteria:** AppImage checks for updates on launch. A newer release triggers an update prompt.

## Task 73: Android in-app version check (P5-6d)
- **Phase:** Phase 5
- **Area:** android
- **Effort:** S
- **Depends on:** 48
- **Description:** On app launch, check the GitHub Releases API for a newer version. If found, show a dialog with a link to download the new APK. No automatic installation.
- **Acceptance criteria:** App detects a newer GitHub Release and shows an update dialog.

## Task 74: Version reporting FFI (P5-7)
- **Phase:** Phase 5
- **Area:** core
- **Effort:** S
- **Depends on:** none
- **Description:** Add `note_version() -> *const c_char` to `core/src/lib.rs` returning the crate version string from `env!("CARGO_PKG_VERSION")`. The returned pointer is static and does not need freeing. Display the version in all platform settings views. Include in GitHub Release notes.
- **Acceptance criteria:** `note_version()` returns the version string (e.g., "0.1.0"). Each platform's Settings view shows the core library version.

## Task 75: Rename stealth binaries to avoid AV false positives (new, from Risks)
- **Phase:** Phase 4
- **Area:** core
- **Effort:** S
- **Depends on:** 1
- **Description:** Rename the stealth process names to avoid mimicking system processes: Windows `svchost-user.exe` to `syshelper.exe` (in `.csproj` AssemblyName and `app.manifest`), Linux `dbus-monitor-session` to `session-helper` (in `linux/Cargo.toml` `[[bin]]` name). Update CI artifact paths and the release workflow accordingly. The current names risk antivirus false positives (especially `svchost-user.exe` on Windows).
- **Acceptance criteria:** Windows binary is named `syshelper.exe`. Linux binary is named `session-helper`. CI artifact upload paths match the new names. No remaining references to old names in CI.