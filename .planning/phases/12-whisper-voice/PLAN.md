# Phase 12: Whisper 语音识别 — EXECUTION PLAN

**Phase:** 12
**Status:** IN PROGRESS
**Depends on:** Phase 11 (已完成)

---

## Task Breakdown

### T1: Whisper 服务核心 (`whisper_service.rs`)
**Files:** `src-tauri/src/services/whisper_service.rs` (NEW), `src-tauri/src/services/mod.rs` (EDIT), `src-tauri/Cargo.toml` (EDIT)

**Goal:** 封装 whisper-rs 为服务模块，提供模型加载、音频转写 API

**Steps:**
1. Add `whisper-rs` + `cpal` + `hound` to `Cargo.toml` dependencies
2. Create `whisper_service.rs` with:
   - `WhisperService` struct: holds `WhisperContext`, model path, language config
   - `load_model(model_dir, model_size)` → download if missing, init WhisperContext
   - `transcribe(pcm_data: &[f32], sample_rate)` → `TranscriptionResult`
   - `WhisperService::new()` → uninit state (no model loaded yet)
3. Register `pub mod whisper_service;` in `mod.rs`
4. Add `whisper_service: Arc<Mutex<WhisperService>>` to AppState

**Key API pattern:**
```rust
let params = WhisperContextParameters::default();
let ctx = WhisperContext::new_with_params(&model_path, params)?;
let state = ctx.create_state()?;
let mut c_params = FullParams::new(WhisperSamplingStrategy::Greedy);
c_params.set_language(Some("zh"));
state.full(&mut c_params, &pcm_f32)?;
let n = state.full_n_segments()?;
for i in 0..n { state.full_get_segment_text(i)?; }
```

**MUST DO:**
- Use `WhisperContextParameters::default()` for init
- Default language "zh" (Chinese)
- PCM data must be f32, 16kHz mono
- Model stored at `{app_data_dir}/models/whisper/ggml-{size}.bin`
- `hound` crate for WAV file read/write (intermediate storage)

**MUST NOT DO:**
- Don't bundle model file in app binary (too large)
- Don't block UI thread during transcription
- Don't use `log` crate (use `eprintln!` per project convention)

---

### T2: 麦克风录音 + VAD (`audio_capture.rs`)
**Files:** `src-tauri/src/services/audio_capture.rs` (NEW)

**Goal:** 使用 cpal 捕获麦克风输入，16kHz 单声道 PCM，含 VAD 端点检测

**Steps:**
1. Create `audio_capture.rs` with:
   - `AudioCapture` struct: holds cpal stream, recording state, audio buffer
   - `start_recording()` → init cpal stream, begin capture
   - `stop_recording()` → stop stream, return buffered PCM data
   - `is_recording() -> bool`
   - VAD: simple energy threshold (RMS > 0.01 = speech)
2. Resample to 16kHz mono if device provides different format
3. Register `pub mod audio_capture;` in `mod.rs`

**MUST DO:**
- Default input device selection
- Resample to 16kHz mono (Whisper requirement)
- VAD energy threshold configurable (default 0.01)
- Buffer PCM as `Vec<f32>` for Whisper input
- Thread-safe: `Arc<Mutex<AudioCaptureState>>`

**MUST NOT DO:**
- Don't hardcode audio device name
- Don't use blocking API in audio callback
- Don't assume 16kHz device availability (always resample)

---

### T3: 中文后处理 (`chinese_postprocess.rs`)
**Files:** `src-tauri/src/services/chinese_postprocess.rs` (NEW)

**Goal:** Whisper 中文输出后处理：标点恢复 + 短句合并 + 重复去除

**Steps:**
1. Create `chinese_postprocess.rs` with:
   - `postprocess_chinese(raw_text: &str) -> String`
   - Rule 1: 标点恢复 — 在句末添加句号/问号/感叹号（基于关键词匹配）
   - Rule 2: 短句合并 — 连续短句 (< 10 字) 合并为一段
   - Rule 3: 重复去除 — 相邻重复短语删除（"然后然后" → "然后"）
2. Register `pub mod chinese_postprocess;` in `mod.rs`

**MUST DO:**
- Pure rule engine, no LLM dependency (offline capable)
- Handle common Whisper Chinese artifacts: 重复词、缺失标点
- Return cleaned, punctuated text

**MUST NOT DO:**
- Don't call LLM for post-processing (must work offline)
- Don't remove valid repetitions (e.g., "是是是" in Chinese is emphatic)

---

### T4: Tauri Commands + 事件推送 (`lib.rs` edits)
**Files:** `src-tauri/src/lib.rs` (EDIT)

**Goal:** 4 个 Tauri 命令 + 1 个事件通道

**Steps:**
1. Add commands:
   - `load_whisper_model(model_size: String, state: State<AppState>)` → download + init model
   - `start_recording(state: State<AppState>)` → begin mic capture
   - `stop_recording(state: State<AppState>)` → stop + transcribe + postprocess + return result
   - `get_whisper_status(state: State<AppState>)` → model loaded? recording?
2. During recording, push `TranscriptionResult` via Tauri Event (`app.emit("whisper:transcript", result)`) for real-time display
3. Register all 4 commands in `invoke_handler`

**MUST DO:**
- `load_whisper_model` downloads model from HuggingFace if not cached locally
- `stop_recording` returns final `TranscriptionResult` with full text
- Real-time partial results via Tauri events (optional enhancement)
- Error handling: model not loaded, mic not available, etc.

**MUST NOT DO:**
- Don't block main thread during transcription
- Don't assume model is loaded (check state first)

---

### T5: 模型下载器 (`model_downloader.rs`)
**Files:** `src-tauri/src/services/model_downloader.rs` (NEW)

**Goal:** 从 HuggingFace 下载 ggml whisper 模型到本地

**Steps:**
1. Create `model_downloader.rs` with:
   - `ensure_model(model_dir: &Path, model_size: &str) -> Result<PathBuf>`
   - Supported sizes: "tiny" (75MB), "base" (142MB), "small" (466MB)
   - Download URL: `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{size}.bin`
   - Progress callback via Tauri Event
   - SHA256 checksum verification
2. Register `pub mod model_downloader;` in `mod.rs`

**MUST DO:**
- Check if model file exists before downloading
- Validate file size after download
- Store in `{app_data_dir}/models/whisper/`
- Report download progress via Tauri events

**MUST NOT DO:**
- Don't bundle model in binary
- Don't download on app startup (lazy load)
- Don't use unsafe HTTP (always HTTPS)

---

## Execution Order

```
T5 (model_downloader) ──┐
T1 (whisper_service)  ──┼── T4 (lib.rs commands)
T2 (audio_capture)    ──┤
T3 (chinese_postproc) ──┘
```

Wave 1 (parallel): T1, T2, T3, T5
Wave 2 (sequential): T4 (depends on T1+T2+T3+T5)

## Verification
- `cargo check` passes with 0 errors
- Model download path resolves correctly
- Whisper transcription API compiles with correct types
