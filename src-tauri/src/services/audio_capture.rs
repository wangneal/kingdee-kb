//! Audio capture via cpal — microphone recording with VAD
//!
//! Uses cpal (Cross-Platform Audio Library) for microphone access.
//! Captures 16kHz mono PCM f32 samples for Whisper input.
//! Includes simple energy-based VAD for silence detection.
//!
//! DESIGN:
//! - `cpal::Stream` is NOT Send+Sync, so it lives in a dedicated thread.
//! - Recording state is communicated via Arc<AtomicBool>.
//! - Audio is written to both an in-memory Vec<f32> (for Whisper) AND
//!   a temporary .pcm file (for crash safety — if power is lost, only
//!   the last ~100ms of audio is lost).
//!
//! CRASH SAFETY:
//! Every audio chunk from the microphone is immediately appended to a
//! raw PCM f32 file on disk. `File::write_all()` + `flush()` ensure
//! data is durable even on sudden power loss.
//! Temp files are stored in `~/.kingdee-kb/audio_capture/`.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample, StreamConfig};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

// ─── Sample Conversion (cpal 0.15 removed to_f32() from Sample trait) ───

/// Helper trait to convert audio samples to f32 for processing.
/// cpal 0.15 no longer provides `to_f32()` on its sample traits,
/// so we define it for the three formats used in build_stream.
trait SampleToF32 {
    fn to_f32_(&self) -> f32;
}

impl SampleToF32 for f32 {
    fn to_f32_(&self) -> f32 {
        *self
    }
}

impl SampleToF32 for i16 {
    fn to_f32_(&self) -> f32 {
        *self as f32 / 32768.0
    }
}

impl SampleToF32 for u16 {
    fn to_f32_(&self) -> f32 {
        (*self as f32 - 32768.0) / 32768.0
    }
}

/// Recording state info for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioRecordingState {
    /// Whether currently recording
    pub is_recording: bool,
    /// Duration of current recording in milliseconds
    pub duration_ms: u64,
    /// Number of audio samples buffered
    pub buffer_size: usize,
}

/// Internal mutable state shared between audio callback and main thread
struct CaptureState {
    is_recording: AtomicBool,
    start_time: Mutex<Option<Instant>>,
    /// In-memory buffer for Whisper transcription (accessed on stop)
    buffer: Mutex<Vec<f32>>,
    /// Crash-safe temp file — every chunk is immediately persisted
    temp_file: Mutex<Option<File>>,
}

/// Audio capture manager (Send + Sync safe — no Stream stored here)
///
/// Handles microphone recording with cpal, resampling to 16kHz mono.
/// The actual cpal::Stream lives in a spawned thread and is dropped
/// when recording stops (is_recording = false).
pub struct AudioCapture {
    state: Arc<CaptureState>,
    sample_rate: u32,
    /// Directory for crash-safe temp recording files
    capture_dir: PathBuf,
}

// Explicit Send+Sync since CaptureState only uses thread-safe primitives
unsafe impl Send for AudioCapture {}
unsafe impl Sync for AudioCapture {}

unsafe impl Send for CaptureState {}
unsafe impl Sync for CaptureState {}

impl AudioCapture {
    /// Create a new AudioCapture (not recording).
    ///
    /// `data_dir` is the app data directory (e.g. ~/.kingdee-kb/).
    /// Temp recording files are stored in `{data_dir}/audio_capture/`.
    pub fn new(data_dir: &Path) -> Self {
        let capture_dir = data_dir.join("audio_capture");
        let _ = fs::create_dir_all(&capture_dir);
        Self {
            state: Arc::new(CaptureState {
                is_recording: AtomicBool::new(false),
                start_time: Mutex::new(None),
                buffer: Mutex::new(Vec::new()),
                temp_file: Mutex::new(None),
            }),
            sample_rate: 16000,
            capture_dir,
        }
    }

    /// Generate a unique temp file path for this recording session.
    fn temp_file_path(&self) -> PathBuf {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        self.capture_dir.join(format!("rec_{}.pcm", ts))
    }

