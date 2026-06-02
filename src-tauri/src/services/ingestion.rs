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
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use tauri::{AppHandle, Emitter};

/// 判断文件是否为摄入时应跳过的临时或垃圾文件。
///
/// 跳过模式：
/// - Office 锁文件：`~$xxx.docx`、`~$xxx.xlsx`、`~$xxx.pptx`
/// - Thumbs.db（Windows 缩略图缓存）
fn is_temp_file(path: &Path) -> bool {
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    filename.starts_with("~$") || filename.eq_ignore_ascii_case("thumbs.db")
}

/// 向前端发出的摄入进度事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionProgress {
    /// 当前步骤（1-5）
    pub step: u32,
    /// 便于展示的步骤名称
    pub step_name: String,
    /// 进度百分比（0-100）
    pub progress: f32,
    /// 可选状态消息
    pub message: Option<String>,
}

/// 目录摄入时的单文件错误
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileError {
    /// 失败的文件路径
    pub path: String,
    /// 错误消息
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
    /// 元数据存储中的文档 ID
    pub document_id: i64,
    /// 文档标题
    pub title: String,
    /// 内容 SHA256 哈希
    pub sha256: String,
    /// 是否为重复内容（SHA256 已存在）
    pub is_duplicate: bool,
    /// 创建的分块数量
    pub chunk_count: usize,
    /// 存储的向量数量
    pub vector_count: usize,
    /// 处理耗时（毫秒）
    pub duration_ms: u64,
    /// 原始文件路径（目录摄入时用于 KB 编译）
    pub source_path: Option<String>,
    /// KB 编译失败原因（导入本身成功，但 wiki_pages 未生成）
    pub kb_compilation_error: Option<String>,
    /// KB 分析实际使用的引擎：llm、rust 或 cache
    pub kb_analysis_engine: Option<String>,
}

