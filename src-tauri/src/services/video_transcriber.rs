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
    pub minutes: String,
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

/// 检查 FFmpeg 是否可用（已下载）。首次调用会自动下载。
///
/// 返回 (available, path_or_error_message)
pub fn check_ffmpeg_available() -> (bool, String) {
    let ffmpeg_path = ffmpeg_sidecar::paths::ffmpeg_path();
    if ffmpeg_path.exists() {
        (true, ffmpeg_path.display().to_string())
    } else {
        (false, "FFmpeg 二进制尚未下载，首次使用时将自动下载".to_string())
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
    let video_str = video_path
        .to_str()
        .ok_or("视频文件路径包含非法字符")?;

    let ffmpeg_path = ffmpeg_sidecar::paths::ffmpeg_path();

    eprintln!("[VideoTranscriber] Using ffmpeg at: {}", ffmpeg_path.display());

    // 创建临时 PCM 文件
    let temp_dir = data_dir.join("video_temp");
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| format!("创建临时目录失败: {}", e))?;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let temp_path = temp_dir.join(format!("audio_{}.pcm", ts));

    // ffmpeg → 输出到文件（流式写入，不会占用 stdout 缓冲区内存）
    let temp_str = temp_path.to_str().ok_or("临时文件路径非法")?;

    let status = std::process::Command::new(&ffmpeg_path)
        .args([
            "-i", video_str,
            "-vn",           // no video
            "-ac", "1",      // mono
            "-ar", "16000",  // 16kHz
            "-f", "f32le",   // raw float32 little-endian
            "-acodec", "pcm_f32le",
            "-y",            // overwrite
            temp_str,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
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

    eprintln!(
        "[VideoTranscriber] Extracted {} samples ({:.1}s) → {}",
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
    let mut file = std::fs::File::open(pcm_path)
        .map_err(|e| format!("打开 PCM 文件失败: {}", e))?;

    let file_size = file.metadata()
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
        let bytes_read = file.read(&mut buf)
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

// ─── Meeting Minutes Generation ──────────────────────────────────────────

const MEETING_MINUTES_PROMPT: &str = "\
你是一位专业的会议纪要撰写助手。请根据以下会议/访谈的语音转写文本，生成结构化的会议纪要。

【输出格式要求】
## 会议纪要

### 基本信息
- **会议主题**：（从内容推断）
- **会议类型**：（需求调研/项目评审/方案讨论/其他）
- **关键参与者**：（从上下文推断）

### 核心议题
（列出 3-7 个主要讨论议题）

### 关键决策
（列出会议中做出的明确决定，用编号列表）

### 待办事项
（列出后续行动项，包含负责人和截止时间如果提及）

### 风险与关注点
（列出识别到的风险或需要关注的问题）

### 详细讨论记录
（按时间顺序整理关键对话要点）

---
【注意事项】
1. 语音转写可能有错误，请根据上下文合理推断正确内容
2. 不要编造转写文本中没有的信息
3. 保持客观中立，准确反映讨论内容
4. 使用中文输出";

pub fn generate_meeting_minutes(
    transcript: &str,
    llm_service: &crate::services::llm_service::LLMService,
) -> Result<MeetingMinutesResult, String> {
    let start = std::time::Instant::now();

    // 如果转写文本超长（>30000字），截断以避免 token 超限
    let truncated = if transcript.len() > 30000 {
        eprintln!(
            "[VideoTranscriber] Transcript too long ({} chars), truncating to 30000",
            transcript.len()
        );
        &transcript[..30000]
    } else {
        transcript
    };

    let user_prompt = format!(
        "以下是会议/访谈的语音转写文本：\n\n---\n{}\n---\n\n请生成结构化的会议纪要。",
        truncated
    );

    let minutes = llm_service.generate_text_sync(MEETING_MINUTES_PROMPT, &user_prompt)?;

    Ok(MeetingMinutesResult {
        minutes,
        generation_time_ms: start.elapsed().as_millis() as u64,
    })
}

/// 清理临时 PCM 文件
pub fn cleanup_temp_file(path: &Path) {
    if path.exists() {
        if let Err(e) = std::fs::remove_file(path) {
            eprintln!("[VideoTranscriber] Failed to cleanup temp file {}: {}", path.display(), e);
        }
    }
}