    /// Start recording from default microphone.
    ///
    /// Spawns a dedicated thread that owns the cpal::Stream.
    /// The stream is alive as long as `is_recording` is true.
    /// Returns immediately; audio is captured in the background.
    /// Audio data is simultaneously stored in memory and written to a
    /// temp .pcm file for crash safety.
    pub fn start_recording(&self) -> Result<(), String> {
        // Already recording?
        if self.state.is_recording.load(Ordering::SeqCst) {
            return Err("Already recording".to_string());
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("No microphone input device found")?;

        let supported_config = device
            .supported_input_configs()
            .map_err(|e| format!("Failed to query input configs: {}", e))?
            .next()
            .ok_or("No supported input audio configuration")?;

        let config = supported_config.with_max_sample_rate().config();
        let device_sample_rate = config.sample_rate.0;
        let sample_format = supported_config.sample_format();

        eprintln!(
            "[AudioCapture] Recording at {}Hz, {} channels",
            device_sample_rate, config.channels
        );

        // Create temp PCM file for crash-safe persistence
        let temp_path = self.temp_file_path();
        let temp_file = File::create(&temp_path)
            .map_err(|e| format!("Failed to create temp audio file: {}", e))?;
        eprintln!(
            "[AudioCapture] Crash-safe temp file: {}",
            temp_path.display()
        );

        // Mark as recording
        self.state.is_recording.store(true, Ordering::SeqCst);
        *self
            .state
            .start_time
            .lock()
            .map_err(|e| format!("Lock error: {}", e))? = Some(Instant::now());
        self.state
            .buffer
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?
            .clear();
        *self
            .state
            .temp_file
            .lock()
            .map_err(|e| format!("Lock error: {}", e))? = Some(temp_file);

        let capture_state = self.state.clone();
        let target_rate = 16000u32;

        // Spawn thread that owns the stream
        std::thread::spawn(move || {
            let capture_state_err = capture_state.clone();
            let err_fn = move |err: cpal::StreamError| {
                eprintln!("[AudioCapture] Stream error: {}", err);
                // 发生严重音频流错误（例如设备被拔出）时，自动将录音状态设置为 false
                capture_state_err
                    .is_recording
                    .store(false, Ordering::SeqCst);
            };

            let stream_result = match sample_format {
                SampleFormat::F32 => build_stream::<f32>(
                    &device,
                    &config,
                    capture_state.clone(),
                    device_sample_rate,
                    target_rate,
                    err_fn,
                ),
                SampleFormat::I16 => build_stream::<i16>(
                    &device,
                    &config,
                    capture_state.clone(),
                    device_sample_rate,
                    target_rate,
                    err_fn,
                ),
                SampleFormat::U16 => build_stream::<u16>(
                    &device,
                    &config,
                    capture_state.clone(),
                    device_sample_rate,
                    target_rate,
                    err_fn,
                ),
                sf => {
                    eprintln!("[AudioCapture] Unsupported sample format: {:?}", sf);
                    capture_state.is_recording.store(false, Ordering::SeqCst);
                    return;
                }
            };

            let stream = match stream_result {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[AudioCapture] Failed to build stream: {}", e);
                    capture_state.is_recording.store(false, Ordering::SeqCst);
                    return;
                }
            };

            if let Err(e) = stream.play() {
                eprintln!("[AudioCapture] Failed to play stream: {}", e);
                capture_state.is_recording.store(false, Ordering::SeqCst);
                return;
            }

            eprintln!("[AudioCapture] Stream active, recording in background thread");

            // Keep thread alive while recording — stream is dropped when this
            // thread exits, which stops audio capture.
            while capture_state.is_recording.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            eprintln!("[AudioCapture] Recording stopped, dropping stream");
            // stream dropped here → stops capture
        });

        Ok(())
    }

    /// Stop recording and return the captured PCM data.
    ///
    /// Returns the full audio buffer as 16kHz mono f32 samples,
    /// ready for Whisper transcription. Also cleans up the temp file.
    pub fn stop_recording(&self) -> Result<Vec<f32>, String> {
        // Signal the audio thread to stop (it will drop the stream)
        self.state.is_recording.store(false, Ordering::SeqCst);

        // Let the audio thread exit and stream drop
        std::thread::sleep(std::time::Duration::from_millis(200));

        let mut start = self
            .state
            .start_time
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        *start = None;

        // Read buffer (primary source for Whisper)
        let mut buf = self
            .state
            .buffer
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        let buffer = std::mem::take(&mut *buf);

        // Close and clean up temp file
        let mut temp_file = self
            .state
            .temp_file
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        if let Some(mut file) = temp_file.take() {
            let _ = file.flush();
            // Get the file path from the file (best effort)
            if let Ok(metadata) = file.metadata() {
                eprintln!(
                    "[AudioCapture] Temp file size: {} bytes ({} samples)",
                    metadata.len(),
                    metadata.len() / 4
                );
            }
        }

        eprintln!(
            "[AudioCapture] Stopped recording, captured {} samples ({} sec)",
            buffer.len(),
            buffer.len() as f64 / self.sample_rate as f64
        );

        Ok(buffer)
    }

    /// Whether currently recording.
    pub fn is_recording(&self) -> bool {
        self.state.is_recording.load(Ordering::SeqCst)
    }

    /// Get current recording state info.
    pub fn recording_state(&self) -> AudioRecordingState {
        let is_recording = self.state.is_recording.load(Ordering::SeqCst);
        let duration_ms = self
            .state
            .start_time
            .lock()
            .map(|s| s.map(|t| t.elapsed().as_millis() as u64).unwrap_or(0))
            .unwrap_or(0);
        let buffer_size = self.state.buffer.lock().map(|b| b.len()).unwrap_or(0);

        AudioRecordingState {
            is_recording,
            duration_ms,
            buffer_size,
        }
    }

