# Codebase Analysis

## Rust Core Library (core/)

**Current State:** The core library is a genuine, non-stub implementation with a fully wired end-to-end pipeline: cpal audio capture -> linear-interpolation resampler (to 16kHz mono f32) -> mpsc channel -> whisper-rs transcription in 4-second chunks -> SQLite persistence (with FTS5) + C callback delivery. All five modules are real implementations:

AUDIO (audio.rs): Full cpal-based capture with device enumeration, F32/I16/U16 sample format support, channel downmixing, resampling, RMS level metering callback, and a command-driven thread (Start/Stop/Pause/Resume/Shutdown). The Stream lives on a dedicated thread since it is !Send.

TRANSCRIBE (transcribe.rs): Real whisper-rs integration. Loads a Whisper GGML model synchronously on a worker thread, buffers audio to 4-second chunks, runs inference with greedy sampling, computes per-segment confidence from token probabilities, persists segments to SQLite, and delivers results via callback. Handles shutdown flush with zero-padding for short buffers.

STORAGE (storage.rs): Complete SQLite layer with WAL mode, foreign keys, full CRUD for meetings/segments/notes/settings, FTS5 virtual table with sync triggers, transactional cascade deletes, and three export formats (markdown, text, JSON).

SUMMARIZE (summarize.rs): Cloud summarizer behind the "cloud" feature flag. Calls the Anthropic Messages API with a structured prompt, parses the JSON response into a Summary struct (summary, key_points, action_items, decisions). Includes a stub summarizer for testing.

FFI (lib.rs): 15 exported C functions with catch_unwind panic guards, null-pointer checks, JSON serialization across the boundary, and proper ownership semantics (caller-owned strings freed via note_free_string). The opaque NoteHandle bundles DB, model path, audio capture, transcriber, meeting state, callback, and tokio runtime.

BUILD (build.rs + cbindgen): Generates core/include/notetaker.h (exists, 135 lines, complete). Platform-specific link flags for macOS/iOS/Android/Windows. cbindgen.toml configured with C language, include guard, documentation.

CI (build.yml): Matrix build for macOS (macos+ios), Windows, Linux (linux+android). Produces dist/core/ artifacts. Six downstream app jobs download artifacts and attempt builds (most skip with warnings due to missing project files).

