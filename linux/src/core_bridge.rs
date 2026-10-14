//! Bridge between GTK UI and notetaker-core.
//!
//! All core usage is confined to this module. Views only interact with
//! [`CoreBridge`], keeping the coupling to a single file.

use notetaker::audio::{self, AudioCapture, AudioLevelCallback};
use notetaker::models::{AudioDevice, Meeting, TranscriptSegment};
use notetaker::storage::Database;
use notetaker::transcribe::{LocalTranscriber, TranscriptCallback};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::runtime::Handle;

pub struct CoreBridge {
    db: Arc<Database>,
    model_path: String,
    capture: Arc<Mutex<Option<AudioCapture>>>,
    transcriber: Arc<Mutex<Option<LocalTranscriber>>>,
    current_meeting_id: Arc<Mutex<Option<i64>>>,
    audio_level: Arc<Mutex<f32>>,
    transcript_tx: Arc<Mutex<Option<async_channel::Sender<TranscriptSegment>>>>,
    recording: Arc<AtomicBool>,
    rt: Handle,
}

impl CoreBridge {
    pub fn new(db_path: &str, model_path: &str, rt: Handle) -> Result<Self, String> {
        let db = Database::open(db_path).map_err(|e| e.to_string())?;
        Ok(Self {
            db: Arc::new(db),
            model_path: model_path.to_owned(),
            capture: Arc::new(Mutex::new(None)),
            transcriber: Arc::new(Mutex::new(None)),
            current_meeting_id: Arc::new(Mutex::new(None)),
            audio_level: Arc::new(Mutex::new(0.0)),
            transcript_tx: Arc::new(Mutex::new(None)),
            recording: Arc::new(AtomicBool::new(false)),
            rt,
        })
    }

    // ── Recording ──────────────────────────────────────────────────────

    pub async fn start_recording(&self, device_id: Option<String>) -> Result<(), String> {
        // Claim the recording slot before the (slow, blocking) model load so a
        // second click during initialization can't create a second meeting and
        // clobber the shared capture/transcriber slots.
        if self
            .recording
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("A recording is already in progress".to_string());
        }

        let db = self.db.clone();
        let capture_slot = self.capture.clone();
        let transcriber_slot = self.transcriber.clone();
        let meeting_slot = self.current_meeting_id.clone();
        let level = self.audio_level.clone();
        let transcript_tx = self.transcript_tx.clone();
        let model_dir = self.model_path.clone();

