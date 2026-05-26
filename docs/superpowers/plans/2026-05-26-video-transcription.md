# 视频文件转写+入库+会议纪要 实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 支持导入视频文件（MP4/WebM/AVI/MOV/MKV），通过 ffmpeg-sidecar 提取音频 → Whisper 本地转写 → 入库知识库 → LLM 生成会议纪要。

**架构：** 新增 `video_transcriber.rs` 服务模块，使用 `ffmpeg-sidecar` crate 从视频文件提取音频为 16kHz 单声道 PCM，复用现有 `WhisperService` 进行转写，复用 `ingestion::ingest_text()` 入库，复用 `LLMService` 生成会议纪要。前端在 Import 页面增加"视频转写"卡片。

**技术栈：** Rust / ffmpeg-sidecar 2.x / whisper-rs 0.16 / Tauri v2 / React + TypeScript / TailwindCSS

---

## 文件结构

| 操作 | 文件路径 | 职责 |
|------|----------|------|
| 修改 | `src-tauri/Cargo.toml` | 添加 ffmpeg-sidecar 依赖 |
| 创建 | `src-tauri/src/services/video_transcriber.rs` | 视频音频提取 + 转写 + 会议纪要生成核心逻辑 |
| 修改 | `src-tauri/src/services/mod.rs` | 注册新模块 |
| 修改 | `src-tauri/src/services/file_extractor.rs` | 添加视频格式支持 |
| 修改 | `src-tauri/src/lib.rs` | 添加 3 个新 Tauri 命令 |
| 修改 | `src/lib/tauri-commands.ts` | 添加前端类型和 API 封装 |
| 修改 | `src/pages/Import.tsx` | 添加视频转写 UI 卡片 |

---

### 任务 1：添加 ffmpeg-sidecar 依赖

**文件：**
- 修改：`src-tauri/Cargo.toml`

- [ ] **步骤 1：在 Cargo.toml 的 [dependencies] 中添加 ffmpeg-sidecar**

在 `# Phase 12: Whisper Voice Recognition` 代码块之后添加注释和依赖：

```toml
# Phase 14: Video Transcription (audio extraction from video)
ffmpeg-sidecar = "2"
```

- [ ] **步骤 2：验证编译**

运行：`cd src-tauri && cargo check`
预期：成功（ffmpeg-sidecar 是纯 Rust + 内嵌二进制）

- [ ] **步骤 3：Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "chore: add ffmpeg-sidecar dependency for video audio extraction"
```

---

### 任务 2：创建 video_transcriber.rs 核心服务

**文件：**
- 创建：`src-tauri/src/services/video_transcriber.rs`

这是核心服务模块，负责：
1. 用 ffmpeg-sidecar 从视频文件提取音频为 16kHz mono f32 PCM
2. 调用 WhisperService 转写
3. 中文后处理
4. 可选：调用 LLM 生成会议纪要

- [ ] **步骤 1：创建 video_transcriber.rs 文件骨架**

```rust
//! 视频文件转写服务 — 从视频提取音频并通过 Whisper 转写为文字
//!
//! 管道：视频文件 → ffmpeg 音频提取 → 16kHz mono PCM →
//!        Whisper 转写 → 中文后处理 → 可选会议纪要生成
//!
//! 依赖：
//! - ffmpeg-sidecar: 内嵌 FFmpeg 二进制，无需系统安装
//! - whisper_service: 本地 Whisper 语音识别
//! - chinese_postprocess: 中文转录后处理

use serde::{Deserialize, Serialize};
use std::path::Path;

// ─── Types ────────────────────────────────────────────────────────────────

/// 视频转写的完整结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoTranscriptionResult {
    /// 视频文件路径
    pub video_path: String,
    /// 完整转写文本
    pub text: String,
    /// 分段转写结果（带时间戳）
    pub segments: Vec<TranscriptionSegment>,
    /// 平均置信度
    pub confidence: f32,
    /// 音频提取耗时（毫秒）
    pub extraction_time_ms: u64,
    /// Whisper 转写耗时（毫秒）
    pub transcription_time_ms: u64,
    /// 视频时长（秒）
    pub duration_secs: f32,
}

