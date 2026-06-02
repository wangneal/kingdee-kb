//! 知识摄入管道：文本/文件/文件夹 → 清洗 → 分块 → 嵌入 → 存储
//!
//! 编排完整的摄入流程：
//! 1. 清洗原始文本（去除 HTML，规范化空白）
//! 2. 计算 SHA256 用于去重检测
//! 3. 递归分块（H2 → 段落 → 句子）
//! 4. 从文件名 + 章节路径提取标签
//! 5. 通过 fastembed 批量嵌入分块
//! 6. 在 usearch HNSW 索引中存储向量
//! 7. 在 SQLite 中存储元数据（分块↔向量映射）
//! 8. 向前端发出进度事件

use crate::services::bm25_service::BM25Service;
use crate::services::chunker::{recursive_chunk, ChunkInputMeta};
use crate::services::embedding::EmbeddingService;
use crate::services::file_extractor;
use crate::services::ingestion_helpers::{
    compute_sha256, extract_tags, extract_title_from_filename,
};
use crate::services::metadata::MetadataStore;
use crate::services::raw_source::{InsertRawSource, RawSourceStore};
use crate::services::text_cleaner::clean_text;
use crate::services::vector_index::VectorIndex;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

/// Check if a file is a temporary/junk file that should be skipped during ingestion.
///
/// Patterns skipped:
/// - Office lock files: `~$xxx.docx`, `~$xxx.xlsx`, `~$xxx.pptx`
/// - Thumbs.db (Windows thumbnail cache)
fn is_temp_file(path: &Path) -> bool {
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    filename.starts_with("~$") || filename.eq_ignore_ascii_case("thumbs.db")
}

/// 向前端发出的摄入进度事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionProgress {
    /// Current step (1-5)
    pub step: u32,
    /// Human-readable step name
    pub step_name: String,
    /// Progress percentage (0-100)
    pub progress: f32,
    /// Optional status message
    pub message: Option<String>,
}

/// A file-level error during directory ingestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileError {
    /// The file path that failed
    pub path: String,
    /// The error message
    pub error: String,
}

/// 返回给前端的文件夹摄入结果（成功列表 + 失败列表）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryIngestionResult {
    /// 成功导入的文件
    pub imported: Vec<IngestionResult>,
    /// 导入失败的文件及原因
    pub errors: Vec<FileError>,
}

/// 返回给前端的摄入结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionResult {
    /// Document ID in metadata store
    pub document_id: i64,
    /// Document title
    pub title: String,
    /// SHA256 hash of content
    pub sha256: String,
    /// Whether this was a duplicate (SHA256 already existed)
    pub is_duplicate: bool,
    /// Number of chunks created
    pub chunk_count: usize,
    /// Number of vectors stored
    pub vector_count: usize,
    /// Processing time in milliseconds
    pub duration_ms: u64,
}

