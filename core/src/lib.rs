//! `notetaker-core` -- shared Rust library for the AI note taker.
//!
//! This crate provides audio capture, local transcription (via whisper.cpp),
//! meeting summarization, and SQLite storage. It exposes a C FFI so that
//! native platform apps (Swift, C#, Kotlin, GTK) can link against it.
//!
//! # FFI conventions
//!
//! - Functions returning `int32_t`: 0 on success, negative error codes on failure.
//! - Functions returning `char*`: caller-owned JSON string freed with [`note_free_string`],
//!   or null on error.
//! - All pointer arguments are null-checked. Passing null is safe (returns an error code).
//! - Panics are caught with `catch_unwind` to prevent unwinding across the FFI boundary.

pub mod audio;
pub mod models;
pub mod storage;
pub mod summarize;
pub mod transcribe;

use std::ffi::{c_char, c_void, CStr, CString};
use std::panic::catch_unwind;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use log::{error, info};

use whisper_rs::WhisperContext;

use crate::audio::AudioCapture;
use crate::storage::Database;
#[cfg(feature = "cloud")]
use crate::summarize::Summarizer;
use crate::transcribe::{self, LocalTranscriber, TranscriberConfig};

// ── Error codes ──────────────────────────────────────────────────────

/// Success.
pub const NOTE_OK: i32 = 0;
/// A null pointer was passed where a valid pointer was expected.
pub const NOTE_ERR_NULL_PTR: i32 = -1;
/// An internal error occurred (see logs for details).
pub const NOTE_ERR_INTERNAL: i32 = -2;
/// The requested resource was not found.
pub const NOTE_ERR_NOT_FOUND: i32 = -3;
/// Invalid argument (e.g. bad UTF-8 in a string).
pub const NOTE_ERR_INVALID_ARG: i32 = -4;
/// A panic was caught at the FFI boundary.
pub const NOTE_ERR_PANIC: i32 = -99;

// ── Opaque handle ────────────────────────────────────────────────────

/// Wrapper for the user-provided C callback context pointer.
/// The caller guarantees the pointer remains valid for the callback's lifetime.
struct CallbackCtx(*mut c_void);
unsafe impl Send for CallbackCtx {}
unsafe impl Sync for CallbackCtx {}

impl CallbackCtx {
    fn ptr(&self) -> *mut c_void {
        self.0
    }
}

/// C function pointer type for transcript callbacks.
type TranscriptCallbackFn = unsafe extern "C" fn(*const c_char, f64, *mut c_void);

/// Opaque handle passed across the FFI boundary.
///
/// This struct is not `repr(C)` -- cbindgen renders it as an opaque pointer
/// automatically, which is the intended usage pattern.
pub struct NoteHandle {
    db: Arc<Database>,
    model_path: String,
    audio_capture: Mutex<Option<AudioCapture>>,
    transcriber: Mutex<Option<LocalTranscriber>>,
    current_meeting_id: Mutex<Option<i64>>,
    transcript_callback: Mutex<Option<(TranscriptCallbackFn, CallbackCtx)>>,
    audio_level: Arc<Mutex<f32>>,
    /// Cached Whisper context keyed by the resolved model file path, so the model
    /// is loaded once per model rather than on every recording start.
    whisper_ctx: Mutex<Option<(String, Arc<WhisperContext>)>>,
    /// Sender into the transcription pipeline when recording via `note_push_audio`
    /// (system-audio feed) rather than the built-in microphone capture.
    push_audio_tx: Mutex<Option<mpsc::Sender<Vec<f32>>>>,
    #[allow(dead_code)]
    runtime: tokio::runtime::Runtime,
}

impl NoteHandle {
    /// Resolve the Whisper model file from `model_path` (a directory, or a legacy
    /// file path whose parent is used) and the `whisper_model` setting.
    fn resolve_model_file(&self) -> String {
        let model_dir = {
            let p = std::path::Path::new(&self.model_path);
            if self.model_path.ends_with(".bin") {
                p.parent()
                    .map(|d| d.to_path_buf())
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
            } else {
                p.to_path_buf()
            }
        };
        let model_name = self
            .db
            .get_setting("whisper_model")
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "tiny".to_string());
        model_dir
            .join(format!("ggml-{}.bin", model_name))
            .to_string_lossy()
            .into_owned()
    }

    /// Return the cached Whisper context for `model_file`, loading (and caching)
    /// it if absent or if a different model was previously cached.
    fn get_or_load_ctx(&self, model_file: &str) -> Result<Arc<WhisperContext>, anyhow::Error> {
        let mut cache = self.whisper_ctx.lock().unwrap();
        if let Some((path, ctx)) = cache.as_ref() {
            if path == model_file {
                return Ok(ctx.clone());
            }
        }
        let ctx = transcribe::load_context(model_file)?;
        *cache = Some((model_file.to_string(), ctx.clone()));
        Ok(ctx)
    }

    /// Build transcription parameters from the `whisper_language` (default "en",
    /// "auto" allowed) and `whisper_threads` (default 4) settings.
    fn transcriber_config(&self) -> TranscriberConfig {
        let language = self
            .db
            .get_setting("whisper_language")
            .ok()
            .flatten()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "en".to_string());
        let n_threads = self
            .db
            .get_setting("whisper_threads")
            .ok()
            .flatten()
            .and_then(|s| s.trim().parse::<i32>().ok())
            .filter(|&n| n > 0)
            .unwrap_or(4);
        TranscriberConfig {
            language,
            n_threads,
        }
    }

    /// Build the transcript callback closure (wraps the stored C function pointer).
    fn build_transcript_callback(&self) -> Option<transcribe::TranscriptCallback> {
        let cb_lock = self.transcript_callback.lock().unwrap();
        cb_lock.as_ref().map(|(fn_ptr, ctx)| {
            let fn_ptr = *fn_ptr;
            // Capture the whole Send-safe CallbackCtx wrapper (not the inner raw
            // pointer) so the closure can move to the transcriber thread.
            let ctx_wrapper = CallbackCtx(ctx.ptr());
            let boxed: transcribe::TranscriptCallback =
                Box::new(move |text: &str, timestamp: f64| {
                    if let Ok(cs) = CString::new(text) {
                        unsafe {
                            fn_ptr(cs.as_ptr(), timestamp, ctx_wrapper.ptr());
                        }
                    }
                });
            boxed
        })
    }
}

