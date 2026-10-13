//! Bridge between GTK UI and notetaker-core.
//!
//! All `notetaker_core` usage is confined to this module. Views only interact
//! with [`CoreBridge`], making reconciliation with the real core a single-file
//! change.
//!
//! ## Assumed notetaker-core API
//!
//! The following types and methods are expected from the `notetaker_core` crate.
//! If the real core diverges, only this file needs updating.
//!
//! ```rust,ignore
//! pub struct NoteTaker { /* ... */ }
//!
//! impl NoteTaker {
//!     pub fn new(db_path: &str, model_path: &str) -> Result<Self, CoreError>;
//!
//!     // Recording
//!     pub fn start_recording(&self, device_id: Option<&str>) -> Result<(), CoreError>;
//!     pub fn stop_recording(&self) -> Result<i64, CoreError>; // returns meeting_id
//!     pub fn pause_recording(&self) -> Result<(), CoreError>;
//!
//!     // Transcript callback
//!     pub fn set_transcript_callback(
//!         &self,
//!         cb: Box<dyn Fn(TranscriptSegment) + Send + Sync>,
//!     );
//!
//!     // Audio metering
//!     pub fn audio_level(&self) -> f32; // 0.0..=1.0
//!
//!     // Summarization
//!     pub fn summarize(&self, meeting_id: i64) -> Result<String, CoreError>;
//!
//!     // Storage
//!     pub fn list_meetings(&self, limit: i32, offset: i32) -> Result<Vec<Meeting>, CoreError>;
//!     pub fn get_meeting(&self, meeting_id: i64) -> Result<Meeting, CoreError>;
//!     pub fn delete_meeting(&self, meeting_id: i64) -> Result<(), CoreError>;
//!     pub fn export_meeting(&self, meeting_id: i64, format: &str) -> Result<String, CoreError>;
//!
//!     // Settings
//!     pub fn get_setting(&self, key: &str) -> Result<Option<String>, CoreError>;
//!     pub fn set_setting(&self, key: &str, value: &str) -> Result<(), CoreError>;
//!
//!     // Audio devices
//!     pub fn list_audio_devices(&self) -> Result<Vec<AudioDevice>, CoreError>;
//! }
//!
//! pub struct Meeting {
//!     pub id: i64,
//!     pub title: String,
//!     pub started_at: String,
//!     pub ended_at: Option<String>,
//!     pub duration_secs: Option<i64>,
//!     pub summary: Option<String>,
//! }
//!
//! pub struct TranscriptSegment {
//!     pub meeting_id: i64,
//!     pub timestamp_ms: i64,
//!     pub text: String,
//!     pub confidence: Option<f64>,
//! }
//!
//! pub struct AudioDevice {
//!     pub id: String,
//!     pub name: String,
//!     pub is_default: bool,
//! }
//!
//! pub struct CoreError(pub String);
//! ```

use notetaker_core::{AudioDevice, CoreError, Meeting, NoteTaker, TranscriptSegment};
use std::sync::Arc;
use tokio::runtime::Handle;

/// Async-safe bridge around the synchronous `notetaker_core::NoteTaker`.
///
/// Every method offloads blocking core calls onto the tokio thread pool via
/// `spawn_blocking`, keeping the GTK main loop responsive.
pub struct CoreBridge {
    core: Arc<NoteTaker>,
    rt: Handle,
}

impl CoreBridge {
    /// Create a new bridge, initializing the underlying core.
    pub fn new(db_path: &str, model_path: &str, rt: Handle) -> Result<Self, String> {
        let core = NoteTaker::new(db_path, model_path).map_err(|e| e.0.clone())?;
        Ok(Self {
            core: Arc::new(core),
            rt,
        })
    }

    // ── Recording ──────────────────────────────────────────────────────

    pub async fn start_recording(&self, device_id: Option<String>) -> Result<(), String> {
        let core = self.core.clone();
        self.rt
            .spawn_blocking(move || {
                core.start_recording(device_id.as_deref())
                    .map_err(|e| e.0.clone())
            })
            .await
            .map_err(|e| e.to_string())?
    }