/// 转写段落（复用 whisper_service 的类型，但独立声明以避免耦合）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// 会议纪要生成结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingMinutesResult {
    /// 生成的会议纪要 Markdown 文本
    pub minutes: String,
    /// LLM 生成耗时（毫秒）
    pub generation_time_ms: u64,
}

/// 视频转写 + 入库 + 会议纪要的完整流水线结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoPipelineResult {
    /// 转写结果
    pub transcription: VideoTranscriptionResult,
    /// 知识库入库结果（可选，用户可能选择不入库）
    pub ingestion_document_id: Option<i64>,
    /// 会议纪要（可选，用户可能选择不生成）
    pub meeting_minutes: Option<MeetingMinutesResult>,
}

// ─── Audio Extraction ─────────────────────────────────────────────────────

/// 从视频文件中提取音频为 16kHz 单声道 f32 PCM 数据
///
/// 使用 ffmpeg-sidecar 内嵌的 FFmpeg 二进制。
/// 输出格式：f32le, 16000Hz, mono
pub fn extract_audio_from_video(video_path: &Path) -> Result<(Vec<f32>, f32), String> {
    let video_str = video_path
        .to_str()
        .ok_or("视频文件路径包含非法字符")?;

    // 使用 ffmpeg-sidecar 提取音频
    // -i input → -f f32le (raw float32 little-endian) → -ar 16000 → -ac 1 (mono)
    let output = ffmpeg_sidecar::command::FFmpegCommand::new()
        .input(video_str)
        .raw_audio()
        .sample_rate(16000)
        .channels(1)
        .output("-")  // stdout
        .run()
        .map_err(|e| format!("FFmpeg 音频提取失败: {}", e))?;

    // 从 stdout 收集原始 PCM 字节
    let raw_bytes = output.stdout;
    if raw_bytes.is_empty() {
        return Err("FFmpeg 输出为空，可能视频没有音频轨道".to_string());
    }

    // 将 f32le 字节转换为 Vec<f32>
    let pcm_f32: Vec<f32> = raw_bytes
        .chunks_exact(4)
        .map(|chunk| {
            let bytes: [u8; 4] = [chunk[0], chunk[1], chunk[2], chunk[3]];
            f32::from_le_bytes(bytes)
        })
        .collect();

    // 计算时长（秒）
    let duration_secs = pcm_f32.len() as f32 / 16000.0;

    Ok((pcm_f32, duration_secs))
}

// ─── Meeting Minutes Generation ──────────────────────────────────────────

/// 生成会议纪要的 LLM 提示词
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

