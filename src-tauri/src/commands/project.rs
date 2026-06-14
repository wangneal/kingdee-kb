use crate::app_state::AppState;
use crate::services::project_store::{Project, ProjectPhase, ProjectProduct, ProjectStore, ProjectSummary};
use tauri::State;

async fn with_project_store<T, F>(state: State<'_, AppState>, task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&ProjectStore) -> Result<T, String> + Send + 'static,
{
    let store = state.project_store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store = store.lock().map_err(|e| e.to_string())?;
        task(&store)
    })
    .await
    .map_err(|e| format!("项目命令执行失败: {}", e))?
}

#[tauri::command]
pub async fn ensure_default_project(state: State<'_, AppState>) -> Result<i64, String> {
    with_project_store(state, |store| store.ensure_default_project()).await
}

#[tauri::command]
pub async fn create_project(
    state: State<'_, AppState>,
    name: String,
    client_name: Option<String>,
    description: Option<String>,
) -> Result<i64, String> {
    with_project_store(state, move |store| {
        store.create_project(
            &name,
            client_name.as_deref().unwrap_or_default(),
            description.as_deref().unwrap_or_default(),
        )
    })
    .await
}

#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Result<Vec<ProjectSummary>, String> {
    with_project_store(state, |store| store.list_projects()).await
}

