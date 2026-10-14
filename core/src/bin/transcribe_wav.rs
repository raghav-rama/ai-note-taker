//! Headless CLI that transcribes a WAV file through the core pipeline.
//!
//! CI runners have no microphone, so this binary feeds a fixture WAV into the
//! same `LocalTranscriber` the apps use and prints the resulting transcript.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use anyhow::Context;

use notetaker::audio::Resampler;
use notetaker::storage::Database;
use notetaker::transcribe::{LocalTranscriber, TranscriptCallback};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: transcribe-wav <wav_path> <model_path>");
        std::process::exit(2);
    }
    let wav_path = &args[1];
    let model_path = &args[2];

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
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<f32>, _>>()?,
    };

    // Drive the SAME production downmix+resample the microphone pipeline uses
    // (core::audio::Resampler), so this CI gate actually covers that path rather
    // than a parallel copy. channels.max(1) guards against a malformed header.
    let mut resampler = Resampler::new(spec.sample_rate, 16000, spec.channels.max(1));
    let samples = resampler.process(&interleaved);

    // Throwaway database so the transcriber has somewhere to persist segments.
    let db_path = std::env::temp_dir().join(format!("transcribe_wav_{}.db", std::process::id()));
    let db_path = db_path
        .to_str()
        .context("Temp database path is not valid UTF-8")?;
    let db = Arc::new(Database::open(db_path)?);
    let meeting_id = db.create_meeting("wav")?;

    let (tx, rx) = mpsc::channel::<Vec<f32>>();

    let collector = Arc::new(Mutex::new(String::new()));
    let collector_cb = Arc::clone(&collector);
    // `done_tx` is owned solely by the callback, which the worker thread drops when it
    // exits via the channel-disconnected path. Draining `done_rx` therefore blocks until
    // the worker has processed every queued chunk, so `stop()` never truncates the audio.
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let callback: TranscriptCallback = Box::new(move |text: &str, _ts: f64| {
        if let Ok(mut s) = collector_cb.lock() {
            s.push_str(text);
            s.push(' ');
        }
        let _ = done_tx.send(());
    });

    let transcriber =
        LocalTranscriber::new(model_path, rx, Arc::clone(&db), meeting_id, Some(callback))?;

    for chunk in samples.chunks(16000) {
        tx.send(chunk.to_vec()).ok();
    }
    drop(tx);

    for _ in done_rx {}
    transcriber.stop();

    println!("{}", collector.lock().unwrap().trim());
    Ok(())
}