/// 使用 LLM 从转写文本生成会议纪要
///
/// 需要 LLM 已配置（api_key + base_url）。如果 LLM 不可用，返回错误。
pub fn generate_meeting_minutes(
    transcript: &str,
    llm_service: &crate::services::llm_service::LLMService,
) -> Result<MeetingMinutesResult, String> {
    let start = std::time::Instant::now();

    let user_prompt = format!(
        "以下是会议/访谈的语音转写文本：\n\n---\n{}\n---\n\n请生成结构化的会议纪要。",
        transcript
    );

    let minutes = llm_service.generate_text_sync(
        MEETING_MINUTES_PROMPT,
        &user_prompt,
    ).map_err(|e| format!("会议纪要生成失败: {}", e))?;

    let generation_time_ms = start.elapsed().as_millis() as u64;

    Ok(MeetingMinutesResult {
        minutes,
        generation_time_ms,
    })
}
```

- [ ] **步骤 2：在 mod.rs 注册模块**

在 `src-tauri/src/services/mod.rs` 末尾添加：

```rust
pub mod video_transcriber;
```

- [ ] **步骤 3：检查 LLMService 是否有同步生成方法**

读取 `src-tauri/src/services/llm_service.rs`，查找是否有类似 `generate_text_sync` 或非流式补全方法。
如果没有，需要添加一个简单的同步封装方法。在 LLMService impl 中添加：

```rust
/// 同步生成文本（非流式），用于后端内部调用
///
/// 返回完整的生成文本。
pub fn generate_text_sync(&self, system_prompt: &str, user_message: &str) -> Result<String, String> {
    let config = self.config.lock().map_err(|e| e.to_string())?;
    
    if config.api_key.is_empty() {
        return Err("LLM API key not configured".to_string());
    }

    let client = ureq::agent();
    
    let body = serde_json::json!({
        "model": config.model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_message }
        ],
        "temperature": config.temperature,
        "max_tokens": config.max_tokens,
    });

    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    
    let response: serde_json::Value = client
        .post(&url)
        .set("Authorization", &format!("Bearer {}", config.api_key))
        .set("Content-Type", "application/json")
        .send_json(&body)
        .map_err(|e| format!("LLM request failed: {}", e))?
        .into_json()
        .map_err(|e| format!("Failed to parse LLM response: {}", e))?;

    let text = response["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();

    Ok(text)
}
```

注意：LLMService 已有 `ureq = "3"` 依赖（见 Cargo.toml），可直接使用。

- [ ] **步骤 4：验证编译**

运行：`cd src-tauri && cargo check`
预期：编译成功

- [ ] **步骤 5：Commit**

```bash
git add src-tauri/src/services/video_transcriber.rs src-tauri/src/services/mod.rs src-tauri/src/services/llm_service.rs
git commit -m "feat: add video_transcriber service for audio extraction and meeting minutes generation"
```

---

### 任务 3：添加 Tauri 命令

**文件：**
- 修改：`src-tauri/src/lib.rs`

添加 3 个新命令：
1. `transcribe_video_file` — 从视频提取音频并转写
2. `transcribe_and_ingest_video` — 转写 + 入库
3. `generate_meeting_minutes_from_transcript` — 从已有转写文本生成会议纪要

- [ ] **步骤 1：在 lib.rs 添加 import**

在文件顶部的 `use services::whisper_service::{TranscriptionResult, WhisperStatus};` 行附近添加：

```rust
use services::video_transcriber::{VideoTranscriptionResult, VideoPipelineResult, MeetingMinutesResult};
```

- [ ] **步骤 2：添加 transcribe_video_file 命令**

在 `stop_whisper_recording` 命令之后，`// ─── 阶段 13` 注释之前添加：

