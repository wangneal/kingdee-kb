use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use tauri::State;

use crate::app_state::AppState;
use crate::services::raw_source::{InsertRawSource, RawSource};

/// 导入一个原始文件到知识库项目中。
///
/// 将源文件复制到 `raw/{project}/sources/{identity}`，计算 SHA256，
/// 并在 raw_sources 表中创建记录。
#[tauri::command]
pub async fn create_raw_source(
    state: State<'_, AppState>,
    project: String,
    identity: String,
    source_path: String,
    mime_type: Option<String>,
) -> Result<RawSource, String> {
    let identity_path = validate_source_identity(&identity)?;
    let project_dir = validate_project_segment(&project)?;

    // 计算 SHA256
    let sha256 = compute_sha256(&source_path)?;

    // 读取文件大小
    let metadata = std::fs::metadata(&source_path)
        .map_err(|e| format!("读取源文件信息失败: {}", e))?;
    let file_size = Some(metadata.len() as i64);

    // 创建目标存储目录
    let storage_dir = state
        .data_dir
        .join("raw")
        .join(&project_dir)
        .join("sources");
    std::fs::create_dir_all(&storage_dir)
        .map_err(|e| format!("创建存储目录失败: {}", e))?;

    // 目标路径 = raw/{project}/sources/{identity}
    let storage_path = storage_dir.join(&identity_path);
    std::fs::copy(&source_path, &storage_path)
        .map_err(|e| format!("复制文件失败: {}", e))?;

    let storage_path_str = storage_path.to_string_lossy().to_string();

    // 插入数据库记录
    let insert = InsertRawSource {
        project: project.clone(),
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
    project: String,
) -> Result<Vec<RawSource>, String> {
    let store = state
        .raw_sources
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.list_by_project(&project)
}

/// 软删除一个原始文件记录（标记为 deleted）。
#[tauri::command]
pub async fn soft_delete_raw_source(
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    let store = state
        .raw_sources
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.soft_delete(id)
}

/// 校验项目目录名，避免 project 参数逃逸数据目录。
fn validate_project_segment(project: &str) -> Result<String, String> {
    if project.trim().is_empty() {
        return Err("项目名称不能为空".to_string());
    }
    let path = Path::new(project);
    if path.is_absolute() || project.contains("..") {
        return Err(format!("项目名称包含非法路径片段: {}", project));
    }
    if path.components().count() != 1 {
        return Err(format!("项目名称不能包含路径分隔符: {}", project));
    }
    Ok(project.to_string())
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
    let mut file = std::fs::File::open(file_path)
        .map_err(|e| format!("打开文件失败: {}", e))?;
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
