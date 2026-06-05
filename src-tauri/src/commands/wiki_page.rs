use tauri::State;

use crate::app_state::AppState;
use crate::services::wiki_page::{
    CreateWikiPage, UpdateWikiPage, WikiLinkTarget, WikiPage, WikiPageBrief,
};

/// 创建维基页面。
#[tauri::command]
pub async fn create_wiki_page(
    state: State<'_, AppState>,
    input: CreateWikiPage,
) -> Result<WikiPage, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.create(&input)
}

/// 根据 ID 获取维基页面。
#[tauri::command]
pub async fn get_wiki_page(state: State<'_, AppState>, id: i64) -> Result<WikiPage, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.get_by_id(id)
}

/// 根据项目 + slug 查找维基页面。
#[tauri::command]
pub async fn get_wiki_page_by_slug(
    state: State<'_, AppState>,
    project_id: i64,
    slug: String,
) -> Result<Option<WikiPage>, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.get_by_slug(project_id, &slug)
}

/// 列出项目下的维基页面，可按状态过滤。
#[tauri::command]
pub async fn list_wiki_pages(
    state: State<'_, AppState>,
    project_id: i64,
    page_status: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<WikiPage>, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.list(project_id, page_status.as_deref(), limit, offset)
}

/// 更新维基页面。
#[tauri::command]
pub async fn update_wiki_page(
    state: State<'_, AppState>,
    id: i64,
    input: UpdateWikiPage,
) -> Result<WikiPage, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.update(id, &input)
}

/// 内部辅助：删除单个 wiki 页面及其关联的源文档、向量、缓存。
/// 同时清除 ingest_cache 和 analysis_cache，确保重新导入时能完整走流程。
fn delete_single_wiki_page(
    state: &State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    // 1. 读取 wiki 页面，解析 sources JSON 获取 document_id 和缓存清理信息
    let (document_ids, project_id, cache_keys) = {
        let store = state
            .wiki_pages
            .lock()
            .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
        let page = store.get_by_id(id)?;
        let pid = page.project_id;
        let page_slug = page.slug.clone();
        let sources_data: Vec<serde_json::Value> = serde_json::from_str(&page.sources)
            .unwrap_or_default();

        // 提取 document_id
        let doc_ids: Vec<i64> = sources_data
            .iter()
            .filter_map(|v| v["document_id"].as_i64())
            .collect();

        // 从文档元数据提取 SHA256 + source_identity，用于清理缓存
        let mut cache_keys: Vec<(String, String)> = Vec::new();
        if let Ok(meta) = state.metadata.lock() {
            for &doc_id in &doc_ids {
                if let Ok(Some(doc)) = meta.get_document(doc_id) {
                    if let Some(ref sha) = doc.sha256 {
                        cache_keys.push((doc.title.clone(), sha.clone()));
                    }
                }
            }
        }

        // 兜底：当 sources 里没有 document_id 时（多数 LLM 编译路径 document_id=None），
        // 用 wiki page 的 slug 反查 ingest_cache.files_written（JSON 数组，包含写入的 slug）。
        if cache_keys.is_empty() {
            if let Ok(ingest) = state.ingest_cache_store.lock() {
                if let Ok(caches) = ingest.list_by_project(pid) {
                    let matched = crate::services::ingest_cache::find_cache_keys_by_slug(
                        &caches, &page_slug,
                    );
                    for (source_identity, sha256) in matched {
                        tracing::info!(
                            "通过 slug 反查命中 ingest_cache: source={}, slug={}",
                            source_identity,
                            page_slug
                        );
                        cache_keys.push((source_identity, sha256));
                    }
                }
            }
        }

        (doc_ids, pid, cache_keys)
    };

    // 2. 级联删除关联的源文档（向量 + BM25 + SQLite 元数据）
    for doc_id in document_ids {
        let vector_keys = {
            let meta = state.metadata.lock().map_err(|e| e.to_string())?;
            match meta.get_vector_keys_by_document_ids(&[doc_id]) {
                Ok(keys) => keys,
                Err(e) => {
                    tracing::warn!("获取文档 {} 的 vector_keys 失败: {}", doc_id, e);
                    continue;
                }
            }
        };
        if let Ok(idx) = state.vector_index.write() {
            if let Err(e) = idx.remove_keys(&vector_keys) {
                tracing::warn!("usearch 删除失败(doc_id={}): {}", doc_id, e);
            }
        }
        if let Ok(bm25) = state.bm25.write() {
            if let Err(e) = bm25.remove_chunks(&vector_keys) {
                tracing::warn!("BM25 删除失败(doc_id={}): {}", doc_id, e);
            }
        }
        {
            let meta = state.metadata.lock().map_err(|e| e.to_string())?;
            if let Err(e) = meta.delete_document(doc_id, Some(project_id)) {
                tracing::warn!("SQLite 删除源文档失败(doc_id={}): {}", doc_id, e);
            }
        }
        tracing::info!("已级联删除源文档: doc_id={}", doc_id);
    }

    // 3. 清除 ingest_cache 和 analysis_cache（关键修复：否则重新导入无法重新编译）
    for (source_identity, sha256) in cache_keys {
        if let Ok(ingest) = state.ingest_cache_store.lock() {
            if let Ok(Some(cache)) = ingest.get_by_key(project_id, &source_identity, &sha256) {
                let _ = ingest.delete(cache.id);
                tracing::info!("已清除 ingest_cache: source={}", source_identity);
            }
        }
        if let Ok(analysis) = state.analysis_cache.lock() {
            if let Ok(Some(cache)) = analysis.get_by_key(project_id, &source_identity, &sha256) {
                let _ = analysis.delete(cache.id);
                tracing::info!("已清除 analysis_cache: source={}", source_identity);
            }
        }
    }

    // 4. 删除 wiki 页面本身
    {
        let store = state
            .wiki_pages
            .lock()
            .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
        store.delete(id)?;
    }

    Ok(())
}