```rust
// ─── 阶段 14: 视频文件转写 ───

/// 从视频文件中提取音频并通过 Whisper 转写。
///
/// 支持格式：mp4, webm, avi, mov, mkv, flv, wmv, m4a, mp3, wav
/// 管道：ffmpeg 提取音频 → VAD → Whisper 转写 → 中文后处理
#[tauri::command]
async fn transcribe_video_file(
    state: State<'_, AppState>,
    video_path: String,
) -> Result<VideoTranscriptionResult, String> {
    let path = std::path::Path::new(&video_path);
    if !path.exists() {
        return Err(format!("视频文件不存在: {}", video_path));
    }

    // 步骤 1: 使用 ffmpeg-sidecar 提取音频
    let (pcm_data, duration_secs) = {
        let extract_start = std::time::Instant::now();
        let (pcm, dur) = services::video_transcriber::extract_audio_from_video(path)?;
        let extraction_time_ms = extract_start.elapsed().as_millis() as u64;

        // 临时用 extraction_time_ms 传递，后面统一组装
        (pcm, (dur, extraction_time_ms))
    };

    let extraction_time_ms = duration_secs.1;
    let duration_secs = duration_secs.0;

    if pcm_data.is_empty() {
        return Err("音频提取结果为空，视频可能没有音频轨道".to_string());
    }

    // 步骤 2: VAD — 检测语音段
    let speech_segments = services::audio_capture::AudioCapture::detect_speech_segments(
        &pcm_data, 16000, 500, 0.01,
    );

    let speech_pcm: Vec<f32> = if speech_segments.is_empty() {
        pcm_data
    } else {
        speech_segments.iter()
            .flat_map(|(start, end)| pcm_data[*start..*end].to_vec())
            .collect()
    };

    // 步骤 3: Whisper 转写
    let whisper_result = {
        let whisper = state.whisper_service.lock().map_err(|e| e.to_string())?;
        if !whisper.is_model_loaded() {
            return Err("Whisper 模型未加载。请先在设置中加载 Whisper 模型。".to_string());
        }
        whisper.transcribe(&speech_pcm)?
    };

    // 步骤 4: 中文后处理
    let processed_text = services::chinese_postprocess::postprocess_chinese(&whisper_result.text);

    let transcription_time_ms = whisper_result.processing_time_ms;

    Ok(VideoTranscriptionResult {
        video_path: video_path.clone(),
        text: processed_text,
        segments: whisper_result.segments.into_iter().map(|s| services::video_transcriber::TranscriptionSegment {
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            text: s.text,
        }).collect(),
        confidence: whisper_result.confidence,
        extraction_time_ms,
        transcription_time_ms,
        duration_secs,
    })
}

/// 从视频提取音频 → 转写 → 入库知识库 → 可选生成会议纪要
///
/// 一站式视频处理管道。`generate_minutes` 控制是否生成会议纪要。
#[tauri::command]
async fn transcribe_and_ingest_video(
    state: State<'_, AppState>,
    video_path: String,
    project: String,
    generate_minutes: bool,
) -> Result<VideoPipelineResult, String> {
    // 步骤 1: 转写
    let transcription = transcribe_video_file(state.clone(), video_path).await?;

    if transcription.text.is_empty() {
        return Err("转写结果为空，无法入库".to_string());
    }

    // 步骤 2: 入库知识库
    let title = std::path::Path::new(&transcription.video_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("视频转写")
        .to_string();

    let ingestion_result = services::ingestion::ingest_text(
        &transcription.text,
        &format!("[视频转写] {}", title),
        &project,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        None, // 不传 app_handle，避免进度事件干扰
    )?;

    // 步骤 3: 可选生成会议纪要
    let meeting_minutes = if generate_minutes {
        Some(services::video_transcriber::generate_meeting_minutes(
            &transcription.text,
            &state.llm,
        )?)
    } else {
        None
    };

    Ok(VideoPipelineResult {
        transcription,
        ingestion_document_id: Some(ingestion_result.document_id),
        meeting_minutes,
    })
}

/// 从已有转写文本生成会议纪要
///
/// 可单独调用，不需要重新转写视频。
#[tauri::command]
async fn generate_meeting_minutes_from_transcript(
    state: State<'_, AppState>,
    transcript: String,
) -> Result<MeetingMinutesResult, String> {
    if transcript.is_empty() {
        return Err("转写文本为空".to_string());
    }

    services::video_transcriber::generate_meeting_minutes(&transcript, &state.llm)
}
```

- [ ] **步骤 3：注册命令到 invoke_handler**

在 `#[tauri::command]` 列表的 `// Phase 12: Whisper Voice Recognition` 之后添加：

```rust
            // Phase 14: Video Transcription
            transcribe_video_file,
            transcribe_and_ingest_video,
            generate_meeting_minutes_from_transcript,
```

- [ ] **步骤 4：验证编译**

运行：`cd src-tauri && cargo check`
预期：编译成功。注意 `State<'_, AppState>` 在 `transcribe_and_ingest_video` 内部调用 `transcribe_video_file` 时需要传递 state 的 clone — 如果编译失败，将转写逻辑提取为内部函数（不依赖 State）。

**重要备选方案**：如果 `transcribe_and_ingest_video` 无法调用另一个 `#[tauri::command]`（因为 State 消耗），则将转写核心逻辑提取为独立函数：

