use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use tauri::State;

use crate::app_state::AppState;
use crate::services::raw_source::{InsertRawSource, RawSource};

/// 导入一个原始文件到知识库项目中。
///
/// 将源文件复制到 `raw/{project_id}/sources/{identity}`，计算 SHA256，
/// 并在 raw_sources 表中创建记录。
#[tauri::command]
pub async fn create_raw_source(
    state: State<'_, AppState>,
    project_id: i64,
    identity: String,
    source_path: String,
    mime_type: Option<String>,
) -> Result<RawSource, String> {
    let identity_path = validate_source_identity(&identity)?;
    let project_dir = validate_project_id_segment(project_id)?;

    // 计算 SHA256
    let sha256 = compute_sha256(&source_path)?;

    // 读取文件大小
    let metadata =
        std::fs::metadata(&source_path).map_err(|e| format!("读取源文件信息失败: {}", e))?;
    let file_size = Some(metadata.len() as i64);

    // 创建目标存储目录
    let storage_dir = state
        .data_dir
        .join("raw")
        .join(&project_dir)
        .join("sources");
    std::fs::create_dir_all(&storage_dir).map_err(|e| format!("创建存储目录失败: {}", e))?;

    // 目标路径 = raw/{project_id}/sources/{identity}
    let storage_path = storage_dir.join(&identity_path);
    std::fs::copy(&source_path, &storage_path).map_err(|e| format!("复制文件失败: {}", e))?;

    let storage_path_str = storage_path.to_string_lossy().to_string();

    // 插入数据库记录
    let insert = InsertRawSource {
        project_id,
        identity: identity.clone(),
        original_path: source_path,
        storage_path: storage_path_str,
        sha256,
        file_size,
        mime_type,
    };

    let store = state
        .raw_sources
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.insert(&insert)
}

/// 列出指定项目的所有活跃原始文件。
#[tauri::command]
pub async fn list_raw_sources(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<RawSource>, String> {
    let store = state
        .raw_sources
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.list_by_project(project_id)
}

/// 软删除一个原始文件记录，同时级联清除关联的文档、分块、向量和全文索引。
///
/// 级联流程：
/// 1. 读取 raw_source 记录，获取 (sha256, project_id, identity)
/// 2. 按三字段定位关联的 documents，收集 vector_keys
/// 3. 从 usearch 删除向量
/// 4. 从 BM25/tantivy 删除全文索引
/// 5. 从 SQLite 删除 documents + chunks
/// 6. 软删除 raw_source 本身
///
/// 防止已删除的敏感文件仍被 AI 问答召回。
#[tauri::command]
pub async fn soft_delete_raw_source(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    // 1. 读取 raw_source 记录
    let source = {
        let store = state
            .raw_sources
            .lock()
            .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
        store.get_by_id(id)?
    };

    // 2. 查找关联文档并收集 vector_keys
    let (document_ids, all_vector_keys) = {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        let docs =
            meta.list_documents_by_source_key(&source.sha256, source.project_id, &source.identity)?;
        let doc_ids: Vec<i64> = docs.iter().map(|d| d.id).collect();
        let keys = if doc_ids.is_empty() {
            Vec::new()
        } else {
            meta.get_vector_keys_by_document_ids(&doc_ids)?
        };
        (doc_ids, keys)
    };

    // 3. 从 usearch 删除向量
    if !all_vector_keys.is_empty() {
        if let Ok(idx) = state.vector_index.write() {
            if let Err(e) = idx.remove_keys(&all_vector_keys) {
                tracing::warn!("usearch 向量删除失败 (raw_source={}): {}", id, e);
            }
        }
    }

    // 4. 从 BM25/tantivy 删除全文索引
    if !all_vector_keys.is_empty() {
        let _ = state.get_or_init_bm25();
        if let Ok(bm25) = state.bm25.write() {
            if let Err(e) = bm25.remove_chunks(&all_vector_keys) {
                tracing::warn!("BM25 索引删除失败 (raw_source={}): {}", id, e);
            }
        }
    }

    // 5. 从 SQLite 删除文档 + 分块（失败时阻止软删除，防止数据残留）
    if !document_ids.is_empty() {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        meta.delete_documents_batch(document_ids, Some(source.project_id))
            .map_err(|e| format!("级联删除文档失败 (raw_source={}): {}", id, e))?;
    }

    // 6. 软删除 raw_source 本身
    {
        let store = state
            .raw_sources
            .lock()
            .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
        store.soft_delete(id)
    }
}

/// 校验项目 ID 目录名，避免路径逃逸数据目录。
fn validate_project_id_segment(project_id: i64) -> Result<String, String> {
    if project_id <= 0 {
        return Err(format!("项目 ID 无效: {}", project_id));
    }
    Ok(project_id.to_string())
}

/// 校验 source identity，只允许相对路径中的普通片段。
fn validate_source_identity(identity: &str) -> Result<PathBuf, String> {
    if identity.trim().is_empty() {
        return Err("identity 不能为空".to_string());
    }

    let path = Path::new(identity);
    if path.is_absolute() {
        return Err(format!("identity 不能是绝对路径: {}", identity));
    }

    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("identity 包含非法路径片段: {}", identity));
            }
        }
    }

    if clean.as_os_str().is_empty() {
        return Err("identity 不能为空".to_string());
    }
    Ok(clean)
}

/// 计算文件的 SHA256 哈希值
fn compute_sha256(file_path: &str) -> Result<String, String> {
    let mut file = std::fs::File::open(file_path).map_err(|e| format!("打开文件失败: {}", e))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let bytes_read = file
            .read(&mut buffer)
            .map_err(|e| format!("读取文件失败: {}", e))?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    let hash = hasher.finalize();
    Ok(format!("{:x}", hash))
}
