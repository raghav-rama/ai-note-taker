//! Headless CLI that transcribes a WAV file through the core pipeline.
//!
//! CI runners have no microphone, so this binary feeds a fixture WAV into the
//! same `LocalTranscriber` the apps use and prints the resulting transcript.
//!
//! Two modes:
//!   transcribe-wav <wav> <model>              direct LocalTranscriber path
//!   transcribe-wav --via-ffi <wav> <model>    through the C FFI push-audio path
//! The second mode exercises note_start_recording_push / note_push_audio so CI
//! covers the system-audio entry point with raw PCM input.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context};

use notetaker::audio::Resampler;
use notetaker::models::TranscriptSegment;
use notetaker::storage::Database;
use notetaker::transcribe::{self, LocalTranscriber, TranscriberConfig, TranscriptCallback};

fn main() -> anyhow::Result<()> {
    let raw: Vec<String> = std::env::args().collect();
    let via_ffi = raw.iter().any(|a| a == "--via-ffi");
    let positionals: Vec<&String> = raw.iter().skip(1).filter(|a| !a.starts_with("--")).collect();
    if positionals.len() < 2 {
        eprintln!("usage: transcribe-wav [--via-ffi] <wav_path> <model_path>");
        std::process::exit(2);
    }
    let wav_path = positionals[0].as_str();
    let model_path = positionals[1].as_str();

    let samples = read_wav_resampled(wav_path)?;

    let text = if via_ffi {
        transcribe_via_ffi(model_path, &samples)?
    } else {
        transcribe_direct(model_path, &samples)?
    };

    println!("{}", text.trim());
    Ok(())
}

/// Read a WAV file and run it through the production downmix+resample path.
fn read_wav_resampled(wav_path: &str) -> anyhow::Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(wav_path)
        .with_context(|| format!("Failed to open WAV file: {}", wav_path))?;
    let spec = reader.spec();

    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            // Normalize by the full-scale value for the bit depth (32768.0 at 16-bit).
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<Vec<f32>, _>>()?
        }
        hound::SampleFormat::Float => {
            reader.samples::<f32>().collect::<Result<Vec<f32>, _>>()?
        }
    };

    // Drive the SAME production downmix+resample the microphone pipeline uses
    // (core::audio::Resampler). channels.max(1) guards against a malformed header.
    let mut resampler = Resampler::new(spec.sample_rate, 16000, spec.channels.max(1));
    Ok(resampler.process(&interleaved))
}

/// Open a throwaway database in the system temp dir.
fn temp_db() -> anyhow::Result<(String, Arc<Database>)> {
    let path = std::env::temp_dir().join(format!("transcribe_wav_{}.db", std::process::id()));
    let path = path
        .to_str()
        .context("Temp database path is not valid UTF-8")?
        .to_string();
    let db = Arc::new(Database::open(&path)?);
    Ok((path, db))
}

/// Transcribe by driving `LocalTranscriber` directly.
fn transcribe_direct(model_path: &str, samples: &[f32]) -> anyhow::Result<String> {
    let (_db_path, db) = temp_db()?;
    let meeting_id = db.create_meeting("wav")?;

    let (tx, rx) = mpsc::channel::<Vec<f32>>();

    let collector = Arc::new(Mutex::new(String::new()));
    let collector_cb = Arc::clone(&collector);
    // `done_tx` is owned solely by the callback, which the worker thread drops when
    // it exits via the channel-disconnected path. Draining `done_rx` therefore
    // blocks until every queued chunk is processed, so `stop()` never truncates.
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let callback: TranscriptCallback = Box::new(move |text: &str, _ts: f64| {
        if let Ok(mut s) = collector_cb.lock() {
            s.push_str(text);
            s.push(' ');
        }
        let _ = done_tx.send(());
    });

    let ctx = transcribe::load_context(model_path)?;
    let transcriber = LocalTranscriber::new(
        ctx,
        TranscriberConfig::default(),
        rx,
        Arc::clone(&db),
        meeting_id,
        Some(callback),
    )?;

    for chunk in samples.chunks(16000) {
        tx.send(chunk.to_vec()).ok();
    }
    drop(tx);

    for _ in done_rx {}
    transcriber.stop();

    let text = collector.lock().unwrap().clone();
    Ok(text)
}

/// Transcribe through the C FFI push-audio path (note_start_recording_push +
/// note_push_audio), then read the stored segments back via note_get_segments.
fn transcribe_via_ffi(model_path: &str, samples: &[f32]) -> anyhow::Result<String> {
    use std::ffi::{CStr, CString};

    let db_path = std::env::temp_dir().join(format!("transcribe_wav_ffi_{}.db", std::process::id()));
    let db_c = CString::new(db_path.to_string_lossy().into_owned())?;
    let model_c = CString::new(model_path)?;

    let handle = notetaker::note_init(db_c.as_ptr(), model_c.as_ptr());
    if handle.is_null() {
        bail!("note_init returned null");
    }

    let rc = notetaker::note_start_recording_push(handle);
    if rc != notetaker::NOTE_OK {
        notetaker::note_destroy(handle);
        bail!("note_start_recording_push failed: {}", rc);
    }

    for chunk in samples.chunks(16000) {
        let rc = notetaker::note_push_audio(handle, chunk.as_ptr(), chunk.len());
        if rc != notetaker::NOTE_OK {
            notetaker::note_destroy(handle);
            bail!("note_push_audio failed: {}", rc);
        }
    }

    let rc = notetaker::note_stop_recording(handle);
    if rc != notetaker::NOTE_OK {
        notetaker::note_destroy(handle);
        bail!("note_stop_recording failed: {}", rc);
    }

    // The push session created meeting id 1 in the fresh database.
    let segments_ptr = notetaker::note_get_segments(handle, 1);
    if segments_ptr.is_null() {
        notetaker::note_destroy(handle);
        bail!("note_get_segments returned null");
    }
    let json = unsafe { CStr::from_ptr(segments_ptr) }
        .to_string_lossy()
        .into_owned();
    notetaker::note_free_string(segments_ptr);
    notetaker::note_destroy(handle);

    let segments: Vec<TranscriptSegment> =
        serde_json::from_str(&json).context("Failed to parse segments JSON")?;
    let text = segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(text)
}