```rust
/// 内部转写逻辑（非 Tauri 命令，可被其他函数调用）
fn do_transcribe_video(
    whisper_service: &std::sync::MutexGuard<'_, services::whisper_service::WhisperService>,
    video_path: &str,
) -> Result<VideoTranscriptionResult, String> {
    let path = std::path::Path::new(video_path);
    if !path.exists() {
        return Err(format!("视频文件不存在: {}", video_path));
    }

    let extract_start = std::time::Instant::now();
    let (pcm_data, duration_secs) = services::video_transcriber::extract_audio_from_video(path)?;
    let extraction_time_ms = extract_start.elapsed().as_millis() as u64;

    if pcm_data.is_empty() {
        return Err("音频提取结果为空".to_string());
    }

    let speech_segments = services::audio_capture::AudioCapture::detect_speech_segments(
        &pcm_data, 16000, 500, 0.01,
    );

    let speech_pcm: Vec<f32> = if speech_segments.is_empty() {
        pcm_data
    } else {
        speech_segments.iter()
            .flat_map(|(start, end)| pcm_data[*start..*end].to_vec())
            .collect()
    };

    let whisper_result = whisper_service.transcribe(&speech_pcm)?;
    let processed_text = services::chinese_postprocess::postprocess_chinese(&whisper_result.text);

    Ok(VideoTranscriptionResult {
        video_path: video_path.to_string(),
        text: processed_text,
        segments: whisper_result.segments.into_iter().map(|s| services::video_transcriber::TranscriptionSegment {
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            text: s.text,
        }).collect(),
        confidence: whisper_result.confidence,
        extraction_time_ms,
        transcription_time_ms: whisper_result.processing_time_ms,
        duration_secs,
    })
}
```

然后 `transcribe_video_file` 和 `transcribe_and_ingest_video` 都调用 `do_transcribe_video`。

- [ ] **步骤 5：Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: add Tauri commands for video transcription pipeline"
```

---

### 任务 4：扩展 file_extractor 支持视频格式

**文件：**
- 修改：`src-tauri/src/services/file_extractor.rs`

让 `ingest_file()` 也能处理视频文件（自动走转写管道而非直接提取文本）。

- [ ] **步骤 1：添加视频格式到 supported_extensions**

在 `file_extractor.rs` 的 `supported_extensions()` 函数中，添加视频和音频格式：

```rust
pub fn supported_extensions() -> &'static [&'static str] {
    &[
        "md", "txt", "text", "markdown", "html", "htm", "pdf", "docx", "xlsx", "xls",
        // Phase 14: Video/Audio formats (需要转写管道，不能直接提取文本)
        "mp4", "webm", "avi", "mov", "mkv", "flv", "wmv", "m4a", "mp3", "wav",
    ]
}
```

- [ ] **步骤 2：添加 is_video_format 辅助函数**

```rust
/// 检查是否为视频/音频格式（需要转写管道处理）
pub fn is_video_format(file_path: &Path) -> bool {
    let extension = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    
    matches!(
        extension.as_str(),
        "mp4" | "webm" | "avi" | "mov" | "mkv" | "flv" | "wmv" | "m4a" | "mp3" | "wav"
    )
}
```

- [ ] **步骤 3：在 extract_text 的 match 中添加视频格式分支**

```rust
        // 视频/音频：不能直接提取文本，返回特定错误
        // 调用方应检测 is_video_format() 并走转写管道
        "mp4" | "webm" | "avi" | "mov" | "mkv" | "flv" | "wmv" | "m4a" | "mp3" | "wav" => {
            Err(format!(
                "视频/音频文件 (.{}) 需要通过转写管道处理，请使用视频转写功能",
                extension
            ))
        }
```

- [ ] **步骤 4：验证编译**

运行：`cd src-tauri && cargo check`
预期：编译成功

- [ ] **步骤 5：Commit**

```bash
git add src-tauri/src/services/file_extractor.rs
git commit -m "feat: extend file_extractor with video/audio format recognition"
```

---

### 任务 5：前端 TypeScript 类型和 API 封装

**文件：**
- 修改：`src/lib/tauri-commands.ts`

- [ ] **步骤 1：在文件末尾（`exportReport` 之后）添加视频转写相关类型和 API**

```typescript
// ── Phase 14: Video Transcription ───────────────────────────────────────

