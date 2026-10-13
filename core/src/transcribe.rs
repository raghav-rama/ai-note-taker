//! Transcription module using whisper-rs (whisper.cpp bindings).
//!
//! Processes audio in streaming chunks of 3-5 seconds, runs inference
//! via a local Whisper model, and delivers results through a callback.

use anyhow::Result;
use log::{error, info, warn};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::storage::Database;

/// Minimum audio duration in samples at 16kHz to avoid whisper.cpp errors.
/// Whisper requires at least ~1 second of audio.
const MIN_SAMPLES: usize = 16000;

/// Target chunk size in samples at 16kHz (~4 seconds).
const CHUNK_SIZE: usize = 16000 * 4;

/// A transcribed segment before database insertion.
#[derive(Debug, Clone)]
pub struct RawSegment {
    /// Timestamp in milliseconds from the start of the recording.
    pub timestamp_ms: i64,
    /// Transcribed text.
    pub text: String,
    /// Average token probability as a confidence proxy (0.0 to 1.0), if available.
    pub confidence: Option<f64>,
}

/// Callback type for delivering transcript segments.
/// Parameters: text, timestamp in seconds.
pub type TranscriptCallback = Box<dyn Fn(&str, f64) + Send + 'static>;

/// Local transcription engine using whisper.cpp.
pub struct LocalTranscriber {
    /// Handle to the transcription worker thread.
    thread_handle: Option<thread::JoinHandle<()>>,
    /// Send a `None` to signal shutdown.
    shutdown_tx: mpsc::Sender<()>,
}

impl LocalTranscriber {
    /// Start a transcription worker that reads audio chunks from `audio_rx`.
    ///
    /// - `model_path`: path to a Whisper GGML model file.
    /// - `audio_rx`: receiver of resampled 16kHz mono f32 audio chunks.
    /// - `db`: shared database handle for persisting segments.
    /// - `meeting_id`: the current meeting being recorded.
    /// - `callback`: invoked with each transcribed segment's text and timestamp (seconds).
    ///
    /// The model is loaded synchronously before returning. If the model file
    /// is missing or corrupt, this returns an error immediately.
    pub fn new(
        model_path: &str,
        audio_rx: mpsc::Receiver<Vec<f32>>,
        db: Arc<Database>,
        meeting_id: i64,
        callback: Option<TranscriptCallback>,
    ) -> Result<Self> {
        let model_path = model_path.to_string();
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

        // Channel for the worker thread to report whether model loading succeeded.
        let (load_tx, load_rx) = mpsc::channel::<std::result::Result<(), String>>();

        // Load the Whisper model on the worker thread (can be large).
        let thread_handle = thread::spawn(move || {
            let ctx = match WhisperContext::new_with_params(
                &model_path,
                WhisperContextParameters::new(),
            ) {
                Ok(ctx) => {
                    let _ = load_tx.send(Ok(()));
                    ctx
                }
                Err(e) => {
                    let msg = format!("Failed to load Whisper model from {}: {:?}", model_path, e);
                    error!("{}", msg);
                    let _ = load_tx.send(Err(msg));
                    return;
                }
            };

            info!("Whisper model loaded from {}", model_path);

            let mut buffer: Vec<f32> = Vec::with_capacity(CHUNK_SIZE * 2);
            let mut total_samples_processed: usize = 0;

            loop {
                // Check for shutdown.
                if shutdown_rx.try_recv().is_ok() {
                    // Process any remaining audio in the buffer, padding if needed.
                    if !buffer.is_empty() {
                        if buffer.len() < MIN_SAMPLES {
                            buffer.resize(MIN_SAMPLES, 0.0);
                        }
                        Self::transcribe_chunk(
                            &ctx,
                            &buffer,
                            total_samples_processed,
                            &db,
                            meeting_id,
                            &callback,
                        );
                    }
                    info!("Transcriber shutting down");
                    return;
                }

                // Collect audio chunks.
                match audio_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                    Ok(chunk) => {
                        buffer.extend_from_slice(&chunk);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        // No data yet, keep waiting.
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        // Audio source closed. Process remaining buffer, padding
                        // to MIN_SAMPLES if needed (whisper.cpp needs >= 1s).
                        if !buffer.is_empty() {
                            if buffer.len() < MIN_SAMPLES {
                                buffer.resize(MIN_SAMPLES, 0.0);
                            }
                            Self::transcribe_chunk(
                                &ctx,
                                &buffer,
                                total_samples_processed,
                                &db,
                                meeting_id,
                                &callback,
                            );
                        }
                        info!("Audio channel closed, transcriber exiting");
                        return;
                    }
                }

                // Process when we have enough audio.
                if buffer.len() >= CHUNK_SIZE {
                    let chunk: Vec<f32> = buffer.drain(..CHUNK_SIZE).collect();
                    Self::transcribe_chunk(
                        &ctx,
                        &chunk,
                        total_samples_processed,
                        &db,
                        meeting_id,
                        &callback,
                    );
                    total_samples_processed += CHUNK_SIZE;
                }
            }
        });

