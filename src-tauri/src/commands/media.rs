use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::app_state::AppState;
use crate::services::asr_provider::FileAsr;
use crate::services::model_downloader;
use crate::services::video_transcriber::{
    MeetingMinutesResult, VideoPipelineResult, VideoTranscriptionResult,
};
use crate::services::whisper_service::{TranscriptionResult, WhisperStatus};

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

/// 开始麦克风录音。
#[tauri::command]
pub fn start_whisper_recording(state: State<'_, AppState>) -> Result<(), String> {
    let capture = state.audio_capture.write().map_err(|e| e.to_string())?;
    capture.start_recording()
}

/// 停止录音并转录音频。
/// provider: 可选 ASR 服务商（tencent），为空则使用本地 Whisper
#[tauri::command]
pub async fn stop_whisper_recording(
    state: State<'_, AppState>,
    provider: Option<String>,
) -> Result<TranscriptionResult, String> {
    // 使用在线 ASR（腾讯云）
    if let Some(ref asr_provider) = provider {
        let asr_provider = asr_provider.as_str();

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

        let config = crate::services::asr_provider::AsrConfig::default();
        let start = std::time::Instant::now();

        let result = match asr_provider {
            "tencent" => {
                let (secret_id, secret_key) = {
                    let cfg = state.asr_config.read().map_err(|e| e.to_string())?;
                    let sid = cfg.tencent_secret_id.clone()
                        .ok_or_else(|| "请先在设置中配置腾讯云 SecretId".to_string())?;
                    let sk = cfg.tencent_secret_key.clone()
                        .ok_or_else(|| "请先在设置中配置腾讯云 SecretKey".to_string())?;
                    (sid, sk)
                };
                let provider = crate::services::tencent_asr::TencentOneShotProvider::new(
                    secret_id, secret_key,
                );
                provider.recognize_file(&pcm_data, &config).await
                    .map_err(|e| format!("腾讯 ASR 识别失败: {:?}", e))?
            }
            _ => return Err(format!("不支持的 ASR 服务商: {}", asr_provider)),
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
        &pcm_data, 16000, 500, 0.01,
    );

    if speech_segments.is_empty() {
        return Ok(TranscriptionResult {
            text: String::new(),
            segments: Vec::new(),
            confidence: 0.0,
            processing_time_ms: 0,
        });
    }

    let speech_pcm: Vec<f32> = speech_segments
        .iter()
        .flat_map(|(start, end)| pcm_data[*start..*end].to_vec())
        .collect();

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
    project: String,
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

    emit_video_progress(Some(&app_handle), "ingesting", 0.0, "正在入库知识库...");
    let title = std::path::Path::new(&transcription.video_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("视频转写")
        .to_string();

    let ingestion_result = crate::services::ingestion::ingest_text(
        &transcription.text,
        &format!("[视频转写] {}", title),
        &project,
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

#[derive(Serialize)]
pub struct AsrProviderInfo {
    r#type: String,
    name: String,
    description: String,
    supports_streaming: bool,
    supports_file: bool,
}

/// 列出所有可用的 ASR Provider
#[tauri::command]
pub fn list_asr_providers() -> Result<Vec<AsrProviderInfo>, String> {
    Ok(vec![
        AsrProviderInfo {
            r#type: "whisper".to_string(),
            name: "本地 Whisper".to_string(),
            description: "本地离线语音识别，无需网络，支持中文/英文。需要先下载模型（约 80MB）。首次使用时自动下载。".to_string(),
            supports_streaming: false,
            supports_file: true,
        },
        AsrProviderInfo {
            r#type: "tencent".to_string(),
            name: "腾讯云语音识别".to_string(),
            description: "腾讯云在线语音识别（一句话识别），识别精度高，需配置 SecretId/SecretKey。".to_string(),
            supports_streaming: false,
            supports_file: true,
        },
    ])
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