// ── Helper functions ─────────────────────────────────────────────────

/// Convert a raw C string pointer to a Rust `&str`. Returns `None` if null or invalid UTF-8.
unsafe fn cstr_to_str<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    CStr::from_ptr(ptr).to_str().ok()
}

/// Convert a Rust string into a C-owned `char*`. The caller must free it with
/// [`note_free_string`]. Returns null on allocation failure.
fn string_to_cptr(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ── Error reporting ──────────────────────────────────────────────────

use std::cell::RefCell;
use std::sync::OnceLock;

thread_local! {
    /// The most recent error message produced on this thread, retrievable via
    /// [`note_last_error`]. FFI entry points run on the caller's thread, so the
    /// synchronous failures a consumer cares about (missing model, no microphone,
    /// locked database, no API key, `note_init` returning null) land here.
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

/// Record `msg` as this thread's last error. Interior NUL bytes are replaced so
/// the conversion never fails.
fn set_last_error(msg: impl Into<String>) {
    let cleaned = msg.into().replace('\0', " ");
    let cs = CString::new(cleaned).unwrap_or_default();
    LAST_ERROR.with(|e| *e.borrow_mut() = Some(cs));
}

/// Clear this thread's last error. Called at the start of each fallible FFI entry
/// point so `note_last_error` reflects only the most recent call and never returns
/// a stale message from an earlier, unrelated one (for example after a legitimate
/// not-found that returns null without setting an error).
fn clear_last_error() {
    LAST_ERROR.with(|e| *e.borrow_mut() = None);
}

/// C function pointer type for the structured log callback.
/// Parameters: level (1=error, 2=warn, 3=info, 4=debug, 5=trace), message, ctx.
type LogCallbackFn = unsafe extern "C" fn(i32, *const c_char, *mut c_void);

/// Registered C log sink. `Send`/`Sync` because the caller guarantees the
/// context pointer outlives the callback and is safe to use from any thread.
struct CLogCallback {
    cb: LogCallbackFn,
    ctx: CallbackCtx,
}
unsafe impl Send for CLogCallback {}
unsafe impl Sync for CLogCallback {}

static LOG_SINK: Mutex<Option<CLogCallback>> = Mutex::new(None);
static LOGGER_INIT: OnceLock<bool> = OnceLock::new();

/// A `log::Log` implementation that forwards records to the registered C sink.
struct CLogger;

impl log::Log for CLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        // Copy the callback pointer and context out, then release the lock BEFORE
        // invoking the callback. Otherwise a sink that logs (directly or by
        // re-entering a note_* function) would try to re-lock LOG_SINK on a thread
        // that already holds it and deadlock (std Mutex is not reentrant).
        let (cb, ctx) = {
            let Ok(guard) = LOG_SINK.lock() else { return };
            match *guard {
                Some(ref sink) => (sink.cb, sink.ctx.ptr()),
                None => return,
            }
        };
        let level = match record.level() {
            log::Level::Error => 1,
            log::Level::Warn => 2,
            log::Level::Info => 3,
            log::Level::Debug => 4,
            log::Level::Trace => 5,
        };
        if let Ok(cs) = CString::new(format!("{}", record.args()).replace('\0', " ")) {
            unsafe {
                cb(level, cs.as_ptr(), ctx);
            }
        }
    }

    fn flush(&self) {}
}

static LOGGER: CLogger = CLogger;

// The `note_last_error` and `note_set_log_callback` FFI entry points live at the
// end of this file so the generated C header lists them after the rest of the
// surface; they use the thread-local and logger infrastructure defined above.

// ── Lifecycle ────────────────────────────────────────────────────────

