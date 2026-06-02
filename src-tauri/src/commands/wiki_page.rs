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
pub async fn get_wiki_page(
    state: State<'_, AppState>,
    id: i64,
) -> Result<WikiPage, String> {
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
    project: String,
    slug: String,
) -> Result<Option<WikiPage>, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.get_by_slug(&project, &slug)
}

/// 列出项目下的维基页面，可按状态过滤。
#[tauri::command]
pub async fn list_wiki_pages(
    state: State<'_, AppState>,
    project: String,
    page_status: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<WikiPage>, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.list(&project, page_status.as_deref(), limit, offset)
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

/// 删除维基页面。
#[tauri::command]
pub async fn delete_wiki_page(
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.delete(id)
}

/// 批准维基页面的候选内容（将 content_candidate 提升为 content）。
#[tauri::command]
pub async fn approve_wiki_page(
    state: State<'_, AppState>,
    id: i64,
) -> Result<WikiPage, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.approve_candidate(id)
}

/// 拒绝维基页面的候选内容（清空候选字段）。
#[tauri::command]
pub async fn reject_wiki_page(
    state: State<'_, AppState>,
    id: i64,
) -> Result<WikiPage, String> {
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
    project: String,
) -> Result<usize, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.seed_demo_pages(&project)
}

/// 搜索 wikilink 候选页面（按标题模糊搜索，排除自身）。
#[tauri::command]
pub async fn search_wikilink_candidates(
    state: State<'_, AppState>,
    project: String,
    query: String,
    exclude_slug: String,
    limit: Option<i64>,
) -> Result<Vec<WikiPageBrief>, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.search_wikilink_candidates(&project, &query, &exclude_slug, limit.unwrap_or(20))
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

/// 获取 wikilink 目标页面详情（批量查询被引页面的标题/slug/type/status）。
#[tauri::command]
pub async fn get_wikilink_targets(
    state: State<'_, AppState>,
    slugs: Vec<String>,
) -> Result<Vec<WikiLinkTarget>, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.get_wikilink_targets(&slugs)
}

/// 获取反向链接（哪些页面引用了当前页面）。
#[tauri::command]
pub async fn get_backlinks(
    state: State<'_, AppState>,
    project: String,
    slug: String,
) -> Result<Vec<WikiPageBrief>, String> {
    let store = state
        .wiki_pages
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.get_backlinks(&project, &slug)
}