export interface VideoTranscriptionSegment {
  start_ms: number;
  end_ms: number;
  text: string;
}

export interface VideoTranscriptionResult {
  video_path: string;
  text: string;
  segments: VideoTranscriptionSegment[];
  confidence: number;
  extraction_time_ms: number;
  transcription_time_ms: number;
  duration_secs: number;
}

export interface MeetingMinutesResult {
  minutes: string;
  generation_time_ms: number;
}

export interface VideoPipelineResult {
  transcription: VideoTranscriptionResult;
  ingestion_document_id: number | null;
  meeting_minutes: MeetingMinutesResult | null;
}

/**
 * 从视频文件提取音频并转写。
 * 需要先加载 Whisper 模型（loadWhisperModel）。
 */
export async function transcribeVideoFile(
  videoPath: string,
): Promise<VideoTranscriptionResult> {
  return invoke("transcribe_video_file", { videoPath });
}

/**
 * 视频转写一站式管道：提取音频 → 转写 → 入库 → 可选生成会议纪要。
 */
export async function transcribeAndIngestVideo(
  videoPath: string,
  project: string,
  generateMinutes: boolean,
): Promise<VideoPipelineResult> {
  return invoke("transcribe_and_ingest_video", {
    videoPath,
    project,
    generateMinutes,
  });
}

/**
 * 从已有转写文本生成会议纪要。
 */
export async function generateMeetingMinutesFromTranscript(
  transcript: string,
): Promise<MeetingMinutesResult> {
  return invoke("generate_meeting_minutes_from_transcript", { transcript });
}
```

- [ ] **步骤 2：验证 TypeScript 编译**

运行：`npx tsc --noEmit`
预期：无错误

- [ ] **步骤 3：Commit**

```bash
git add src/lib/tauri-commands.ts
git commit -m "feat: add TypeScript types and API wrappers for video transcription"
```

---

### 任务 6：前端 UI — Import 页面添加视频转写卡片

**文件：**
- 修改：`src/pages/Import.tsx`

在现有 Import 页面中添加第三个导入方式：视频/音频转写。

- [ ] **步骤 1：添加视频转写状态和导入**

在 Import.tsx 顶部的 import 中添加新的 API 和图标：

```typescript
import {
  // ... 现有 imports
  Video,
  FileAudio,
} from "lucide-react";
import {
  // ... 现有 tauri-commands imports
  transcribeAndIngestVideo,
  type VideoPipelineResult,
  getWhisperStatus,
} from "../lib/tauri-commands";
```

- [ ] **步骤 2：添加视频转写状态**

在现有 `isDragging` state 之后添加：

```typescript
  // Video transcription state
  const [videoFeedback, setVideoFeedback] = useState<ImportFeedback | null>(null);
  const [videoProject, setVideoProject] = useState("default");
  const [videoCustomProject, setVideoCustomProject] = useState("");
  const [videoGeneratingMinutes, setVideoGeneratingMinutes] = useState(true);
  const [videoResult, setVideoResult] = useState<VideoPipelineResult | null>(null);
  const [whisperReady, setWhisperReady] = useState(false);
  const [whisperChecking, setWhisperChecking] = useState(true);
```

- [ ] **步骤 3：添加 Whisper 状态检查 effect**

```typescript
  // Check Whisper model status on mount
  useEffect(() => {
    getWhisperStatus()
      .then((status) => setWhisperReady(status.model_loaded))
      .catch(() => setWhisperReady(false))
      .finally(() => setWhisperChecking(false));
  }, []);