/// Initialize the notetaker engine.
///
/// - `db_path`: path to the SQLite database file (created if absent).
/// - `model_path`: path to the directory containing Whisper GGML model files
///   (the specific model is selected per recording via the `whisper_model` setting).
///
/// Returns an opaque `NoteHandle*` on success, or null on failure.
#[no_mangle]
pub extern "C" fn note_init(db_path: *const c_char, model_path: *const c_char) -> *mut NoteHandle {
    let result = catch_unwind(|| {
        clear_last_error();
        let db_path = match unsafe { cstr_to_str(db_path) } {
            Some(s) => s,
            None => {
                set_last_error("note_init: db_path is null or invalid UTF-8");
                error!("note_init: db_path is null or invalid UTF-8");
                return std::ptr::null_mut();
            }
        };
        let model_path = match unsafe { cstr_to_str(model_path) } {
            Some(s) => s,
            None => {
                set_last_error("note_init: model_path is null or invalid UTF-8");
                error!("note_init: model_path is null or invalid UTF-8");
                return std::ptr::null_mut();
            }
        };

        let db = match Database::open(db_path) {
            Ok(db) => Arc::new(db),
            Err(e) => {
                set_last_error(format!("note_init: failed to open database: {}", e));
                error!("note_init: failed to open database: {}", e);
                return std::ptr::null_mut();
            }
        };

        let runtime = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("note_init: failed to create tokio runtime: {}", e));
                error!("note_init: failed to create tokio runtime: {}", e);
                return std::ptr::null_mut();
            }
        };

        info!(
            "Notetaker initialized (db={}, model={})",
            db_path, model_path
        );

        let handle = Box::new(NoteHandle {
            db,
            model_path: model_path.to_string(),
            audio_capture: Mutex::new(None),
            transcriber: Mutex::new(None),
            current_meeting_id: Mutex::new(None),
            transcript_callback: Mutex::new(None),
            audio_level: Arc::new(Mutex::new(0.0)),
            whisper_ctx: Mutex::new(None),
            push_audio_tx: Mutex::new(None),
            runtime,
        });

        Box::into_raw(handle)
    });

    result.unwrap_or(std::ptr::null_mut())
}

/// Destroy the notetaker handle and free all resources.
///
/// After calling this, the handle must not be used again.
#[no_mangle]
pub extern "C" fn note_destroy(handle: *mut NoteHandle) {
    if handle.is_null() {
        return;
    }
    let _ = catch_unwind(|| {
        unsafe {
            let _ = Box::from_raw(handle);
        }
        info!("Notetaker destroyed");
    });
}

// ── Recording ────────────────────────────────────────────────────────

/// Start recording from the specified audio device.
///
/// - `device_id`: device identifier from [`note_list_audio_devices`], or null
///   to use the system default input device.
///
/// Returns 0 on success, or a negative error code.
#[no_mangle]
pub extern "C" fn note_start_recording(
    handle: *mut NoteHandle,
    device_id: *const c_char,
) -> i32 {
    if handle.is_null() {
        return NOTE_ERR_NULL_PTR;
    }
    match catch_unwind(|| {
        let h = unsafe { &*handle };
        clear_last_error();
        let device = unsafe { cstr_to_str(device_id) };

        // Guard against starting a new recording while one is active.
        if h.current_meeting_id.lock().unwrap().is_some() {
            set_last_error("Cannot start recording: a recording session is already active");
            error!("Cannot start recording: a recording session is already active");
            return NOTE_ERR_INTERNAL;
        }

        // Create a new meeting.
        let meeting_id = match h.db.create_meeting("Untitled Meeting") {
            Ok(id) => id,
            Err(e) => {
                set_last_error(format!("Failed to create meeting: {}", e));
                error!("Failed to create meeting: {}", e);
                return NOTE_ERR_INTERNAL;
            }
        };

        *h.current_meeting_id.lock().unwrap() = Some(meeting_id);

        // Roll back the active-session state and the orphan meeting row when
        // setup fails after the meeting has been created.
        let cleanup = || {
            *h.current_meeting_id.lock().unwrap() = None;
            let _ = h.db.delete_meeting(meeting_id);
        };

        // Set up the audio -> transcriber pipeline.
        let (audio_tx, audio_rx) = mpsc::channel::<Vec<f32>>();

        // Mirror the RMS level into shared state for `note_get_audio_level`.
        let level_arc = h.audio_level.clone();
        let level_cb: audio::AudioLevelCallback = Box::new(move |lvl: f32| {
            if let Ok(mut g) = level_arc.lock() {
                *g = lvl;
            }
        });

        // Build the audio capture.
        let capture = match AudioCapture::new(device, audio_tx, Some(level_cb)) {
            Ok(c) => c,
            Err(e) => {
                set_last_error(format!("Failed to create audio capture: {}", e));
                error!("Failed to create audio capture: {}", e);
                cleanup();
                return NOTE_ERR_INTERNAL;
            }
        };

        // Build the transcript callback, then resolve + load the (cached) model
        // and start the transcriber. The context is loaded once per model and
        // reused across recordings, so only the first start pays the load cost.
        let callback = h.build_transcript_callback();

        let model_file = h.resolve_model_file();
        let ctx = match h.get_or_load_ctx(&model_file) {
            Ok(c) => c,
            Err(e) => {
                set_last_error(format!("Failed to load Whisper model: {}", e));
                error!("Failed to load Whisper model: {}", e);
                cleanup();
                return NOTE_ERR_INTERNAL;
            }
        };
        let config = h.transcriber_config();

        let transcriber = match LocalTranscriber::new(
            ctx,
            config,
            audio_rx,
            h.db.clone(),
            meeting_id,
            callback,
        ) {
            Ok(t) => t,
            Err(e) => {
                set_last_error(format!("Failed to start transcriber: {}", e));
                error!("Failed to start transcriber: {}", e);
                cleanup();
                return NOTE_ERR_INTERNAL;
            }
        };

        // Start audio capture.
        if let Err(e) = capture.start() {
            set_last_error(format!("Failed to start audio: {}", e));
            error!("Failed to start audio: {}", e);
            cleanup();
            return NOTE_ERR_INTERNAL;
        }

        *h.audio_capture.lock().unwrap() = Some(capture);
        *h.transcriber.lock().unwrap() = Some(transcriber);

        info!("Recording started for meeting {}", meeting_id);
        NOTE_OK
    }) {
        Ok(code) => code,
        Err(_) => NOTE_ERR_PANIC,
    }
}