/// 删除维基页面，同时级联删除关联的源文档、向量、BM25 数据。
///
/// 删除后可重新导入同一文件，完整走摄入+编译流程。
#[tauri::command]
pub async fn delete_wiki_page(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    delete_single_wiki_page(&state, id)
}

/// 批量删除维基页面，同时级联删除关联的源文档、向量、BM25 数据。
///
/// 返回成功删除的数量。
#[tauri::command]
pub async fn batch_delete_wiki_pages(
    state: State<'_, AppState>,
    ids: Vec<i64>,
) -> Result<usize, String> {
    let mut deleted = 0;
    for id in ids {
        match delete_single_wiki_page(&state, id) {
            Ok(()) => deleted += 1,
            Err(e) => {
                tracing::warn!("批量删除 wiki_page 失败(id={}): {}", id, e);
            }
        }
    }
    Ok(deleted)
}

/// 批准维基页面的候选内容（将 content_candidate 提升为 content）。
#[tauri::command]
pub async fn approve_wiki_page(state: State<'_, AppState>, id: i64) -> Result<WikiPage, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.approve_candidate(id)
}

/// 拒绝维基页面的候选内容（清空候选字段）。
#[tauri::command]
pub async fn reject_wiki_page(state: State<'_, AppState>, id: i64) -> Result<WikiPage, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.reject_candidate(id)
}

/// 插入 50 条种子演示数据到指定项目（用于阶段五知识图谱开发测试）。
#[tauri::command]
pub async fn seed_demo_wiki_pages(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<usize, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.seed_demo_pages(project_id)
}

/// 搜索 wikilink 候选页面（按标题模糊搜索，排除自身）。
#[tauri::command]
pub async fn search_wikilink_candidates(
    state: State<'_, AppState>,
    project_id: i64,
    query: String,
    exclude_slug: String,
    limit: Option<i64>,
) -> Result<Vec<WikiPageBrief>, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.search_wikilink_candidates(project_id, &query, &exclude_slug, limit.unwrap_or(20))
}

/// 添加 wikilink（追加 slug 到页面的 wikilinks JSON 数组，去重）。
#[tauri::command]
pub async fn add_wikilink(
    state: State<'_, AppState>,
    page_id: i64,
    target_slug: String,
) -> Result<WikiPage, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.add_wikilink(page_id, &target_slug)
}

/// 移除 wikilink（从页面的 wikilinks JSON 数组中删除 slug）。
#[tauri::command]
pub async fn remove_wikilink(
    state: State<'_, AppState>,
    page_id: i64,
    target_slug: String,
) -> Result<WikiPage, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.remove_wikilink(page_id, &target_slug)
}

/// 获取 wikilink 目标页面详情（按项目过滤，批量查询被引页面的标题/slug/type/status）。
#[tauri::command]
pub async fn get_wikilink_targets(
    state: State<'_, AppState>,
    project_id: i64,
    slugs: Vec<String>,
) -> Result<Vec<WikiLinkTarget>, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.get_wikilink_targets(project_id, &slugs)
}

/// 获取反向链接（哪些页面引用了当前页面）。
#[tauri::command]
pub async fn get_backlinks(
    state: State<'_, AppState>,
    project_id: i64,
    slug: String,
) -> Result<Vec<WikiPageBrief>, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.get_backlinks(project_id, &slug)
}