The core compiles and CI passes. The Linux app compiles and links against the core. Other platforms have source files (Swift, C#, Kotlin) but lack Xcode project files, so their CI jobs emit warnings and skip.

### Gaps

- **[BLOCKER] [xl]** Mic-only audio capture -- no system audio / loopback support: AudioCapture only uses cpal input_devices() (microphone). The design spec explicitly requires system audio capture: ScreenCaptureKit on macOS, WASAPI loopback on Windows, PipeWire monitor on Linux. A note-taker that hears only the local mic transcribes one side of the meeting. cpal has no loopback capability on macOS/Linux/iOS. Core needs a note_push_audio(const float*, size_t) FFI entry point so native layers can feed system audio PCM from platform-specific APIs. This is an architectural gap that touches every platform bridge.

- **[BLOCKER] [medium]** No hardware acceleration features enabled in CI builds: scripts/build-core.sh passes no --features flags to cargo. macOS/iOS builds lack 'metal' (GPU acceleration via Metal) and 'coreml' (Apple Neural Engine). Windows/Linux lack 'cuda'/'vulkan'. The shipped artifacts are CPU-only whisper inference which cannot keep up with real-time audio on most devices (especially with base/small+ models). The 'cloud' feature is also never enabled, so summarization is dead code in every shipped binary.

- **[MAJOR] [small]** note_start_recording state leak on partial failure: lib.rs line 214 sets current_meeting_id before AudioCapture::new, LocalTranscriber::new, and capture.start() are attempted. If any of those fail, the function returns NOTE_ERR_INTERNAL but leaves current_meeting_id set. Every subsequent call to note_start_recording fails with 'already active' until note_stop_recording is called. An orphaned meeting row is also left in the database with no ended_at.

- **[MAJOR] [small]** Trailing audio lost on recording stop: In note_stop_recording, AudioCapture is dropped (closing the sender side of audio_tx) and joined BEFORE transcriber.stop() is called. The transcriber's shutdown path flushes only the internal buffer, but audio chunks already queued in the mpsc channel between audio capture and the transcriber are never drained. The last several seconds of speech can be silently discarded.

- **[MAJOR] [large]** Naive streaming strategy causes boundary artifacts and unbounded memory growth: Fixed 4-second chunks with no_context=true, no overlap, and no VAD. Words cut at chunk boundaries produce garbled text. Zero-padded flush causes hallucinations. Each 4s chunk runs through whisper's full 30s inference window (7.5x wasted compute). A new WhisperState is created per chunk. The unbounded mpsc channel and unbounded Vec buffer provide no backpressure -- if inference is slower than real-time (likely on CPU), memory and latency grow without bound.

- **[MAJOR] [medium]** Whisper model reloaded on every recording start: WhisperContext is created inside LocalTranscriber::new, which blocks the caller for the full model load each time note_start_recording is called. For the 'tiny' model this is fast, but 'base' or 'small' models take several seconds. The model should be loaded once in note_init (or lazily cached in NoteHandle) and shared across recording sessions.

- **[MAJOR] [medium]** FFI surface missing critical functions the UIs need: notetaker.h exposes 15 functions but is missing: note_resume_recording (AudioCapture::resume exists but is unexposed), note_search_meetings (FTS5 is implemented but unreachable from FFI), note_get_segments (cannot display transcript in UI), notes CRUD (insert_note/get_notes are dead code), rename meeting title (always 'Untitled Meeting'), audio level callback passthrough (plumbed in audio.rs but passed None at lib.rs:220), recording state query. Each new FFI function requires updates to all five platform CoreBridge wrappers.

- **[MAJOR] [small]** No error reporting mechanism for callers: Every failure returns null or -2 (NOTE_ERR_INTERNAL) with a log::error! call, but no logger is ever installed and there is no note_set_log_callback or note_last_error function. Native apps cannot distinguish 'model file missing' from 'no microphone' from 'database locked'. This will make every platform integration appear broken during bring-up.

- **[MAJOR] [small]** reqwest native-tls will break cross-compilation when cloud feature is enabled: reqwest in Cargo.toml uses default features which pulls in native-tls. Android/iOS cross-compiles with openssl will fail. Should use default-features = false, features = ['json', 'rustls-tls']. Also: no request timeout configured (default is infinite -- can hang forever), no markdown fence stripping on the API response JSON, and structured summary fields (key_points, action_items, decisions) are serialized to JSON for the caller but only the flat 'summary' string is persisted to the database.

- **[MAJOR] [medium]** API key stored in plaintext SQLite: The anthropic_api_key is stored via the generic settings table in plaintext SQLite. For a 'privacy-first' product this is inadequate. Should use platform-native secure storage: Keychain (macOS/iOS), DPAPI/Credential Manager (Windows), Secret Service / libsecret (Linux), Android Keystore. This requires FFI hooks for the native layer to provide key storage.

- **[MAJOR] [medium]** Near-zero test coverage: Only one test exists: test_stub_summarizer in summarize.rs which tests a mock, not the real code. No tests for: storage (schema creation, CRUD, FTS5 queries, export, cascade delete), transcription (chunk processing, confidence computation, buffer flush), audio (resampler correctness, device enumeration), FFI (null pointer handling, lifecycle, JSON serialization), or models (serde round-trip).

- **[MINOR] [small]** No database schema versioning or migration system: create_schema uses CREATE TABLE IF NOT EXISTS only. There is no PRAGMA user_version check or migration system. Any schema change in a future version will either fail silently (missing columns) or require users to delete their database. For a product that stores meeting data, this will cause data loss.

- **[MINOR] [small]** Hardcoded transcription parameters ignore user settings: Language is hardcoded to whisper default ('en'), n_threads is hardcoded to 4, and the settings table (which stores arbitrary key-value pairs) is never consulted by the transcriber. Users cannot configure language, thread count, or model size preference.

- **[MINOR] [small]** FTS5 MATCH takes raw user input -- syntax errors on special characters: search_meetings passes the query string directly to FTS5 MATCH. Characters like double quotes, asterisks, colons, and parentheses are FTS5 operators and will cause SQLite syntax errors. The query needs escaping or wrapping in double quotes.

- **[MINOR] [small]** Audio thread build failure not surfaced synchronously: AudioCapture::new spawns a thread that builds the cpal input stream. If the stream build fails, the error is logged but start() can still succeed (it sends a Start command to a thread that is about to exit). This results in a silent no-audio recording with no error reported to the user.

- **[MINOR] [medium]** Resampler quality and real-time safety issues: Linear interpolation resampling without an anti-aliasing filter introduces aliasing artifacts that degrade transcription accuracy (especially for 48kHz->16kHz downsampling). Vec allocations and Mutex locking happen inside the real-time audio callback, which can cause glitches. A lock-free ring buffer and pre-allocated output buffer would be more appropriate.

- **[MINOR] [small]** CLAUDE.md claims cloud transcription fallback that does not exist: CLAUDE.md states 'Cloud (OpenAI Whisper API) is opt-in fallback behind a feature flag' but no cloud transcription implementation exists anywhere in the codebase. Only local whisper-rs transcription is implemented. The cloud feature flag enables summarization (Anthropic API), not transcription.

### Risks

- Real-time inference cannot keep up on CPU: without metal/coreml/cuda features enabled, whisper-rs runs CPU-only inference. A 4-second chunk on the 'base' model takes 2-8 seconds on CPU depending on hardware, meaning the unbounded buffer grows continuously and latency becomes unusable. The 'tiny' model is faster but has significantly worse accuracy.
- Mic-only capture may invalidate the product premise: a meeting note-taker that only hears the local microphone (not other participants via system audio) produces a one-sided transcript. The design spec requires system audio capture but the architecture has no path for it -- cpal cannot do loopback on macOS/iOS, and the FFI has no push-audio entry point for native layers to feed platform-specific audio.
- Silent failures with no error channel will make platform integration appear broken: every FFI error returns a generic -2 or null with no way to retrieve the error message. During bring-up of five platform bridges simultaneously, developers cannot tell whether the model file is missing, the microphone is unavailable, the database is locked, or the audio format is unsupported.
- Whisper model distribution is unsolved: the core library requires a GGML model file at runtime but nothing downloads, bundles, or manages models. CI artifacts are inert without a model. The tiny model is 75MB, base is 142MB, small is 466MB -- too large to bundle in mobile app stores without on-demand download.
- The build script produces CPU-only artifacts for all platforms. Even if the metal/cuda/vulkan features are later enabled, the CI matrix does not differentiate feature sets per platform, and the macOS universal binary (aarch64+x86_64) cannot use Metal since x86_64 Macs may lack it.
- Five platform bridges must stay in sync with the C header. Each FFI change is a coordination point across Swift, C#, Kotlin, and Rust wrappers. With 6-10 missing FFI functions, this is a large batch of cross-platform work.
- The tokio runtime is always created even without the cloud feature, adding ~2MB to the binary and startup overhead. If cloud is never enabled, this is pure waste. More critically, CloudSummarizer::summarize calls runtime.block_on from a non-async context, which will panic if called from within a tokio context.
- No crash/error telemetry: combined with no-error-reporting FFI, bugs in production will be invisible. The privacy-first stance precludes opt-out telemetry but an opt-in crash reporter is warranted.

### Dependencies

- whisper-rs 0.13 -> whisper-rs-sys -> whisper.cpp (requires cmake + C++ compiler on every target; the cmake crate drives the build)
- cpal 0.15 -> ALSA (libasound2-dev on Linux), CoreAudio (macOS/iOS), WASAPI (Windows), Oboe (Android)
- rusqlite 0.32 with bundled feature (compiles SQLite from source -- no system dependency, includes FTS5)
- cbindgen 0.27 (build dependency -- generates C header; header is BOTH committed at core/include/notetaker.h AND regenerated on build, creating drift risk)
- tokio 1.x with rt-multi-thread (always linked even without cloud feature -- could be made optional)
- reqwest 0.12 with json feature (optional, behind cloud flag -- pulls native-tls by default)
- serde + serde_json 1.x (JSON serialization across FFI boundary)
- Runtime: a Whisper GGML model file must be present on disk -- nothing in core downloads or manages models; scripts/download-model.sh exists but CI does not run it, so CI artifacts are non-functional without the app layer fetching a model
- Downstream: five platform CoreBridge wrappers (Swift x2, C#, Rust/GTK, Kotlin/JNI) are pinned to the current 15-symbol C header; every FFI addition requires updating all five
- Android NDK r26d (for cargo-ndk cross-compilation)
- Xcode (for macOS/iOS builds -- currently missing .xcodeproj files)
- .NET 8 SDK (for Windows WinUI 3 build)

---

## Apple platform apps (macOS + iOS)

**Current State:** The Apple platform has substantial Swift code across three directories:

**macOS app (apple/NoteTaker/)**: 8 Swift files comprising a full SwiftUI app with an AppDelegate, MenuBarExtra, tab-based UI (Recording, History, Settings), a CoreBridge wrapping C FFI, an AudioManager with CoreAudio device enumeration, a StealthManager applying NSWindow.sharingType = .none to all windows, a MeetingViewModel with recording lifecycle and JSON parsing, and views for live transcript display, meeting history with NavigationSplitView, meeting notes with collapsible sections and export, and settings with audio device picker, model selection, and API key fields. The notetaker.h header is present at apple/NoteTaker/Core/notetaker.h.

**iOS app (apple/iOS/)**: 7 Swift files comprising a separate SwiftUI app (UtilityApp) with tab navigation, an iOSStealthManager implementing the UITextField.isSecureTextEntry canvas trick for capture prevention plus UIScreen.isCaptured detection, an iOSAudioManager handling AVAudioSession lifecycle and iOS 17 permission APIs, iOSRecordingView with TimelineView-based timer, iOSHistoryView with pull-to-refresh and swipe-to-delete, iOSNotesView with structured summary display and UIActivityViewController export, and iOSSettingsView with Keychain-backed API key storage.

**Shared code (apple/Shared/)**: 3 files: CoreBridge.swift (ObservableObject-based, using import NoteCore), Models/Meeting.swift (Codable structs for Meeting, TranscriptSegment, Note, Summary, AudioDevice, MeetingDetail with custom ISO8601 date decoding), and a module.modulemap + notetaker.h for the NoteCore clang module.

**Stealth features**: Both are correctly implemented. macOS uses NSWindow.sharingType = .none with lifecycle notification observers that catch lazily-created windows (MenuBarExtra). iOS uses the UITextField.isSecureTextEntry canvas trick with SecureContainerView/SecureHostingController to wrap the entire view hierarchy, plus UIScreen.capturedDidChangeNotification for banner alerts. Both have graceful fallback.

**What exists**: Source files only. There is NO .xcodeproj, .xcworkspace, Package.swift, or project.yml anywhere in the repository. The CI build.yml references xcodebuild with -scheme macOS and -scheme iOS, but these schemes do not exist because there is no project file. Both macOS and iOS app CI jobs currently print a warning and skip.

**FFI integration**: All Swift files correctly call the C FFI functions (note_init, note_start_recording, note_stop_recording, note_pause_recording, note_set_transcript_callback, note_summarize, note_free_string, note_list_meetings, note_get_meeting, note_delete_meeting, note_export_meeting, note_set_setting, note_get_setting, note_list_audio_devices). The function signatures match the core header. The macOS CoreBridge uses direct calls with no import statement. The iOS/Shared CoreBridge uses import NoteCore via the modulemap.

### Gaps

- **[BLOCKER] [medium]** No Xcode project file exists (.xcodeproj, Package.swift, or project.yml): There is no project file anywhere in the repository. xcodebuild cannot build without one. Recommendation: add a project.yml for XcodeGen (brew install xcodegen, xcodegen generate in CI). This avoids checking in a binary .pbxproj and fits the CI-only build model. Two targets needed: macOS app (NoteTaker/SystemHelper) and iOS app (iOS/Utility). The project.yml must define SWIFT_INCLUDE_PATHS to include Shared/Core/include for the NoteCore module, LIBRARY_SEARCH_PATHS for dist/core/macos or dist/core/ios, and OTHER_LDFLAGS for -lnotetaker plus required frameworks.

- **[BLOCKER] [small]** macOS CoreBridge has no import path for C FFI symbols: apple/NoteTaker/Core/CoreBridge.swift calls note_init, note_destroy, etc. directly without 'import NoteCore' and without a bridging header. The iOS/Shared CoreBridge.swift has 'import NoteCore' and a proper modulemap at Shared/Core/include/module.modulemap. The macOS app must either: (a) also use import NoteCore by including Shared/Core/include in SWIFT_INCLUDE_PATHS, or (b) define a bridging header. Without this, the macOS Swift compiler cannot find any of the C FFI functions.

- **[BLOCKER] [medium]** Shared/ directory is iOS-only in practice; name collisions with macOS: CLAUDE.md says apple/Shared/ is shared Swift code, but in reality the macOS and iOS apps have incompatible parallel implementations. NoteTaker/Core/CoreBridge.swift (macOS) is a non-observable Sendable class using closures, while Shared/Core/CoreBridge.swift (iOS) is an @MainActor ObservableObject with an initialize() method. NoteTaker/Models/MeetingViewModel.swift defines Meeting/TranscriptSegment inline with different field names (durationSeconds, notes as [MeetingNote]) vs Shared/Models/Meeting.swift which uses Codable with snake_case CodingKeys (durationSecs, no embedded segments). If both targets include Shared/, there will be redeclaration errors. Must either: migrate macOS to use the Shared models and CoreBridge, or rename one set.

- **[MAJOR] [medium]** FFI JSON contract mismatches between macOS app and Rust core: Several data contract disagreements between the macOS Swift code and the actual Rust core JSON output: (1) note_get_meeting: the core returns a flat Meeting JSON {id, title, started_at, ...} but macOS MeetingViewModel.parseMeetingDict expects embedded 'segments' and 'notes' arrays that are NOT returned by the core's get_meeting (the core only includes those in export_meeting with format=json, wrapped as {meeting, segments, notes}). (2) note_init model_path: macOS passes a full file path 'models/ggml-tiny.bin' but the core documentation says model_path is a path to the model file and iOS passes a directory. Need to verify which the core actually expects. (3) Settings key inconsistency: macOS saves 'model_name', iOS saves 'whisper_model' for the same setting.

- **[MAJOR] [small]** FFI JSON contract mismatches between iOS app and Rust core: The iOS Shared/Models/Meeting.swift Summary struct expects field 'overview' but the Rust core models.rs Summary struct has field 'summary'. So JSONDecoder will fail to parse the summary JSON returned by note_summarize. The iOS MeetingDetail expects a nested {meeting: {...}, segments: [...], notes: [...]} shape, but note_get_meeting returns a flat Meeting struct. Only note_export_meeting with format=json returns the nested shape. The iOS CoreBridge.getMeeting() will always fail to decode.

- **[MAJOR] [medium]** macOS core links as dylib but CI produces unsigned app: build-core.sh produces a .dylib for macOS (lib/notetaker.dylib) but a .a (static lib) for iOS. The macOS dylib requires an Embed Frameworks build phase and @executable_path/../Frameworks rpath, or the .app will crash on launch. Additionally, CODE_SIGNING_ALLOWED=NO produces an .app that macOS Gatekeeper will refuse to open. Recommendation: either switch macOS to a static library (simpler, no embed phase needed) or use CODE_SIGN_IDENTITY=- for ad-hoc signing so users can run after xattr -cr.

- **[MAJOR] [small]** Required Xcode linker flags not documented or configured: The Rust core build.rs emits framework link flags (Foundation, Accelerate, Security) for the Rust linker, but when consuming the static library from Xcode, these must be explicitly added in OTHER_LDFLAGS: -lnotetaker -lc++ -framework Foundation -framework Accelerate -framework Security. The modulemap's 'link notetaker' only covers the library itself. Without these, Xcode will report undefined symbol errors at link time.

- **[MAJOR] [small]** CI output paths and scheme names are incorrect: The CI build.yml expects artifacts at apple/build/Release/*.app but xcodebuild writes to DerivedData by default unless -derivedDataPath or SYMROOT is specified. The CI references -scheme macOS and -scheme iOS but no scheme exists because there is no project. These must be aligned with whatever project.yml generates.

- **[MAJOR] [small]** Core library built without cloud feature in CI: The CI build-core.sh does not pass --features cloud when building. Without the cloud feature, note_summarize always returns null with error 'Summarization requires the cloud feature'. Both apps have prominent Summarize buttons that will silently fail. The macOS build should also pass --features metal for GPU-accelerated whisper inference on Apple Silicon. CI command should be: scripts/build-core.sh macos ios --features cloud,metal.

- **[MINOR] [small]** iOS deployment target mismatch: The iOS app uses iOS 17+ APIs: AVAudioApplication.requestRecordPermission(), two-parameter onChange modifier, navigationDestination(item:). However, build-core.sh hardcodes IPHONEOS_DEPLOYMENT_TARGET=16.0. The Xcode project must set the deployment target to 17.0 minimum to match the API usage, and build-core.sh should be updated to match.

- **[MINOR] [small]** macOS UI has non-functional placeholders: Several macOS features are placeholders: (1) downloadModel() in SettingsView is a fake progress loop (sleeps 5 seconds, no actual download). (2) whisperApiKey and claudeApiKey bind to view model @State vars that are never persisted -- the caption says 'stored in keychain' but there is no KeychainHelper on macOS (iOS has one). (3) audioLevel is declared on both AudioManager and MeetingViewModel but nothing feeds a value into it, so the waveform visualization in RecordingView is always flat at 0.

- **[MINOR] [small]** iOS meetingTitle never sent to core; no FFI for updating title: The iOS iOSRecordingView has a meetingTitle text field but the value is never sent to the core library. There is no note_update_meeting_title FFI function. The macOS app has the same issue. All meetings are created with the hardcoded title 'Untitled Meeting' in the Rust core.

- **[MINOR] [small]** iOS has no audio device picker despite loading devices: iOSSettingsView calls coreBridge.listAudioDevices() and stores them in audioDevices but never displays a picker for them. On iOS this is less critical since the system typically has one input, but the dead code should be cleaned up or the picker added.

### Risks

- iOS apps cannot be installed on physical devices without code signing and a provisioning profile. The user's goal of 'just download it from GitHub' is achievable for macOS (ad-hoc signing + xattr -cr) but NOT for iOS without an Apple Developer account ($99/year) and CI secrets for the signing certificate and profile. iOS CI can only validate that the code compiles.
- macOS Gatekeeper will quarantine unsigned .app downloads from GitHub. Users must run 'xattr -cr AppName.app' before launching. Using CODE_SIGN_IDENTITY=- (ad-hoc signing) helps but does not fully bypass Gatekeeper for apps downloaded from the internet.
- The iOS stealth feature (UITextField.isSecureTextEntry canvas trick) relies on undocumented internal UIKit layer structure (_UITextFieldCanvasView). Apple may change this in any iOS update, breaking the capture prevention silently. The code has graceful fallback (content renders normally without protection) but the security guarantee is fragile.
- The two CoreBridge implementations (macOS closure-based vs iOS ObservableObject) and two Meeting model implementations (manual JSON parsing vs Codable) are a maintenance hazard. Any FFI contract change in the Rust core must be updated in both, and the current divergences (overview vs summary, flat vs nested meeting JSON, model_name vs whisper_model) show they are already out of sync.
- Building whisper-rs from source in CI requires cmake and significant compile time (~5-10 minutes). The macOS CI runner must compile whisper.cpp twice (aarch64 + x86_64 for universal binary) plus once for iOS (aarch64-apple-ios). This is the slowest step and can cause CI timeouts on the 6-hour GitHub Actions limit for free accounts.

### Dependencies

- XcodeGen must be installed in CI (brew install xcodegen) and a project.yml must be added to apple/ to generate the .xcodeproj before xcodebuild can run
- The core static library (libnotetaker.a for iOS, libnotetaker.a or .dylib for macOS) must be built first and placed at dist/core/macos/ and dist/core/ios/ -- this already works in CI via the core job
- The generated notetaker.h header from cbindgen must be available at a known path (core/include/notetaker.h is generated by build.rs, and hand-written copies exist at apple/Shared/Core/include/notetaker.h and apple/NoteTaker/Core/notetaker.h) -- the CI header path must match SWIFT_INCLUDE_PATHS
- An Apple Developer account (free or paid) is needed for iOS device installation -- CI can only do unsigned builds for simulator or compile-only validation without one
- The Whisper GGML model file must be bundled or downloaded at runtime -- neither app currently includes a model download mechanism that works (macOS fakes it, iOS has nothing)

---

## Windows App (windows/NoteHelper/)

**Current State:** The Windows app is a fully scaffolded WinUI 3 (Windows App SDK 1.5) application with 16 source files. Here is what exists and works today:

**Project files:** Both NoteHelper.sln and NoteHelper.csproj exist and are structurally valid. The .sln targets x64 and arm64 in Debug/Release configurations. The .csproj uses the correct SDK (`Microsoft.NET.Sdk`), targets `net8.0-windows10.0.22621.0`, has `UseWinUI=true`, and enables MSIX tooling with self-contained WinAppSDK.

**P/Invoke into notetaker DLL:** CoreBridge.cs (322 lines) is a comprehensive, production-quality P/Invoke bridge into notetaker.dll. It declares all 13 FFI functions from the C header (`note_init`, `note_destroy`, `note_start_recording`, `note_stop_recording`, `note_pause_recording`, `note_set_transcript_callback`, `note_summarize`, `note_free_string`, `note_list_meetings`, `note_get_meeting`, `note_delete_meeting`, `note_export_meeting`, `note_set_setting`, `note_get_setting`, `note_list_audio_devices`). The signatures align exactly with `core/include/notetaker.h`. It handles UTF-8 marshaling, native string consumption with `note_free_string`, GC-pinned callback delegates, and proper IDisposable cleanup.

**WinUI 3 views -- fully implemented, not stubs:** Four XAML page views with complete code-behind:
- RecordingPage (232 lines CS + 131 lines XAML): record/stop/pause buttons, live timer (DispatcherTimer + Stopwatch), audio level meter (placeholder random values), live transcript display via RichTextBlock with timestamped paragraphs, CoreBridge integration for start/stop/pause recording and transcript callbacks.
- HistoryPage (243 lines CS + 123 lines XAML): meeting list via ListView with DataTemplate, search/filter via AutoSuggestBox, context menu with View Notes/Export/Delete, JSON deserialization of meetings from CoreBridge, file export via FileSavePicker, delete confirmation dialog.
- NotesPage (242 lines CS + 225 lines XAML): meeting detail view with AI summary section (Expander), key points/action items/decisions sections, full transcript expander, "Summarize" button calling CoreBridge.Summarize on a background thread, JSON-structured summary parsing, clipboard export.
- SettingsPage (387 lines CS + 206 lines XAML): audio device selection (ComboBox populated from CoreBridge.ListAudioDevices), loopback toggle, transcription mode (local/cloud radio buttons), whisper model picker with download placeholder, language selection, API key inputs (OpenAI + Anthropic PasswordBoxes), data management (export all, open data folder, clear all data).

**MainWindow (114 lines CS + 82 lines XAML):** NavigationView with Recording/History/Notes menu items plus Settings. Custom title bar with recording indicator (red dot + timer). StealthService integration on window load. Minimize-to-tray on close (args.Handled = true).

**App.xaml.cs (121 lines):** Application lifecycle, system tray icon via H.NotifyIcon.WinUI with "System Helper" tooltip and "SH" generated icon, show/exit context menu, single-instance support, unhandled exception handler. Uses P/Invoke for ShowWindow and SetForegroundWindow.

**Stealth (SetWindowDisplayAffinity):** StealthService.cs (88 lines) is fully implemented. It P/Invokes `user32.dll!SetWindowDisplayAffinity` with `WDA_EXCLUDEFROMCAPTURE = 0x00000011`, called from MainWindow.NavView_Loaded. Also provides RemoveExclusion() and GetCurrentAffinity() methods. Error reporting via Marshal.GetLastWin32Error.

**Process name obfuscation:** The .csproj sets `<AssemblyName>svchost-user</AssemblyName>`, and app.manifest declares the assembly identity as `svchost-user.exe`. The window title, tray tooltip, and tray icon all say "System Helper" -- nothing reveals "note taker".

**NuGet dependencies:** Microsoft.WindowsAppSDK 1.5.240627000, Microsoft.Windows.SDK.BuildTools 10.0.22621.3233, CommunityToolkit.Mvvm 8.2.2, CommunityToolkit.WinUI.Controls.SettingsControls 8.0.240109, H.NotifyIcon.WinUI 2.1.3.

**DLL copy mechanism:** The .csproj has a conditional ItemGroup that copies `core/target/release/notetaker.dll` to the output directory if the file exists, but this path is for local builds and will not resolve in CI.

**CI status:** The build.yml has an `app-windows` job that runs on windows-latest, sets up .NET 8, downloads the core-windows artifact to dist/core/, and runs `dotnet build windows/NoteHelper.sln -c Release -p:Platform=x64`. However, the .sln is at `windows/NoteHelper/NoteHelper.sln` (not `windows/NoteHelper.sln`), so the CI job prints the "not found" warning and skips. Additionally, the upload path `windows/bin/x64/Release/` likely does not match the actual dotnet output path.

### Gaps

- **[BLOCKER] [small]** CI .sln path is wrong -- build always skips: The build.yml checks for `windows/NoteHelper.sln` but the actual file is at `windows/NoteHelper/NoteHelper.sln`. The bash condition `if [ -f "windows/NoteHelper.sln" ]` always evaluates false, so the Windows app build is silently skipped with a warning. Fix: change the path to `windows/NoteHelper/NoteHelper.sln` and the dotnet build command to `dotnet build windows/NoteHelper/NoteHelper.sln -c Release -p:Platform=x64`.

- **[BLOCKER] [small]** notetaker.dll not copied from CI artifact to the .csproj expected location: The .csproj copies notetaker.dll from a relative path `../../../core/target/release/notetaker.dll` which only works for local builds. In CI, the core artifact is downloaded to `dist/core/windows/notetaker.dll`. Either the CI workflow needs to copy the DLL to the path the .csproj expects, or the .csproj Condition and path need a second ItemGroup for the CI layout (e.g., `dist/core/windows/notetaker.dll`). Without this, the app will build but the DLL will not be bundled in the output.

- **[MAJOR] [small]** CI upload artifact path likely wrong: The CI uploads from `windows/bin/x64/Release/` but `dotnet build` with the WinUI 3 project will output to `windows/NoteHelper/bin/x64/Release/net8.0-windows10.0.22621.0/`. The upload-artifact step will find no files and produce a warning. Fix: update the path or use a glob like `windows/NoteHelper/bin/x64/Release/**/*`.

- **[MINOR] [small]** No Assets directory exists (Content Include references it): The .csproj has `<Content Include="Assets\**" />` but no `Assets/` directory exists under windows/NoteHelper/. This may cause a build warning or error depending on how MSBuild handles the missing glob. An empty Assets directory or removing the glob would fix it.

- **[MINOR] [medium]** CoreBridge is instantiated independently per page with no shared lifecycle: RecordingPage, HistoryPage, NotesPage, and SettingsPage each create their own CoreBridge instance with their own note_init call. This means multiple NoteHandle instances pointing at the same SQLite DB, which could cause locking issues. A proper singleton/DI pattern (e.g., App-level CoreBridge) would be needed for production use.

- **[MINOR] [small]** RecordingPage.GetMainWindow() always returns null: The GetMainWindow() method in RecordingPage.xaml.cs has a stub implementation that always returns null. The title bar recording indicator (SetRecordingState) is called on null and therefore silently no-ops. A fix would be to store the MainWindow reference in the App class and retrieve it.

- **[MINOR] [medium]** API keys stored in SQLite via CoreBridge.SetSetting, not Windows Credential Manager: The UI comments mention Windows Credential Manager but the code calls _core.SetSetting('openai_api_key', key) which stores it in plain text in the SQLite database. For production, these should use the Windows Credential Manager (CredWrite/CredRead) or DPAPI.

- **[MINOR] [medium]** Model download is a placeholder (shows dialog, no actual download): SettingsPage.DownloadModel_Click shows a dialog telling the user to manually download the model file. There is no HttpClient download logic. This is expected for a scaffold but needs implementation for a usable product.

- **[MINOR] [small]** Audio level meter uses random values instead of real data: RecordingPage.Timer_Tick generates random values (10-70) for the audio level bar. The core FFI does not expose an audio level function, so this would need a new FFI function or the app would need to compute RMS from audio samples via a callback.

### Risks

- The .sln and .csproj have never actually been built in CI because of the path mismatch -- there may be additional compile errors lurking (e.g., the PInvoke.User32 inline declarations in App.xaml.cs reference ShowWindowCommand enum and DllImport which compile fine in isolation but could conflict with a PInvoke.Windows NuGet if one is ever added).
- WinUI 3 apps built with `dotnet build` on CI may produce an unpackaged app that lacks MSIX packaging. The .csproj has EnableMsixTooling=true and WindowsAppSDKSelfContained=true, but a proper MSIX bundle needs a .appinstaller or signing, which is not set up.
- The H.NotifyIcon.WinUI library (system tray) may not work in an unpackaged deployment scenario without additional configuration.
- Multiple CoreBridge instances opening the same SQLite file simultaneously risks SQLITE_BUSY errors under concurrent use (e.g., recording on RecordingPage while HistoryPage tries to load meetings).
- The Export_Click in HistoryPage uses reflection to access _mainWindow (a private field on App) for the file picker HWND -- this is fragile and will break if the field is renamed or the access pattern changes.

### Dependencies

- Microsoft .NET 8.0 SDK (setup-dotnet@v4 with dotnet-version 8.0.x)
- Windows App SDK 1.5.240627000 (NuGet, restored by dotnet build)
- Microsoft.Windows.SDK.BuildTools 10.0.22621.3233 (NuGet)
- CommunityToolkit.Mvvm 8.2.2 (NuGet)
- CommunityToolkit.WinUI.Controls.SettingsControls 8.0.240109 (NuGet)
- H.NotifyIcon.WinUI 2.1.3 (NuGet, system tray icon)
- notetaker.dll -- the Rust core library compiled for x86_64-pc-windows-msvc (from the core CI job)
- Windows 10 SDK 10.0.22621 (included in windows-latest GitHub runner)
- Windows 10 2004+ at runtime (for WDA_EXCLUDEFROMCAPTURE support)

---

## Linux GTK4 App (linux/)

**Current State:** The Linux GTK4 app is a Rust application using gtk4 0.9 (v4_12 feature) and libadwaita 0.7 (v1_4 feature). It links against notetaker-core as a path dependency through the workspace (root Cargo.toml lists both core and linux as members). The crate is imported as `notetaker` (the lib name in core/Cargo.toml).

COMPILATION AND LINKAGE: The app compiles against notetaker-core correctly. AudioCapture uses an internal command-channel + thread architecture (cmd_tx/thread_handle), NOT holding the cpal::Stream directly, so it is Send-safe and works inside the Arc<Mutex<Option<AudioCapture>>> in core_bridge.rs. The core_bridge correctly calls AudioCapture::new(device_id, audio_tx, level_callback) matching the real signature. CI installs libgtk-4-dev, libadwaita-1-dev, cmake, and libasound2-dev in the app-linux job.

UI COMPLETENESS: Four views exist behind an AdwViewSwitcherBar/AdwViewSwitcherTitle navigation pattern: (1) Recording view with title entry, timer (1s tick), audio level bar, record/stop toggle, and live transcript in a scrolled TextView. (2) History view with search entry and AdwActionRow list populated from the database. (3) Notes view with summary frame, collapsible Key Points/Action Items/Decisions expanders, full transcript expander, and Generate Summary / Export buttons. (4) Settings view with audio device ComboRow (async populated), OpenAI/Anthropic API key PasswordEntryRows, cloud transcription SwitchRow, Whisper model size ComboRow, download button, and data management rows. All views use AdwClamp(600) for comfortable reading width.

TRANSCRIPTION PIPELINE: Currently a placeholder drain. The spawn_blocking in start_recording creates an audio channel, starts AudioCapture, and spawns a thread that reads audio chunks via audio_rx.recv() but discards them with a TODO comment. Real segments are never produced.

STEALTH: Binary is named "dbus-monitor-session" (via [[bin]] in Cargo.toml). App ID is "org.freedesktop.SessionMonitor". Window title is "Session Monitor". Data directory uses "session-monitor". apply_capture_exclusion_hints() is a documented no-op placeholder acknowledging that GTK4/Wayland/X11 have no reliable API for capture exclusion -- this is an accepted platform limitation, not a bug.

CORE BRIDGE WIRING: CoreBridge wraps Database, AudioCapture, and meeting state. It exposes start/stop/pause_recording, subscribe_transcripts (async_channel), list/get/delete/export meetings, summarize, list_audio_devices, and get/set_setting. All async methods use rt.spawn_blocking correctly.

### Gaps

- **[BLOCKER] [small]** CI artifact upload path mismatch -- binary never downloadable: build.yml line 217 uploads 'target/release/notetaker-linux' but the [[bin]] name in linux/Cargo.toml is 'dbus-monitor-session', so the actual binary is at 'target/release/dbus-monitor-session'. With 'if-no-files-found: warn', the artifact is silently empty. This directly blocks the user's stated goal of downloading from GitHub. Fix: change the upload path to 'target/release/dbus-monitor-session'.

- **[MAJOR] [medium]** Transcription pipeline is a placeholder drain -- no whisper integration: In core_bridge.rs start_recording(), the thread spawned to read audio_rx just drains chunks with no processing. The core library already has LocalTranscriber::new(model_path, audio_rx, db, meeting_id, callback) that loads a whisper model, reads audio, and persists segments. Wiring it in requires: (a) composing the model file path from the models directory + whisper_model setting + '.bin' extension, (b) creating a TranscriptCallback that converts (&str, f64) into TranscriptSegment and sends on transcript_tx, (c) adding an Arc<Mutex<Option<LocalTranscriber>>> slot to CoreBridge so stop_recording can call .take().stop(). However, there is a prerequisite blocker: no model download mechanism exists (see next gap), so LocalTranscriber would fail on load every time without a pre-placed model file.

- **[MAJOR] [medium]** Whisper model download button has no click handler: In settings.rs, download_btn ('Download Selected Model') is created and appended to the UI but has no connect_clicked handler. There is a scripts/download-model.sh in the repo but no in-app download path. Without this, local transcription cannot work because no model file will exist. Needs an HTTP download (e.g., from Hugging Face ggml model URLs) with a progress indicator, writing to the model_path directory.

- **[MAJOR] [medium]** Notes view is unreachable -- no navigation from history to notes: In notes.rs, current_meeting_id is initialized as Rc<RefCell<Option<i64>>> = None and is never set by any code path. In history.rs, each AdwActionRow is marked activatable(true) but list_box has no connect_row_activated handler. There is no cross-view state mechanism (no shared SelectedMeeting signal, no NavigationView push to a detail page). The Generate Summary and Export buttons check for current_meeting_id and always find None. The entire Notes tab is inert.

- **[MAJOR] [medium]** Summarize never calls the actual AI summarization engine: CoreBridge::summarize() assembles the transcript text from DB segments but returns a hardcoded format string ('Transcript (N segments, M chars). Cloud summarization requires an API key...') instead of calling notetaker::summarize. Additionally, linux/Cargo.toml does not enable the 'cloud' feature flag on notetaker-core, which gates the actual Claude API summarization code in core/src/summarize.rs.

- **[MINOR] [small]** Audio device selection not passed to start_recording: Settings view populates the device ComboRow from list_audio_devices() and the recording view calls bridge.start_recording(None), always using the system default device. The ComboRow selection is never stored or retrieved, so user device selection is ignored.

- **[MINOR] [small]** Title entry in recording view is decorative only: The title_entry widget text is never read or used. CoreBridge::start_recording hardcodes 'Untitled Meeting' via db.create_meeting(). There is no update_meeting_title method in the bridge or in Database (Database has no title-update method).

- **[MINOR] [small]** Export writes to stderr only, no file save dialog: In notes.rs, the export_btn handler calls bridge.export_meeting(mid, 'markdown') and eprintln!s the result with a TODO comment about GtkFileDialog. The export content never reaches a file. Export All and Clear All Data rows in settings also have no click handlers.

- **[MINOR] [small]** No error feedback to user -- all errors go to stderr: Every error path in the app (failed recording start/stop, failed meeting load, failed export) uses eprintln!. There are no AdwToast, AdwMessageDialog, or any other user-visible error indicators. The app silently fails from the user's perspective.

- **[MINOR] [small]** set_hide_on_close(true) with no re-show mechanism: Window has set_hide_on_close(true) but there is no system tray icon, StatusNotifierItem, or GtkApplication::activate re-present logic. Closing the window hides it with no way to bring it back except re-launching the process, which would create a second window in connect_activate.

- **[MINOR] [small]** History search is client-side title filter only, ignores FTS5: The history view fetches all meetings (limit 50) and does a client-side .contains() on the title. The core Database has search_meetings() using FTS5 full-text search across transcripts, but it is never called. Search does not cover transcript content.

- **[MINOR] [small]** Pause recording has no UI control: CoreBridge exposes pause_recording() (and AudioCapture has both pause and resume), but the recording view only has a Record/Stop toggle. There is no pause/resume button.

- **[MINOR] [large]** No packaging for distribution (AppImage, Flatpak, .deb): CI produces a bare dynamically-linked ELF binary requiring libgtk-4 >= 4.12 and libadwaita >= 1.4 at runtime. There is no .desktop file, no icon, no AppImage/Flatpak/deb packaging. For the user's goal of 'download from GitHub and run', the raw binary requires the user to manually install exact GTK4/libadwaita versions.

### Risks

- CI artifact is currently empty (blocker): the path mismatch means no one can download the Linux app from GitHub Actions today, directly defeating the stated project goal
- Runtime library availability: the bare ELF binary requires exact GTK4/libadwaita versions that are not universally installed; ubuntu-latest (24.04) provides them but older distros or Fedora/Arch may have version mismatches
- The core-linux artifact is downloaded into dist/core/ in the app-linux CI job but never used -- cargo rebuilds core from the path dep; this is wasted CI time but not a correctness issue
- No model download path exists: even after wiring LocalTranscriber, the app cannot function for transcription without a manually placed whisper model file, making the first-run experience broken
- API keys stored in plaintext SQLite via set_setting -- no encryption, no keyring integration; the PasswordEntryRow in the UI gives a false sense of security
- ubuntu-latest libadwaita version (1.5 on 24.04) satisfies v1_4 feature today, but GitHub could change the runner image; pinning the runner version would be safer

### Dependencies

- gtk4 >= 4.12 (libgtk-4-dev) -- CI installs this; runtime needs matching shared libs
- libadwaita >= 1.4 (libadwaita-1-dev) -- CI installs this; ubuntu-latest (24.04) ships 1.5
- notetaker-core (path dependency ../core) -- workspace member, compiles from source in CI
- cpal (via notetaker-core) -- audio capture; needs libasound2-dev at build time (installed in CI)
- whisper-rs / whisper.cpp (via notetaker-core) -- needs cmake at build time (installed in CI) and a downloaded GGML model file at runtime
- tokio multi-thread runtime -- async bridge between GTK main loop and blocking core operations
- async-channel -- transcript segment delivery from core to GTK UI
- SQLite (via rusqlite in notetaker-core) -- local storage with FTS5

---

## Android App (android/)

**Current State:** The Android app is the most complete UI layer in the repository. It has a valid Gradle project structure (root build.gradle.kts, settings.gradle.kts, app/build.gradle.kts, gradle.properties) using AGP 8.5.2, Kotlin 2.0.21, Compose BOM 2024.10.01, and JDK 17. The package is com.system.service.helper with the project named "SystemService".

FILES PRESENT (13 files):
- android/build.gradle.kts -- root Gradle config with AGP 8.5.2, Kotlin 2.0.21
- android/settings.gradle.kts -- rootProject.name = "SystemService"
- android/gradle.properties -- standard Gradle/AndroidX properties
- android/app/build.gradle.kts -- app module: compileSdk 34, minSdk 26, abiFilters arm64-v8a + x86_64
- android/app/src/main/AndroidManifest.xml -- RECORD_AUDIO, FOREGROUND_SERVICE, POST_NOTIFICATIONS
- MainActivity.kt -- full implementation: FLAG_SECURE, CoreBridge.init(), bottom nav, permission handling
- CoreBridge.kt -- JNI bridge object: System.loadLibrary("notetaker"), 14 external native declarations, Kotlin Flow-based reactive state
- RecordingService.kt -- foreground service with microphone type, notification with stop action
- RecordingScreen.kt -- live recording UI: timer, audio level bar, real-time transcript LazyColumn, pulsing indicator
- HistoryScreen.kt -- meeting list with SearchBar, swipe-to-dismiss delete, JSON parsing from CoreBridge
- NotesScreen.kt -- meeting detail view: summary card with generate button, expandable sections (key points, action items, decisions, transcript), share via Intent
- SettingsScreen.kt -- audio device picker, cloud transcription toggle, Whisper model picker, API key fields with show/hide, about card
- Theme.kt -- Material3 theme with custom light/dark color schemes, dynamic color support on Android 12+

STEALTH: FLAG_SECURE is set in MainActivity.onCreate() before setContent. Package name (com.system.service.helper), app label ("System Service"), notification title ("System Service"), notification text ("Background task running"), channel name ("Background Tasks") are all generic. No custom launcher icon (uses android:drawable/sym_def_app_icon). The Settings "About" card shows "System Service" as the name.

COMPOSE VIEWS: All four screens are real, fully implemented Compose UIs -- not stubs. They use modern Material3 APIs (SearchBar with inputField lambda, SwipeToDismissBox, ExposedDropdownMenuBox with MenuAnchorType, surfaceContainerLow, AutoMirrored icons). The API set is consistent with BOM 2024.10.01 / material3 1.3.0.

JNI LOADING: CoreBridge.kt calls System.loadLibrary("notetaker") in its object init block and declares 14 external native functions matching the convention Java_com_system_service_helper_native_CoreBridge_nativeXxx. It also defines a @JvmStatic onTranscript callback for the native side to call back into Kotlin.

CI: The existing build.yml has an app-android job that (a) downloads core-linux artifacts, (b) copies jniLibs from dist/core/android/jniLibs/, (c) checks for android/gradlew and runs assembleDebug, (d) uploads the debug APK. However, the CI step is gated on `if [ -f "android/gradlew" ]` and there is NO gradlew or gradle/wrapper/ directory, so it always skips.

### Gaps

- **[BLOCKER] [small]** No Gradle wrapper (gradlew, gradle/wrapper/) -- CI always skips Android build: The CI job checks `if [ -f android/gradlew ]` and skips when absent. There is no gradlew script, no gradle-wrapper.jar, no gradle-wrapper.properties. Without these, the Android app cannot build in CI at all. Fix: either generate the wrapper locally (`cd android && gradle wrapper --gradle-version 8.7`) and commit it, or switch CI to use gradle/actions/setup-gradle@v4 which provisions Gradle without a committed wrapper. AGP 8.5.2 requires Gradle 8.7+.

- **[BLOCKER] [large]** No JNI bridge implementation in the Rust core -- all 14 native methods will throw UnsatisfiedLinkError: CoreBridge.kt declares 14 external native functions (nativeInit, nativeDestroy, nativeStartRecording, etc.) expecting JNI symbols like Java_com_system_service_helper_native_CoreBridge_nativeInit. The core Rust library (core/src/lib.rs) exports only C FFI functions (note_init, note_destroy, etc.) and has zero JNI code -- no jni crate dependency in Cargo.toml, no Java_com_* symbols anywhere in the codebase. Even with a perfectly built libnotetaker.so loaded via System.loadLibrary, every call from Kotlin will crash with UnsatisfiedLinkError. Needs: (1) add `jni = "0.21"` to Cargo.toml, (2) create a new module (e.g. core/src/android_jni.rs gated with #[cfg(target_os="android")]) implementing all 14 JNI entry points that delegate to the existing note_* C functions, (3) implement the onTranscript callback via JNI (store a global JavaVM reference, AttachCurrentThread on the transcriber thread, call CoreBridge.onTranscript). This is approximately 400-600 lines of Rust.

- **[MAJOR] [medium]** No Whisper model bundled or downloaded -- transcription will fail on first use: MainActivity passes filesDir.resolve("models") (a directory path) to CoreBridge.init(), but note_init expects a path to an actual GGML model file. No model file is bundled in the APK assets, and there is no download-on-first-launch logic anywhere in the Android app. The tiny model is ~75MB. Options: (1) bundle in assets/ and copy to filesDir on first launch (inflates APK to ~80MB), (2) download on first launch from a hosted URL (better UX, needs a download progress screen), (3) use Android App Bundle dynamic delivery. CI would need a step to run scripts/download-model.sh and place the model in the right location.

- **[MAJOR] [medium]** Meeting JSON contract mismatch -- NotesScreen parses fields that do not exist on the Meeting struct: NotesScreen.kt parses key_points, action_items, decisions, and transcript from the JSON returned by note_get_meeting. But core/src/models.rs Meeting struct only has id, title, started_at, ended_at, duration_secs, and summary -- no key_points, action_items, decisions, or transcript fields. These will always be empty lists/empty strings in the UI. The transcript segments are stored separately (in TranscriptSegment rows) and there is no FFI function to retrieve them for a given meeting. Needs either: (1) a new note_get_meeting_full FFI function that joins segments, or (2) a note_get_segments FFI function + corresponding JNI bridge + Kotlin data layer.

- **[MAJOR] [small]** Settings key mismatch for Claude API key -- summarization can never find the key: SettingsScreen.kt writes the Claude API key under the setting name 'claude_api_key'. But note_summarize in lib.rs reads the setting 'anthropic_api_key'. These never match, so summarization always fails with 'No API key configured' even when the user enters one. Fix: change either the Kotlin key or the Rust key to match.

- **[MAJOR] [small]** Core Android build never enables the 'cloud' feature flag -- note_summarize always returns null: build-core.sh's build_android function runs cargo ndk build without --features cloud. The summarize module's CloudSummarizer is behind #[cfg(feature = "cloud")], so on Android the note_summarize function unconditionally returns null (with an error log 'Summarization requires the cloud feature'). The UI's 'Generate Summary' button will never produce a result. Fix: pass --features cloud to the cargo ndk build command. Note: this pulls in reqwest, which on Android should use rustls-tls to avoid OpenSSL cross-compilation issues with the NDK.

- **[MAJOR] [medium]** nativeGetAudioLevel has no backing FFI function in core -- audio level meter will crash: CoreBridge.kt declares `external fun nativeGetAudioLevel(handle: Long): Float` and RecordingScreen polls it every 100ms while recording. But lib.rs has no note_get_audio_level FFI function. AudioCapture::new accepts an AudioLevelCallback, but (a) note_start_recording passes None for it, and (b) there is no mechanism to query the level after the fact. This will be an UnsatisfiedLinkError (subsumed by the broader JNI gap), but even after JNI is implemented, the level metering needs: a new FFI function note_get_audio_level, storage of the last level in NoteHandle, and wiring the AudioLevelCallback in note_start_recording to update it.

- **[MAJOR] [small]** Timestamp units mismatch between core and Kotlin -- transcript timestamps will display wrong: The C FFI transcript callback signature is (const char* text, double timestamp_secs, void* ctx) -- the second parameter is seconds. CoreBridge.onTranscript receives it as timestampMs and does .toLong(). RecordingScreen.formatTimestamp divides by 1000. So a 12.5-second timestamp becomes 12ms, which displays as 00:00. Fix: either multiply by 1000 in onTranscript (change to (timestampSecs * 1000).toLong()), or rename the field and fix formatTimestamp to treat it as seconds.

- **[MINOR] [small]** nativeStartRecording passes empty string instead of null for default device: CoreBridge.startRecording calls nativeStartRecording(handle, deviceId ?: ""). An empty string will be passed to core as Some("") rather than None, causing AudioCapture to try to find a device with an empty-string ID (which will fail) instead of using the system default. Fix: pass null from Kotlin and handle it in JNI as None.

- **[MINOR] [small]** armeabi-v7a built by core but excluded by app abiFilters -- wasted CI time: build-core.sh builds for armeabi-v7a (32-bit ARM) but app/build.gradle.kts abiFilters only includes arm64-v8a and x86_64. The 32-bit .so is built and discarded. Fix: either add armeabi-v7a to abiFilters (supports older devices) or remove it from the cargo ndk build targets.

- **[MINOR] [small]** No explicit kotlinx-coroutines-android dependency: CoreBridge uses MutableSharedFlow/MutableStateFlow, NotesScreen uses withContext(Dispatchers.IO), and RecordingScreen uses collectAsStateWithLifecycle -- all requiring coroutines. The dependency resolves transitively through Compose and lifecycle-runtime-ktx, but is not declared explicitly. This is fragile if transitive deps change. Add: implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.1").

- **[MINOR] [small]** NotesScreen stores raw JSON as summary text instead of extracting the summary string: In NotesScreen, when 'Generate Summary' succeeds, it sets `meeting = m.copy(summary = result)` where result is the return value of CoreBridge.summarize() -- which is the full JSON object string from note_summarize (containing summary, key_points, action_items, etc.), not just the summary text. The card will display raw JSON. Fix: parse the JSON and extract the summary field, or have the JNI layer extract it.

### Risks

- whisper-rs cmake cross-compilation with NDK r26d: The core-linux CI job builds for Android targets, but whether the Android .so files actually end up in dist/core/android/jniLibs/ has not been verified from a successful CI run. cmake-based whisper.cpp cross-compilation to Android is historically fragile.
- APK size: Bundling even the tiny Whisper model (~75MB) will produce an ~80MB+ APK. This may require Android App Bundle (AAB) with on-demand delivery or a download-on-first-launch approach.
- cpal/oboe audio device enumeration on Android: cpal's Android backend (via oboe) may return an empty device list from note_list_audio_devices, making the Settings audio device picker show nothing. Android audio input typically goes through the system default without enumeration.
- ANR risk: nativeStartRecording is called from the foreground service's onStartCommand on the main thread. Inside core, this synchronously loads the Whisper model (LocalTranscriber::new) which can take seconds. This will trigger Android's ANR dialog. The JNI bridge should move model loading to a background thread.
- Android 12+ microphone privacy indicator: FLAG_SECURE hides the app from screen capture, but Android 12+ shows a persistent status bar indicator whenever an app uses the microphone. This cannot be suppressed and partially undermines the stealth design on newer Android versions.
- libc++_shared.so bundling: build.rs emits cargo:rustc-link-lib=c++_shared for Android. cargo-ndk is expected to copy libc++_shared.so into jniLibs alongside libnotetaker.so. If it does not, the app will crash at library load time with an UnsatisfiedLinkError for c++ symbols.
- No release signing configuration: app/build.gradle.kts has no signingConfigs for release builds. assembleDebug produces a debug APK suitable for sideloading but not for Play Store distribution. A signing key + CI secrets would be needed for release.

### Dependencies

- Gradle wrapper (gradle-wrapper.jar + gradle-wrapper.properties + gradlew script) or gradle/actions/setup-gradle in CI -- required for any Android build
- jni crate (version 0.21+) added to core/Cargo.toml -- required for the JNI bridge implementation
- Android NDK r26d (already configured in CI via nttld/setup-ndk) -- required for cross-compiling the Rust core to Android targets
- cargo-ndk (already installed in CI) -- required for building the Rust cdylib for Android ABIs
- Java 17 (Temurin, already configured in CI via actions/setup-java) -- required by AGP 8.5.2
- Whisper GGML model file (tiny ~75MB) -- must be either bundled in APK assets or downloaded at runtime for transcription to work
- reqwest with rustls-tls feature -- needed if cloud feature is enabled for Android (avoids OpenSSL cross-compile issues with NDK)
- libc++_shared.so -- cargo-ndk should bundle this alongside libnotetaker.so (build.rs already links it); verify it ends up in jniLibs

---

## CI/CD Pipeline and Distribution Strategy

**Current State:** The project has a single GitHub Actions workflow (.github/workflows/build.yml) with 8 jobs: 3 core library builds (macOS, Windows, Linux) and 5 app builds (macOS, iOS, Windows, Linux, Android). The core library builds succeed on all three runners and upload real artifacts to dist/core/ (macOS: 2 files including universal dylib; Windows: 2 files including DLL + lib; Linux: 1 file for .so). However, ALL FIVE app build jobs "succeed" only by printing warnings and skipping -- none produce real app binaries. Specific findings:

1. **macOS App**: CI checks for `apple/` dir but the runner does not see it (the checkout appears to not include the directory, or the `xcodebuild -scheme macOS` fails because there is no .xcodeproj file -- only Swift source files exist under apple/NoteTaker/ and apple/iOS/ with no Xcode project). The CI path expects `apple/build/Release/*.app` which is never created. upload-artifact with `if-no-files-found: warn` causes it to pass silently.

2. **iOS App**: Same issue as macOS -- no .xcodeproj exists. The CI logs show "apple/ directory not found -- skipping iOS app build". upload-artifact finds no .app at `apple/build/Release-iphoneos/*.app`.

3. **Windows App**: CI looks for `windows/NoteHelper.sln` at the repo root but the actual .sln is at `windows/NoteHelper/NoteHelper.sln` (one directory deeper). The path mismatch causes the skip. upload-artifact finds nothing at `windows/bin/x64/Release/`.

4. **Linux App**: CI runs `cargo build --release -p notetaker-linux` but the binary name is `dbus-monitor-session` (per `[[bin]] name = "dbus-monitor-session"` in linux/Cargo.toml), not `notetaker-linux`. The CI actually builds it successfully but the upload path `target/release/notetaker-linux` does not match the actual binary name. The log shows the build did run (took ~90s) but the upload found nothing.

5. **Android App**: No `gradlew` wrapper exists in the android/ directory (confirmed: only build.gradle.kts, settings.gradle.kts, gradle.properties, and app/ exist). The CI logs confirm "android/gradlew not found -- skipping Android app build".

The core build script (scripts/build-core.sh) is well-structured, handling all targets with proper toolchain setup, lipo for macOS universal binaries, cargo-ndk for Android, and cbindgen for C headers. The download-model.sh script supports tiny/base/small/medium Whisper models from HuggingFace.

No GitHub Releases workflow exists. No code signing is configured. No auto-update mechanism exists. Whisper models are downloaded on demand (not bundled).

### Gaps

- **[BLOCKER] [large]** No Xcode project for macOS/iOS apps: The apple/ directory contains Swift source files (NoteTaker/, iOS/, Shared/) but no .xcodeproj or .xcworkspace. The CI workflow runs xcodebuild which requires a project file. An Xcode project (or Swift Package Manager project with xcodebuild support) must be created that references the Swift sources, links to the core library (libnotetaker.dylib for macOS, libnotetaker.a for iOS), includes the C FFI header (notetaker.h), and configures build settings (deployment targets, signing, capabilities for microphone access). A Package.swift is referenced in the design spec but does not exist. This must support two schemes: one for macOS (.app bundle) and one for iOS (.app archive).

- **[BLOCKER] [small]** Windows .sln path mismatch in CI: The CI workflow checks for windows/NoteHelper.sln but the actual solution file is at windows/NoteHelper/NoteHelper.sln. The dotnet build command uses the wrong path. Fix: change the CI path to windows/NoteHelper/NoteHelper.sln. Additionally, the .csproj references the core DLL at a relative path (../../../core/target/release/notetaker.dll) but CI downloads the core artifact to dist/core/windows/. The CI needs to either copy notetaker.dll to the expected location or override the reference path during build.

- **[BLOCKER] [small]** Android missing Gradle wrapper: The android/ directory has Gradle build files but no gradlew/gradlew.bat wrapper. The CI skips the entire Android build because of this. Fix: run 'gradle wrapper' to generate gradlew, gradlew.bat, and gradle/wrapper/ directory with the correct Gradle distribution URL. This is a one-command fix but is blocking the entire Android CI pipeline.

- **[BLOCKER] [small]** Linux binary name mismatch in CI: The linux/Cargo.toml defines [[bin]] name = 'dbus-monitor-session' (the stealth process name), but the CI upload-artifact path expects target/release/notetaker-linux. The Linux app actually compiles successfully in CI but the artifact is never uploaded because the binary name doesn't match. Fix: change the CI upload path to target/release/dbus-monitor-session.

- **[BLOCKER] [medium]** No GitHub Releases workflow for downloadable artifacts: The user wants to download built artifacts from GitHub rather than building locally. Currently there is only a CI build workflow with 7-day retention artifacts. A release workflow is needed that triggers on version tags (e.g., v0.1.0), builds all platforms, creates a GitHub Release, and attaches downloadable binaries: macOS .app in a .dmg or .zip, Windows .msix or portable .zip, Linux AppImage or .deb or tarball, Android .apk, iOS .ipa (or TestFlight). The workflow should also include the appropriate Whisper model (or provide a first-run download mechanism).

- **[MAJOR] [large]** No code signing for any platform: macOS requires code signing and notarization for Gatekeeper (otherwise users get 'unidentified developer' errors). iOS requires provisioning profiles and code signing for any installation. Windows SmartScreen warns on unsigned executables. Android requires APK signing for installation. The CI needs: (1) Apple Developer certificate and provisioning profiles stored as GitHub secrets, with a keychain setup step; (2) Windows code signing certificate or self-signed for testing; (3) Android keystore for release signing; (4) Apple notarization credentials for macOS. Without at least macOS notarization and Android signing, the artifacts will be difficult or impossible for end users to install.

- **[MAJOR] [medium]** No macOS .dmg or installer packaging: Even once the macOS app builds, the CI only uploads a bare .app bundle. For distribution, a .dmg disk image (with drag-to-Applications layout) or a .pkg installer should be created. Tools like create-dmg can automate this in CI. The .dmg should include the .app bundle and a symlink to /Applications.

- **[MAJOR] [medium]** No Windows installer/MSIX packaging: The Windows .csproj has EnableMsixTooling=true and WindowsAppSDKSelfContained=true but there's no CI step to produce an MSIX package or portable installer. The build output is just loose files in bin/x64/Release/. For distribution, either MSIX packaging (for Microsoft Store or sideloading) or an NSIS/WiX installer should be produced. The MSIX route requires a signing certificate.

- **[MAJOR] [medium]** No Linux packaging (AppImage/deb/Flatpak): The Linux build produces a single binary but no distributable package. For users to install without building, an AppImage (most portable, no root needed), .deb package (for Debian/Ubuntu), or Flatpak should be created. An AppImage would bundle the binary, the core .so library, and an AppRun script. The binary also needs the Whisper model, GTK4, and libadwaita at runtime.

- **[MAJOR] [medium]** No Whisper model distribution strategy: The Whisper GGML models (75MB for tiny, up to 1.5GB for medium) are downloaded via scripts/download-model.sh but there's no integration with the app for first-run download. Options: (1) Bundle the tiny model in the app artifact (adds ~75MB to every download); (2) Download on first launch with a progress UI; (3) Offer model selection in settings and download on demand. Option 2 or 3 is recommended. The download URL (huggingface.co) needs to be accessible from the app, and the model path needs to be configured per platform (app support directory on macOS, AppData on Windows, XDG on Linux, app files on Android).

- **[MINOR] [large]** No auto-update mechanism: Once distributed, users need a way to get updates. Options per platform: macOS Sparkle framework, Windows WinGet/MSIX auto-update, Linux AppImage with appimageupdatetool or system package manager, Android Play Store or in-app APK update. For an MVP, GitHub Releases with manual download is acceptable, with auto-update as a later enhancement.

- **[MAJOR] [large]** iOS distribution requires TestFlight or ad-hoc provisioning: iOS apps cannot be distributed as downloadable .ipa files to arbitrary users without either TestFlight (requires Apple Developer Program, $99/year, App Store Connect setup) or enterprise/ad-hoc provisioning (requires device UDIDs). For a privacy-focused tool that avoids app stores, ad-hoc distribution with a limited set of registered devices is the only option outside TestFlight. This is a fundamental platform limitation that affects the distribution strategy.

- **[MAJOR] [small]** CI workflow does not fail on app build skip: All app build jobs use if-no-files-found: warn on upload-artifact, and the build steps use || echo '::warning::...' which swallows the exit code. This means CI always shows green even when nothing is built. Once builds are real, these should be changed to if-no-files-found: error and the build steps should fail properly, so broken builds are caught.

### Risks

- The core library compiles on all platforms in CI, but none of the five app builds produce real artifacts -- every app job silently skips. The green CI status is misleading.
- The Apple apps have no Xcode project, which is the largest single piece of work. Creating a proper .xcodeproj that links the Rust FFI library, includes the bridging header, and configures entitlements (microphone, screen capture) is non-trivial.
- macOS notarization is required for users to run the app without Gatekeeper warnings. Without an Apple Developer account and notarization setup, macOS distribution is impractical for non-technical users.
- iOS distribution is fundamentally gated by Apple's ecosystem -- no way to distribute a downloadable .ipa to arbitrary users without TestFlight or enterprise certificates.
- The Windows .csproj targets WinUI 3 with WindowsAppSDK self-contained, which produces large binaries (~100MB+). MSIX packaging adds complexity and requires a signing certificate for sideloading.
- Whisper model size (75MB-1.5GB) will significantly affect app download/install size and first-run experience. Bundling even the tiny model increases artifact size substantially.
- The stealth/generic process names (dbus-monitor-session on Linux, svchost-user on Windows) may trigger false positives in antivirus software, especially the Windows name which mimics a system process.
- Android distribution outside the Play Store requires users to enable 'install from unknown sources', which is a UX friction point for a privacy-focused tool.
- The core library uses whisper-rs 0.13 which builds whisper.cpp from source via cmake -- this is a slow CI step (~5-8 minutes) and may break with runner environment changes.
- No CI caching strategy for the Whisper cpp build -- each core library build recompiles whisper.cpp from source, contributing to the ~14 minute total CI time.

### Dependencies

- Apple Developer Program membership ($99/year) for macOS code signing, notarization, and iOS provisioning profiles
- Xcode project file (.xcodeproj) or Swift Package Manager configuration for macOS and iOS apps -- currently missing entirely
- Android Gradle wrapper (gradlew) -- trivial to generate but blocking the build
- Code signing certificates stored as GitHub Actions secrets: Apple (p12 + provisioning), Windows (optional, Authenticode), Android (keystore)
- Whisper model hosting (currently HuggingFace, which is fine) and a first-run download UX in each app
- GitHub Actions runner availability: macos-latest for Apple builds, windows-latest for Windows, ubuntu-latest for Linux/Android
- Windows App SDK 1.5 and .NET 8 SDK on the Windows runner (already set up in CI)
- Android NDK r26d on the Linux runner (already set up in CI)
- GTK4 and libadwaita development libraries on the Linux runner (already set up in CI)

---

## End-to-end feature completeness of AI Note Taker (core library + 5 platform apps + CI/CD)

**Current State:** The project has a well-structured Rust core library (core/) that compiles and implements: (1) audio capture via cpal with resampling to 16kHz mono f32, multi-format support (f32/i16/u16), device enumeration, and RMS level metering; (2) local transcription via whisper-rs with streaming 4-second chunks, confidence scoring, and automatic database persistence; (3) SQLite storage with WAL mode, FTS5 full-text search, CRUD for meetings/segments/notes/settings, and export in markdown/text/JSON; (4) Claude API summarization behind a `cloud` feature flag with structured JSON output (summary, key points, action items, decisions); (5) a complete C FFI surface with error codes, panic guards, opaque handle, and cbindgen header generation.

Five platform apps exist as source code: Linux (GTK4/libadwaita, compiles and has full UI with 4 views), macOS (SwiftUI with StealthManager, CoreBridge, views), Windows (WinUI 3 with P/Invoke CoreBridge, StealthService, XAML views), iOS (SwiftUI with stealth and audio managers), Android (Jetpack Compose with JNI bridge, foreground service, permission handling). All have recording, history, notes, and settings views scaffolded.

CI exists as a GitHub Actions workflow (build.yml) that builds the core library on 3 runners (macOS, Windows, Linux) and attempts to build all 5 platform apps, uploading artifacts. The core library builds succeed on all platforms. The Linux app compiles successfully.

### Gaps

- **[BLOCKER] [large]** No Xcode project exists -- macOS and iOS apps cannot build: The apple/ directory contains only Swift source files and Info.plists. There is no .xcodeproj, .xcworkspace, or Package.swift anywhere. CI's xcodebuild commands are silently swallowed by '|| echo warning'. The Makefile references 'apple/NoteHelper.xcodeproj' which does not exist. Need to create an Xcode project (or SPM Package.swift) with a modulemap for the NoteCore C FFI header, link the static/dylib core library, configure code signing (ad-hoc for CI), and set up schemes for both macOS and iOS targets.

- **[BLOCKER] [large]** Android JNI layer does not exist -- native methods have no implementation: android/.../CoreBridge.kt declares 14 'external fun' JNI methods (nativeInit, nativeStartRecording, nativeSummarize, nativeGetAudioLevel, etc.) but core/src/lib.rs exports zero JNI symbols. The Rust core only has C FFI functions (note_init, note_start_recording, etc.), not the Java_com_system_service_helper_native_CoreBridge_* symbols that JNI requires. A new jni.rs module or a JNI wrapper crate is needed. Additionally, nativeGetAudioLevel has no core counterpart at all. Also, android/gradlew does not exist, so CI skips the build entirely.

- **[BLOCKER] [medium]** No Whisper model in any artifact -- transcription fails immediately at runtime: note_start_recording creates a LocalTranscriber that calls WhisperContext::new_with_params with the model path. Without a .bin model file, this returns an error and recording fails. The design spec says 'bundled tiny model (~75MB)' but nothing in CI downloads or bundles a model. The download-model.sh script exists but is never called by CI or any app. Need a CI step to download ggml-tiny.bin and include it in each platform artifact, plus in-app model download UI for larger models.

- **[BLOCKER] [small]** Linux core_bridge does not wire transcription -- audio is captured but discarded: linux/src/core_bridge.rs:72-83 spawns a thread that drains audio_rx with a TODO comment: 'feed samples into LocalTranscriber and emit real segments. For now the pipeline just drains.' The _model_path variable is unused (note the underscore prefix). Additionally, main.rs:25 passes a directory path as model_path, but LocalTranscriber::new expects a file path (e.g., models/ggml-tiny.bin). The Linux app captures audio and shows the UI but produces no transcription.

- **[BLOCKER] [small]** Windows CI references wrong .sln path -- build silently skipped: CI build.yml:163 checks for 'windows/NoteHelper.sln' but the file is at 'windows/NoteHelper/NoteHelper.sln' (inside a subdirectory). The conditional fails silently, emitting only a CI warning. The Makefile also has the wrong path: 'dotnet build windows/NoteHelper.sln'. Both need to be corrected to 'windows/NoteHelper/NoteHelper.sln'.

- **[BLOCKER] [small]** Linux CI artifact upload path is wrong -- binary is not uploaded: CI build.yml:218 uploads 'target/release/notetaker-linux' but linux/Cargo.toml names the binary 'dbus-monitor-session' (the stealth process name). The actual binary is at target/release/dbus-monitor-session. The upload finds no file and silently emits 'if-no-files-found: warn'. Fix the path in build.yml to target/release/dbus-monitor-session.

- **[MAJOR] [small]** cloud feature never enabled in CI -- summarization always returns null: core/Cargo.toml has 'default = []' and build-core.sh runs plain 'cargo build --lib' without --features cloud. The linux/Cargo.toml depends on notetaker-core with default features. So note_summarize always hits the #[cfg(not(feature = "cloud"))] branch that returns null with 'Summarization requires the cloud feature'. Need to add a CI variant or a feature flag in linux/Cargo.toml: notetaker-core = { path = "../core", features = ["cloud"] }.

- **[MAJOR] [small]** FTS5 search not exposed over FFI -- search is dead code from all apps: Database::search_meetings() exists in storage.rs and works correctly, but there is no note_search_meetings FFI function in lib.rs (nor in the design spec's FFI surface). The Linux history view does client-side title-only filtering instead of using FTS5. Same gap for insert_note/get_notes -- the Note CRUD is in storage.rs but has no FFI export, so AI-generated summary sections cannot be saved as structured notes from any FFI consumer.

- **[MAJOR] [small]** FFI missing resume recording function: AudioCapture has start/stop/pause/resume methods. The FFI exports note_start_recording, note_stop_recording, note_pause_recording but not note_resume_recording. Once paused, there is no FFI call to resume. The Swift CoreBridge has no resumeRecording method either. Need to add note_resume_recording to lib.rs.

- **[MAJOR] [small]** Audio level metering not exposed over FFI -- only Linux gets levels: lib.rs:220 passes None as the level_callback to AudioCapture::new, so no audio level data is available to any FFI consumer. The Linux core_bridge wires its own level callback directly. macOS, Windows, iOS, and Android apps cannot show audio level meters without a note_get_audio_level() FFI function or a callback mechanism.

- **[MAJOR] [small]** note_start_recording leaks state on failure -- leaves handle in broken state: lib.rs:214 sets current_meeting_id before AudioCapture::new and LocalTranscriber::new which can fail (e.g., missing model file). On failure, the function returns NOTE_ERR_INTERNAL but leaves current_meeting_id set, so all subsequent start_recording calls fail with 'a recording session is already active'. An orphan meeting row is left in the database too. Need to clear current_meeting_id on error paths.

- **[MAJOR] [medium]** README claims SQLCipher encryption -- actually plain SQLite: README.md states 'Encrypted local storage via SQLite with SQLCipher' and the design spec says 'SQLite with SQLCipher encryption'. However, core/Cargo.toml uses rusqlite = { features = ["bundled"] } without the 'bundled-sqlcipher' feature. The database is unencrypted. Either switch to features = ["bundled-sqlcipher"] and add PRAGMA key handling, or correct the documentation.

- **[MAJOR] [medium]** API keys stored in plaintext SQLite -- not in platform keychain as spec claims: The design spec says 'API keys in platform keychain (Keychain/Credential Manager/Secret Service/Keystore)'. In reality, linux/src/views/settings.rs:128 saves API keys via bridge.set_setting('anthropic_api_key', &text) which stores them as plain text in the SQLite settings table. Same pattern in all other platforms via the FFI set_setting call. Platform keychain integration is not implemented anywhere.

- **[MAJOR] [medium]** No tests beyond one trivial stub -- core correctness unverified: The only test is summarize.rs:test_stub_summarizer which exercises a mock that returns hard-coded data. There are no tests for: audio resampling correctness, storage CRUD operations, FTS5 trigger wiring, export format generation, FFI lifecycle (init/destroy), error handling paths, or the transcription pipeline. core/tests/ directory does not exist. CI runs 'cargo test' but it only exercises the one stub test.

- **[MAJOR] [medium]** No headless/CLI proof-of-pipeline -- cannot verify transcription works in CI: There is no way to verify the mic-to-text pipeline works without a GUI and a microphone. A core/examples/ or core/src/bin/ that takes a WAV file, runs it through AudioCapture's resampler and LocalTranscriber, and prints text would let CI verify the pipeline with a sample audio file. This is the fastest path to proving whisper-rs actually produces text from audio.

- **[MINOR] [medium]** Cloud transcription fallback (OpenAI Whisper API) not implemented: The design spec and Linux settings UI reference a 'Use Cloud Transcription' toggle that falls back to OpenAI Whisper API. The toggle saves a setting but nothing reads it. There is no OpenAI client code anywhere in the codebase. Only local whisper-rs transcription exists.

- **[MINOR] [small]** Transcription quality: no overlap, no context, fixed language: transcribe.rs uses set_no_context(true) and fixed 4s non-overlapping chunks with no sliding window overlap. This loses context at chunk boundaries, causing fragmented output. The spec says 'language auto-detection with manual override' but no language parameter is set (whisper defaults to English). These are quality issues, not functional blockers.

- **[MINOR] [small]** Meeting title not editable after creation -- spec requires editable titles: Database has no update_meeting_title method. All meetings are created as 'Untitled Meeting' in note_start_recording (lib.rs:206). The spec says titles should be 'editable, auto-generated from first words' but there is no FFI function to rename a meeting and no code to auto-title from transcript content.

- **[MINOR] [small]** Linux stealth is a documented no-op: linux/src/window.rs:144 apply_capture_exclusion_hints is an explicit no-op with comments explaining that GTK4/Wayland/X11 have no reliable capture exclusion API. The generic app ID and window title provide minimal obfuscation only. This is honest but means the Linux platform has no actual screen capture protection.

### Risks

- Stealth process names (svchost-user.exe, dbus-monitor-session) will likely trigger Windows Defender/SmartScreen and may confuse Linux security tools -- directly conflicts with 'download CI artifacts and run' goal
- Unsigned macOS .app from CI will be quarantined by Gatekeeper; users need xattr -cr or right-click Open to run -- ad-hoc signing in CI helps but notarization requires an Apple Developer account
- GTK4 v4_12 and libadwaita v1_4 features pin the Linux app to Ubuntu 24.04+ era libraries -- the bare binary from CI will not run on older distros without AppImage/Flatpak packaging
- whisper.cpp compilation from source adds 5-10 minutes to CI builds and may fail on resource-constrained runners; caching helps but first build or cache miss is slow
- All 'works' claims about core library are based on code reading only -- zero automated tests verify runtime behavior; a model-loading or audio-device failure in CI artifacts would be discovered only by the end user
- The 75MB+ Whisper model bundled in CI artifacts makes downloads large; GitHub artifact retention is 90 days by default and artifacts count toward storage quota
- cpal audio capture requires actual audio hardware access -- CI runners have no microphone, so the record/transcribe pipeline can never be integration-tested in CI; only a CLI tool that processes a WAV file can verify the pipeline
- Cloud summarization uses a hardcoded model ID 'claude-sonnet-4-20250514' which may not match available models when the user configures their API key

### Dependencies

- Rust 1.75+ toolchain with cross-compilation targets (aarch64-apple-darwin, x86_64-apple-darwin, aarch64-apple-ios, x86_64-pc-windows-msvc, aarch64-linux-android, etc.)
- cmake (required by whisper-rs-sys to build whisper.cpp from source)
- whisper-rs 0.13 / whisper.cpp -- compiles from source during cargo build, needs ~5-10 min first build
- Whisper GGML model file (ggml-tiny.bin ~75MB minimum, downloaded from huggingface.co/ggerganov/whisper.cpp)
- Xcode 15+ on macOS runner for macOS and iOS app builds
- .NET 8 SDK on Windows runner for WinUI 3 app build
- GTK4 4.12+ and libadwaita 1.4+ dev libraries (Ubuntu 24.04+) for Linux app build
- Android NDK r26d + cargo-ndk for Android core library cross-compilation
- Java 17 + Gradle for Android app build
- tokio runtime (always linked, even without cloud feature)
- reqwest (only with cloud feature, for Claude API summarization)
- rusqlite with bundled SQLite (includes FTS5)
- cpal 0.15 for cross-platform audio capture
- cbindgen 0.27 for C header generation from Rust FFI

---