/// Start recording from an external audio feed instead of the microphone.
///
/// No microphone `AudioCapture` is created; the native layer must feed resampled
/// 16kHz mono f32 samples via [`note_push_audio`]. This is the entry point for
/// system-audio capture (ScreenCaptureKit, WASAPI loopback, PipeWire monitors).
///
/// Returns 0 on success, or a negative error code.
#[no_mangle]
pub extern "C" fn note_start_recording_push(handle: *mut NoteHandle) -> i32 {
    if handle.is_null() {
        return NOTE_ERR_NULL_PTR;
    }
    match catch_unwind(|| {
        let h = unsafe { &*handle };
        clear_last_error();

        if h.current_meeting_id.lock().unwrap().is_some() {
            set_last_error("Cannot start recording: a recording session is already active");
            error!("Cannot start recording: a recording session is already active");
            return NOTE_ERR_INTERNAL;
        }

        let meeting_id = match h.db.create_meeting("Untitled Meeting") {
            Ok(id) => id,
            Err(e) => {
                set_last_error(format!("Failed to create meeting: {}", e));
                error!("Failed to create meeting: {}", e);
                return NOTE_ERR_INTERNAL;
            }
        };
        *h.current_meeting_id.lock().unwrap() = Some(meeting_id);

        let cleanup = || {
            *h.current_meeting_id.lock().unwrap() = None;
            let _ = h.db.delete_meeting(meeting_id);
        };

        let (audio_tx, audio_rx) = mpsc::channel::<Vec<f32>>();
        let callback = h.build_transcript_callback();

        let model_file = h.resolve_model_file();
        let ctx = match h.get_or_load_ctx(&model_file) {
            Ok(c) => c,
            Err(e) => {
                set_last_error(format!("Failed to load Whisper model: {}", e));
                error!("Failed to load Whisper model: {}", e);
                cleanup();
                return NOTE_ERR_INTERNAL;
            }
        };
        let config = h.transcriber_config();

        let transcriber = match LocalTranscriber::new(
            ctx,
            config,
            audio_rx,
            h.db.clone(),
            meeting_id,
            callback,
        ) {
            Ok(t) => t,
            Err(e) => {
                set_last_error(format!("Failed to start transcriber: {}", e));
                error!("Failed to start transcriber: {}", e);
                cleanup();
                return NOTE_ERR_INTERNAL;
            }
        };

        *h.push_audio_tx.lock().unwrap() = Some(audio_tx);
        *h.transcriber.lock().unwrap() = Some(transcriber);

        info!("Push recording started for meeting {}", meeting_id);
        NOTE_OK
    }) {
        Ok(code) => code,
        Err(_) => NOTE_ERR_PANIC,
    }
}

/// Feed a block of audio into an active push-mode recording.
///
/// - `samples`: pointer to `count` 32-bit float samples, 16kHz mono, in [-1.0, 1.0].
/// - `count`: number of samples (0 is a no-op success).
///
/// Requires a session started with [`note_start_recording_push`]. Returns 0 on
/// success, or a negative error code.
#[no_mangle]
pub extern "C" fn note_push_audio(handle: *mut NoteHandle, samples: *const f32, count: usize) -> i32 {
    if handle.is_null() || samples.is_null() {
        return NOTE_ERR_NULL_PTR;
    }
    match catch_unwind(|| {
        let h = unsafe { &*handle };
        let guard = h.push_audio_tx.lock().unwrap();
        let tx = match guard.as_ref() {
            Some(tx) => tx,
            None => {
                set_last_error("note_push_audio: no push-mode recording is active");
                return NOTE_ERR_NOT_FOUND;
            }
        };
        // `count == 0` yields an empty (never-dereferenced) slice.
        let slice = unsafe { std::slice::from_raw_parts(samples, count) };
        match tx.send(slice.to_vec()) {
            Ok(()) => NOTE_OK,
            Err(_) => {
                set_last_error("note_push_audio: transcription pipeline has closed");
                NOTE_ERR_INTERNAL
            }
        }
    }) {
        Ok(code) => code,
        Err(_) => NOTE_ERR_PANIC,
    }
}

/// Stop the current recording session.
///
/// Finalizes the meeting, stops audio capture and transcription.
/// Returns 0 on success, or a negative error code.
#[no_mangle]
pub extern "C" fn note_stop_recording(handle: *mut NoteHandle) -> i32 {
    if handle.is_null() {
        return NOTE_ERR_NULL_PTR;
    }
    match catch_unwind(|| {
        let h = unsafe { &*handle };
        clear_last_error();

        // Stop audio capture (drops the stream, closing its sender).
        if let Some(capture) = h.audio_capture.lock().unwrap().take() {
            let _ = capture.stop();
        }

        // Close the push-audio sender too (push mode has no AudioCapture), so the
        // transcriber's drain sees a disconnected channel and terminates.
        let _ = h.push_audio_tx.lock().unwrap().take();

        // Stop transcriber (processes the remaining buffer and joins its thread).
        if let Some(transcriber) = h.transcriber.lock().unwrap().take() {
            transcriber.stop();
        }

        // Reset the metered level so note_get_audio_level reads 0 when idle.
        if let Ok(mut g) = h.audio_level.lock() {
            *g = 0.0;
        }

        // End the meeting in the database.
        if let Some(meeting_id) = h.current_meeting_id.lock().unwrap().take() {
            if let Err(e) = h.db.end_meeting(meeting_id) {
                set_last_error(format!("Failed to end meeting {}: {}", meeting_id, e));
                error!("Failed to end meeting {}: {}", meeting_id, e);
                return NOTE_ERR_INTERNAL;
            }
            info!("Recording stopped for meeting {}", meeting_id);
        }

        NOTE_OK
    }) {
        Ok(code) => code,
        Err(_) => NOTE_ERR_PANIC,
    }
}

