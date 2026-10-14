//! Transcription module using whisper-rs (whisper.cpp bindings).
//!
//! Audio arrives as resampled 16kHz mono f32 and is accumulated into overlapping
//! windows, run through a single long-lived Whisper inference state (so decoder
//! context carries across windows), and delivered through a callback.
//!
//! The quality-sensitive buffering logic — window sizing, overlap, the 30s memory
//! cap, energy-based silence detection, and timestamp-based de-duplication of the
//! re-transcribed overlap region — lives in small whisper-free helpers
//! ([`AudioChunker`], [`SegmentDeduper`], [`rms`]) so it can be unit-tested
//! without a model.

use anyhow::Result;
use log::{error, info, warn};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContextParameters};
// Re-exported so FFI consumers (e.g. the Linux bridge) can name the cached
// context type without depending on whisper-rs directly.
pub use whisper_rs::WhisperContext;

use crate::storage::Database;

/// Sample rate of the audio fed to the transcriber (Whisper's native rate).
/// Private so cbindgen does not export it into the C header as a `#define`.
const SAMPLE_RATE: usize = 16000;

/// Target window size in samples (~4 seconds).
const CHUNK_SIZE: usize = SAMPLE_RATE * 4;

/// Overlap retained between consecutive windows (~1 second), so a word split
/// across a window boundary is seen whole in the next window.
const OVERLAP_SIZE: usize = SAMPLE_RATE;

/// Minimum samples to hand to whisper.cpp (it needs ~1 second).
const MIN_SAMPLES: usize = SAMPLE_RATE;

/// Hard cap on the internal buffer (~30 seconds). If inference falls behind
/// real time, the oldest audio is dropped rather than growing without bound.
const MAX_BUFFER: usize = SAMPLE_RATE * 30;

/// RMS level below which a window is treated as silence and skipped (no
/// inference). Kept low so quiet speech is not discarded.
const SILENCE_RMS: f32 = 0.008;

/// Transcription parameters read from settings by the caller.
#[derive(Debug, Clone)]
pub struct TranscriberConfig {
    /// Whisper language code (e.g. "en"), or "auto" for auto-detection.
    pub language: String,
    /// Number of inference threads.
    pub n_threads: i32,
}

impl Default for TranscriberConfig {
    fn default() -> Self {
        Self {
            language: "en".to_string(),
            n_threads: 4,
        }
    }
}

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

/// Load a Whisper model into a reference-counted context.
///
/// Callers cache the returned context so the (multi-second) load happens once per
/// model rather than on every recording start.
pub fn load_context(model_path: &str) -> Result<Arc<WhisperContext>> {
    let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::new())
        .map_err(|e| anyhow::anyhow!("Failed to load Whisper model from {}: {:?}", model_path, e))?;
    info!("Whisper model loaded from {}", model_path);
    Ok(Arc::new(ctx))
}

/// Root-mean-square level of a buffer of samples (0.0 for empty input).
pub(crate) fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

/// Accumulates incoming audio and yields fixed-size, overlapping windows while
/// capping total buffered memory. Whisper-free so it is unit-testable.
pub(crate) struct AudioChunker {
    buffer: Vec<f32>,
    chunk_size: usize,
    overlap: usize,
    max_buffer: usize,
    /// Absolute sample index (from stream start) of `buffer[0]`.
    base_pos: usize,
}

impl AudioChunker {
    fn new(chunk_size: usize, overlap: usize, max_buffer: usize) -> Self {
        debug_assert!(overlap < chunk_size);
        debug_assert!(max_buffer >= chunk_size);
        Self {
            buffer: Vec::with_capacity(chunk_size * 2),
            chunk_size,
            overlap,
            max_buffer,
            base_pos: 0,
        }
    }

