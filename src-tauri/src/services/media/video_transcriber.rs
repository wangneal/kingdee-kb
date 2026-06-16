//! 视频文件转写服务 — 从视频提取音频并通过 Whisper 转写为文字
//!
//! 管道：视频文件 → ffmpeg 流式音频提取 → 临时 PCM 文件 →
//!        分片读取 → Whisper 分段转写 → 中文后处理 → 可选会议纪要生成
//!
//! 内存安全：
//! - 不全量加载 PCM 到内存
//! - 音频先写入临时文件，然后分片（30s/片）读取送 Whisper
//! - 即使 2 小时视频也不会 OOM
//!
//! 依赖：
//! - ffmpeg-sidecar: 内嵌 FFmpeg 二进制，无需系统安装
//! - whisper_service: 本地 Whisper 语音识别
//! - chinese_postprocess: 中文转录后处理

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::process::Stdio;

// ─── Constants ────────────────────────────────────────────────────────────

/// 每个转写分片的时长（秒）— 30s 在内存和精度间取平衡
const CHUNK_DURATION_SECS: usize = 30;
/// 采样率
const SAMPLE_RATE: usize = 16000;
/// 每个分片的样本数
const CHUNK_SAMPLES: usize = CHUNK_DURATION_SECS * SAMPLE_RATE;
/// 每个样本的字节数（f32le）
const BYTES_PER_SAMPLE: usize = 4;
/// 每个分片的字节数
const CHUNK_BYTES: usize = CHUNK_SAMPLES * BYTES_PER_SAMPLE;

// ─── Types ────────────────────────────────────────────────────────────────

/// 视频转写的完整结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoTranscriptionResult {
    pub video_path: String,
    pub text: String,
    pub segments: Vec<TranscriptionSegment>,
    pub confidence: f32,
    pub extraction_time_ms: u64,
    pub transcription_time_ms: u64,
    pub duration_secs: f32,
}

/// 转写段落
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// 会议纪要生成结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingMinutesResult {
    /// 纪要正文（markdown）
    pub minutes: String,
    /// 纪要落盘的绝对路径
    pub file_path: String,
    /// 登记到 products 表的产物 ID（若登记失败则为 None）
    pub product_id: Option<i64>,
    /// 登记到 meeting_minutes 表的纪要 ID（未关联会议时为 None）
    pub minutes_id: Option<i64>,
    pub generation_time_ms: u64,
}

/// 视频转写 + 入库 + 会议纪要的完整流水线结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoPipelineResult {
    pub transcription: VideoTranscriptionResult,
    pub ingestion_document_id: Option<i64>,
    pub meeting_minutes: Option<MeetingMinutesResult>,
}

// ─── Audio Extraction (streaming to temp file) ───────────────────────────

/// 确保 FFmpeg 可用：若二进制不存在则自动下载并解压（ffmpeg-sidecar 内置）。
///
/// 首次调用会联网下载（约 80MB，耗时取决于网络），后续从缓存目录直接复用。
/// 返回 ffmpeg 可执行文件路径。
pub fn ensure_ffmpeg() -> Result<std::path::PathBuf, String> {
    let ffmpeg_path = ffmpeg_sidecar::paths::ffmpeg_path();
    if ffmpeg_path.exists() {
        return Ok(ffmpeg_path);
    }
    tracing::info!("[VideoTranscriber] FFmpeg 未安装，开始自动下载...");
    ffmpeg_sidecar::download::auto_download()
        .map_err(|e| format!("FFmpeg 自动下载失败: {}", e))?;
    tracing::info!("[VideoTranscriber] FFmpeg 下载完成: {}", ffmpeg_path.display());
    Ok(ffmpeg_path)
}

/// 检查 FFmpeg 是否可用（不触发下载）。供前端展示状态用。
///
/// 返回 (available, path_or_message)
pub fn check_ffmpeg_available() -> (bool, String) {
    let ffmpeg_path = ffmpeg_sidecar::paths::ffmpeg_path();
    if ffmpeg_path.exists() {
        (true, ffmpeg_path.display().to_string())
    } else {
        (
            false,
            "FFmpeg 尚未下载，首次视频转写时将自动下载（约 80MB）".to_string(),
        )
    }
}