        let outcome = self
            .rt
            .spawn_blocking(move || -> Result<(), String> {
                let meeting_id = db
                    .create_meeting("Untitled Meeting")
                    .map_err(|e| e.to_string())?;
                *meeting_slot.lock().unwrap() = Some(meeting_id);

                // Build the transcriber (which loads the model synchronously and
                // can fail) before starting capture, so a missing model doesn't
                // leave a live capture or an orphan meeting row behind.
                let build = || -> Result<(AudioCapture, LocalTranscriber), String> {
                    // Channel carrying resampled 16kHz mono audio from capture
                    // into the transcriber.
                    let (audio_tx, audio_rx) = std::sync::mpsc::channel::<Vec<f32>>();

                    // Forward each transcribed segment to the UI, if subscribed.
                    let tx_slot = transcript_tx.lock().unwrap().clone();
                    let callback: Option<TranscriptCallback> = tx_slot.map(|tx| {
                        let cb: TranscriptCallback =
                            Box::new(move |text: &str, timestamp_secs: f64| {
                                let _ = tx.try_send(TranscriptSegment {
                                    id: 0,
                                    meeting_id,
                                    timestamp_ms: (timestamp_secs * 1000.0).round() as i64,
                                    text: text.to_owned(),
                                    confidence: None,
                                });
                            });
                        cb
                    });

                    // Resolve the model file from the configured model name.
                    let model_name = db
                        .get_setting("whisper_model")
                        .map_err(|e| e.to_string())?
                        .unwrap_or_else(|| "tiny".to_string());
                    let model_file = format!("{model_dir}/ggml-{model_name}.bin");

                    let transcriber = LocalTranscriber::new(
                        &model_file,
                        audio_rx,
                        db.clone(),
                        meeting_id,
                        callback,
                    )
                    .map_err(|e| e.to_string())?;

                    // Audio level callback for UI metering.
                    let level_cb = level.clone();
                    let level_callback: AudioLevelCallback = Box::new(move |l| {
                        *level_cb.lock().unwrap() = l;
                    });

                    let cap =
                        AudioCapture::new(device_id.as_deref(), audio_tx, Some(level_callback))
                            .map_err(|e| e.to_string())?;
                    cap.start().map_err(|e| e.to_string())?;

                    Ok((cap, transcriber))
                };

                match build() {
                    Ok((cap, transcriber)) => {
                        // If a stop ran during the (slow) model load it will have
                        // taken our meeting id; don't install a now-orphaned live
                        // capture/transcriber — tear them down instead.
                        if *meeting_slot.lock().unwrap() == Some(meeting_id) {
                            *capture_slot.lock().unwrap() = Some(cap);
                            *transcriber_slot.lock().unwrap() = Some(transcriber);
                            Ok(())
                        } else {
                            let _ = cap.stop();
                            transcriber.stop();
                            Err("Recording was stopped during startup".to_string())
                        }
                    }
                    Err(e) => {
                        *meeting_slot.lock().unwrap() = None;
                        let _ = db.delete_meeting(meeting_id);
                        Err(e)
                    }
                }
            })
            .await;

        // Release the claim on any failure (join error or setup error) so the
        // user can retry; on success it stays held until stop_recording.
        match outcome {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => {
                self.recording.store(false, Ordering::SeqCst);
                Err(e)
            }
            Err(join_err) => {
                self.recording.store(false, Ordering::SeqCst);
                Err(join_err.to_string())
            }
        }
    }

    pub async fn stop_recording(&self) -> Result<i64, String> {
        // Atomically take ownership of the active session's resources up front.
        // A duplicate or overlapping stop then finds the slots already emptied
        // and bails out, so two stops can't both tear down and finalize meetings
        // out from under each other. The `recording` claim is deliberately NOT
        // cleared here — it is released inside the blocking task only once
        // teardown is complete, so no new recording can start mid-teardown.
        let cap = self.capture.lock().unwrap().take();
        let transcriber = self.transcriber.lock().unwrap().take();
        let meeting_id = match self.current_meeting_id.lock().unwrap().take() {
            Some(id) => id,
            None => return Err("No active recording".to_string()),
        };

        let db = self.db.clone();
        let recording = self.recording.clone();

        self.rt
            .spawn_blocking(move || {
                // Stop and drop audio capture FIRST so its audio_tx sender is
                // closed. If the transcriber were stopped first, its shutdown
                // drain would run against a still-live capture and, when
                // inference is slower than real-time, never empty the backlog —
                // hanging the worker-thread join (and this stop future) forever.
                if let Some(cap) = cap {
                    let _ = cap.stop();
                }

                // With the sender gone the transcriber drains the now-finite
                // backlog, flushes a final segment, and its thread joins.
                if let Some(transcriber) = transcriber {
                    transcriber.stop();
                }

                let result = db.end_meeting(meeting_id).map_err(|e| e.to_string());

                // Release the claim only now that capture, transcriber, and the
                // meeting have all been finalized, so a fresh recording can start
                // even if ending the meeting in the DB failed.
                recording.store(false, Ordering::SeqCst);

                result.map(|_| meeting_id)
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

    pub async fn get_transcript(&self, meeting_id: i64) -> Result<Vec<TranscriptSegment>, String> {
        let db = self.db.clone();
        self.rt
            .spawn_blocking(move || db.get_segments(meeting_id).map_err(|e| e.to_string()))
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