    /// Append samples. If the buffer would exceed `max_buffer`, the oldest
    /// samples are dropped (advancing the absolute base position) so memory is
    /// bounded even when inference cannot keep up.
    fn push(&mut self, samples: &[f32]) {
        self.buffer.extend_from_slice(samples);
        if self.buffer.len() > self.max_buffer {
            let drop = self.buffer.len() - self.max_buffer;
            self.buffer.drain(..drop);
            self.base_pos += drop;
        }
    }

    /// If a full window is available, return `(absolute_offset_samples, window)`
    /// and advance by `chunk_size - overlap`, retaining the overlap tail for the
    /// next window.
    fn next_window(&mut self) -> Option<(usize, Vec<f32>)> {
        if self.buffer.len() < self.chunk_size {
            return None;
        }
        let window = self.buffer[..self.chunk_size].to_vec();
        let offset = self.base_pos;
        let advance = self.chunk_size - self.overlap;
        self.buffer.drain(..advance);
        self.base_pos += advance;
        Some((offset, window))
    }

    /// Drain whatever remains as a final window (padded to `min_samples` so
    /// whisper.cpp accepts it), or `None` if the buffer is empty. Used at
    /// shutdown so trailing audio is not lost.
    fn flush(&mut self, min_samples: usize) -> Option<(usize, Vec<f32>)> {
        if self.buffer.is_empty() {
            return None;
        }
        let offset = self.base_pos;
        let mut window = std::mem::take(&mut self.buffer);
        let real_len = window.len();
        if window.len() < min_samples {
            window.resize(min_samples, 0.0);
        }
        self.base_pos += real_len;
        Some((offset, window))
    }
}

/// Drops segments whose audio was already emitted by a previous (overlapping)
/// window, keyed on absolute millisecond timestamps. Whisper-free, unit-testable.
pub(crate) struct SegmentDeduper {
    /// Absolute end (ms) of the most recently emitted audio; -1 before anything.
    emitted_until_ms: i64,
}

impl SegmentDeduper {
    fn new() -> Self {
        Self {
            emitted_until_ms: -1,
        }
    }

    /// Returns true if a segment spanning `[start_ms, end_ms)` should be emitted.
    /// A segment that starts within already-emitted audio is a re-transcription
    /// of the overlap region and is dropped.
    fn accept(&mut self, start_ms: i64, end_ms: i64) -> bool {
        if start_ms < self.emitted_until_ms {
            return false;
        }
        self.emitted_until_ms = self.emitted_until_ms.max(end_ms);
        true
    }
}

/// Local transcription engine using whisper.cpp.
pub struct LocalTranscriber {
    /// Handle to the transcription worker thread.
    thread_handle: Option<thread::JoinHandle<()>>,
    /// Send a `()` to signal shutdown.
    shutdown_tx: mpsc::Sender<()>,
}

impl LocalTranscriber {
    /// Start a transcription worker that reads audio chunks from `audio_rx`.
    ///
    /// - `ctx`: a preloaded Whisper context (see [`load_context`]); shared so the
    ///   model is not reloaded per recording.
    /// - `config`: language and thread settings.
    /// - `audio_rx`: receiver of resampled 16kHz mono f32 audio chunks.
    /// - `db`: shared database handle for persisting segments.
    /// - `meeting_id`: the current meeting being recorded.
    /// - `callback`: invoked with each transcribed segment's text and timestamp (seconds).
    ///
    /// Returns once the worker has successfully created its inference state, so a
    /// failure to do so is reported to the caller immediately.
    pub fn new(
        ctx: Arc<WhisperContext>,
        config: TranscriberConfig,
        audio_rx: mpsc::Receiver<Vec<f32>>,
        db: Arc<Database>,
        meeting_id: i64,
        callback: Option<TranscriptCallback>,
    ) -> Result<Self> {
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
        // The worker reports whether it created its inference state so the caller
        // gets an immediate error instead of a silently dead transcriber.
        let (ready_tx, ready_rx) = mpsc::channel::<std::result::Result<(), String>>();

        let thread_handle = thread::spawn(move || {
            // `ctx` is owned by this closure for the thread's lifetime; `state`
            // (created from it) is dropped first at scope end.
            let mut state = match ctx.create_state() {
                Ok(s) => {
                    let _ = ready_tx.send(Ok(()));
                    s
                }
                Err(e) => {
                    let msg = format!("Failed to create Whisper state: {:?}", e);
                    error!("{}", msg);
                    let _ = ready_tx.send(Err(msg));
                    return;
                }
            };

            run_worker(
                &mut state,
                &config,
                audio_rx,
                shutdown_rx,
                &db,
                meeting_id,
                &callback,
            );
        });

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                thread_handle: Some(thread_handle),
                shutdown_tx,
            }),
            Ok(Err(msg)) => {
                let _ = thread_handle.join();
                Err(anyhow::anyhow!("{}", msg))
            }
            Err(_) => {
                let _ = thread_handle.join();
                Err(anyhow::anyhow!("Transcriber thread exited unexpectedly"))
            }
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