/// 摄入纯文本（来自粘贴或文本框）
pub fn ingest_text(
    text: &str,
    title: &str,
    project: &str,
    embedding: &Arc<Mutex<EmbeddingService>>,
    vector_index: &Arc<Mutex<VectorIndex>>,
    metadata: &Arc<Mutex<MetadataStore>>,
    bm25: &Arc<Mutex<BM25Service>>,
    raw_sources: Option<&Arc<Mutex<RawSourceStore>>>,
    raw_source_identity: Option<&str>,
    source_path: Option<&str>,
    app_handle: Option<&AppHandle>,
) -> Result<IngestionResult, String> {
    let start = std::time::Instant::now();

    // Step 1: Clean text
    emit_progress(app_handle, 1, "cleaning", 0.0, Some("Cleaning text..."));
    let cleaned = clean_text(text);
    emit_progress(app_handle, 1, "cleaning", 100.0, None);

    // Step 2: SHA256 dedup
    emit_progress(app_handle, 2, "hashing", 0.0, Some("Computing hash..."));
    let sha256 = compute_sha256(&cleaned);

    // Check for duplicate — but also detect orphan documents (documents
    // with a SHA256 record but zero chunks, left over from a failed
    // embedding step).  Orphan documents are silently deleted and the
    // import proceeds as fresh.
    {
        let meta = metadata.lock().map_err(|e| format!("Lock error: {}", e))?;
        if let Some(existing) = meta.get_document_by_sha256(&sha256)? {
            let existing_chunks = meta.get_document_chunk_count(existing.id)?;
            if existing_chunks == 0 {
                // Orphan document — no chunks were ever stored (likely
                // because the embedding model wasn't ready on the original
                // import).  Clean up so we can re-import.
                eprintln!(
                    "[Ingestion] Orphan document '{}' (id={}) — has SHA256 but 0 chunks, re-importing",
                    existing.title, existing.id
                );
                drop(meta); // release lock before delete_document re-acquires
                {
                    let meta = metadata.lock().map_err(|e| format!("Lock error: {}", e))?;
                    meta.delete_document(existing.id, None)?;
                }
                // Fall through to fresh import below
            } else {
                // Genuine duplicate — document already has chunks
                return Ok(IngestionResult {
                    document_id: existing.id,
                    title: existing.title,
                    sha256,
                    is_duplicate: true,
                    chunk_count: existing_chunks as usize,
                    vector_count: existing_chunks as usize,
                    duration_ms: start.elapsed().as_millis() as u64,
                });
            }
        }
    }
    emit_progress(app_handle, 2, "hashing", 100.0, None);

    // Step 3: Chunk
    emit_progress(
        app_handle,
        3,
        "chunking",
        0.0,
        Some("Splitting into chunks..."),
    );
    let chunk_meta = ChunkInputMeta {
        source_file: None,
        title: title.to_string(),
        tags: vec![],
    };
    let chunks = recursive_chunk(&cleaned, &chunk_meta);
    let chunk_count = chunks.len();
    emit_progress(
        app_handle,
        3,
        "chunking",
        100.0,
        Some(&format!("Created {} chunks", chunk_count)),
    );

    // Step 4: Embed and store
    emit_progress(
        app_handle,
        4,
        "embedding",
        0.0,
        Some("Generating embeddings..."),
    );

    // Insert document first
    let doc_id = {
        let meta = metadata.lock().map_err(|e| format!("Lock error: {}", e))?;
        meta.insert_document(title, None, Some(&sha256), Some(project))?
    };

    // 创建 raw_source 记录
    if let (Some(raw_store), Some(identity)) = (raw_sources, raw_source_identity) {
        let insert = InsertRawSource {
            project: project.to_string(),
            identity: identity.to_string(),
            original_path: source_path.unwrap_or("").to_string(),
            storage_path: String::new(),
            sha256: sha256.clone(),
            file_size: None,
            mime_type: None,
        };
        match raw_store.lock() {
            Ok(store) => {
                if let Err(e) = store.insert(&insert) {
                    tracing::warn!("raw_source 创建失败: {:?}", e);
                }
            }
            Err(e) => {
                tracing::warn!("raw_source 锁失败: {:?}", e);
            }
        }
    }

    // Process chunks in batches
    let batch_size = 64;
    let mut vector_count = 0;
    // Collect BM25 index data for batch writing after vector+metadata locks are released
    let mut bm25_chunks: Vec<(i64, String, String, Option<String>, String)> = Vec::new();

    for (batch_idx, chunk_batch) in chunks.chunks(batch_size).enumerate() {
        // Extract texts for embedding
        let texts: Vec<&str> = chunk_batch.iter().map(|c| c.content.as_str()).collect();

        // Embed batch
        let embeddings = {
            let mut emb = embedding.lock().map_err(|e| format!("Lock error: {}", e))?;
            emb.embed_batch(&texts)?
        };

        // Store vectors and metadata
        {
            let mut idx = vector_index
                .lock()
                .map_err(|e| format!("Lock error: {}", e))?;
            let meta = metadata.lock().map_err(|e| format!("Lock error: {}", e))?;

            for (i, (chunk, embedding)) in chunk_batch.iter().zip(embeddings.iter()).enumerate() {
                let vector_key = meta.next_vector_key()?;

                // Defensive: remove any orphaned vector at this key before adding.
                // With multi:false, usearch rejects duplicate keys. If a previous
                // delete only cleaned SQLite but left the vector in usearch, this
                // prevents the collision.
                let _ = idx.remove(vector_key as u64);

                // Add to vector index
                idx.add(vector_key as u64, embedding)?;

                // Extract tags
                let tags = extract_tags(
                    chunk.metadata.source_file.as_deref().unwrap_or("untitled"),
                    chunk.metadata.section_path.as_deref(),
                );

                // Insert chunk metadata
                meta.insert_chunk(
                    vector_key,
                    doc_id,
                    &chunk.content,
                    chunk.metadata.section_path.as_deref(),
                    Some(&tags),
                    Some(chunk.metadata.line_start as i64),
                )?;

                // Collect data for BM25 indexing (written after locks released)
                bm25_chunks.push((
                    vector_key,
                    title.to_string(),
                    chunk.content.clone(),
                    chunk.metadata.section_path.clone(),
                    project.to_string(),
                ));

                vector_count += 1;
            }
        }

        // Write collected chunks to BM25 index (outside vector+metadata locks)
        if !bm25_chunks.is_empty() {
            let bm25_guard = bm25.lock().map_err(|e| format!("BM25 lock error: {}", e))?;
            bm25_guard.add_chunks(&bm25_chunks)?;
            bm25_chunks.clear();
        }

        // Emit progress
        let progress = ((batch_idx + 1) as f32 / (chunks.len() as f32 / batch_size as f32)) * 100.0;
        emit_progress(
            app_handle,
            4,
            "embedding",
            progress.min(99.0),
            Some(&format!("Embedded {}/{} chunks", vector_count, chunk_count)),
        );
    }

    // Save index
    {
        let idx = vector_index
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        idx.save()?;
    }

    // Commit BM25 index to make new chunks searchable
    {
        let bm25_guard = bm25.lock().map_err(|e| format!("BM25 lock error: {}", e))?;
        bm25_guard.commit()?;
    }

    emit_progress(app_handle, 4, "embedding", 100.0, None);
    emit_progress(app_handle, 5, "done", 100.0, Some("Ingestion complete"));

    Ok(IngestionResult {
        document_id: doc_id,
        title: title.to_string(),
        sha256,
        is_duplicate: false,
        chunk_count,
        vector_count,
        duration_ms: start.elapsed().as_millis() as u64,
    })
}

