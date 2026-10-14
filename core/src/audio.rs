//! Audio capture module using cpal.
//!
//! Provides device enumeration, real-time audio capture with resampling
//! to 16kHz mono f32 (Whisper's native format), and audio level metering.
//!
//! The cpal `Stream` is `!Send`, so it lives on a dedicated thread that
//! receives commands over a channel.

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use log::{error, info, warn};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::models::AudioDevice;

/// Commands sent to the audio thread.
enum AudioCommand {
    Start,
    Stop,
    Pause,
    Resume,
    Shutdown,
}

/// Callback invoked with the current RMS audio level (0.0 to 1.0).
pub type AudioLevelCallback = Box<dyn Fn(f32) + Send + Sync + 'static>;

/// Audio capture engine wrapping cpal.
///
/// Audio samples are resampled to 16kHz mono f32 and sent to `audio_tx`.
/// The `Stream` object lives on a background thread since it is `!Send`.
pub struct AudioCapture {
    cmd_tx: mpsc::Sender<AudioCommand>,
    thread_handle: Option<thread::JoinHandle<()>>,
}

/// State for linear interpolation resampling.
pub struct Resampler {
    source_rate: u32,
    target_rate: u32,
    channels: u16,
    /// Fractional position in source samples.
    frac_pos: f64,
    /// Previous sample value for interpolation.
    prev_sample: f32,
}

impl Resampler {
    pub fn new(source_rate: u32, target_rate: u32, channels: u16) -> Self {
        Self {
            source_rate,
            target_rate,
            channels,
            frac_pos: 0.0,
            prev_sample: 0.0,
        }
    }

    /// Resample interleaved multi-channel input to mono 16kHz f32.
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        // First downmix to mono by averaging channels.
        let mono: Vec<f32> = input
            .chunks(self.channels as usize)
            .map(|frame| {
                let sum: f32 = frame.iter().sum();
                sum / self.channels as f32
            })
            .collect();

        if self.source_rate == self.target_rate {
            return mono;
        }

        let ratio = self.source_rate as f64 / self.target_rate as f64;
        let mut output = Vec::with_capacity((mono.len() as f64 / ratio) as usize + 1);

        for &sample in &mono {
            let cur_sample = sample;
            // Emit output samples while the fractional position is within this source sample.
            while self.frac_pos < 1.0 {
                let interp =
                    self.prev_sample + (cur_sample - self.prev_sample) * self.frac_pos as f32;
                output.push(interp);
                self.frac_pos += ratio;
            }
            self.frac_pos -= 1.0;
            self.prev_sample = cur_sample;
        }

        output
    }
}