/// Pause the current recording session.
///
/// Audio capture is paused but the meeting remains active.
/// Returns 0 on success, or a negative error code.
#[no_mangle]
pub extern "C" fn note_pause_recording(handle: *mut NoteHandle) -> i32 {
    if handle.is_null() {
        return NOTE_ERR_NULL_PTR;
    }
    match catch_unwind(|| {
        let h = unsafe { &*handle };
        clear_last_error();
        let guard = h.audio_capture.lock().unwrap();
        if let Some(ref capture) = *guard {
            if let Err(e) = capture.pause() {
                set_last_error(format!("Failed to pause recording: {}", e));
                error!("Failed to pause recording: {}", e);
                return NOTE_ERR_INTERNAL;
            }
            info!("Recording paused");
            NOTE_OK
        } else {
            set_last_error("No active recording to pause");
            error!("No active recording to pause");
            NOTE_ERR_NOT_FOUND
        }
    }) {
        Ok(code) => code,
        Err(_) => NOTE_ERR_PANIC,
    }
}

/// Resume a paused recording session.
///
/// Returns 0 on success, or a negative error code.
#[no_mangle]
pub extern "C" fn note_resume_recording(handle: *mut NoteHandle) -> i32 {
    if handle.is_null() {
        return NOTE_ERR_NULL_PTR;
    }
    match catch_unwind(|| {
        let h = unsafe { &*handle };
        clear_last_error();
        let guard = h.audio_capture.lock().unwrap();
        if let Some(ref capture) = *guard {
            if let Err(e) = capture.resume() {
                set_last_error(format!("Failed to resume recording: {}", e));
                error!("Failed to resume recording: {}", e);
                return NOTE_ERR_INTERNAL;
            }
            info!("Recording resumed");
            NOTE_OK
        } else {
            set_last_error("No active recording to resume");
            error!("No active recording to resume");
            NOTE_ERR_NOT_FOUND
        }
    }) {
        Ok(code) => code,
        Err(_) => NOTE_ERR_PANIC,
    }
}

/// Get the most recent RMS audio input level (0.0 to 1.0).
///
/// Returns the level for UI metering, or 0.0 if the handle is null or no
/// recording is active.
#[no_mangle]
pub extern "C" fn note_get_audio_level(handle: *mut NoteHandle) -> f32 {
    if handle.is_null() {
        return 0.0;
    }
    catch_unwind(|| {
        let h = unsafe { &*handle };
        h.audio_level.lock().map(|g| *g).unwrap_or(0.0)
    })
    .unwrap_or(0.0)
}

// ── Transcription callback ───────────────────────────────────────────

/// Set the callback invoked for each new transcript segment.
///
/// - `cb`: C function pointer `void(const char* text, double timestamp_secs, void* ctx)`,
///   or null to clear. The `text` pointer is only valid for the duration of the call.
/// - `ctx`: opaque context pointer passed through to the callback.
#[no_mangle]
pub extern "C" fn note_set_transcript_callback(
    handle: *mut NoteHandle,
    cb: Option<unsafe extern "C" fn(*const c_char, f64, *mut c_void)>,
    ctx: *mut c_void,
) {
    if handle.is_null() {
        return;
    }
    let _ = catch_unwind(|| {
        let h = unsafe { &*handle };
        let mut guard = h.transcript_callback.lock().unwrap();
        match cb {
            Some(fn_ptr) => {
                *guard = Some((fn_ptr, CallbackCtx(ctx)));
            }
            None => {
                *guard = None;
            }
        }
    });
}

// ── Summarization ────────────────────────────────────────────────────