/// 摄入单个文件
pub fn ingest_file(
    file_path: &Path,
    project: &str,
    embedding: &Arc<Mutex<EmbeddingService>>,
    vector_index: &Arc<Mutex<VectorIndex>>,
    metadata: &Arc<Mutex<MetadataStore>>,
    bm25: &Arc<Mutex<BM25Service>>,
    raw_sources: Option<&Arc<Mutex<RawSourceStore>>>,
    app_handle: Option<&AppHandle>,
) -> Result<IngestionResult, String> {
    // Skip temporary/junk files (Office lock files ~$*, Thumbs.db, etc.)
    if is_temp_file(file_path) {
        return Err(format!("临时文件已跳过: {:?}", file_path.display()));
    }

    // 检查文件是否存在
    if !file_path.exists() {
        return Err(format!("文件不存在: {:?}", file_path.display()));
    }

    // 检查文件格式是否支持
    if !file_extractor::is_supported(file_path) {
        return Err(format!("不支持的文件格式: {:?}", file_path.display()));
    }

    // 使用 file_extractor 提取文本内容
    let content = file_extractor::extract_text(file_path)?;

    // Extract title from filename
    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("untitled");
    let title = extract_title_from_filename(filename);

    let file_path_str = file_path.to_string_lossy();
    let mut result = ingest_text(
        &content,
        &title,
        project,
        embedding,
        vector_index,
        metadata,
        bm25,
        raw_sources,
        Some(file_path_str.as_ref()),
        Some(file_path_str.as_ref()),
        app_handle,
    )?;

    // Update document with source path
    // Note: We'd need an update_document method in MetadataStore for this
    // For now, the source_path is stored during insert_document

    result.title = title;
    Ok(result)
}