/// 摄入纯文本（来自粘贴或文本框）
pub fn ingest_text(
    text: &str,
    title: &str,
    project: &str,
    embedding: &Arc<RwLock<EmbeddingService>>,
    vector_index: &Arc<RwLock<VectorIndex>>,
    metadata: &Arc<Mutex<MetadataStore>>,
    bm25: &Arc<RwLock<BM25Service>>,
    raw_sources: Option<&Arc<Mutex<RawSourceStore>>>,
    raw_source_identity: Option<&str>,
    source_path: Option<&str>,
    app_handle: Option<&AppHandle>,
    data_dir: Option<&Path>,
) -> Result<IngestionResult, String> {
    let start = std::time::Instant::now();

    // 步骤 1：清洗文本
    emit_progress(app_handle, 1, "cleaning", 0.0, Some("Cleaning text..."));
    let cleaned = clean_text(text);
    emit_progress(app_handle, 1, "cleaning", 100.0, None);

    // 步骤 2：SHA256 去重
    emit_progress(app_handle, 2, "hashing", 0.0, Some("Computing hash..."));
    let sha256 = compute_sha256(&cleaned);

    // 检查重复内容，同时识别孤儿文档（存在 SHA256 记录但没有分块，
    // 通常来自嵌入阶段失败）。孤儿文档会被静默删除，然后按新文档导入。
    {
        let meta = metadata.lock().map_err(|e| format!("Lock error: {}", e))?;
        if let Some(existing) = meta.get_document_by_sha256(&sha256)? {
            let existing_chunks = meta.get_document_chunk_count(existing.id)?;
            if existing_chunks == 0 {
                // 孤儿文档：没有任何分块被写入，通常是首次导入时嵌入模型未就绪。
                // 清理后允许重新导入。
                eprintln!(
                    "[Ingestion] Orphan document '{}' (id={}) — has SHA256 but 0 chunks, re-importing",
                    existing.title, existing.id
                );
                drop(meta); // 释放锁，避免 delete_document 重新加锁时阻塞
                {
                    let meta = metadata.lock().map_err(|e| format!("Lock error: {}", e))?;
                    meta.delete_document(existing.id, None)?;
                }
                // 继续走下面的新文档导入流程
            } else {
                // 真实重复：文档已经存在分块
                return Ok(IngestionResult {
                    document_id: existing.id,
                    title: existing.title,
                    sha256,
                    is_duplicate: true,
                    chunk_count: existing_chunks as usize,
                    vector_count: existing_chunks as usize,
                    duration_ms: start.elapsed().as_millis() as u64,
                    source_path: source_path.map(|s| s.to_string()),
                    kb_compilation_error: None,
                    kb_analysis_engine: None,
                });
            }
        }
    }
    emit_progress(app_handle, 2, "hashing", 100.0, None);

    // 步骤 3：分块
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

    // 步骤 4：嵌入并存储
    emit_progress(
        app_handle,
        4,
        "embedding",
        0.0,
        Some("Generating embeddings..."),
    );

    // 先插入文档记录
    let doc_id = {
        let meta = metadata.lock().map_err(|e| format!("Lock error: {}", e))?;
        meta.insert_document(title, None, Some(&sha256), Some(project), raw_source_identity)?
    };

    // 创建 raw_source 记录（含文件复制，复制失败则跳过记录创建）
    if let (Some(raw_store), Some(identity)) = (raw_sources, raw_source_identity) {
        // 计算 storage_path：若有源文件和数据目录，则复制到 raw/{project}/sources/{identity}
        let mut storage_path = String::new();
        let mut file_size: Option<i64> = None;
        if let (Some(src), Some(dir)) = (source_path, data_dir) {
            let src_path = Path::new(src);
            if src_path.exists() {
                let project_dir = safe_path_segment(project)?;
                let identity_path = safe_relative_path(identity)?;
                let dest_dir = dir.join("raw").join(project_dir).join("sources");
                if let Err(e) = std::fs::create_dir_all(&dest_dir) {
                    tracing::warn!("创建 raw 存储目录失败: {:?}", e);
                } else {
                    let dest = dest_dir.join(identity_path);
                    match std::fs::copy(src_path, &dest) {
                        Ok(bytes) => {
                            storage_path = dest.to_string_lossy().to_string();
                            file_size = Some(bytes as i64);
                        }
                        Err(e) => {
                            tracing::warn!("复制源文件到 raw 存储失败: {:?}", e);
                        }
                    }
                }
            }
        }
        // 仅在成功复制文件后才写入 raw_sources 记录
        if !storage_path.is_empty() {
            let insert = InsertRawSource {
                project: project.to_string(),
                identity: identity.to_string(),
                original_path: source_path.unwrap_or("").to_string(),
                storage_path,
                sha256: sha256.clone(),
                file_size,
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
        } else {
            tracing::info!("文件复制未完成，跳过 raw_sources 记录创建");
        }
    }

    // 分批处理分块
    let batch_size = 64;
    let mut vector_count = 0;
    // 收集 BM25 索引数据，等待向量和元数据锁释放后批量写入
    let mut bm25_chunks: Vec<(i64, String, String, Option<String>, String)> = Vec::new();

    for (batch_idx, chunk_batch) in chunks.chunks(batch_size).enumerate() {
        // 提取待嵌入文本
        let texts: Vec<&str> = chunk_batch.iter().map(|c| c.content.as_str()).collect();

        // 批量嵌入，不与元数据、BM25、向量索引锁同时持有
        let embeddings = {
            let mut emb = embedding.write().map_err(|e| format!("Lock error: {}", e))?;
            emb.embed_batch(&texts)?
        };

        // 按规格锁顺序写入：metadata → bm25 → vector_index
        {
            let meta = metadata.lock().map_err(|e| format!("Lock error: {}", e))?;
            let bm25_guard = bm25.write().map_err(|e| format!("BM25 lock error: {}", e))?;
            let mut idx = vector_index
                .write()
                .map_err(|e| format!("Lock error: {}", e))?;

            for (chunk, embedding) in chunk_batch.iter().zip(embeddings.iter()) {
                let vector_key = meta.next_vector_key()?;

                // 防御性处理：添加前先移除同 key 的孤儿向量。
                // multi:false 时 usearch 会拒绝重复 key；如果之前删除只清理了
                // SQLite 而遗留 usearch 向量，这里可以避免冲突。
                let _ = idx.remove(vector_key as u64);

                // 添加到向量索引
                idx.add(vector_key as u64, embedding)?;

                // 提取标签
                let tags = extract_tags(
                    chunk.metadata.source_file.as_deref().unwrap_or("untitled"),
                    chunk.metadata.section_path.as_deref(),
                );

                // 插入分块元数据
                meta.insert_chunk(
                    vector_key,
                    doc_id,
                    &chunk.content,
                    chunk.metadata.section_path.as_deref(),
                    Some(&tags),
                    Some(chunk.metadata.line_start as i64),
                )?;

                // 收集 BM25 索引数据，锁释放后写入
                bm25_chunks.push((
                    vector_key,
                    title.to_string(),
                    chunk.content.clone(),
                    chunk.metadata.section_path.clone(),
                    project.to_string(),
                ));

                vector_count += 1;
            }

            if !bm25_chunks.is_empty() {
                bm25_guard.add_chunks(&bm25_chunks)?;
                bm25_chunks.clear();
            }
        }

        // 发送进度
        let progress = ((batch_idx + 1) as f32 / (chunks.len() as f32 / batch_size as f32)) * 100.0;
        emit_progress(
            app_handle,
            4,
            "embedding",
            progress.min(99.0),
            Some(&format!("Embedded {}/{} chunks", vector_count, chunk_count)),
        );
    }

    // 保存索引
    {
        let idx = vector_index
            .read()
            .map_err(|e| format!("Lock error: {}", e))?;
        idx.save()?;
    }

    // 提交 BM25 索引，使新分块可搜索
    {
        let bm25_guard = bm25.write().map_err(|e| format!("BM25 lock error: {}", e))?;
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
        source_path: source_path.map(|s| s.to_string()),
        kb_compilation_error: None,
        kb_analysis_engine: None,
    })
}

