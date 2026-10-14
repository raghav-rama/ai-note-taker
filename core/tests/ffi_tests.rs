//! FFI lifecycle and safety tests. These call the `extern "C"` surface directly
//! (the functions are safe to call from Rust; only dereferencing returned C
//! strings needs `unsafe`).

use std::ffi::{c_void, CStr, CString};
use std::os::raw::c_char;

use notetaker::{
    note_destroy, note_free_string, note_get_audio_level, note_get_meeting, note_get_segments,
    note_get_setting, note_get_summary, note_init, note_last_error, note_list_meetings,
    note_pause_recording, note_resume_recording, note_search_meetings, note_set_log_callback,
    note_set_setting, note_start_recording, note_stop_recording, note_summarize,
    note_update_meeting_title, NOTE_ERR_NULL_PTR, NOTE_OK,
};

/// A unique temp database path.
fn tmp_db() -> CString {
    let mut p = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!(
        "notetaker_ffi_{}_{}.db",
        std::process::id(),
        nanos
    ));
    CString::new(p.to_string_lossy().into_owned()).unwrap()
}

/// Read and free a C string returned by the library.
unsafe fn take_cstr(ptr: *mut c_char) -> String {
    assert!(!ptr.is_null(), "expected a non-null C string");
    let s = CStr::from_ptr(ptr).to_string_lossy().into_owned();
    note_free_string(ptr);
    s
}

#[test]
fn init_and_destroy() {
    let db = tmp_db();
    let model = CString::new("/nonexistent/models").unwrap();
    let h = note_init(db.as_ptr(), model.as_ptr());
    assert!(!h.is_null());
    note_destroy(h);
}

#[test]
fn init_null_arg_returns_null_and_sets_last_error() {
    let model = CString::new("/x").unwrap();
    let h = note_init(std::ptr::null(), model.as_ptr());
    assert!(h.is_null());
    let err = note_last_error();
    assert!(!err.is_null(), "last_error should be set after a failed init");
    let msg = unsafe { take_cstr(err) };
    assert!(msg.contains("db_path"), "unexpected error message: {msg}");
}

#[test]
fn null_handle_calls_are_safe() {
    assert_eq!(
        note_start_recording(std::ptr::null_mut(), std::ptr::null()),
        NOTE_ERR_NULL_PTR
    );
    assert_eq!(note_stop_recording(std::ptr::null_mut()), NOTE_ERR_NULL_PTR);
    assert_eq!(note_pause_recording(std::ptr::null_mut()), NOTE_ERR_NULL_PTR);
    assert_eq!(note_resume_recording(std::ptr::null_mut()), NOTE_ERR_NULL_PTR);
    assert_eq!(
        note_update_meeting_title(std::ptr::null_mut(), 1, std::ptr::null()),
        NOTE_ERR_NULL_PTR
    );
    assert_eq!(note_get_audio_level(std::ptr::null_mut()), 0.0);
    assert!(note_list_meetings(std::ptr::null_mut(), 10, 0).is_null());
    assert!(note_get_meeting(std::ptr::null_mut(), 1).is_null());
    assert!(note_get_segments(std::ptr::null_mut(), 1).is_null());
    assert!(note_search_meetings(std::ptr::null_mut(), std::ptr::null()).is_null());
    assert!(note_get_summary(std::ptr::null_mut(), 1).is_null());
    assert!(note_summarize(std::ptr::null_mut(), 1).is_null());
    assert!(note_get_setting(std::ptr::null_mut(), std::ptr::null()).is_null());
    assert_eq!(
        note_set_setting(std::ptr::null_mut(), std::ptr::null(), std::ptr::null()),
        NOTE_ERR_NULL_PTR
    );
    // No-ops that must not crash.
    note_destroy(std::ptr::null_mut());
    note_free_string(std::ptr::null_mut());
}

#[test]
fn settings_and_storage_through_ffi() {
    let db = tmp_db();
    let model = CString::new("/nonexistent/models").unwrap();
    let h = note_init(db.as_ptr(), model.as_ptr());
    assert!(!h.is_null());

    let k = CString::new("whisper_model").unwrap();
    let v = CString::new("tiny").unwrap();
    assert_eq!(note_set_setting(h, k.as_ptr(), v.as_ptr()), NOTE_OK);
    let got = unsafe { take_cstr(note_get_setting(h, k.as_ptr())) };
    assert_eq!(got, "tiny");

    // Empty DB → empty JSON array.
    let list = unsafe { take_cstr(note_list_meetings(h, 10, 0)) };
    assert_eq!(list, "[]");

    // Missing meeting → null; its segments → empty array (not null).
    assert!(note_get_meeting(h, 999).is_null());
    let segs = unsafe { take_cstr(note_get_segments(h, 999)) };
    assert_eq!(segs, "[]");

    // Search with no matches → empty array.
    let q = CString::new("anything").unwrap();
    let sr = unsafe { take_cstr(note_search_meetings(h, q.as_ptr())) };
    assert_eq!(sr, "[]");

    note_destroy(h);
}

#[test]
fn double_stop_is_safe() {
    let db = tmp_db();
    let model = CString::new("/nonexistent/models").unwrap();
    let h = note_init(db.as_ptr(), model.as_ptr());
    assert!(!h.is_null());
    // No active recording: stopping is a safe no-op, and stays safe when repeated.
    assert_eq!(note_stop_recording(h), NOTE_OK);
    assert_eq!(note_stop_recording(h), NOTE_OK);
    note_destroy(h);
}

#[test]
fn failed_start_reports_error_and_leaves_no_orphan_meeting() {
    let db = tmp_db();
    // Model dir does not exist, and CI runners have no audio input device:
    // either way note_start_recording must fail cleanly.
    let model = CString::new("/nonexistent/models").unwrap();
    let h = note_init(db.as_ptr(), model.as_ptr());
    assert!(!h.is_null());

    let rc = note_start_recording(h, std::ptr::null());
    assert_ne!(rc, NOTE_OK, "start should fail without device/model");

    // A clean (non-panic) failure also records a message; free it if present.
    // (The last_error mechanism itself is asserted in the init test; here the
    // important guarantee is the state-leak fix below.)
    let err = note_last_error();
    if !err.is_null() {
        note_free_string(err);
    }

    // The state-leak fix: a failed start must not leave an orphan meeting row,
    // so the meeting list is still empty.
    let list = unsafe { take_cstr(note_list_meetings(h, 10, 0)) };
    assert_eq!(list, "[]", "failed start left an orphan meeting");

    note_destroy(h);
}

#[test]
fn log_callback_install_and_detach_is_safe() {
    extern "C" fn sink(_level: i32, _msg: *const c_char, _ctx: *mut c_void) {}
    note_set_log_callback(Some(sink), std::ptr::null_mut());
    note_set_log_callback(None, std::ptr::null_mut());
}