/// The worker loop: accumulate audio into overlapping windows and transcribe
/// each through the shared `state`.
#[allow(clippy::too_many_arguments)]
fn run_worker(
    state: &mut whisper_rs::WhisperState,
    config: &TranscriberConfig,
    audio_rx: mpsc::Receiver<Vec<f32>>,
    shutdown_rx: mpsc::Receiver<()>,
    db: &Database,
    meeting_id: i64,
    callback: &Option<TranscriptCallback>,
) {
    let mut chunker = AudioChunker::new(CHUNK_SIZE, OVERLAP_SIZE, MAX_BUFFER);
    let mut deduper = SegmentDeduper::new();

    let drain_windows =
        |chunker: &mut AudioChunker, state: &mut whisper_rs::WhisperState, deduper: &mut SegmentDeduper| {
            while let Some((offset, window)) = chunker.next_window() {
                transcribe_window(state, config, &window, offset, deduper, db, meeting_id, callback);
            }
        };

    loop {
        // Shutdown: drain anything still queued, flush the tail, and exit.
        if shutdown_rx.try_recv().is_ok() {
            while let Ok(chunk) = audio_rx.try_recv() {
                chunker.push(&chunk);
                drain_windows(&mut chunker, state, &mut deduper);
            }
            if let Some((offset, window)) = chunker.flush(MIN_SAMPLES) {
                transcribe_window(
                    state, config, &window, offset, &mut deduper, db, meeting_id, callback,
                );
            }
            info!("Transcriber shutting down");
            return;
        }

        match audio_rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(chunk) => {
                chunker.push(&chunk);
                drain_windows(&mut chunker, state, &mut deduper);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // Audio source closed: flush the remaining buffer and exit.
                if let Some((offset, window)) = chunker.flush(MIN_SAMPLES) {
                    transcribe_window(
                        state, config, &window, offset, &mut deduper, db, meeting_id, callback,
                    );
                }
                info!("Audio channel closed, transcriber exiting");
                return;
            }
        }
    }
}