impl AudioCapture {
    /// Create a new audio capture that sends resampled chunks to `audio_tx`.
    ///
    /// `device_id` selects the input device; `None` uses the default.
    /// `level_callback` is called with the RMS level for UI metering.
    pub fn new(
        device_id: Option<&str>,
        audio_tx: mpsc::Sender<Vec<f32>>,
        level_callback: Option<AudioLevelCallback>,
    ) -> Result<Self> {
        let host = cpal::default_host();

        let device = if let Some(id) = device_id {
            host.input_devices()
                .context("Failed to enumerate input devices")?
                .find(|d| d.name().map(|n| n == id).unwrap_or(false))
                .ok_or_else(|| anyhow::anyhow!("Audio device not found: {}", id))?
        } else {
            host.default_input_device()
                .ok_or_else(|| anyhow::anyhow!("No default input device available"))?
        };

        let device_name = device.name().unwrap_or_else(|_| "unknown".to_string());
        info!("Using audio input device: {}", device_name);

        let config = device
            .default_input_config()
            .context("Failed to get default input config")?;

        let sample_format = config.sample_format();
        let source_rate = config.sample_rate().0;
        let channels = config.channels();
        let stream_config: StreamConfig = config.into();

        let level_cb = level_callback.map(|cb| Arc::new(cb));

        let (cmd_tx, cmd_rx) = mpsc::channel::<AudioCommand>();

        let thread_handle = thread::spawn(move || {
            let resampler = Arc::new(Mutex::new(Resampler::new(source_rate, 16000, channels)));

            let audio_tx_clone = audio_tx.clone();
            let level_cb_clone = level_cb.clone();
            let resampler_clone = resampler.clone();

            let err_fn = |err: cpal::StreamError| {
                error!("Audio stream error: {}", err);
            };

            let build_result = match sample_format {
                SampleFormat::F32 => device.build_input_stream(
                    &stream_config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        // Compute RMS level for metering.
                        if let Some(ref cb) = level_cb_clone {
                            if !data.is_empty() {
                                let sum_sq: f32 = data.iter().map(|s| s * s).sum();
                                let rms = (sum_sq / data.len() as f32).sqrt();
                                cb(rms.min(1.0));
                            }
                        }
                        // Resample and send.
                        let resampled = resampler_clone.lock().unwrap().process(data);
                        if !resampled.is_empty() {
                            let _ = audio_tx_clone.send(resampled);
                        }
                    },
                    err_fn,
                    None,
                ),
                SampleFormat::I16 => {
                    let audio_tx_i16 = audio_tx.clone();
                    let level_cb_i16 = level_cb.clone();
                    let resampler_i16 = resampler.clone();
                    device.build_input_stream(
                        &stream_config,
                        move |data: &[i16], _: &cpal::InputCallbackInfo| {
                            let float_data: Vec<f32> =
                                data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                            if let Some(ref cb) = level_cb_i16 {
                                if !float_data.is_empty() {
                                    let sum_sq: f32 = float_data.iter().map(|s| s * s).sum();
                                    let rms = (sum_sq / float_data.len() as f32).sqrt();
                                    cb(rms.min(1.0));
                                }
                            }
                            let resampled = resampler_i16.lock().unwrap().process(&float_data);
                            if !resampled.is_empty() {
                                let _ = audio_tx_i16.send(resampled);
                            }
                        },
                        err_fn,
                        None,
                    )
                }
                SampleFormat::U16 => {
                    let audio_tx_u16 = audio_tx;
                    let level_cb_u16 = level_cb;
                    let resampler_u16 = resampler;
                    device.build_input_stream(
                        &stream_config,
                        move |data: &[u16], _: &cpal::InputCallbackInfo| {
                            let float_data: Vec<f32> = data
                                .iter()
                                .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                                .collect();
                            if let Some(ref cb) = level_cb_u16 {
                                if !float_data.is_empty() {
                                    let sum_sq: f32 = float_data.iter().map(|s| s * s).sum();
                                    let rms = (sum_sq / float_data.len() as f32).sqrt();
                                    cb(rms.min(1.0));
                                }
                            }
                            let resampled = resampler_u16.lock().unwrap().process(&float_data);
                            if !resampled.is_empty() {
                                let _ = audio_tx_u16.send(resampled);
                            }
                        },
                        err_fn,
                        None,
                    )
                }
                fmt => {
                    error!("Unsupported sample format: {:?}", fmt);
                    return;
                }
            };

            let stream = match build_result {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to build input stream: {}", e);
                    return;
                }
            };

            // Wait for Start command before playing.
            loop {
                match cmd_rx.recv() {
                    Ok(AudioCommand::Start) => {
                        if let Err(e) = stream.play() {
                            error!("Failed to start audio stream: {}", e);
                            return;
                        }
                        info!("Audio capture started");
                        break;
                    }
                    Ok(AudioCommand::Shutdown) => return,
                    _ => {}
                }
            }

            // Main command loop.
            loop {
                match cmd_rx.recv() {
                    Ok(AudioCommand::Stop) | Ok(AudioCommand::Shutdown) => {
                        // Stream is dropped when this thread exits.
                        info!("Audio capture stopped");
                        return;
                    }
                    Ok(AudioCommand::Pause) => {
                        if let Err(e) = stream.pause() {
                            warn!("Failed to pause audio stream: {}", e);
                        }
                        info!("Audio capture paused");
                    }
                    Ok(AudioCommand::Resume) | Ok(AudioCommand::Start) => {
                        if let Err(e) = stream.play() {
                            warn!("Failed to resume audio stream: {}", e);
                        }
                        info!("Audio capture resumed");
                    }
                    Err(_) => {
                        // Command channel closed.
                        return;
                    }
                }
            }
        });

        Ok(Self {
            cmd_tx,
            thread_handle: Some(thread_handle),
        })
    }

    /// Start capturing audio.
    pub fn start(&self) -> Result<()> {
        self.cmd_tx
            .send(AudioCommand::Start)
            .map_err(|_| anyhow::anyhow!("Audio thread has exited"))
    }

    /// Stop capturing audio. The capture cannot be restarted.
    pub fn stop(&self) -> Result<()> {
        self.cmd_tx
            .send(AudioCommand::Stop)
            .map_err(|_| anyhow::anyhow!("Audio thread has exited"))
    }

    /// Pause audio capture (can be resumed).
    pub fn pause(&self) -> Result<()> {
        self.cmd_tx
            .send(AudioCommand::Pause)
            .map_err(|_| anyhow::anyhow!("Audio thread has exited"))
    }

    /// Resume a paused capture.
    pub fn resume(&self) -> Result<()> {
        self.cmd_tx
            .send(AudioCommand::Resume)
            .map_err(|_| anyhow::anyhow!("Audio thread has exited"))
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(AudioCommand::Shutdown);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}

/// Enumerate available audio input devices.
pub fn list_audio_devices() -> Result<Vec<AudioDevice>> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.name().ok());

    let mut devices = Vec::new();
    for device in host
        .input_devices()
        .context("Failed to enumerate input devices")?
    {
        if let Ok(name) = device.name() {
            let is_default = default_name.as_deref() == Some(&name);
            devices.push(AudioDevice {
                id: name.clone(),
                name,
                is_default,
            });
        }
    }
    Ok(devices)
}

#[cfg(test)]
mod tests {
    use super::Resampler;

    #[test]
    fn downmixes_interleaved_stereo_to_mono() {
        // Two stereo frames at a matching rate: channels are averaged, no resample.
        let mut r = Resampler::new(16000, 16000, 2);
        let out = r.process(&[1.0, 3.0, -2.0, 2.0]);
        assert_eq!(out, vec![2.0, 0.0]);
    }

    #[test]
    fn resamples_48k_to_16k_via_interpolation() {
        // source_rate != target_rate drives the interpolation path that the
        // 16kHz jfk.wav smoke-test fixture never exercises. 48k -> 16k keeps
        // roughly one of every three input samples.
        let mut r = Resampler::new(48000, 16000, 1);
        let input: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.01).sin()).collect();
        let out = r.process(&input);

        let expected = input.len() / 3;
        assert!(
            (out.len() as i64 - expected as i64).abs() <= 2,
            "expected ~{expected} samples, got {}",
            out.len()
        );
        assert!(out.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn upsamples_8k_to_16k_via_interpolation() {
        // Upsampling exercises the same path in the other direction (~2x samples).
        let mut r = Resampler::new(8000, 16000, 1);
        let input: Vec<f32> = (0..800).map(|i| (i as f32 * 0.02).sin()).collect();
        let out = r.process(&input);

        let expected = input.len() * 2;
        assert!(
            (out.len() as i64 - expected as i64).abs() <= 2,
            "expected ~{expected} samples, got {}",
            out.len()
        );
    }
}