```

- [ ] **步骤 4：添加视频文件选择和处理函数**

```typescript
  const handleVideoImport = useCallback(async () => {
    const filePath = await open({
      multiple: false,
      filters: [
        {
          name: "视频/音频文件",
          extensions: ["mp4", "webm", "avi", "mov", "mkv", "flv", "wmv", "m4a", "mp3", "wav"],
        },
      ],
    });
    if (!filePath) return;

    const proj = getProjectName(videoProject, videoCustomProject);
    setVideoFeedback({ status: "loading", message: "正在提取音频并转写，这可能需要几分钟..." });
    setVideoResult(null);

    try {
      const result = await transcribeAndIngestVideo(
        filePath as string,
        proj,
        videoGeneratingMinutes,
      );
      setVideoFeedback({
        status: "success",
        message: `转写完成！${result.transcription.duration_secs.toFixed(0)}秒视频 → ${result.transcription.text.length}字。已入库知识库。${result.meeting_minutes ? " 会议纪要已生成。" : ""}`,
      });
      setVideoResult(result);
    } catch (err) {
      setVideoFeedback({
        status: "error",
        message: String(err),
      });
    }
  }, [videoProject, videoCustomProject, videoGeneratingMinutes]);
```

- [ ] **步骤 5：添加视频转写 UI 卡片**

在 Import.tsx 的 return JSX 中，在文件导入卡片和文本导入卡片之间（或之后）添加视频转写卡片。找到现有卡片布局模式，在同级位置添加：

```tsx
{/* Video/Audio Transcription Card */}
<div className="bg-white rounded-xl shadow-sm border border-gray-200 p-6">
  <div className="flex items-center gap-3 mb-4">
    <div className="p-2 bg-purple-100 rounded-lg">
      <Video className="w-5 h-5 text-purple-600" />
    </div>
    <div>
      <h3 className="font-semibold text-gray-900">视频/音频转写</h3>
      <p className="text-sm text-gray-500">导入录屏或音频文件，自动转写为文字</p>
    </div>
  </div>

  {/* Whisper model status */}
  {whisperChecking ? (
    <p className="text-sm text-gray-400 mb-3">正在检查语音识别模型...</p>
  ) : whisperReady ? (
    <p className="text-sm text-green-600 mb-3 flex items-center gap-1">
      <CheckCircle2 className="w-4 h-4" />
      语音识别模型已就绪
    </p>
  ) : (
    <p className="text-sm text-amber-600 mb-3">
      ⚠️ 请先在「研究助手」页面加载 Whisper 模型
    </p>
  )}

  {/* Project selector */}
  <div className="flex gap-2 mb-3">
    <select
      value={videoProject}
      onChange={(e) => setVideoProject(e.target.value)}
      className="px-3 py-2 border border-gray-200 rounded-lg text-sm bg-white"
    >
      {projectOptions.map((opt) => (
        <option key={opt.value} value={opt.value}>
          {opt.label}
        </option>
      ))}
    </select>
    {videoProject === "custom" && (
      <input
        type="text"
        placeholder="项目名称"
        value={videoCustomProject}
        onChange={(e) => setVideoCustomProject(e.target.value)}
        className="flex-1 px-3 py-2 border border-gray-200 rounded-lg text-sm"
      />
    )}
  </div>

  {/* Generate minutes toggle */}
  <label className="flex items-center gap-2 mb-3 text-sm text-gray-700">
    <input
      type="checkbox"
      checked={videoGeneratingMinutes}
      onChange={(e) => setVideoGeneratingMinutes(e.target.checked)}
      className="rounded border-gray-300"
    />
    自动生成会议纪要
  </label>

  {/* Import button */}
  <button
    onClick={handleVideoImport}
    disabled={!whisperReady}
    className="w-full py-2.5 px-4 bg-purple-600 text-white rounded-lg text-sm font-medium hover:bg-purple-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors flex items-center justify-center gap-2"
  >
    <FileAudio className="w-4 h-4" />
    选择视频/音频文件
  </button>

  {/* Feedback */}
  {videoFeedback && (
    <div
      className={`mt-3 p-3 rounded-lg text-sm ${
        videoFeedback.status === "loading"
          ? "bg-blue-50 text-blue-700"
          : videoFeedback.status === "success"
          ? "bg-green-50 text-green-700"
          : "bg-red-50 text-red-700"
      }`}
    >
      {videoFeedback.status === "loading" && (
        <Loader2 className="w-4 h-4 animate-spin inline mr-2" />
      )}
      {videoFeedback.message}
    </div>
  )}

  {/* Transcription result preview */}
  {videoResult && videoResult.transcription.text && (
    <details className="mt-3">
      <summary className="text-sm font-medium text-gray-700 cursor-pointer">
        转写结果预览（{videoResult.transcription.text.length}字）
      </summary>
      <div className="mt-2 p-3 bg-gray-50 rounded-lg text-sm text-gray-600 max-h-60 overflow-y-auto whitespace-pre-wrap">
        {videoResult.transcription.text.slice(0, 2000)}
        {videoResult.transcription.text.length > 2000 && "..."}
      </div>
    </details>
  )}

  {/* Meeting minutes preview */}
  {videoResult?.meeting_minutes && (
    <details className="mt-2">
      <summary className="text-sm font-medium text-gray-700 cursor-pointer">
        会议纪要预览
      </summary>
      <div className="mt-2 p-3 bg-gray-50 rounded-lg text-sm text-gray-600 max-h-60 overflow-y-auto whitespace-pre-wrap">
        {videoResult.meeting_minutes.minutes}
      </div>
    </details>
  )}
