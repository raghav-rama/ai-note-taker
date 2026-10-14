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

use crate::audio::AudioCapture;
use crate::storage::Database;
#[cfg(feature = "cloud")]
use crate::summarize::Summarizer;
use crate::transcribe::LocalTranscriber;

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
    #[allow(dead_code)]
    runtime: tokio::runtime::Runtime,
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
        let db_path = match unsafe { cstr_to_str(db_path) } {
            Some(s) => s,
            None => {
                error!("note_init: db_path is null or invalid UTF-8");
                return std::ptr::null_mut();
            }
        };
        let model_path = match unsafe { cstr_to_str(model_path) } {
            Some(s) => s,
            None => {
                error!("note_init: model_path is null or invalid UTF-8");
                return std::ptr::null_mut();
            }
        };

        let db = match Database::open(db_path) {
            Ok(db) => Arc::new(db),
            Err(e) => {
                error!("note_init: failed to open database: {}", e);
                return std::ptr::null_mut();
            }
        };

        let runtime = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
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
        let device = unsafe { cstr_to_str(device_id) };

        // Guard against starting a new recording while one is active.
        if h.current_meeting_id.lock().unwrap().is_some() {
            error!("Cannot start recording: a recording session is already active");
            return NOTE_ERR_INTERNAL;
        }

        // Create a new meeting.
        let meeting_id = match h.db.create_meeting("Untitled Meeting") {
            Ok(id) => id,
            Err(e) => {
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
                error!("Failed to create audio capture: {}", e);
                cleanup();
                return NOTE_ERR_INTERNAL;
            }
        };

        // Build the transcript callback closure (wraps the C function pointer).
        let cb_lock = h.transcript_callback.lock().unwrap();
        let callback: Option<transcribe::TranscriptCallback> =
            cb_lock.as_ref().map(|(fn_ptr, ctx)| {
                let fn_ptr = *fn_ptr;
                // Wrap the raw pointer in our Send-safe wrapper so the
                // closure can be moved to the transcriber thread.
                // Use .ptr() method so that Rust 2021 captures the whole
                // CallbackCtx (which is Send) rather than the inner *mut c_void.
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
            });
        drop(cb_lock);

        // Resolve the model file. `model_path` is normally a models directory
        // (Linux/Android/iOS), in which we compose ggml-{whisper_model}.bin from
        // the configured model name (default "tiny"). Legacy callers (macOS,
        // Windows) still pass a full model file path — detect that and use it
        // verbatim so composing doesn't produce `.../ggml-tiny.bin/ggml-...bin`.
        let model_file = if std::path::Path::new(&h.model_path).is_dir() {
            let model_name = h
                .db
                .get_setting("whisper_model")
                .ok()
                .flatten()
                .unwrap_or_else(|| "tiny".to_string());
            format!("{}/ggml-{}.bin", h.model_path, model_name)
        } else {
            h.model_path.clone()
        };

        // Start the transcriber.
        let transcriber = match LocalTranscriber::new(
            &model_file,
            audio_rx,
            h.db.clone(),
            meeting_id,
            callback,
        ) {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to start transcriber: {}", e);
                cleanup();
                return NOTE_ERR_INTERNAL;
            }
        };

        // Start audio capture.
        if let Err(e) = capture.start() {
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

        // Stop audio capture (drops the stream).
        if let Some(capture) = h.audio_capture.lock().unwrap().take() {
            let _ = capture.stop();
        }

        // Stop transcriber (processes remaining buffer).
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
        let guard = h.audio_capture.lock().unwrap();
        if let Some(ref capture) = *guard {
            if let Err(e) = capture.pause() {
                error!("Failed to pause recording: {}", e);
                return NOTE_ERR_INTERNAL;
            }
            info!("Recording paused");
            NOTE_OK
        } else {
            error!("No active recording to pause");
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

        // Gather all transcript segments for this meeting.
        let segments = match h.db.get_segments(meeting_id) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get segments for meeting {}: {}", meeting_id, e);
                return std::ptr::null_mut();
            }
        };

        if segments.is_empty() {
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
                Ok(Some(key)) => key,
                _ => {
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

                    if let Err(e) = h.db.update_meeting_summary(meeting_id, &summary.summary) {
                        error!("Failed to save summary: {}", e);
                    }

                    return string_to_cptr(json);
                }
                Err(e) => {
                    error!("Summarization failed: {}", e);
                    return std::ptr::null_mut();
                }
            }
        }

        #[cfg(not(feature = "cloud"))]
        {
            error!("Summarization requires the 'cloud' feature");
            std::ptr::null_mut()
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
                error!("Failed to get meeting {}: {}", meeting_id, e);
                std::ptr::null_mut()
            }
        }
    }) {
        Ok(ptr) => ptr,
        Err(_) => std::ptr::null_mut(),
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
        match h.db.delete_meeting(meeting_id) {
            Ok(()) => {
                info!("Deleted meeting {}", meeting_id);
                NOTE_OK
            }
            Err(e) => {
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