#[tauri::command]
pub async fn get_project(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Option<Project>, String> {
    with_project_store(state, move |store| store.get_project(project_id)).await
}

#[tauri::command]
pub async fn get_project_phases(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<ProjectPhase>, String> {
    with_project_store(state, move |store| store.get_project_phases(project_id)).await
}

#[tauri::command]
pub async fn update_project(
    state: State<'_, AppState>,
    project_id: i64,
    name: String,
    client_name: String,
    description: String,
) -> Result<(), String> {
    with_project_store(state, move |store| {
        store.update_project(project_id, &name, &client_name, &description)
    })
    .await
}

#[tauri::command]
pub async fn update_project_phase_plan(
    state: State<'_, AppState>,
    project_id: i64,
    phase_key: String,
    planned_start: Option<String>,
    planned_end: Option<String>,
) -> Result<(), String> {
    with_project_store(state, move |store| {
        store.update_phase_plan(
            project_id,
            &phase_key,
            planned_start.as_deref(),
            planned_end.as_deref(),
        )
    })
    .await
}

#[tauri::command]
pub async fn archive_project(state: State<'_, AppState>, project_id: i64) -> Result<(), String> {
    with_project_store(state, move |store| store.archive_project(project_id)).await
}

#[tauri::command]
pub async fn restore_project(state: State<'_, AppState>, project_id: i64) -> Result<(), String> {
    with_project_store(state, move |store| store.restore_project(project_id)).await
}

#[tauri::command]
pub async fn set_current_project_phase(
    state: State<'_, AppState>,
    project_id: i64,
    phase_key: String,
) -> Result<(), String> {
    with_project_store(state, move |store| {
        store.set_current_phase(project_id, &phase_key)
    })
    .await
}

#[tauri::command]
pub async fn list_project_products(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<ProjectProduct>, String> {
    with_project_store(state, move |store| store.list_project_products(project_id)).await
}

#[tauri::command]
pub async fn add_project_product(
    state: State<'_, AppState>,
    project_id: i64,
    product_name: String,
    product_version: String,
) -> Result<i64, String> {
    with_project_store(state, move |store| {
        store.add_project_product(project_id, &product_name, &product_version)
    })
    .await
}

#[tauri::command]
pub async fn delete_project_product(
    state: State<'_, AppState>,
    project_id: i64,
    product_id: i64,
) -> Result<(), String> {
    with_project_store(state, move |store| {
        store.delete_project_product(project_id, product_id)
    })
    .await
}

// ─── 项目硬删除（B2：add-project-delete-and-manual-wiki） ───

/// 硬删除项目及其所有关联数据（不可恢复）。
///
/// 流程：
/// 1. 拒绝默认项目（与 `archive_project` 一致）
/// 2. 拒绝有 pending 摄入队列任务的项目（防竞态 + 尊重用户意图）
/// 3. 收集项目所有 document 的 vector_keys + chunk_ids，从 vector_index / bm25 移除
/// 4. 删 raw_sources 行（同时 `data_dir/raw/<project_id>/` 文件目录）
/// 5. SQLite `DELETE FROM projects WHERE id=?`，依赖外键 `ON DELETE CASCADE` 级联
///    documents / chunks / wiki_pages / project_products / project_phases /
///    meeting_records / ingest_cache / analysis_cache
///
/// 错误码（前端据此提示）：
/// - `"项目不存在: <id>"`
/// - `"默认项目不能删除"`
/// - `"项目有 N 个待处理队列任务，请先处理/重试后再删除"`
/// - `"向量索引清理失败: ..."` / `"BM25 索引清理失败: ..."` /
///   `"数据库级联删除失败: ..."` / `"原始文件目录清理失败: ..."`
#[tauri::command]
pub async fn delete_project(state: State<'_, AppState>, project_id: i64) -> Result<(), String> {
    // 1. 校验：项目存在 + 非默认项目
    let project_opt: Option<crate::services::project_store::Project> = {
        let store = state.project_store.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let store = store.lock().map_err(|e| e.to_string())?;
            store.get_project(project_id)
        })
        .await
        .map_err(|e| format!("查询项目失败: {}", e))??
    };
    let project = project_opt.ok_or_else(|| format!("项目不存在: {}", project_id))?;
    if project.name == "默认项目" {
        return Err("默认项目不能删除".to_string());
    }

    // 2. 校验：无 pending 摄入队列任务
    {
        let queue = state
            .ingest_queue
            .lock()
            .map_err(|e| format!("获取队列锁失败: {}", e))?;
        let pending_count = queue
            .visible_items()
            .iter()
            .filter(|i| i.project_id == project_id && i.status == "pending")
            .count();
        if pending_count > 0 {
            return Err(format!(
                "项目有 {} 个待处理队列任务，请先处理/重试后再删除",
                pending_count
            ));
        }
    }

    // 3. 收集 project 所有 document 的 vector_keys + chunk_ids
    let documents = state
        .metadata
        .lock()
        .map_err(|e| format!("metadata 锁失败: {}", e))?
        .list_documents(Some(project_id))
        .map_err(|e| format!("列项目文档失败: {}", e))?;
    let doc_ids: Vec<i64> = documents.iter().map(|d| d.id).collect();

    let mut all_vector_keys: Vec<i64> = Vec::new();
    let mut all_chunk_ids: Vec<i64> = Vec::new();
    {
        let meta = state
            .metadata
            .lock()
            .map_err(|e| format!("metadata 锁失败: {}", e))?;
        for doc_id in &doc_ids {
            let chunks = meta
                .get_chunks_by_document(*doc_id)
                .map_err(|e| format!("取文档分块失败: doc_id={}, {}", doc_id, e))?;
            for chunk in chunks {
                all_vector_keys.push(chunk.vector_key);
                all_chunk_ids.push(chunk.id);
            }
        }
    }

    // 4. 从 vector_index 移除（按 vector_key）
    if !all_vector_keys.is_empty() {
        let idx = state
            .vector_index
            .write()
            .map_err(|e| format!("vector_index 写锁失败: {}", e))?;
        idx.remove_keys(&all_vector_keys)
            .map_err(|e| format!("向量索引清理失败: {}", e))?;
    }

    // 5. 从 bm25 移除（按 chunk_id）
    if !all_chunk_ids.is_empty() {
        let bm25 = state
            .bm25
            .write()
            .map_err(|e| format!("bm25 写锁失败: {}", e))?;
        bm25.remove_chunks(&all_chunk_ids)
            .map_err(|e| format!("BM25 索引清理失败: {}", e))?;
    }

    // 6. SQLite 级联删除 projects 行（依赖各子表 ON DELETE CASCADE）
    {
        let store = state.project_store.clone();
        let project_id = project_id;
        tauri::async_runtime::spawn_blocking(move || {
            let store = store.lock().map_err(|e| e.to_string())?;
            store.delete_project(project_id)
        })
        .await
        .map_err(|e| format!("数据库级联删除调度失败: {}", e))?
        .map_err(|e| format!("数据库级联删除失败: {}", e))?;
    }

    // 7. 删除 raw_sources 行 + 物理文件目录 data_dir/raw/<project_id>/
    let raw_dir = state.data_dir.join("raw").join(project_id.to_string());
    if raw_dir.exists() {
        std::fs::remove_dir_all(&raw_dir)
            .map_err(|e| format!("原始文件目录清理失败 ({:?}): {}", raw_dir, e))?;
    }

    Ok(())
}
