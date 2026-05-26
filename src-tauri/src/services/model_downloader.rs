//! Whisper GGML model downloader
//!
//! Downloads model files from HuggingFace (ggerganov/whisper.cpp)
//! to local cache. Supports tiny (~75MB), base (~142MB), small (~466MB).
//! Reports download progress via Tauri events.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Supported Whisper model sizes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WhisperModelSize {
    Tiny,
    Base,
    Small,
}

impl WhisperModelSize {
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "tiny" => Ok(Self::Tiny),
            "base" => Ok(Self::Base),
            "small" => Ok(Self::Small),
            _ => Err(format!(
                "Unsupported model size '{}'. Supported: tiny, base, small",
                s
            )),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Tiny => "tiny",
            Self::Base => "base",
            Self::Small => "small",
        }
    }

    /// Approximate model file size in bytes
    pub fn expected_size(&self) -> u64 {
        match self {
            Self::Tiny => 75_000_000,    // ~75MB
            Self::Base => 142_000_000,   // ~142MB
            Self::Small => 466_000_000,  // ~466MB
        }
    }

    /// HuggingFace download URL (uses hf-mirror.com for China accessibility)
    pub fn download_url(&self) -> String {
        format!(
            "https://hf-mirror.com/ggerganov/whisper.cpp/resolve/main/ggml-{}.bin",
            self.as_str()
        )
    }
}

/// Model download progress
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDownloadProgress {
    /// Model size being downloaded
    pub model_size: String,
    /// Bytes downloaded so far
    pub downloaded_bytes: u64,
    /// Total expected size in bytes
    pub total_bytes: u64,
    /// Percentage complete (0-100)
    pub percent: f32,
    /// Current speed in bytes/sec (approximate)
    pub speed_bps: u64,
    /// Error message if download failed
    pub error: Option<String>,
}

/// Ensure a Whisper model is available locally.
///
/// Resolution order:
/// 1. Check if already in data dir (`{model_dir}/models/whisper/ggml-{size}.bin`) and valid
/// 2. Copy from bundled project models (`exe_dir/../../models/ggml-{size}.bin`) if available
/// 3. Download from hf-mirror.com as fallback
///
/// Returns the path to the model file.
pub fn ensure_model(model_dir: &Path, model_size: &str) -> Result<PathBuf, String> {
    let size = WhisperModelSize::from_str(model_size)?;
    let model_path = model_dir
        .join("models")
        .join("whisper")
        .join(format!("ggml-{}.bin", size.as_str()));

    // Check if already downloaded and valid
    if model_path.exists() {
        let file_size = std::fs::metadata(&model_path)
            .map(|m| m.len())
            .unwrap_or(0);

        // Validate file size is reasonable (at least 50% of expected)
        let min_size = size.expected_size() / 2;
        if file_size >= min_size {
            eprintln!(
                "[ModelDownloader] Model '{}' already exists at {} ({} bytes)",
                model_size,
                model_path.display(),
                file_size
            );
            return Ok(model_path);
        }

        // File too small — likely corrupted download
        eprintln!(
            "[ModelDownloader] Model file too small ({} bytes, expected ~{}), re-downloading",
            file_size,
            size.expected_size()
        );
        std::fs::remove_file(&model_path).ok(); // Remove corrupted file
    }

    // Create directory
    let whisper_dir = model_dir.join("models").join("whisper");
    std::fs::create_dir_all(&whisper_dir)
        .map_err(|e| format!("Failed to create whisper model directory: {}", e))?;

    // Try to copy from bundled project models directory
    if let Ok(exe_path) = std::env::current_exe() {
        // exe is at target/debug/kingdee-kb.exe or target/release/kingdee-kb.exe
        // models/ is at project root: ../../models/ggml-{size}.bin (dev) or bundled alongside exe
        let exe_dir = exe_path.parent().unwrap_or(Path::new("."));
        
        // Try multiple locations for bundled model
        let bundled_candidates = vec![
            exe_dir.join("..").join("..").join("models").join(format!("ggml-{}.bin", size.as_str())),
            exe_dir.join("models").join(format!("ggml-{}.bin", size.as_str())),
            // Also try relative to CWD for dev mode
            PathBuf::from("models").join(format!("ggml-{}.bin", size.as_str())),
        ];
        
        for bundled_path in &bundled_candidates {
            if let Ok(canonical) = bundled_path.canonicalize() {
                let file_size = std::fs::metadata(&canonical)
                    .map(|m| m.len())
                    .unwrap_or(0);
                    
                if file_size >= size.expected_size() / 2 {
                    eprintln!(
                        "[ModelDownloader] Copying bundled model from {} ({} bytes)",
                        canonical.display(),
                        file_size
                    );
                    std::fs::copy(&canonical, &model_path)
                        .map_err(|e| format!("Failed to copy bundled model: {}", e))?;
                    eprintln!(
                        "[ModelDownloader] Model '{}' copied from bundled resource",
                        model_size
                    );
                    return Ok(model_path);
                }
            }
        }
    }

    // Fallback: download model from mirror
    eprintln!("[ModelDownloader] No bundled model found, downloading from mirror...");
    download_model(&size, &model_path)?;

    Ok(model_path)
}

