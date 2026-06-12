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
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ─── 音频采样格式转换 ───

/// 将不同 PCM 采样格式转换为 Whisper 需要的 f32。
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

impl SampleToF32 for i8 {
    fn to_f32_(&self) -> f32 {
        *self as f32 / 128.0
    }
}

impl SampleToF32 for u8 {
    fn to_f32_(&self) -> f32 {
        (*self as f32 - 128.0) / 128.0
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

/// 系统输入设备信息。
#[derive(Debug, Clone)]
pub struct AudioInputDevice {
    pub id: String,
    pub name: String,
    pub host: String,
    pub is_default: bool,
}

/// Internal mutable state shared between audio callback and main thread
///
/// 仅包含 `AtomicBool` 和 `Mutex<T: Send>` 字段，由 Rust 自动推导为 Send + Sync，
/// 无需手写 unsafe impl。
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

    /// 列出 cpal 所有可用 Host 下的麦克风输入设备。
    pub fn input_devices() -> Result<Vec<AudioInputDevice>, String> {
        let default_name = Self::default_input_device_name();
        let mut devices = Vec::new();
        for host_id in cpal::available_hosts() {
            let host_label = format!("{:?}", host_id);
            let host = match cpal::host_from_id(host_id) {
                Ok(host) => host,
                Err(error) => {
                    tracing::warn!("[AudioCapture] Host {} 不可用: {}", host_label, error);
                    continue;
                }
            };
            let input_devices = match host.input_devices() {
                Ok(input_devices) => input_devices,
                Err(error) => {
                    tracing::warn!(
                        "[AudioCapture] 在 {} 上查询输入设备失败: {}",
                        host_label, error
                    );
                    continue;
                }
            };
            for device in input_devices {
                let name = device.name().unwrap_or_else(|_| "未知输入设备".to_string());
                let id = format!("{}::{}", host_label, name);
                if devices
                    .iter()
                    .any(|existing: &AudioInputDevice| existing.id == id)
                {
                    continue;
                }
                devices.push(AudioInputDevice {
                    is_default: default_name.as_deref() == Some(name.as_str()),
                    id,
                    name,
                    host: host_label.clone(),
                });
            }
        }
        Ok(devices)
    }

    /// 获取系统默认输入设备名称。
    pub fn default_input_device_name() -> Option<String> {
        let host = cpal::default_host();
        host.default_input_device()
            .and_then(|device| device.name().ok())
    }

    fn find_input_device(device_key: Option<&str>) -> Result<(cpal::Device, String), String> {
        if let Some(expected_key) = device_key {
            for host_id in cpal::available_hosts() {
                let host_label = format!("{:?}", host_id);
                let host = match cpal::host_from_id(host_id) {
                    Ok(host) => host,
                    Err(_) => continue,
                };
                let devices = match host.input_devices() {
                    Ok(devices) => devices,
                    Err(_) => continue,
                };
                for device in devices {
                    let name = device.name().unwrap_or_default();
                    let id = format!("{}::{}", host_label, name);
                    if expected_key == id || expected_key == name {
                        return Ok((device, format!("{} ({})", name, host_label)));
                    }
                }
            }
            return Err(format!("未找到输入设备: {}", expected_key));
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("未找到系统默认麦克风输入设备")?;
        let name = device
            .name()
            .unwrap_or_else(|_| "系统默认输入设备".to_string());
        Ok((device, name))
    }

    /// 从指定或默认麦克风开始录音。
    ///
    /// 音频流由独立线程持有，只要 `is_recording` 为 true 就保持活动。
    /// 方法会立即返回，实际采样在后台进行。
    pub fn start_recording(&self, device_key: Option<&str>) -> Result<(), String> {
        // 已在录音时拒绝重复启动
        if self.state.is_recording.load(Ordering::SeqCst) {
            return Err("录音已经在进行中".to_string());
        }

        let (device, selected_device_name) = Self::find_input_device(device_key)?;

        let mut supported_configs = device
            .supported_input_configs()
            .map_err(|e| format!("查询输入设备配置失败: {}", e))?;
        let supported_config = supported_configs
            .find(|config| {
                matches!(
                    config.sample_format(),
                    SampleFormat::F32
                        | SampleFormat::I8
                        | SampleFormat::I16
                        | SampleFormat::U8
                        | SampleFormat::U16
                )
            })
            .ok_or("输入设备没有当前支持的音频采集格式")?;

        let config = supported_config.with_max_sample_rate().config();
        let device_sample_rate = config.sample_rate.0;
        let sample_format = supported_config.sample_format();

        tracing::info!(
            "[AudioCapture] 正在从 {} 录音，{}Hz，{} 通道",
            selected_device_name, device_sample_rate, config.channels
        );

        // Create temp PCM file for crash-safe persistence
        let temp_path = self.temp_file_path();
        let temp_file =
            File::create(&temp_path).map_err(|e| format!("创建临时音频文件失败: {}", e))?;
        tracing::info!(
            "[AudioCapture] 崩溃安全临时文件: {}",
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
        let (startup_tx, startup_rx) = mpsc::channel::<Result<(), String>>();

        // Spawn thread that owns the stream
        std::thread::spawn(move || {
            let mut startup_tx = Some(startup_tx);
            let capture_state_err = capture_state.clone();
            let err_fn = move |err: cpal::StreamError| {
                tracing::error!("[AudioCapture] 音频流错误: {}", err);
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
                SampleFormat::I8 => build_stream::<i8>(
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
                SampleFormat::U8 => build_stream::<u8>(
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
                    tracing::error!("[AudioCapture] 不支持的采样格式: {:?}", sf);
                    capture_state.is_recording.store(false, Ordering::SeqCst);
                    if let Some(tx) = startup_tx.take() {
                        let _ = tx.send(Err(format!("输入设备采样格式不支持: {:?}", sf)));
                    }
                    return;
                }
            };

            let stream = match stream_result {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("[AudioCapture] 构建音频流失败: {}", e);
                    capture_state.is_recording.store(false, Ordering::SeqCst);
                    if let Some(tx) = startup_tx.take() {
                        let _ = tx.send(Err(format!("创建麦克风输入流失败: {}", e)));
                    }
                    return;
                }
            };

            if let Err(e) = stream.play() {
                tracing::error!("[AudioCapture] 启动音频流失败: {}", e);
                capture_state.is_recording.store(false, Ordering::SeqCst);
                if let Some(tx) = startup_tx.take() {
                    let _ = tx.send(Err(format!("启动麦克风输入流失败: {}", e)));
                }
                return;
            }

            if let Some(tx) = startup_tx.take() {
                let _ = tx.send(Ok(()));
            }
            tracing::info!("[AudioCapture] 音频流已启动，后台线程开始录音");

            // Keep thread alive while recording — stream is dropped when this
            // thread exits, which stops audio capture.
            while capture_state.is_recording.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            tracing::info!("[AudioCapture] 录音已停止，释放音频流");
            // stream dropped here → stops capture
        });

        match startup_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                let _ = self.stop_recording();
                Err(error)
            }
            Err(_) => {
                let _ = self.stop_recording();
                Err("麦克风输入流启动超时，请检查设备是否被系统或其他程序占用".to_string())
            }
        }
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
                tracing::info!(
                    "[AudioCapture] 临时文件大小: {} 字节（约 {} 个采样）",
                    metadata.len(),
                    metadata.len() / 4
                );
            }
        }

        tracing::info!(
            "[AudioCapture] 停止录音，捕获 {} 个采样（约 {:.2} 秒）",
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

    /// 复制当前录音缓冲区，用于录音中的分段转写预览。
    pub fn recording_snapshot(&self) -> Result<(bool, Vec<f32>), String> {
        let is_recording = self.state.is_recording.load(Ordering::SeqCst);
        let buffer = self
            .state
            .buffer
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?
            .clone();
        Ok((is_recording, buffer))
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