/// Run Whisper inference on a single window and persist/emit its (de-duplicated)
/// segments. Silent windows are skipped without running inference.
#[allow(clippy::too_many_arguments)]
fn transcribe_window(
    state: &mut whisper_rs::WhisperState,
    config: &TranscriberConfig,
    audio: &[f32],
    offset_samples: usize,
    deduper: &mut SegmentDeduper,
    db: &Database,
    meeting_id: i64,
    callback: &Option<TranscriptCallback>,
) {
    // Skip inference on near-silent audio (saves CPU and avoids hallucinated text).
    if rms(audio) < SILENCE_RMS {
        return;
    }

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_n_threads(config.n_threads);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_special(false);
    params.set_print_timestamps(false);
    params.set_single_segment(false);
    // Carry decoder context across windows (the state is reused per session).
    params.set_no_context(false);
    params.set_token_timestamps(true);
    // "auto" (or empty) lets whisper.cpp detect the language; otherwise force it.
    if !config.language.is_empty() && config.language != "auto" {
        params.set_language(Some(config.language.as_str()));
    }

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

    let offset_ms = (offset_samples as i64 * 1000) / SAMPLE_RATE as i64;

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

        // Whisper timestamps are centiseconds (10ms) relative to this window.
        let t0_cs = state.full_get_segment_t0(i).unwrap_or(0);
        let t1_cs = state.full_get_segment_t1(i).unwrap_or(t0_cs);
        let start_ms = offset_ms + t0_cs * 10;
        let end_ms = offset_ms + t1_cs * 10;

        // Drop segments re-transcribed from the overlap of a previous window.
        if !deduper.accept(start_ms, end_ms) {
            continue;
        }

        let confidence = compute_segment_confidence(state, i);

        if let Err(e) = db.insert_segment(meeting_id, start_ms, &text, confidence) {
            error!("Failed to save transcript segment: {}", e);
        }

        if let Some(cb) = callback {
            cb(&text, start_ms as f64 / 1000.0);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_of_silence_is_zero() {
        assert_eq!(rms(&[]), 0.0);
        assert_eq!(rms(&[0.0; 100]), 0.0);
    }

    #[test]
    fn rms_of_full_scale_is_one() {
        let s = vec![1.0f32; 50];
        assert!((rms(&s) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn chunker_emits_full_windows_with_overlap_offsets() {
        // chunk 10, overlap 4 → advance 6 per window.
        let mut c = AudioChunker::new(10, 4, 1000);
        c.push(&vec![1.0; 10]);
        let (off0, w0) = c.next_window().expect("first window");
        assert_eq!(off0, 0);
        assert_eq!(w0.len(), 10);
        // Only 4 (overlap) remain; not enough for another window yet.
        assert!(c.next_window().is_none());

        // Add 6 more → 10 buffered again; next window starts at advance=6.
        c.push(&vec![1.0; 6]);
        let (off1, w1) = c.next_window().expect("second window");
        assert_eq!(off1, 6);
        assert_eq!(w1.len(), 10);
    }

    #[test]
    fn chunker_caps_memory_and_advances_base() {
        // max_buffer 20; pushing 30 drops the oldest 10.
        let mut c = AudioChunker::new(10, 4, 20);
        c.push(&vec![0.5; 30]);
        // base_pos advanced by the dropped amount (10); a window now starts there.
        let (off, w) = c.next_window().expect("window after cap");
        assert_eq!(off, 10);
        assert_eq!(w.len(), 10);
    }

    #[test]
    fn chunker_flush_pads_short_tail() {
        let mut c = AudioChunker::new(10, 4, 100);
        c.push(&vec![0.3; 3]);
        assert!(c.next_window().is_none());
        let (off, w) = c.flush(8).expect("flush tail");
        assert_eq!(off, 0);
        assert_eq!(w.len(), 8); // padded from 3 up to min 8
    }

    #[test]
    fn chunker_flush_empty_is_none() {
        let mut c = AudioChunker::new(10, 4, 100);
        assert!(c.flush(8).is_none());
    }

    #[test]
    fn deduper_drops_overlap_resegments_keeps_new() {
        let mut d = SegmentDeduper::new();
        // First window emits [0,2000) and [2000,4000).
        assert!(d.accept(0, 2000));
        assert!(d.accept(2000, 4000));
        // Next (overlapping) window re-transcribes [3000,4000) → dropped.
        assert!(!d.accept(3000, 4000));
        // Genuinely new audio past the emitted point is kept.
        assert!(d.accept(4000, 6000));
        // A late/again-overlapping segment is dropped.
        assert!(!d.accept(5000, 6000));
    }

    #[test]
    fn deduper_accepts_first_segment_at_zero() {
        let mut d = SegmentDeduper::new();
        assert!(d.accept(0, 500));
    }

    #[test]
    fn config_default_is_english_four_threads() {
        let c = TranscriberConfig::default();
        assert_eq!(c.language, "en");
        assert_eq!(c.n_threads, 4);
    }
}