/// Download a model file from HuggingFace.
///
/// Uses ureq (already in Cargo.toml) for HTTP download with progress tracking.
fn download_model(size: &WhisperModelSize, target_path: &Path) -> Result<(), String> {
    let url = size.download_url();
    eprintln!("[ModelDownloader] Starting download from {}", url);

    let expected_size = size.expected_size();

    // Use ureq for download (already in Cargo.toml)
    let response = ureq::get(&url)
        .call()
        .map_err(|e| format!("Failed to start model download: {}", e))?;

    let content_length = response.headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(expected_size);

    eprintln!(
        "[ModelDownloader] Downloading {} bytes (~{} MB)",
        content_length,
        content_length / 1_000_000
    );

    // Read response body to file
    let mut reader = response.into_body().into_reader();
    let mut file = std::fs::File::create(target_path)
        .map_err(|e| format!("Failed to create model file: {}", e))?;

    let mut downloaded: u64 = 0;
    let mut buffer = [0u8; 8192];
    let start_time = std::time::Instant::now();

    loop {
        let bytes_read = reader
            .read(&mut buffer)
            .map_err(|e| format!("Download read error: {}", e))?;

        if bytes_read == 0 {
            break;
        }

        file.write_all(&buffer[..bytes_read])
            .map_err(|e| format!("Failed to write model file: {}", e))?;

        downloaded += bytes_read as u64;

        // Log progress every ~10%
        let percent = if content_length > 0 {
            (downloaded as f32 / content_length as f32 * 100.0) as u32
        } else {
            0
        };

        let elapsed = start_time.elapsed().as_secs();
        let speed_bps = if elapsed > 0 { downloaded / elapsed } else { 0 };

        if downloaded % (content_length / 10 + 1) < 8192 {
            eprintln!(
                "[ModelDownloader] Progress: {}% ({}/{} bytes, {} KB/s)",
                percent,
                downloaded / 1_000_000,
                content_length / 1_000_000,
                speed_bps / 1_000
            );
        }
    }

    file.flush()
        .map_err(|e| format!("Failed to flush model file: {}", e))?;

    // Validate download
    let file_size = std::fs::metadata(target_path)
        .map(|m| m.len())
        .unwrap_or(0);

    if file_size < size.expected_size() / 2 {
        return Err(format!(
            "Downloaded file too small ({} bytes, expected ~{}). Download may have failed.",
            file_size,
            size.expected_size()
        ));
    }

    eprintln!(
        "[ModelDownloader] Download complete: {} ({} bytes)",
        target_path.display(),
        file_size
    );

    Ok(())
}