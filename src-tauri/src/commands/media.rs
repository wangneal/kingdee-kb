use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::app_state::AppState;
use crate::services::audio_capture::AudioCapture;
use crate::services::llm_service::ChatMessage;
use crate::services::model_downloader;
use crate::services::token::truncate_to_tokens;
use crate::services::video_transcriber::{
    MeetingMinutesResult, VideoPipelineResult, VideoTranscriptionResult,
};
use crate::services::whisper_service::{TranscriptionResult, WhisperStatus};

const WHISPER_SAMPLE_RATE: usize = 16000;
const REALTIME_MIN_SAMPLES: usize = WHISPER_SAMPLE_RATE * 5;
const MIN_SPEECH_SAMPLES: usize = WHISPER_SAMPLE_RATE;
const MIN_SPEECH_RMS: f32 = 0.008;

fn pcm_rms(pcm_data: &[f32]) -> f32 {
    if pcm_data.is_empty() {
        return 0.0;
    }
    (pcm_data.iter().map(|sample| sample * sample).sum::<f32>() / pcm_data.len() as f32).sqrt()
}

fn collect_speech_pcm(
    pcm_data: &[f32],
    speech_segments: &[(usize, usize)],
    padding_ms: usize,
) -> Vec<f32> {
    if speech_segments.is_empty() {
        return Vec::new();
    }

    let padding_samples = WHISPER_SAMPLE_RATE * padding_ms / 1000;
    let gap_samples = WHISPER_SAMPLE_RATE / 5;
    let mut merged_segments: Vec<(usize, usize)> = Vec::new();

    for (start, end) in speech_segments {
        let padded_start = start.saturating_sub(padding_samples);
        let padded_end = (end + padding_samples).min(pcm_data.len());
        if padded_end <= padded_start {
            continue;
        }

        if let Some((_, last_end)) = merged_segments.last_mut() {
            if padded_start <= *last_end {
                *last_end = (*last_end).max(padded_end);
                continue;
            }
        }
        merged_segments.push((padded_start, padded_end));
    }

    let mut speech_pcm = Vec::new();
    for (index, (start, end)) in merged_segments.iter().enumerate() {
        if index > 0 {
            speech_pcm.extend(std::iter::repeat(0.0).take(gap_samples));
        }
        speech_pcm.extend_from_slice(&pcm_data[*start..*end]);
    }
    speech_pcm
}

/// 加载 Whisper 模型用于语音转录。
#[tauri::command]
pub async fn load_whisper_model(
    state: State<'_, AppState>,
    model_size: String,
) -> Result<(), String> {
    let data_dir = &state.data_dir;
    let _model_path = model_downloader::ensure_model(data_dir, &model_size)?;

    let mut whisper = state.whisper_service.write().map_err(|e| e.to_string())?;
    whisper.load_model(data_dir, &model_size)?;

    Ok(())
}

/// 获取 Whisper 服务状态
#[tauri::command]
pub fn get_whisper_status(state: State<'_, AppState>) -> Result<WhisperStatus, String> {
    let whisper = state.whisper_service.read().map_err(|e| e.to_string())?;
    Ok(whisper.status())
}

#[derive(Serialize)]
pub struct AudioInputDeviceInfo {
    id: String,
    name: String,
    host: String,
    is_default: bool,
}

/// 列出系统可见的麦克风输入设备。
#[tauri::command]
pub fn list_audio_input_devices() -> Result<Vec<AudioInputDeviceInfo>, String> {
    let devices = AudioCapture::input_devices()?;
    Ok(devices
        .into_iter()
        .map(|device| AudioInputDeviceInfo {
            id: device.id,
            name: device.name,
            host: device.host,
            is_default: device.is_default,
        })
        .collect())
}