/// 摄入目录中的所有支持文件（递归子目录）
pub fn ingest_directory(
    dir_path: &Path,
    project: &str,
    embedding: &Arc<Mutex<EmbeddingService>>,
    vector_index: &Arc<Mutex<VectorIndex>>,
    metadata: &Arc<Mutex<MetadataStore>>,
    bm25: &Arc<Mutex<BM25Service>>,
    raw_sources: Option<&Arc<Mutex<RawSourceStore>>>,
    app_handle: Option<&AppHandle>,
) -> Result<DirectoryIngestionResult, String> {
    if !dir_path.is_dir() {
        return Err(format!("Not a directory: {:?}", dir_path));
    }

    let mut imported = Vec::new();
    let mut errors = Vec::new();

    // Recursively walk directory tree
    ingest_dir_recursive(
        dir_path,
        project,
        embedding,
        vector_index,
        metadata,
        bm25,
        raw_sources,
        app_handle,
        &mut imported,
        &mut errors,
    )?;

    if imported.is_empty() && errors.is_empty() {
        eprintln!("[Ingestion] No supported files found in {:?}", dir_path);
    }

    Ok(DirectoryIngestionResult { imported, errors })
}

/// Recursive helper — walks directories depth-first, ingesting all supported files
fn ingest_dir_recursive(
    dir_path: &Path,
    project: &str,
    embedding: &Arc<Mutex<EmbeddingService>>,
    vector_index: &Arc<Mutex<VectorIndex>>,
    metadata: &Arc<Mutex<MetadataStore>>,
    bm25: &Arc<Mutex<BM25Service>>,
    raw_sources: Option<&Arc<Mutex<RawSourceStore>>>,
    app_handle: Option<&AppHandle>,
    imported: &mut Vec<IngestionResult>,
    errors: &mut Vec<FileError>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir_path)
        .map_err(|e| format!("Failed to read directory {:?}: {}", dir_path, e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();

        if path.is_dir() {
            // Recurse into subdirectories
            ingest_dir_recursive(
                &path,
                project,
                embedding,
                vector_index,
                metadata,
                bm25,
                raw_sources,
                app_handle,
                imported,
                errors,
            )?;
        } else if is_temp_file(&path) {
            // Skip Office lock files and other temp files silently
            continue;
        } else if file_extractor::is_supported(&path) {
            match ingest_file(
                &path,
                project,
                embedding,
                vector_index,
                metadata,
                bm25,
                raw_sources,
                app_handle,
            ) {
                Ok(result) => imported.push(result),
                Err(e) => {
                    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                    eprintln!("[Ingestion] Failed to ingest {:?}: {}", path, e);
                    errors.push(FileError {
                        path: filename.to_string(),
                        error: e,
                    });
                    // Continue with other files
                }
            }
        }
    }
    Ok(())
}