        // Wait for the model to load before returning. This ensures callers
        // get an immediate error if the model file is missing or corrupt.
        match load_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(msg)) => {
                // Thread will exit on its own since it already returned.
                let _ = thread_handle.join();
                return Err(anyhow::anyhow!("{}", msg));
            }
            Err(_) => {
                // Thread panicked or exited before sending.
                let _ = thread_handle.join();
                return Err(anyhow::anyhow!("Transcriber thread exited unexpectedly"));
            }
        }

        Ok(Self {
            thread_handle: Some(thread_handle),
            shutdown_tx,
        })
    }

    /// Run Whisper inference on a single audio chunk.
    fn transcribe_chunk(
        ctx: &WhisperContext,
        audio: &[f32],
        samples_offset: usize,
        db: &Database,
        meeting_id: i64,
        callback: &Option<TranscriptCallback>,
    ) {
        let mut state = match ctx.create_state() {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to create Whisper state: {:?}", e);
                return;
            }
        };

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(4);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        params.set_single_segment(false);
        params.set_no_context(true);
        params.set_token_timestamps(true);

        if let Err(e) = state.full(params, audio) {
            error!("Whisper inference failed: {:?}", e);
            return;
        }

        let n_segments = match state.full_n_segments() {
            Ok(n) => n,
            Err(e) => {
                error!("Failed to get segment count: {:?}", e);
                return;
            }
        };

        for i in 0..n_segments {
            let text = match state.full_get_segment_text(i) {
                Ok(t) => t.trim().to_string(),
                Err(e) => {
                    warn!("Failed to get segment {} text: {:?}", i, e);
                    continue;
                }
            };

            if text.is_empty() {
                continue;
            }

            // Whisper timestamps are in centiseconds (10ms units).
            let t0_cs = state.full_get_segment_t0(i).unwrap_or(0);
            // Convert to absolute milliseconds.
            let offset_ms = (samples_offset as i64 * 1000) / 16000;
            let timestamp_ms = offset_ms + t0_cs * 10;

            // Compute average token probability as confidence.
            let confidence = Self::compute_segment_confidence(&state, i);

            // Persist to database.
            if let Err(e) = db.insert_segment(meeting_id, timestamp_ms, &text, confidence) {
                error!("Failed to save transcript segment: {}", e);
            }

            // Invoke callback.
            if let Some(ref cb) = callback {
                let timestamp_secs = timestamp_ms as f64 / 1000.0;
                cb(&text, timestamp_secs);
            }
        }
    }

    /// Compute the average token probability for a segment as a confidence score.
    fn compute_segment_confidence(
        state: &whisper_rs::WhisperState,
        segment: std::ffi::c_int,
    ) -> Option<f64> {
        let n_tokens = match state.full_n_tokens(segment) {
            Ok(n) if n > 0 => n,
            _ => return None,
        };

        let mut sum = 0.0f64;
        let mut count = 0;
        for t in 0..n_tokens {
            if let Ok(prob) = state.full_get_token_prob(segment, t) {
                sum += prob as f64;
                count += 1;
            }
        }

        if count > 0 {
            Some(sum / count as f64)
        } else {
            None
        }
    }

    /// Signal the transcriber to stop and wait for it to finish.
    pub fn stop(mut self) {
        let _ = self.shutdown_tx.send(());
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for LocalTranscriber {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}