/// 开始麦克风录音。
#[tauri::command]
pub async fn start_whisper_recording(
    state: State<'_, AppState>,
    device_name: Option<String>,
) -> Result<(), String> {
    let mut candidates: Vec<Option<String>> = Vec::new();
    if let Some(device_key) = device_name {
        candidates.push(Some(device_key));
    } else {
        candidates.push(None);
        for device in AudioCapture::input_devices().unwrap_or_default() {
            if !device.is_default {
                candidates.push(Some(device.id));
            }
        }
    }

    let mut tried = Vec::new();
    for candidate in candidates {
        let label = candidate
            .clone()
            .unwrap_or_else(|| "系统默认输入设备".to_string());
        tried.push(label.clone());
        let capture = state.audio_capture.write().map_err(|e| e.to_string())?;
        match capture.start_recording(candidate.as_deref()) {
            Ok(()) => return Ok(()),
            Err(error) => {
                tried.push(format!("{}({})", label, error));
                continue;
            }
        }
    }

    Err(format!(
        "未能启动任何麦克风输入流，请检查系统麦克风权限、输入设备或是否被其他程序占用。已尝试: {}",
        tried.join("、")
    ))
}

#[derive(Serialize)]
pub struct RecordingPreviewResult {
    text: String,
    sample_count: usize,
    processing_time_ms: u64,
}

/// 转写录音中的新增片段，用于前端实时预览。
#[tauri::command]
pub async fn transcribe_whisper_recording_chunk(
    state: State<'_, AppState>,
    from_sample: usize,
) -> Result<RecordingPreviewResult, String> {
    let (is_recording, pcm_data) = {
        let capture = state.audio_capture.read().map_err(|e| e.to_string())?;
        capture.recording_snapshot()?
    };
    if !is_recording {
        return Ok(RecordingPreviewResult {
            text: String::new(),
            sample_count: pcm_data.len(),
            processing_time_ms: 0,
        });
    }

    let start_sample = from_sample.min(pcm_data.len());
    let pending = &pcm_data[start_sample..];
    if pending.len() < REALTIME_MIN_SAMPLES {
        return Ok(RecordingPreviewResult {
            text: String::new(),
            sample_count: start_sample,
            processing_time_ms: 0,
        });
    }

    let speech_segments = crate::services::audio_capture::AudioCapture::detect_speech_segments(
        pending,
        WHISPER_SAMPLE_RATE as u32,
        500,
        0.015,
    );
    if speech_segments.is_empty() {
        return Ok(RecordingPreviewResult {
            text: String::new(),
            sample_count: start_sample,
            processing_time_ms: 0,
        });
    }

    let speech_pcm = collect_speech_pcm(pending, &speech_segments, 300);
    if speech_pcm.len() < MIN_SPEECH_SAMPLES || pcm_rms(&speech_pcm) < MIN_SPEECH_RMS {
        return Ok(RecordingPreviewResult {
            text: String::new(),
            sample_count: start_sample,
            processing_time_ms: 0,
        });
    }

    let whisper_result = {
        let whisper = state.whisper_service.write().map_err(|e| e.to_string())?;
        if !whisper.is_model_loaded() {
            return Err("Whisper 模型未加载。请先加载语音模型。".to_string());
        }
        whisper.transcribe(&speech_pcm)?
    };
    let processed_text =
        crate::services::chinese_postprocess::postprocess_chinese(&whisper_result.text);

    Ok(RecordingPreviewResult {
        text: processed_text,
        sample_count: pcm_data.len(),
        processing_time_ms: whisper_result.processing_time_ms,
    })
}