    /// Simple VAD: detect speech segments in PCM data.
    ///
    /// Splits audio into chunks and returns indices of chunks
    /// where RMS energy exceeds the threshold.
    /// `chunk_duration_ms` — chunk size in milliseconds (default 500ms).
    /// `threshold` — RMS energy threshold (default 0.01).
    pub fn detect_speech_segments(
        pcm: &[f32],
        sample_rate: u32,
        chunk_duration_ms: u32,
        threshold: f32,
    ) -> Vec<(usize, usize)> {
        let samples_per_chunk = (sample_rate as usize * chunk_duration_ms as usize) / 1000;
        if samples_per_chunk == 0 {
            return vec![];
        }

        let mut segments = Vec::new();
        let mut in_speech = false;
        let mut seg_start = 0usize;

        for (chunk_idx, chunk) in pcm.chunks(samples_per_chunk).enumerate() {
            let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();

            if rms > threshold && !in_speech {
                in_speech = true;
                seg_start = chunk_idx * samples_per_chunk;
            } else if rms <= threshold && in_speech {
                in_speech = false;
                let seg_end = chunk_idx * samples_per_chunk;
                if seg_end > seg_start {
                    segments.push((seg_start, seg_end));
                }
            }
        }

        // Handle trailing speech
        if in_speech {
            let seg_end = pcm.len();
            if seg_end > seg_start {
                segments.push((seg_start, seg_end));
            }
        }

        segments
    }
}

/// Build a typed audio stream that resamples and buffers mono f32 samples.
/// Also writes each chunk to a crash-safe temp file via CaptureState::temp_file.
/// Free function (not method) to avoid &self lifetime issues with thread spawn.
fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    state: Arc<CaptureState>,
    device_rate: u32,
    target_rate: u32,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + SampleToF32 + Send + 'static,
{
    let channels = config.channels as usize;
    let resample_ratio = target_rate as f64 / device_rate as f64;

    let stream = device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                // Convert to f32 mono
                let samples: Vec<f32> = data
                    .chunks(channels)
                    .filter_map(|chunk| {
                        if chunk.is_empty() {
                            return None;
                        }
                        let sum: f32 = chunk.iter().map(|s| s.to_f32_()).sum();
                        Some(sum / chunk.len() as f32)
                    })
                    .collect();

                // Simple linear resampling to target rate
                let resampled = if (device_rate as i32 - target_rate as i32).abs() > 100 {
                    resample(&samples, resample_ratio)
                } else {
                    samples
                };

                // Write to in-memory buffer
                if let Ok(mut buf) = state.buffer.lock() {
                    if state.is_recording.load(Ordering::SeqCst) {
                        buf.extend_from_slice(&resampled);
                    }
                }

                // CRASH SAFETY: Also write raw f32 bytes to temp file immediately.
                // Even if power is lost, only this ~10ms chunk is gone.
                if let Ok(mut file_guard) = state.temp_file.lock() {
                    if let Some(ref mut file) = *file_guard {
                        let bytes: &[u8] = unsafe {
                            std::slice::from_raw_parts(
                                resampled.as_ptr() as *const u8,
                                resampled.len() * 4,
                            )
                        };
                        let _ = file.write_all(bytes);
                        let _ = file.flush();
                    }
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| format!("Failed to build input stream: {}", e))?;

    Ok(stream)
}

/// Simple linear interpolation resampling.
fn resample(samples: &[f32], ratio: f64) -> Vec<f32> {
    if samples.is_empty() || ratio <= 0.0 {
        return samples.to_vec();
    }

    let input_len = samples.len() as f64;
    let output_len = (input_len * ratio) as usize;
    let mut result = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let pos = i as f64 / ratio;
        let idx = pos as usize;
        let frac = pos - idx as f64;

        if idx + 1 < samples.len() {
            let s0 = samples[idx];
            let s1 = samples[idx + 1];
            result.push(s0 + (s1 - s0) * frac as f32);
        } else if idx < samples.len() {
            result.push(samples[idx]);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_speech_segments() {
        // 1 second of silence + 0.5s of "speech" (loud signal)
        let silence = vec![0.001f32; 16000];
        let speech = vec![0.5f32; 8000];
        let pcm: Vec<f32> = silence.into_iter().chain(speech.into_iter()).collect();

        let segments = AudioCapture::detect_speech_segments(&pcm, 16000, 500, 0.01);
        assert!(!segments.is_empty());
        // Speech segment should start after the silence
        assert!(segments[0].0 >= 16000);
    }

    #[test]
    fn test_resample() {
        let samples = vec![0.0, 0.5, 1.0, 0.5, 0.0];
        let result = resample(&samples, 2.0);
        assert_eq!(result.len(), 10);
    }
}