    pub async fn stop_recording(&self) -> Result<i64, String> {
        let core = self.core.clone();
        self.rt
            .spawn_blocking(move || core.stop_recording().map_err(|e| e.0.clone()))
            .await
            .map_err(|e| e.to_string())?
    }

    pub async fn pause_recording(&self) -> Result<(), String> {
        let core = self.core.clone();
        self.rt
            .spawn_blocking(move || core.pause_recording().map_err(|e| e.0.clone()))
            .await
            .map_err(|e| e.to_string())?
    }

    /// Register a transcript callback. Segments are sent over the returned
    /// channel so the UI can drain them on the GTK main thread.
    pub fn subscribe_transcripts(&self) -> async_channel::Receiver<TranscriptSegment> {
        let (tx, rx) = async_channel::bounded(256);
        self.core.set_transcript_callback(Box::new(move |seg| {
            // Best-effort send; dropping segments is acceptable under back-pressure.
            let _ = tx.send_blocking(seg);
        }));
        rx
    }

    /// Current audio input level (0.0 to 1.0). Called from UI timer, so
    /// synchronous is fine — this is expected to be very fast.
    pub fn audio_level(&self) -> f32 {
        self.core.audio_level()
    }

    // ── Meetings / Storage ─────────────────────────────────────────────

    pub async fn summarize(&self, meeting_id: i64) -> Result<String, String> {
        let core = self.core.clone();
        self.rt
            .spawn_blocking(move || core.summarize(meeting_id).map_err(|e| e.0.clone()))
            .await
            .map_err(|e| e.to_string())?
    }

    pub async fn list_meetings(&self, limit: i32, offset: i32) -> Result<Vec<Meeting>, String> {
        let core = self.core.clone();
        self.rt
            .spawn_blocking(move || core.list_meetings(limit, offset).map_err(|e| e.0.clone()))
            .await
            .map_err(|e| e.to_string())?
    }

    pub async fn get_meeting(&self, meeting_id: i64) -> Result<Meeting, String> {
        let core = self.core.clone();
        self.rt
            .spawn_blocking(move || core.get_meeting(meeting_id).map_err(|e| e.0.clone()))
            .await
            .map_err(|e| e.to_string())?
    }

    pub async fn delete_meeting(&self, meeting_id: i64) -> Result<(), String> {
        let core = self.core.clone();
        self.rt
            .spawn_blocking(move || core.delete_meeting(meeting_id).map_err(|e| e.0.clone()))
            .await
            .map_err(|e| e.to_string())?
    }

    pub async fn export_meeting(&self, meeting_id: i64, format: &str) -> Result<String, String> {
        let format = format.to_owned();
        let core = self.core.clone();
        self.rt
            .spawn_blocking(move || {
                core.export_meeting(meeting_id, &format)
                    .map_err(|e| e.0.clone())
            })
            .await
            .map_err(|e| e.to_string())?
    }

    // ── Settings ───────────────────────────────────────────────────────

    pub async fn get_setting(&self, key: &str) -> Result<Option<String>, String> {
        let key = key.to_owned();
        let core = self.core.clone();
        self.rt
            .spawn_blocking(move || core.get_setting(&key).map_err(|e| e.0.clone()))
            .await
            .map_err(|e| e.to_string())?
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        let key = key.to_owned();
        let value = value.to_owned();
        let core = self.core.clone();
        self.rt
            .spawn_blocking(move || core.set_setting(&key, &value).map_err(|e| e.0.clone()))
            .await
            .map_err(|e| e.to_string())?
    }

    // ── Audio Devices ──────────────────────────────────────────────────

    pub async fn list_audio_devices(&self) -> Result<Vec<AudioDevice>, String> {
        let core = self.core.clone();
        self.rt
            .spawn_blocking(move || core.list_audio_devices().map_err(|e| e.0.clone()))
            .await
            .map_err(|e| e.to_string())?
    }
}
