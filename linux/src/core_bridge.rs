//! Bridge between GTK UI and notetaker-core.
//!
//! All core usage is confined to this module. Views only interact with
//! [`CoreBridge`], keeping the coupling to a single file.

use notetaker::audio::{self, AudioCapture};
use notetaker::models::{AudioDevice, Meeting, TranscriptSegment};
use notetaker::storage::Database;
use notetaker::transcribe::LocalTranscriber;
use std::sync::{Arc, Mutex};
use tokio::runtime::Handle;

/// Async-safe bridge around the notetaker-core modules.
///
/// Every method offloads blocking core calls onto the tokio thread pool via
/// `spawn_blocking`, keeping the GTK main loop responsive.
pub struct CoreBridge {
    db: Arc<Database>,
    model_path: String,
    capture: Arc<Mutex<Option<AudioCapture>>>,
    current_meeting_id: Arc<Mutex<Option<i64>>>,
    audio_level: Arc<Mutex<f32>>,
    rt: Handle,
}

impl CoreBridge {
    /// Create a new bridge, initializing the underlying core.
    pub fn new(db_path: &str, model_path: &str, rt: Handle) -> Result<Self, String> {
        let db = Database::open(db_path).map_err(|e| e.to_string())?;
        Ok(Self {
            db: Arc::new(db),
            model_path: model_path.to_owned(),
            capture: Arc::new(Mutex::new(None)),
            current_meeting_id: Arc::new(Mutex::new(None)),
            audio_level: Arc::new(Mutex::new(0.0)),
            rt,
        })
    }

    // ── Recording ──────────────────────────────────────────────────────

    pub async fn start_recording(&self, _device_id: Option<String>) -> Result<(), String> {
        let db = self.db.clone();
        let capture_slot = self.capture.clone();
        let meeting_slot = self.current_meeting_id.clone();
        let level = self.audio_level.clone();
        let model_path = self.model_path.clone();

        self.rt
            .spawn_blocking(move || {
                // Create a new meeting in the database.
                let meeting_id = db
                    .create_meeting("Untitled Meeting")
                    .map_err(|e| e.to_string())?;
                *meeting_slot.lock().unwrap() = Some(meeting_id);

                // Set up audio level callback.
                let level_cb = level.clone();
                let level_callback: Box<dyn Fn(f32) + Send + Sync> =
                    Box::new(move |l| {
                        *level_cb.lock().unwrap() = l;
                    });

                // Create audio capture — uses default device for now.
                let cap = AudioCapture::new(level_callback)
                    .map_err(|e| e.to_string())?;
                cap.start().map_err(|e| e.to_string())?;
                *capture_slot.lock().unwrap() = Some(cap);

                Ok(())
            })
            .await
            .map_err(|e| e.to_string())?
    }

    pub async fn stop_recording(&self) -> Result<i64, String> {
        let capture_slot = self.capture.clone();
        let meeting_slot = self.current_meeting_id.clone();
        let db = self.db.clone();

        self.rt
            .spawn_blocking(move || {
                // Stop audio capture.
                if let Some(cap) = capture_slot.lock().unwrap().take() {
                    let _ = cap.stop();
                }

                // End the meeting.
                let meeting_id = meeting_slot
                    .lock()
                    .unwrap()
                    .take()
                    .ok_or_else(|| "No active meeting".to_string())?;
                db.end_meeting(meeting_id).map_err(|e| e.to_string())?;

                Ok(meeting_id)
            })
            .await
            .map_err(|e| e.to_string())?
    }

    pub async fn pause_recording(&self) -> Result<(), String> {
        let capture_slot = self.capture.clone();
        self.rt
            .spawn_blocking(move || {
                if let Some(ref cap) = *capture_slot.lock().unwrap() {
                    cap.pause().map_err(|e| e.to_string())
                } else {
                    Err("No active recording".to_string())
                }
            })
            .await
            .map_err(|e| e.to_string())?
    }

    /// Current audio input level (0.0 to 1.0). Called from UI timer, so
    /// synchronous is fine — this is expected to be very fast.
    pub fn audio_level(&self) -> f32 {
        *self.audio_level.lock().unwrap()
    }

    // ── Meetings / Storage ─────────────────────────────────────────────

    pub async fn summarize(&self, meeting_id: i64) -> Result<String, String> {
        // Summarization requires cloud feature — return a placeholder for now.
        let db = self.db.clone();
        self.rt
            .spawn_blocking(move || {
                let segments = db.get_segments(meeting_id).map_err(|e| e.to_string())?;
                let transcript: String = segments
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                if transcript.is_empty() {
                    Ok("No transcript available to summarize.".to_string())
                } else {
                    Ok(format!(
                        "Transcript ({} segments, {} chars). Cloud summarization requires an API key — configure one in Settings.",
                        segments.len(),
                        transcript.len()
                    ))
                }
            })
            .await
            .map_err(|e| e.to_string())?
    }

    pub async fn list_meetings(&self, limit: i32, offset: i32) -> Result<Vec<Meeting>, String> {
        let db = self.db.clone();
        self.rt
            .spawn_blocking(move || db.list_meetings(limit, offset).map_err(|e| e.to_string()))
            .await
            .map_err(|e| e.to_string())?
    }

    pub async fn get_meeting(&self, meeting_id: i64) -> Result<Meeting, String> {
        let db = self.db.clone();
        self.rt
            .spawn_blocking(move || {
                db.get_meeting(meeting_id)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("Meeting {meeting_id} not found"))
            })
            .await
            .map_err(|e| e.to_string())?
    }

    pub async fn delete_meeting(&self, meeting_id: i64) -> Result<(), String> {
        let db = self.db.clone();
        self.rt
            .spawn_blocking(move || db.delete_meeting(meeting_id).map_err(|e| e.to_string()))
            .await
            .map_err(|e| e.to_string())?
    }

    pub async fn export_meeting(&self, meeting_id: i64, format: &str) -> Result<String, String> {
        let format = format.to_owned();
        let db = self.db.clone();
        self.rt
            .spawn_blocking(move || {
                db.export_meeting(meeting_id, &format)
                    .map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())?
    }

    // ── Settings ───────────────────────────────────────────────────────

    pub async fn get_setting(&self, key: &str) -> Result<Option<String>, String> {
        let key = key.to_owned();
        let db = self.db.clone();
        self.rt
            .spawn_blocking(move || db.get_setting(&key).map_err(|e| e.to_string()))
            .await
            .map_err(|e| e.to_string())?
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        let key = key.to_owned();
        let value = value.to_owned();
        let db = self.db.clone();
        self.rt
            .spawn_blocking(move || db.set_setting(&key, &value).map_err(|e| e.to_string()))
            .await
            .map_err(|e| e.to_string())?
    }

    // ── Audio Devices ──────────────────────────────────────────────────

    pub async fn list_audio_devices(&self) -> Result<Vec<AudioDevice>, String> {
        self.rt
            .spawn_blocking(move || audio::list_audio_devices().map_err(|e| e.to_string()))
            .await
            .map_err(|e| e.to_string())?
    }
}
