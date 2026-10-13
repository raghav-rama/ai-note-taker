//! Bridge between GTK UI and notetaker-core.
//!
//! All core usage is confined to this module. Views only interact with
//! [`CoreBridge`], keeping the coupling to a single file.

use notetaker::audio::{self, AudioCapture, AudioLevelCallback};
use notetaker::models::{AudioDevice, Meeting, TranscriptSegment};
use notetaker::storage::Database;
use std::sync::{Arc, Mutex};
use tokio::runtime::Handle;

pub struct CoreBridge {
    db: Arc<Database>,
    model_path: String,
    capture: Arc<Mutex<Option<AudioCapture>>>,
    current_meeting_id: Arc<Mutex<Option<i64>>>,
    audio_level: Arc<Mutex<f32>>,
    transcript_tx: Arc<Mutex<Option<async_channel::Sender<TranscriptSegment>>>>,
    rt: Handle,
}

impl CoreBridge {
    pub fn new(db_path: &str, model_path: &str, rt: Handle) -> Result<Self, String> {
        let db = Database::open(db_path).map_err(|e| e.to_string())?;
        Ok(Self {
            db: Arc::new(db),
            model_path: model_path.to_owned(),
            capture: Arc::new(Mutex::new(None)),
            current_meeting_id: Arc::new(Mutex::new(None)),
            audio_level: Arc::new(Mutex::new(0.0)),
            transcript_tx: Arc::new(Mutex::new(None)),
            rt,
        })
    }

    // ── Recording ──────────────────────────────────────────────────────

    pub async fn start_recording(&self, device_id: Option<String>) -> Result<(), String> {
        let db = self.db.clone();
        let capture_slot = self.capture.clone();
        let meeting_slot = self.current_meeting_id.clone();
        let level = self.audio_level.clone();
        let transcript_tx = self.transcript_tx.clone();
        let _model_path = self.model_path.clone();

        self.rt
            .spawn_blocking(move || {
                let meeting_id = db
                    .create_meeting("Untitled Meeting")
                    .map_err(|e| e.to_string())?;
                *meeting_slot.lock().unwrap() = Some(meeting_id);

                // Audio level callback for UI metering.
                let level_cb = level.clone();
                let level_callback: AudioLevelCallback =
                    Box::new(move |l| {
                        *level_cb.lock().unwrap() = l;
                    });

                // Channel for raw audio data from the capture thread.
                let (audio_tx, audio_rx) = std::sync::mpsc::channel::<Vec<f32>>();

                let dev_id = device_id.as_deref();
                let cap = AudioCapture::new(dev_id, audio_tx, Some(level_callback))
                    .map_err(|e| e.to_string())?;
                cap.start().map_err(|e| e.to_string())?;
                *capture_slot.lock().unwrap() = Some(cap);

                // Spawn a thread that reads audio chunks and produces
                // placeholder transcript segments. Real transcription would
                // feed audio_rx into a Whisper model; for now we just drain
                // the channel so the audio pipeline doesn't block.
                let tx_slot = transcript_tx.lock().unwrap().clone();
                let db_for_seg = db.clone();
                std::thread::spawn(move || {
                    let mut chunk_count: i64 = 0;
                    while let Ok(_samples) = audio_rx.recv() {
                        chunk_count += 1;
                        // TODO: feed samples into LocalTranscriber and emit
                        // real segments. For now the pipeline just drains.
                        let _ = (&tx_slot, &db_for_seg, chunk_count);
                    }
                });

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
                if let Some(cap) = capture_slot.lock().unwrap().take() {
                    let _ = cap.stop();
                }

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

    pub fn audio_level(&self) -> f32 {
        *self.audio_level.lock().unwrap()
    }

    /// Subscribe to incoming transcript segments.
    ///
    /// Returns an `async_channel::Receiver` that yields segments as they
    /// arrive from the transcription pipeline. The recording view drains
    /// this into its text buffer.
    pub fn subscribe_transcripts(&self) -> async_channel::Receiver<TranscriptSegment> {
        let (tx, rx) = async_channel::unbounded();
        *self.transcript_tx.lock().unwrap() = Some(tx);
        rx
    }

    // ── Meetings / Storage ─────────────────────────────────────────────

    pub async fn summarize(&self, meeting_id: i64) -> Result<String, String> {
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
