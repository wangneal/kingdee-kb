# Phase 12: Whisper 语音识别 — CONTEXT

**Phase:** 12
**Goal:** 本地麦克风 → Whisper 实时转写 → 问题推荐引擎
**Depends on:** 独立（可与 Phase 11 并行，Phase 11 已完成）

---

## 1. Problem Statement

实施顾问在调研访谈时，需要边听边记录。目前只能手动打字输入，效率极低。Whisper 本地语音识别可以让顾问：
- 录音并实时转写为文字
- 转写结果直接送入问题推荐引擎（Phase 11）获取相关推荐
- 零费用、离线运行（whisper-rs 绑定 whisper.cpp）

## 2. Existing Infrastructure

### 2a. Tauri 2.x App
- `app_state.rs`: AppState 持有所有服务实例
- `lib.rs`: Tauri commands 通过 `generate_handler!` 宏注册
- `services/mod.rs`: 模块注册

### 2b. Phase 11 Question Recommend
- `question_recommend.rs`: `recommend_questions()`, `generate_followup_questions()`, `smart_fill_for_question()`
- 转写结果可作为 `RecommendRequest.query` 输入

### 2c. LLM Service
- `llm_service.rs`: OpenAI-compatible API，可用于中文后处理（标点恢复）

## 3. Requirements Breakdown

| # | Task | Description |
|---|------|-------------|
| T1 | whisper-rs 集成 | 加载 GGML 模型，初始化 WhisperContext，提供转录 API |
| T2 | 桌面麦克风捕获 | Windows WASAPI/CPAL 录音，16kHz 单声道 PCM |
| T3 | 流式转写 pipeline | 录音缓冲 → VAD 端点检测 → Whisper 转写 → 前端推送 |
| T4 | 中文后处理 | 标点恢复、短句合并、重复词去除 |

## 4. Design Decisions

1. **whisper-rs (whisper.cpp bindings)**: 本地推理，零费用，支持中文
2. **cpal crate**: 跨平台音频捕获，Windows 用 WASAPI 后端
3. **模型选择**: 默认 tiny (~75MB) 用于实时转写，可选 small (~500MB) 提升精度
4. **VAD (Voice Activity Detection)**: 简单能量阈值检测，避免静音段送入 Whisper
5. **流式架构**: 录音线程 → 音频缓冲区 → 转写线程 → Tauri Event 推送前端
6. **中文后处理**: 规则引擎（正则标点恢复 + 短句合并），不依赖 LLM（保持离线能力）

## 5. Key Types

```rust
// whisper_service.rs
pub struct WhisperService {
    ctx: Option<WhisperContext>,  // None before model loaded
    model_path: Option<PathBuf>,
    language: String,             // "zh" default
}

pub struct TranscriptionResult {
    pub text: String,
    pub segments: Vec<TranscriptionSegment>,
    pub confidence: f32,
    pub processing_time_ms: u64,
}

pub struct TranscriptionSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

pub struct AudioRecordingState {
    pub is_recording: bool,
    pub duration_ms: u64,
    pub buffer_size: usize,
}

// Tauri commands
// start_recording() → 开始麦克风录音
// stop_recording() → 停止录音 + 转写
// load_whisper_model(model_size: String) → 加载模型
// get_whisper_status() → 模型加载状态
```

## 6. Constraints

- Whisper 模型需用户首次下载（tiny ~75MB），存于 `~/.kingdee-kb/models/whisper/`
- 录音为 16kHz 单声道 PCM（Whisper 要求）
- VAD 能量阈值可配置（默认 0.01）
- 转写在独立线程，不阻塞 UI
- 中文后处理纯规则引擎，不依赖 LLM（保持离线）
- Windows 平台优先（WASAPI），cpal 提供跨平台抽象