/// Generate an AI summary for a completed meeting.
///
/// Returns a JSON string with the summary, or null on error.
/// The caller must free the returned string with [`note_free_string`].
///
/// Requires the `cloud` feature and a configured API key (setting `anthropic_api_key`).
#[no_mangle]
pub extern "C" fn note_summarize(handle: *mut NoteHandle, meeting_id: i64) -> *mut c_char {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    match catch_unwind(|| {
        let h = unsafe { &*handle };
        clear_last_error();

        // Gather all transcript segments for this meeting.
        let segments = match h.db.get_segments(meeting_id) {
            Ok(s) => s,
            Err(e) => {
                set_last_error(format!(
                    "Failed to get segments for meeting {}: {}",
                    meeting_id, e
                ));
                error!("Failed to get segments for meeting {}: {}", meeting_id, e);
                return std::ptr::null_mut();
            }
        };

        if segments.is_empty() {
            set_last_error(format!("No transcript segments for meeting {}", meeting_id));
            error!("No transcript segments for meeting {}", meeting_id);
            return std::ptr::null_mut();
        }

        let transcript: String = segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        // Suppress unused-variable warning when cloud feature is disabled.
        let _ = &transcript;

        #[cfg(feature = "cloud")]
        {
            let api_key = match h.db.get_setting("anthropic_api_key") {
                Ok(Some(key)) if !key.trim().is_empty() => key,
                _ => {
                    set_last_error("No API key configured (set 'anthropic_api_key' setting)");
                    error!("No API key configured (set 'anthropic_api_key' setting)");
                    return std::ptr::null_mut();
                }
            };

            let model = h.db.get_setting("anthropic_model").ok().flatten();

            let summarizer = summarize::CloudSummarizer::new(
                api_key,
                model,
                h.runtime.handle().clone(),
            );

            match summarizer.summarize(&transcript) {
                Ok(summary) => {
                    // Store the summary in the meeting record.
                    let json = match serde_json::to_string(&summary) {
                        Ok(j) => j,
                        Err(e) => {
                            error!("Failed to serialize summary: {}", e);
                            return std::ptr::null_mut();
                        }
                    };

                    // Persist the flat narrative (shown in meeting lists/detail)
                    // and the structured fields (richer rendering) side by side.
                    if let Err(e) = h.db.update_meeting_summary(meeting_id, &summary.summary) {
                        error!("Failed to save summary: {}", e);
                    }
                    if let Err(e) = h.db.upsert_summary(meeting_id, &summary) {
                        error!("Failed to save structured summary: {}", e);
                    }

                    return string_to_cptr(json);
                }
                Err(e) => {
                    set_last_error(format!("Summarization failed: {}", e));
                    error!("Summarization failed: {}", e);
                    return std::ptr::null_mut();
                }
            }
        }

        #[cfg(not(feature = "cloud"))]
        {
            set_last_error("Summarization requires the 'cloud' feature");
            error!("Summarization requires the 'cloud' feature");
            std::ptr::null_mut()
        }
    }) {
        Ok(ptr) => ptr,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Get a previously generated structured summary for a meeting as JSON.
///
/// Returns a JSON object (`{summary, key_points, action_items, decisions}`), or
/// null if none has been generated or on error. Free with [`note_free_string`].
/// Unlike [`note_summarize`], this does not require the `cloud` feature or network
/// access -- it reads what was stored at summarization time.
#[no_mangle]
pub extern "C" fn note_get_summary(handle: *mut NoteHandle, meeting_id: i64) -> *mut c_char {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    match catch_unwind(|| {
        let h = unsafe { &*handle };
        clear_last_error();
        match h.db.get_summary(meeting_id) {
            Ok(Some(summary)) => match serde_json::to_string(&summary) {
                Ok(json) => string_to_cptr(json),
                Err(e) => {
                    set_last_error(format!("Failed to serialize summary: {}", e));
                    error!("Failed to serialize summary: {}", e);
                    std::ptr::null_mut()
                }
            },
            Ok(None) => std::ptr::null_mut(),
            Err(e) => {
                set_last_error(format!(
                    "Failed to get summary for meeting {}: {}",
                    meeting_id, e
                ));
                error!("Failed to get summary for meeting {}: {}", meeting_id, e);
                std::ptr::null_mut()
            }
        }
    }) {
        Ok(ptr) => ptr,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Free a string previously returned by the notetaker library.
///
/// Passing null is safe (no-op).
#[no_mangle]
pub extern "C" fn note_free_string(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    let _ = catch_unwind(|| unsafe {
        let _ = CString::from_raw(s);
    });
}

// ── Storage queries ──────────────────────────────────────────────────

/// List meetings as a JSON array.
///
/// - `limit`: maximum number of meetings to return.
/// - `offset`: number of meetings to skip (for pagination).
///
/// Returns a JSON string, or null on error. Free with [`note_free_string`].
#[no_mangle]
pub extern "C" fn note_list_meetings(
    handle: *mut NoteHandle,
    limit: i32,
    offset: i32,
) -> *mut c_char {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    match catch_unwind(|| {
        let h = unsafe { &*handle };
        clear_last_error();
        match h.db.list_meetings(limit, offset) {
            Ok(meetings) => match serde_json::to_string(&meetings) {
                Ok(json) => string_to_cptr(json),
                Err(e) => {
                    error!("Failed to serialize meetings: {}", e);
                    std::ptr::null_mut()
                }
            },
            Err(e) => {
                error!("Failed to list meetings: {}", e);
                std::ptr::null_mut()
            }
        }
    }) {
        Ok(ptr) => ptr,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Get a single meeting by id as a JSON object.
///
/// Returns a JSON string, or null if not found or on error. Free with [`note_free_string`].
#[no_mangle]
pub extern "C" fn note_get_meeting(handle: *mut NoteHandle, meeting_id: i64) -> *mut c_char {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    match catch_unwind(|| {
        let h = unsafe { &*handle };
        clear_last_error();
        match h.db.get_meeting(meeting_id) {
            Ok(Some(meeting)) => match serde_json::to_string(&meeting) {
                Ok(json) => string_to_cptr(json),
                Err(e) => {
                    error!("Failed to serialize meeting: {}", e);
                    std::ptr::null_mut()
                }
            },
            Ok(None) => std::ptr::null_mut(),
            Err(e) => {
                set_last_error(format!("Failed to get meeting {}: {}", meeting_id, e));
                error!("Failed to get meeting {}: {}", meeting_id, e);
                std::ptr::null_mut()
            }
        }
    }) {
        Ok(ptr) => ptr,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Get all transcript segments for a meeting as a JSON array.
///
/// Returns a JSON string (possibly an empty array), or null on error.
/// Free with [`note_free_string`].
#[no_mangle]
pub extern "C" fn note_get_segments(handle: *mut NoteHandle, meeting_id: i64) -> *mut c_char {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    match catch_unwind(|| {
        let h = unsafe { &*handle };
        clear_last_error();
        match h.db.get_segments(meeting_id) {
            Ok(segments) => match serde_json::to_string(&segments) {
                Ok(json) => string_to_cptr(json),
                Err(e) => {
                    set_last_error(format!("Failed to serialize segments: {}", e));
                    error!("Failed to serialize segments: {}", e);
                    std::ptr::null_mut()
                }
            },
            Err(e) => {
                set_last_error(format!(
                    "Failed to get segments for meeting {}: {}",
                    meeting_id, e
                ));
                error!("Failed to get segments for meeting {}: {}", meeting_id, e);
                std::ptr::null_mut()
            }
        }
    }) {
        Ok(ptr) => ptr,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Search meetings by transcript text (FTS5).
///
/// Returns a JSON array of the meetings whose transcripts match `query`, newest
/// first (possibly an empty array), or null on error. Free with
/// [`note_free_string`].
#[no_mangle]
pub extern "C" fn note_search_meetings(
    handle: *mut NoteHandle,
    query: *const c_char,
) -> *mut c_char {
    if handle.is_null() || query.is_null() {
        return std::ptr::null_mut();
    }
    match catch_unwind(|| {
        let h = unsafe { &*handle };
        clear_last_error();
        let q = match unsafe { cstr_to_str(query) } {
            Some(s) => s,
            None => {
                set_last_error("note_search_meetings: invalid query string");
                return std::ptr::null_mut();
            }
        };
        match h.db.search_meetings_full(q) {
            Ok(meetings) => match serde_json::to_string(&meetings) {
                Ok(json) => string_to_cptr(json),
                Err(e) => {
                    set_last_error(format!("Failed to serialize search results: {}", e));
                    error!("Failed to serialize search results: {}", e);
                    std::ptr::null_mut()
                }
            },
            Err(e) => {
                set_last_error(format!("Search failed: {}", e));
                error!("Search failed: {}", e);
                std::ptr::null_mut()
            }
        }
    }) {
        Ok(ptr) => ptr,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Update a meeting's title.
///
/// Returns 0 on success, or a negative error code.
#[no_mangle]
pub extern "C" fn note_update_meeting_title(
    handle: *mut NoteHandle,
    meeting_id: i64,
    title: *const c_char,
) -> i32 {
    if handle.is_null() || title.is_null() {
        return NOTE_ERR_NULL_PTR;
    }
    match catch_unwind(|| {
        let h = unsafe { &*handle };
        clear_last_error();
        let t = match unsafe { cstr_to_str(title) } {
            Some(s) => s,
            None => {
                set_last_error("note_update_meeting_title: invalid title string");
                return NOTE_ERR_INVALID_ARG;
            }
        };
        match h.db.update_meeting_title(meeting_id, t) {
            Ok(()) => {
                info!("Updated title for meeting {}", meeting_id);
                NOTE_OK
            }
            Err(e) => {
                set_last_error(format!("Failed to update meeting title: {}", e));
                error!("Failed to update meeting title: {}", e);
                NOTE_ERR_INTERNAL
            }
        }
    }) {
        Ok(code) => code,
        Err(_) => NOTE_ERR_PANIC,
    }
}

/// Delete a meeting and all its associated data.
///
/// Returns 0 on success, or a negative error code.
#[no_mangle]
pub extern "C" fn note_delete_meeting(handle: *mut NoteHandle, meeting_id: i64) -> i32 {
    if handle.is_null() {
        return NOTE_ERR_NULL_PTR;
    }
    match catch_unwind(|| {
        let h = unsafe { &*handle };
        clear_last_error();
        match h.db.delete_meeting(meeting_id) {
            Ok(()) => {
                info!("Deleted meeting {}", meeting_id);
                NOTE_OK
            }
            Err(e) => {
                set_last_error(format!("Failed to delete meeting {}: {}", meeting_id, e));
                error!("Failed to delete meeting {}: {}", meeting_id, e);
                NOTE_ERR_INTERNAL
            }
        }
    }) {
        Ok(code) => code,
        Err(_) => NOTE_ERR_PANIC,
    }
}

/// Export a meeting in the specified format.
///
/// - `format`: one of "markdown", "text", or "json".
///
/// Returns the exported content as a string, or null on error.
/// Free with [`note_free_string`].
#[no_mangle]
pub extern "C" fn note_export_meeting(
    handle: *mut NoteHandle,
    meeting_id: i64,
    format: *const c_char,
) -> *mut c_char {
    if handle.is_null() || format.is_null() {
        return std::ptr::null_mut();
    }
    match catch_unwind(|| {
        let h = unsafe { &*handle };
        clear_last_error();
        let fmt = match unsafe { cstr_to_str(format) } {
            Some(s) => s,
            None => {
                error!("note_export_meeting: invalid format string");
                return std::ptr::null_mut();
            }
        };
        match h.db.export_meeting(meeting_id, fmt) {
            Ok(content) => string_to_cptr(content),
            Err(e) => {
                error!("Failed to export meeting {}: {}", meeting_id, e);
                std::ptr::null_mut()
            }
        }
    }) {
        Ok(ptr) => ptr,
        Err(_) => std::ptr::null_mut(),
    }
}

// ── Settings ─────────────────────────────────────────────────────────

/// Set a configuration setting.
///
/// Returns 0 on success, or a negative error code.
#[no_mangle]
pub extern "C" fn note_set_setting(
    handle: *mut NoteHandle,
    key: *const c_char,
    value: *const c_char,
) -> i32 {
    if handle.is_null() || key.is_null() || value.is_null() {
        return NOTE_ERR_NULL_PTR;
    }
    match catch_unwind(|| {
        let h = unsafe { &*handle };
        clear_last_error();
        let k = match unsafe { cstr_to_str(key) } {
            Some(s) => s,
            None => return NOTE_ERR_INVALID_ARG,
        };
        let v = match unsafe { cstr_to_str(value) } {
            Some(s) => s,
            None => return NOTE_ERR_INVALID_ARG,
        };
        match h.db.set_setting(k, v) {
            Ok(()) => NOTE_OK,
            Err(e) => {
                error!("Failed to set setting '{}': {}", k, e);
                NOTE_ERR_INTERNAL
            }
        }
    }) {
        Ok(code) => code,
        Err(_) => NOTE_ERR_PANIC,
    }
}

/// Get a configuration setting value.
///
/// Returns the value as a string, or null if not found. Free with [`note_free_string`].
#[no_mangle]
pub extern "C" fn note_get_setting(handle: *mut NoteHandle, key: *const c_char) -> *mut c_char {
    if handle.is_null() || key.is_null() {
        return std::ptr::null_mut();
    }
    match catch_unwind(|| {
        let h = unsafe { &*handle };
        clear_last_error();
        let k = match unsafe { cstr_to_str(key) } {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        match h.db.get_setting(k) {
            Ok(Some(value)) => string_to_cptr(value),
            Ok(None) => std::ptr::null_mut(),
            Err(e) => {
                error!("Failed to get setting '{}': {}", k, e);
                std::ptr::null_mut()
            }
        }
    }) {
        Ok(ptr) => ptr,
        Err(_) => std::ptr::null_mut(),
    }
}

// ── Audio devices ────────────────────────────────────────────────────

/// List available audio input devices as a JSON array.
///
/// Returns a JSON string, or null on error. Free with [`note_free_string`].
/// Does not require an active `NoteHandle` (the handle parameter is for
/// API consistency but the implementation uses the system audio host).
#[no_mangle]
pub extern "C" fn note_list_audio_devices(_handle: *mut NoteHandle) -> *mut c_char {
    match catch_unwind(|| match audio::list_audio_devices() {
        Ok(devices) => match serde_json::to_string(&devices) {
            Ok(json) => string_to_cptr(json),
            Err(e) => {
                error!("Failed to serialize audio devices: {}", e);
                std::ptr::null_mut()
            }
        },
        Err(e) => {
            error!("Failed to list audio devices: {}", e);
            std::ptr::null_mut()
        }
    }) {
        Ok(ptr) => ptr,
        Err(_) => std::ptr::null_mut(),
    }
}

// ── Diagnostics ──────────────────────────────────────────────────────

/// Return the most recent error message for the calling thread, or null if none.
///
/// The returned string is caller-owned; free it with [`note_free_string`].
/// Call this after an FFI function reports failure (a negative code or a null
/// return) to get a human-readable reason.
#[no_mangle]
pub extern "C" fn note_last_error() -> *mut c_char {
    catch_unwind(|| {
        LAST_ERROR.with(|e| match e.borrow().as_ref() {
            Some(cs) => cs.clone().into_raw(),
            None => std::ptr::null_mut(),
        })
    })
    .unwrap_or(std::ptr::null_mut())
}

/// Install a structured log callback.
///
/// - `cb`: `void(int level, const char* message, void* ctx)` invoked for each log
///   record, or null to detach the current sink. The message pointer is only
///   valid for the duration of the call. Levels: 1=error, 2=warn, 3=info,
///   4=debug, 5=trace.
/// - `ctx`: opaque context pointer passed through to the callback.
///
/// The underlying logger is installed once per process; if another logger was
/// already installed (for example by the host app), the callback is stored but
/// will not receive records.
#[no_mangle]
pub extern "C" fn note_set_log_callback(
    // The function-pointer type is spelled inline (not via the `LogCallbackFn`
    // alias) so cbindgen renders it as a nullable C function pointer rather than
    // emitting an opaque `struct Option_LogCallbackFn`.
    cb: Option<unsafe extern "C" fn(i32, *const c_char, *mut c_void)>,
    ctx: *mut c_void,
) {
    let _ = catch_unwind(|| {
        LOGGER_INIT.get_or_init(|| match log::set_logger(&LOGGER) {
            Ok(()) => {
                log::set_max_level(log::LevelFilter::Info);
                true
            }
            Err(_) => false,
        });
        if let Ok(mut guard) = LOG_SINK.lock() {
            *guard = cb.map(|f| CLogCallback {
                cb: f,
                ctx: CallbackCtx(ctx),
            });
        }
    });
}