/// 使用当前 LLM 对语音转写稿做保守校订。
#[tauri::command]
pub async fn review_transcription_text(
    state: State<'_, AppState>,
    text: String,
) -> Result<String, String> {
    let raw_text = text.trim();
    if raw_text.is_empty() {
        return Ok(String::new());
    }

    if !state.llm.is_configured() {
        return Ok(raw_text.to_string());
    }

    let config = state.llm.get_active_config()?;
    let clipped_text = truncate_to_tokens(raw_text, 6000);
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: [
                "你是调研访谈语音转写校订器。",
                "只能修正明显 ASR 错字、断句、标点、繁简混用和重复幻觉片段。",
                "禁止新增用户没有说过的业务事实，禁止扩写、总结、改写成会议纪要。",
                "遇到不确定内容保留原意或标为[听不清]。",
                "只输出校订后的转写正文，不要解释，不要 Markdown。",
            ]
            .join("\n"),
        },
        ChatMessage {
            role: "user".to_string(),
            content: format!(
                "请校订以下语音转写稿。若出现连续重复的无意义短句或噪声幻觉，只保留一次有意义内容，无法判断就删去噪声。\n\n{}",
                clipped_text
            ),
        },
    ];

    let reviewed = state.llm.chat_completion(&messages, &config).await?;
    let reviewed = reviewed.trim();
    if reviewed.is_empty() {
        Ok(raw_text.to_string())
    } else {
        Ok(reviewed.to_string())
    }
}

/// 停止录音并转录音频。
/// provider: 可选 ASR 服务商，为空则使用本地 Whisper
#[tauri::command]
pub async fn stop_whisper_recording(
    state: State<'_, AppState>,
    provider: Option<AsrProviderKind>,
) -> Result<TranscriptionResult, String> {
    // 使用在线 ASR（腾讯云）
    if let Some(asr_provider) = provider {
        // 1. 停止录音获取 PCM 数据
        let pcm_data = {
            let capture = state.audio_capture.write().map_err(|e| e.to_string())?;
            capture.stop_recording()?
        };
        if pcm_data.is_empty() {
            return Ok(TranscriptionResult {
                text: String::new(),
                segments: Vec::new(),
                confidence: 0.0,
                processing_time_ms: 0,
            });
        }

        let start = std::time::Instant::now();

        let result = match asr_provider {
            AsrProviderKind::Tencent => {
                let (secret_id, secret_key) = {
                    let cfg = state.asr_config.read().map_err(|e| e.to_string())?;
                    let sid = cfg
                        .tencent_secret_id
                        .clone()
                        .ok_or_else(|| "请先在设置中配置腾讯云 SecretId".to_string())?;
                    let sk = cfg
                        .tencent_secret_key
                        .clone()
                        .ok_or_else(|| "请先在设置中配置腾讯云 SecretKey".to_string())?;
                    (sid, sk)
                };
                let provider = crate::services::tencent_asr::TencentOneShotProvider::new(
                    secret_id, secret_key,
                );
                let config = crate::services::tencent_asr::TencentAsrConfig::default();
                let pcm16: Vec<u8> = pcm_data
                    .iter()
                    .flat_map(|&s| {
                        let sample = (s * 32768.0).clamp(-32768.0, 32767.0) as i16;
                        sample.to_le_bytes()
                    })
                    .collect();
                provider
                    .recognize_pcm16(&pcm16, &config)
                    .await
                    .map_err(|e| format!("腾讯 ASR 识别失败: {}", e))?
            }
        };

        let processing_time_ms = start.elapsed().as_millis() as u64;
        return Ok(TranscriptionResult {
            text: result.text,
            segments: Vec::new(),
            confidence: result.confidence,
            processing_time_ms,
        });
    }

    // 无 provider → 使用本地 Whisper
    let pcm_data = {
        let capture = state.audio_capture.write().map_err(|e| e.to_string())?;
        capture.stop_recording()?
    };

    if pcm_data.is_empty() {
        return Ok(TranscriptionResult {
            text: String::new(),
            segments: Vec::new(),
            confidence: 0.0,
            processing_time_ms: 0,
        });
    }

    // 语音活动检测：提取有效语音片段
    let speech_segments = crate::services::audio_capture::AudioCapture::detect_speech_segments(
        &pcm_data,
        WHISPER_SAMPLE_RATE as u32,
        500,
        0.012,
    );

    if speech_segments.is_empty() {
        return Ok(TranscriptionResult {
            text: String::new(),
            segments: Vec::new(),
            confidence: 0.0,
            processing_time_ms: 0,
        });
    }

    let speech_pcm = collect_speech_pcm(&pcm_data, &speech_segments, 300);
    if speech_pcm.len() < MIN_SPEECH_SAMPLES || pcm_rms(&speech_pcm) < MIN_SPEECH_RMS {
        return Ok(TranscriptionResult {
            text: String::new(),
            segments: Vec::new(),
            confidence: 0.0,
            processing_time_ms: 0,
        });
    }

    let whisper_result = {
        let whisper = state.whisper_service.write().map_err(|e| e.to_string())?;
        if !whisper.is_model_loaded() {
            return Err("Whisper model not loaded. Call load_whisper_model first.".to_string());
        }
        whisper.transcribe(&speech_pcm)?
    };

    let processed_text =
        crate::services::chinese_postprocess::postprocess_chinese(&whisper_result.text);

    Ok(TranscriptionResult {
        text: processed_text,
        segments: whisper_result.segments,
        confidence: whisper_result.confidence,
        processing_time_ms: whisper_result.processing_time_ms,
    })
}