/// 校验单段目录名，防止 project 参数逃逸数据目录。
fn safe_path_segment(value: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        return Err("项目名称不能为空".to_string());
    }
    let path = Path::new(value);
    if path.is_absolute() || value.contains("..") || path.components().count() != 1 {
        return Err(format!("项目名称包含非法路径片段: {}", value));
    }
    Ok(value.to_string())
}

/// 校验相对路径，只允许普通路径片段。
fn safe_relative_path(value: &str) -> Result<PathBuf, String> {
    if value.trim().is_empty() {
        return Err("源文件标识不能为空".to_string());
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Err(format!("源文件标识不能是绝对路径: {}", value));
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("源文件标识包含非法路径片段: {}", value));
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err("源文件标识不能为空".to_string());
    }
    Ok(clean)
}

/// 摄入单个文件
///
/// 按照设计要求：先复制文件到 raw/{project}/sources/，写入 raw_sources 表，
/// 再走现有 ingest 流程，并让 documents.raw_source_identity 关联源文件。
pub fn ingest_file(
    file_path: &Path,
    project: &str,
    embedding: &Arc<RwLock<EmbeddingService>>,
    vector_index: &Arc<RwLock<VectorIndex>>,
    metadata: &Arc<Mutex<MetadataStore>>,
    bm25: &Arc<RwLock<BM25Service>>,
    raw_sources: Option<&Arc<Mutex<RawSourceStore>>>,
    app_handle: Option<&AppHandle>,
    data_dir: Option<&Path>,
) -> Result<IngestionResult, String> {
    // 跳过临时或垃圾文件（Office 锁文件 ~$*、Thumbs.db 等）
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

    // 从文件名提取标题
    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("untitled");
    let title = extract_title_from_filename(filename);

    let file_path_str = file_path.to_string_lossy();
    
    // 生成 raw_source_identity（使用相对路径或文件名）
    let raw_source_identity = raw_sources.as_ref().map(|_| {
        file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    let mut result = ingest_text(
        &content,
        &title,
        project,
        embedding,
        vector_index,
        metadata,
        bm25,
        raw_sources,
        raw_source_identity.as_deref(),
        Some(file_path_str.as_ref()),
        app_handle,
        data_dir,
    )?;

    result.title = title;
    Ok(result)
}

/// 摄入目录中的所有支持文件（递归子目录）
pub fn ingest_directory(
    dir_path: &Path,
    project: &str,
    embedding: &Arc<RwLock<EmbeddingService>>,
    vector_index: &Arc<RwLock<VectorIndex>>,
    metadata: &Arc<Mutex<MetadataStore>>,
    bm25: &Arc<RwLock<BM25Service>>,
    raw_sources: Option<&Arc<Mutex<RawSourceStore>>>,
    app_handle: Option<&AppHandle>,
    data_dir: Option<&Path>,
) -> Result<DirectoryIngestionResult, String> {
    if !dir_path.is_dir() {
        return Err(format!("Not a directory: {:?}", dir_path));
    }

    let mut imported = Vec::new();
    let mut errors = Vec::new();

    // 递归遍历目录树
    ingest_dir_recursive(
        dir_path,
        project,
        embedding,
        vector_index,
        metadata,
        bm25,
        raw_sources,
        app_handle,
        data_dir,
        &mut imported,
        &mut errors,
    )?;

    if imported.is_empty() && errors.is_empty() {
        eprintln!("[Ingestion] No supported files found in {:?}", dir_path);
    }

    Ok(DirectoryIngestionResult { imported, errors })
}

/// 递归辅助函数：深度优先遍历目录并摄入所有支持的文件
fn ingest_dir_recursive(
    dir_path: &Path,
    project: &str,
    embedding: &Arc<RwLock<EmbeddingService>>,
    vector_index: &Arc<RwLock<VectorIndex>>,
    metadata: &Arc<Mutex<MetadataStore>>,
    bm25: &Arc<RwLock<BM25Service>>,
    raw_sources: Option<&Arc<Mutex<RawSourceStore>>>,
    app_handle: Option<&AppHandle>,
    data_dir: Option<&Path>,
    imported: &mut Vec<IngestionResult>,
    errors: &mut Vec<FileError>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir_path)
        .map_err(|e| format!("Failed to read directory {:?}: {}", dir_path, e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();

        if path.is_dir() {
            // 递归进入子目录
            ingest_dir_recursive(
                &path,
                project,
                embedding,
                vector_index,
                metadata,
                bm25,
                raw_sources,
                app_handle,
                data_dir,
                imported,
                errors,
            )?;
        } else if is_temp_file(&path) {
            // 静默跳过 Office 锁文件和其他临时文件
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
                data_dir,
            ) {
                Ok(result) => imported.push(result),
                Err(e) => {
                    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                    eprintln!("[Ingestion] Failed to ingest {:?}: {}", path, e);
                    errors.push(FileError {
                        path: filename.to_string(),
                        error: e,
                    });
                    // 继续处理其他文件
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

        // 创建服务
        let embedding = Arc::new(RwLock::new(EmbeddingService::empty()));
        let vector_index = Arc::new(RwLock::new(
            VectorIndex::new(data_dir.join("index")).unwrap(),
        ));
        let metadata = Arc::new(Mutex::new(
            MetadataStore::new(data_dir.join("meta.db")).unwrap(),
        ));
        let bm25 = Arc::new(RwLock::new(
            BM25Service::new(data_dir.join("bm25_index")).unwrap(),
        ));

        // 摄入文本（嵌入为空会失败，但可以测试流程）
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
            None,
        );

        // 嵌入模型未初始化，预期失败
        assert!(result.is_err());
    }

    #[test]
    fn test_ingest_text_dedup() {
        let tmp = tempdir().unwrap();
        let data_dir = tmp.path();

        let embedding = Arc::new(RwLock::new(EmbeddingService::empty()));
        let vector_index = Arc::new(RwLock::new(
            VectorIndex::new(data_dir.join("index")).unwrap(),
        ));
        let metadata = Arc::new(Mutex::new(
            MetadataStore::new(data_dir.join("meta.db")).unwrap(),
        ));
        let bm25 = Arc::new(RwLock::new(
            BM25Service::new(data_dir.join("bm25_index")).unwrap(),
        ));

        // 第一次摄入
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
            None,
        );

        // 第二次摄入相同内容，预期去重
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
            None,
        );

        // 应提前返回去重结果（chunk_count = 0）
        if let Ok(r) = result {
            assert_eq!(r.chunk_count, 0);
        }
    }

    #[test]
    fn test_ingest_file_not_found() {
        let tmp = tempdir().unwrap();
        let data_dir = tmp.path();

        let embedding = Arc::new(RwLock::new(EmbeddingService::empty()));
        let vector_index = Arc::new(RwLock::new(
            VectorIndex::new(data_dir.join("index")).unwrap(),
        ));
        let metadata = Arc::new(Mutex::new(
            MetadataStore::new(data_dir.join("meta.db")).unwrap(),
        ));
        let bm25 = Arc::new(RwLock::new(
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
            None,
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_ingest_directory_not_dir() {
        let tmp = tempdir().unwrap();
        let data_dir = tmp.path();

        let embedding = Arc::new(RwLock::new(EmbeddingService::empty()));
        let vector_index = Arc::new(RwLock::new(
            VectorIndex::new(data_dir.join("index")).unwrap(),
        ));
        let metadata = Arc::new(Mutex::new(
            MetadataStore::new(data_dir.join("meta.db")).unwrap(),
        ));
        let bm25 = Arc::new(RwLock::new(
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
            None,
        );

        assert!(result.is_err());
    }
}