/// 从视频文件流式提取音频到临时 PCM 文件
///
/// 返回 (temp_file_path, duration_secs)
/// 调用方负责在使用后删除临时文件。
pub fn extract_audio_to_file(
    video_path: &Path,
    data_dir: &Path,
) -> Result<(std::path::PathBuf, f32), String> {
    let video_str = video_path.to_str().ok_or("视频文件路径包含非法字符")?;

    // 确保 FFmpeg 已下载（首次使用时自动下载）
    let ffmpeg_path = ensure_ffmpeg()?;

    tracing::info!(
        "[VideoTranscriber] 正在使用 ffmpeg: {}",
        ffmpeg_path.display()
    );

    // 创建临时 PCM 文件
    let temp_dir = data_dir.join("video_temp");
    std::fs::create_dir_all(&temp_dir).map_err(|e| format!("创建临时目录失败: {}", e))?;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let temp_path = temp_dir.join(format!("audio_{}.pcm", ts));

    // ffmpeg → 输出到文件（流式写入，不会占用 stdout 缓冲区内存）
    let temp_str = temp_path.to_str().ok_or("临时文件路径非法")?;

    let status = std::process::Command::new(&ffmpeg_path)
        .args([
            "-i",
            video_str,
            "-vn", // no video
            "-ac",
            "1", // mono
            "-ar",
            "16000", // 16kHz
            "-f",
            "f32le", // raw float32 little-endian
            "-acodec",
            "pcm_f32le",
            "-y", // overwrite
            temp_str,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("FFmpeg 进程启动失败: {}", e))?;

    if !status.success() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!(
            "FFmpeg 音频提取失败 (exit {})",
            status.code().unwrap_or(-1)
        ));
    }

    // 读取文件大小计算时长
    let file_size = std::fs::metadata(&temp_path)
        .map_err(|e| format!("读取临时文件信息失败: {}", e))?
        .len();

    if file_size == 0 {
        let _ = std::fs::remove_file(&temp_path);
        return Err("FFmpeg 输出为空，视频可能没有音频轨道".to_string());
    }

    let total_samples = file_size as usize / BYTES_PER_SAMPLE;
    let duration_secs = total_samples as f32 / SAMPLE_RATE as f32;

    tracing::info!(
        "[VideoTranscriber] 已提取 {} 个采样（{:.1}s）→ {}",
        total_samples,
        duration_secs,
        temp_path.display()
    );

    Ok((temp_path, duration_secs))
}

// ─── Chunked Transcription ───────────────────────────────────────────────

/// 分片转写：从临时 PCM 文件中分片读取，逐段送 Whisper
///
/// 内存峰值：仅一个分片（30s × 16kHz × 4 bytes = ~1.9MB），无论视频多长。
/// `progress` 是可选的进度回调，接收 (chunk_index, total_chunks)。
pub fn transcribe_chunks<F>(
    pcm_path: &Path,
    whisper: &crate::services::whisper_service::WhisperService,
    mut progress: Option<F>,
) -> Result<TranscriptionResult, String>
where
    F: FnMut(usize, usize),
{
    let mut file =
        std::fs::File::open(pcm_path).map_err(|e| format!("打开 PCM 文件失败: {}", e))?;

    let file_size = file
        .metadata()
        .map_err(|e| format!("读取 PCM 文件信息失败: {}", e))?
        .len();

    let total_samples = file_size as usize / BYTES_PER_SAMPLE;
    let total_chunks = (total_samples + CHUNK_SAMPLES - 1) / CHUNK_SAMPLES;

    if total_chunks == 0 {
        return Ok(TranscriptionResult {
            segments: Vec::new(),
            text: String::new(),
            confidence: 0.0,
            processing_time_ms: 0,
        });
    }

    let start = std::time::Instant::now();
    let mut all_segments: Vec<TranscriptionSegment> = Vec::new();
    let mut full_text = String::new();
    let mut total_confidence = 0.0f32;
    let mut chunks_with_speech = 0usize;

    // 逐分片读取和转写
    let mut buf = vec![0u8; CHUNK_BYTES];
    let mut chunk_idx = 0usize;

    loop {
        let bytes_read = file
            .read(&mut buf)
            .map_err(|e| format!("读取 PCM 文件失败: {}", e))?;

        if bytes_read == 0 {
            break; // EOF
        }

        // 转换为 f32
        let samples: Vec<f32> = buf[..bytes_read]
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();

        chunk_idx += 1;

        // 简单 VAD：跳过静音分片
        let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
        if rms < 0.005 {
            if let Some(ref mut cb) = progress {
                cb(chunk_idx, total_chunks);
            }
            continue;
        }

        // Whisper 转写
        let result = whisper.transcribe(&samples)?;

        // 偏移时间戳（加上当前分片的时间偏移）
        let time_offset_ms = ((chunk_idx - 1) * CHUNK_DURATION_SECS * 1000) as u64;

        for seg in &result.segments {
            all_segments.push(TranscriptionSegment {
                start_ms: seg.start_ms + time_offset_ms,
                end_ms: seg.end_ms + time_offset_ms,
                text: seg.text.clone(),
            });
        }

        if !result.text.trim().is_empty() {
            full_text.push_str(&result.text);
            full_text.push(' ');
            total_confidence += result.confidence;
            chunks_with_speech += 1;
        }

        // 进度回调
        if let Some(ref mut cb) = progress {
            cb(chunk_idx, total_chunks);
        }
    }

    let text = full_text.trim().to_string();
    let avg_confidence = if chunks_with_speech > 0 {
        total_confidence / chunks_with_speech as f32
    } else {
        0.0
    };

    Ok(TranscriptionResult {
        segments: all_segments,
        text,
        confidence: avg_confidence,
        processing_time_ms: start.elapsed().as_millis() as u64,
    })
}

/// 内部转写结果（不暴露到前端）
pub struct TranscriptionResult {
    pub segments: Vec<TranscriptionSegment>,
    pub text: String,
    pub confidence: f32,
    pub processing_time_ms: u64,
}

// ─── Meeting Minutes 已迁移到 meeting_minutes_service.rs ──────────────────

/// 清理临时 PCM 文件
pub fn cleanup_temp_file(path: &Path) {
    if path.exists() {
        if let Err(e) = std::fs::remove_file(path) {
            tracing::warn!(
                "[VideoTranscriber] 清理临时文件 {} 失败: {}",
                path.display(),
                e
            );
        }
    }
}