/// RAII 临时文件清理
struct TempCleanup(std::path::PathBuf);
impl Drop for TempCleanup {
    fn drop(&mut self) {
        crate::services::video_transcriber::cleanup_temp_file(&self.0);
    }
}

/// 向前端发送视频处理进度事件
fn emit_video_progress(app_handle: Option<&AppHandle>, step: &str, progress: f32, message: &str) {
    if let Some(handle) = app_handle {
        let payload = serde_json::json!({
            "step": step,
            "progress": progress,
            "message": message
        });
        let _ = handle.emit("video_progress", payload);
    }
}

/// 内部转写逻辑（分片流式处理，内存安全）
fn do_transcribe_video(
    whisper_service: &crate::services::whisper_service::WhisperService,
    video_path: &str,
    data_dir: &std::path::Path,
    app_handle: Option<&AppHandle>,
) -> Result<VideoTranscriptionResult, String> {
    let path = std::path::Path::new(video_path);
    if !path.exists() {
        return Err(format!("视频文件不存在: {}", video_path));
    }

    emit_video_progress(app_handle, "extracting", 0.0, "正在提取音频...");
    let extract_start = std::time::Instant::now();
    let (pcm_path, duration_secs) =
        crate::services::video_transcriber::extract_audio_to_file(path, data_dir)?;
    let extraction_time_ms = extract_start.elapsed().as_millis() as u64;

    let _cleanup = TempCleanup(pcm_path.clone());

    emit_video_progress(app_handle, "transcribing", 0.0, "正在转写语音...");

    let app_handle_clone = app_handle.map(|h| h.clone());
    let result = crate::services::video_transcriber::transcribe_chunks(
        &pcm_path,
        whisper_service,
        app_handle_clone.map(|h| {
            move |chunk_idx: usize, total_chunks: usize| {
                let pct = chunk_idx as f32 / total_chunks as f32 * 100.0;
                let msg = format!("转写中 ({}/{})", chunk_idx, total_chunks);
                h.emit(
                    "video_progress",
                    serde_json::json!({
                        "step": "transcribing",
                        "progress": pct,
                        "message": msg
                    }),
                )
                .ok();
            }
        }),
    )?;

    let processed_text = crate::services::chinese_postprocess::postprocess_chinese(&result.text);

    Ok(VideoTranscriptionResult {
        video_path: video_path.to_string(),
        text: processed_text,
        segments: result.segments,
        confidence: result.confidence,
        extraction_time_ms,
        transcription_time_ms: result.processing_time_ms,
        duration_secs,
    })
}

/// 从视频文件中提取音频并通过 Whisper 转写。
#[tauri::command]
pub async fn transcribe_video_file(
    state: State<'_, AppState>,
    video_path: String,
    app_handle: AppHandle,
) -> Result<VideoTranscriptionResult, String> {
    let whisper = state.whisper_service.read().map_err(|e| e.to_string())?;
    if !whisper.is_model_loaded() {
        return Err("Whisper 模型未加载。请先在研究助手页面加载 Whisper 模型。".to_string());
    }
    do_transcribe_video(&*whisper, &video_path, &state.data_dir, Some(&app_handle))
}