</div>
```

- [ ] **步骤 6：验证前端编译**

运行：`npx tsc --noEmit`
预期：无类型错误

- [ ] **步骤 7：Commit**

```bash
git add src/pages/Import.tsx
git commit -m "feat: add video transcription UI card to Import page"
```

---

### 任务 7：端到端验证和 ffmpeg-sidecar 兼容性修复

**文件：**
- 可能修改：`src-tauri/src/services/video_transcriber.rs`

- [ ] **步骤 1：完整编译**

运行：`cd src-tauri && cargo build`
预期：编译成功

**注意 ffmpeg-sidecar 的 API 可能与计划中的用法有差异。** 如果编译失败，检查 ffmpeg-sidecar 2.x 的实际 API：

1. 如果 `FFmpegCommand::new()` 不存在，检查是否需要 `full()` 或其他入口
2. 如果 `raw_audio()` / `sample_rate()` / `channels()` 链式调用不匹配，查阅 crate 文档
3. 如果 stdout 输出方式不同（可能需要 `.pipe()` 或 `.capture()`），调整代码
4. 关键参考：`ffmpeg-sidecar` 的 README 和 examples

使用 Context7 或 grep_app_searchGitHub 查找最新用法：

```bash
# 在 GitHub 上搜索 ffmpeg-sidecar 实际使用示例
```

- [ ] **步骤 2：前端完整构建**

运行：`npm run build`
预期：构建成功

- [ ] **步骤 3：最终 Commit**

```bash
git add -A
git commit -m "feat: complete video transcription pipeline (Phase 14)"
```

---

## 自检

### 规格覆盖度
- ✅ 导入视频文件（MP4/WebM/AVI/MOV/MKV 等）— 任务 3-6
- ✅ ffmpeg-sidecar 内嵌 FFmpeg — 任务 1-2
- ✅ 音频提取为 16kHz mono PCM — 任务 2
- ✅ Whisper 本地转写 — 任务 2-3（复用现有 WhisperService）
- ✅ 中文后处理 — 任务 3
- ✅ 入库知识库 — 任务 3（复用现有 ingestion）
- ✅ 生成会议纪要 — 任务 2-3
- ✅ 前端 UI — 任务 6

### 占位符扫描
- ✅ 无 "TODO" / "TBD" / "待定"
- ✅ 所有步骤都有具体代码
- ✅ 所有文件路径精确

### 类型一致性
- ✅ `VideoTranscriptionResult` 在 Rust 和 TypeScript 中字段一致
- ✅ `MeetingMinutesResult` 在 Rust 和 TypeScript 中字段一致
- ✅ `VideoPipelineResult` 在 Rust 和 TypeScript 中字段一致
- ✅ Tauri invoke 的参数名使用 snake_case（与 Rust 参数名匹配）
