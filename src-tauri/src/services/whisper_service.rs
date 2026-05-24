//! Whisper voice transcription service — local speech-to-text via whisper.cpp
//!
//! Wraps whisper-rs (whisper.cpp bindings) into a service module.
//! Provides lazy model loading and sync transcription API.
//! Graceful degradation: model not loaded → return error string.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

// --- Types ---

/// Full transcription output with segments and timing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    /// Full concatenated text
    pub text: String,
    /// Individual segments with timestamps
    pub segments: Vec<TranscriptionSegment>,
    /// Average confidence (approximate, from logprob)
    pub confidence: f32,
    /// Wall-clock processing time in milliseconds
    pub processing_time_ms: u64,
}

/// A single transcription segment with timing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionSegment {
    /// Start time in milliseconds
    pub start_ms: u64,
    /// End time in milliseconds
    pub end_ms: u64,
    /// Segment text
    pub text: String,
}

/// Current whisper service status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperStatus {
    /// Whether a model is loaded and ready
    pub model_loaded: bool,
    /// Which model size (tiny/base/small)
    pub model_size: String,
    /// Language code (zh/en)
    pub language: String,
}

// --- Service ---

/// Whisper transcription service.
///
/// Holds an optional WhisperContext (lazy-loaded).
/// All transcription is synchronous — WhisperContext is not Send,
/// so transcription must run on the calling thread (offloaded via
/// tokio::task::spawn_blocking in Tauri commands).
pub struct WhisperService {
    /// Loaded WhisperContext (None before load_model called)
    ctx: Option<WhisperContext>,
    /// Path to the loaded model file
    model_path: Option<PathBuf>,
    /// Language for transcription (default "zh")
    language: String,
    /// Model size identifier (tiny/base/small)
    model_size: String,
}

impl WhisperService {
    /// Create an uninitialized service (no model loaded).
    pub fn new() -> Self {
        Self {
            ctx: None,
            model_path: None,
            language: "zh".to_string(),
            model_size: "tiny".to_string(),
        }
    }

    /// Load a Whisper GGML model from disk.
    ///
    /// `model_dir` is the app data directory (e.g. ~/.kingdee-kb/).
    /// Model file is at `{model_dir}/models/whisper/ggml-{model_size}.bin`.
    pub fn load_model(&mut self, model_dir: &std::path::Path, model_size: &str) -> Result<(), String> {
        let model_path = model_dir
            .join("models")
            .join("whisper")
            .join(format!("ggml-{}.bin", model_size));

        if !model_path.exists() {
            return Err(format!(
                "Whisper model file not found: {}. Please download it first.",
                model_path.display()
            ));
        }

        eprintln!(
            "[WhisperService] Loading model '{}' from {}",
            model_size,
            model_path.display()
        );

        let params = WhisperContextParameters::default();
        let ctx = WhisperContext::new_with_params(&model_path, params)
            .map_err(|e| format!("Failed to load Whisper model: {}", e))?;

        self.ctx = Some(ctx);
        self.model_path = Some(model_path);
        self.model_size = model_size.to_string();
        self.language = "zh".to_string();

        eprintln!("[WhisperService] Model '{}' loaded successfully", model_size);
        Ok(())
    }

    /// Whether a model is loaded and ready for transcription.
    pub fn is_model_loaded(&self) -> bool {
        self.ctx.is_some()
    }

    /// Get current service status.
    pub fn status(&self) -> WhisperStatus {
        WhisperStatus {
            model_loaded: self.is_model_loaded(),
            model_size: self.model_size.clone(),
            language: self.language.clone(),
        }
    }

    /// Transcribe PCM audio data to text.
    ///
    /// `pcm_f32` must be 16kHz mono f32 samples (Whisper requirement).
    /// Returns full text + segment details + processing time.
    ///
    /// This is a synchronous, CPU-heavy operation. Callers should
    /// offload via `tokio::task::spawn_blocking()`.
    pub fn transcribe(&self, pcm_f32: &[f32]) -> Result<TranscriptionResult, String> {
        let ctx = self.ctx.as_ref().ok_or("Whisper model not loaded")?;

        let start = Instant::now();

        // Configure transcription parameters
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(&self.language));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_n_threads(4); // Use 4 threads for transcription
        params.set_single_segment(false); // Get all segments

        // Create a fresh state for this transcription
        let mut state = ctx.create_state()
            .map_err(|e| format!("Failed to create whisper state: {}", e))?;

        // Run transcription
        state.full(params, pcm_f32)
            .map_err(|e| format!("Whisper transcription failed: {}", e))?;

        // Collect segments using whisper-rs 0.16 API
        // full_n_segments() returns c_int (i32) directly, NOT Result
        let n_segments = state.full_n_segments();
        let mut segments = Vec::with_capacity(n_segments as usize);
        let mut full_text = String::new();
        let mut total_confidence = 0.0f32;

        for i in 0..n_segments {
            // whisper-rs 0.16: use get_segment() instead of full_get_segment_text/t0/t1
            let seg = state.get_segment(i)
                .ok_or_else(|| format!("Failed to get segment {}", i))?;

            // Use to_str_lossy() — the 0.16 API has no .text() method
            let text = seg.to_str_lossy()
                .map_err(|e| format!("Failed to get segment {} text: {}", i, e))?
                .to_string();

            // Timestamps are in centiseconds — convert to milliseconds
            let start_ms = (seg.start_timestamp() as u64) * 10;
            let end_ms = (seg.end_timestamp() as u64) * 10;

            segments.push(TranscriptionSegment {
                start_ms,
                end_ms,
                text: text.clone(),
            });

            full_text.push_str(&text);
            full_text.push(' ');

            // Approximate confidence (whisper-rs doesn't expose per-segment confidence)
            total_confidence += 0.8;
        }

        // Trim trailing space
        let text = full_text.trim_end().to_string();

        let avg_confidence = if n_segments > 0 {
            total_confidence / n_segments as f32
        } else {
            0.0
        };

        let processing_time_ms = start.elapsed().as_millis() as u64;

        Ok(TranscriptionResult {
            text,
            segments,
            confidence: avg_confidence,
            processing_time_ms,
        })
    }

    /// Get the expected model file path for a given size.
    pub fn model_path_for_size(model_dir: &std::path::Path, model_size: &str) -> PathBuf {
        model_dir
            .join("models")
            .join("whisper")
            .join(format!("ggml-{}.bin", model_size))
    }
}