/// 视频转写一站式管道：提取音频 → 转写 → 入库 → 可选生成会议纪要。
#[tauri::command]
pub async fn transcribe_and_ingest_video(
    state: State<'_, AppState>,
    video_path: String,
    project_id: i64,
    generate_minutes: bool,
    app_handle: AppHandle,
) -> Result<VideoPipelineResult, String> {
    let transcription = {
        let whisper = state.whisper_service.read().map_err(|e| e.to_string())?;
        if !whisper.is_model_loaded() {
            return Err("Whisper 模型未加载。请先在研究助手页面加载 Whisper 模型。".to_string());
        }
        do_transcribe_video(&*whisper, &video_path, &state.data_dir, Some(&app_handle))?
    };

    if transcription.text.is_empty() {
        return Err("转写结果为空，无法入库".to_string());
    }

    state.ensure_bm25_ready();
    emit_video_progress(Some(&app_handle), "ingesting", 0.0, "正在入库知识库...");
    let title = std::path::Path::new(&transcription.video_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("视频转写")
        .to_string();

    let ingestion_result = crate::services::ingestion::ingest_text(
        &transcription.text,
        &format!("[视频转写] {}", title),
        project_id,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        &state.bm25,
        None,
        None,
        None,
        None,
        Some(&state.data_dir),
    )?;

    let meeting_minutes = if generate_minutes {
        emit_video_progress(
            Some(&app_handle),
            "generating_minutes",
            0.0,
            "正在生成会议纪要...",
        );
        Some(
            crate::services::video_transcriber::generate_meeting_minutes(
                &transcription.text,
                &state.llm,
            )?,
        )
    } else {
        None
    };

    emit_video_progress(Some(&app_handle), "done", 100.0, "全部完成");

    Ok(VideoPipelineResult {
        transcription,
        ingestion_document_id: Some(ingestion_result.document_id),
        meeting_minutes,
    })
}

/// 从已有转写文本生成会议纪要。
#[tauri::command]
pub async fn generate_meeting_minutes_from_transcript(
    state: State<'_, AppState>,
    transcript: String,
) -> Result<MeetingMinutesResult, String> {
    if transcript.is_empty() {
        return Err("转写文本为空".to_string());
    }
    crate::services::video_transcriber::generate_meeting_minutes(&transcript, &state.llm)
}

// ─── ASR Provider 管理命令 ────────────────────────────────

/// ASR 服务商类型（单一明确语义，前端按枚举选择，命令层直接 dispatch）
#[derive(Debug, Clone, Copy, Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AsrProviderKind {
    /// 腾讯云一句话识别（REST API）
    Tencent,
}

/// ASR Provider 信息（前端展示用）
#[derive(Serialize)]
pub struct AsrProviderInfo {
    kind: AsrProviderKind,
    name: String,
    description: String,
}

/// 列出所有可用的 ASR Provider
#[tauri::command]
pub fn list_asr_providers() -> Result<Vec<AsrProviderInfo>, String> {
    Ok(vec![AsrProviderInfo {
        kind: AsrProviderKind::Tencent,
        name: "腾讯云语音识别".to_string(),
        description: "腾讯云在线语音识别（一句话识别），识别精度高，需配置 SecretId/SecretKey。".to_string(),
    }])
}

/// 保存 ASR 配置（腾讯云）
#[tauri::command]
pub fn save_asr_config(
    state: State<'_, AppState>,
    tencent_secret_id: Option<String>,
    tencent_secret_key: Option<String>,
) -> Result<(), String> {
    let mut config = state.asr_config.write().map_err(|e| e.to_string())?;
    config.save_tencent(tencent_secret_id, tencent_secret_key)
}

/// 获取 ASR 配置状态
#[tauri::command]
pub fn get_asr_config_status(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let config = state.asr_config.read().map_err(|e| e.to_string())?;
    Ok(config.get_status())
}