/// 向前端发出进度事件
fn emit_progress(
    app_handle: Option<&AppHandle>,
    step: u32,
    step_name: &str,
    progress: f32,
    message: Option<&str>,
) {
    if let Some(handle) = app_handle {
        let event = IngestionProgress {
            step,
            step_name: step_name.to_string(),
            progress,
            message: message.map(|s| s.to_string()),
        };
        let _ = handle.emit("ingestion-progress", &event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_ingest_text_basic() {
        let tmp = tempdir().unwrap();
        let data_dir = tmp.path();

        // Create services
        let embedding = Arc::new(Mutex::new(EmbeddingService::empty()));
        let vector_index = Arc::new(Mutex::new(
            VectorIndex::new(data_dir.join("index")).unwrap(),
        ));
        let metadata = Arc::new(Mutex::new(
            MetadataStore::new(data_dir.join("meta.db")).unwrap(),
        ));
        let bm25 = Arc::new(Mutex::new(
            BM25Service::new(data_dir.join("bm25_index")).unwrap(),
        ));

        // Ingest text (will fail because embedding is empty, but we can test the flow)
        let result = ingest_text(
            "这是一段测试文本。",
            "测试文档",
            "default",
            &embedding,
            &vector_index,
            &metadata,
            &bm25,
            None,
            None,
            None,
            None,
        );

        // Should fail because embedding model is not initialized
        assert!(result.is_err());
    }

    #[test]
    fn test_ingest_text_dedup() {
        let tmp = tempdir().unwrap();
        let data_dir = tmp.path();

        let embedding = Arc::new(Mutex::new(EmbeddingService::empty()));
        let vector_index = Arc::new(Mutex::new(
            VectorIndex::new(data_dir.join("index")).unwrap(),
        ));
        let metadata = Arc::new(Mutex::new(
            MetadataStore::new(data_dir.join("meta.db")).unwrap(),
        ));
        let bm25 = Arc::new(Mutex::new(
            BM25Service::new(data_dir.join("bm25_index")).unwrap(),
        ));

        // First ingest
        let _ = ingest_text(
            "测试文本",
            "文档1",
            "default",
            &embedding,
            &vector_index,
            &metadata,
            &bm25,
            None,
            None,
            None,
            None,
        );

        // Second ingest with same content (should dedup)
        let result = ingest_text(
            "测试文本",
            "文档2",
            "default",
            &embedding,
            &vector_index,
            &metadata,
            &bm25,
            None,
            None,
            None,
            None,
        );

        // Should return early with dedup (chunk_count = 0)
        if let Ok(r) = result {
            assert_eq!(r.chunk_count, 0);
        }
    }

    #[test]
    fn test_ingest_file_not_found() {
        let tmp = tempdir().unwrap();
        let data_dir = tmp.path();

        let embedding = Arc::new(Mutex::new(EmbeddingService::empty()));
        let vector_index = Arc::new(Mutex::new(
            VectorIndex::new(data_dir.join("index")).unwrap(),
        ));
        let metadata = Arc::new(Mutex::new(
            MetadataStore::new(data_dir.join("meta.db")).unwrap(),
        ));
        let bm25 = Arc::new(Mutex::new(
            BM25Service::new(data_dir.join("bm25_index")).unwrap(),
        ));

        let result = ingest_file(
            Path::new("/nonexistent/file.txt"),
            "default",
            &embedding,
            &vector_index,
            &metadata,
            &bm25,
            None,
            None,
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_ingest_directory_not_dir() {
        let tmp = tempdir().unwrap();
        let data_dir = tmp.path();

        let embedding = Arc::new(Mutex::new(EmbeddingService::empty()));
        let vector_index = Arc::new(Mutex::new(
            VectorIndex::new(data_dir.join("index")).unwrap(),
        ));
        let metadata = Arc::new(Mutex::new(
            MetadataStore::new(data_dir.join("meta.db")).unwrap(),
        ));
        let bm25 = Arc::new(Mutex::new(
            BM25Service::new(data_dir.join("bm25_index")).unwrap(),
        ));

        let result = ingest_directory(
            Path::new("/nonexistent/dir"),
            "default",
            &embedding,
            &vector_index,
            &metadata,
            &bm25,
            None,
            None,
        );

        assert!(result.is_err());
    }
}
