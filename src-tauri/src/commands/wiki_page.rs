use std::path::Path;
use tauri::State;

use crate::app_state::AppState;
use crate::services::wiki_page::{
    CreateWikiPage, UpdateWikiPage, WikiLinkTarget, WikiPage, WikiPageBrief,
};

/// 从文件路径中提取文件名（如 "C:/.../需求.docx" → "需求.docx"）
/// raw_source_identity 缺失时，用 source_path 兜底
fn extract_filename(path: &str) -> Option<String> {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
}

/// 从文档元数据构造 cache 清理用的 source_identity
/// 优先级：raw_source_identity（带扩展名）→ source_path 提取文件名 → doc.title 兜底
/// 关键：必须用带扩展名的 identity（与 ingest_cache/analysis_cache 实际写入 key 一致），
/// 不能用无扩展名的 title，否则 cache key 永远不匹配
fn resolve_source_identity_for_cache(
    raw_source_identity: Option<&str>,
    source_path: Option<&str>,
    title: &str,
) -> String {
    raw_source_identity
        .map(|s| s.to_string())
        .or_else(|| source_path.and_then(extract_filename))
        .unwrap_or_else(|| title.to_string())
}

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
fn delete_single_wiki_page(state: &State<'_, AppState>, id: i64) -> Result<(), String> {
    // 1. 读取 wiki 页面，解析 sources JSON 获取缓存清理信息
    //    注意：document_ids 不再用于级联删除（documents 是摄入基础设施，与 wiki 解耦）
    let (_document_ids, project_id, cache_keys) = {
        let store = state
            .wiki_pages
            .lock()
            .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
        let page = store.get_by_id(id)?;
        let pid = page.project_id;
        let page_slug = page.slug.clone();
        let sources_data: Vec<serde_json::Value> =
            serde_json::from_str(&page.sources).unwrap_or_default();

        // 提取 document_id
        let doc_ids: Vec<i64> = sources_data
            .iter()
            .filter_map(|v| v["document_id"].as_i64())
            .collect();

        // 从文档元数据提取 SHA256 + source_identity，用于清理缓存
        // 关键：cache key 必须用 raw_source_identity（带扩展名的文件名，如 "需求.docx"），
        // 不能用 doc.title（无扩展名，如 "需求"），否则与 ingest_cache/analysis_cache 的实际
        // 写入 key 不匹配，导致缓存清理失败，删 wiki 后重导入/重编译仍会命中陈旧缓存。
        let mut cache_keys: Vec<(String, String)> = Vec::new();
        if let Ok(meta) = state.metadata.lock() {
            for &doc_id in &doc_ids {
                if let Ok(Some(doc)) = meta.get_document(doc_id) {
                    if let Some(ref sha) = doc.sha256 {
                        let source_identity = resolve_source_identity_for_cache(
                            doc.raw_source_identity.as_deref(),
                            doc.source_path.as_deref(),
                            &doc.title,
                        );
                        cache_keys.push((source_identity, sha.clone()));
                    }
                }
            }
        }

        // 兜底：当 sources 里没有 document_id 时（多数 LLM 编译路径 document_id=None），
        // 用 wiki page 的 slug 反查 ingest_cache.files_written（JSON 数组，包含写入的 slug）。
        if cache_keys.is_empty() {
            if let Ok(ingest) = state.ingest_cache_store.lock() {
                if let Ok(caches) = ingest.list_by_project(pid) {
                    let matched =
                        crate::services::ingest_cache::find_cache_keys_by_slug(&caches, &page_slug);
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

    // 2. 清除 ingest_cache 和 analysis_cache（关键修复：否则重新导入无法重新编译）
    //    注意：不再级联删除 documents/chunks/vectors。
    //    理由：documents 是摄入基础设施，与 wiki 页面解耦。
    //    - 保留 documents 后，"删 wiki → 强制重编译"路径才能通过 lookup_document_id 找回真实 document_id
    //    - 用户若想彻底删除某个源，应在 source 管理页用独立入口（避免误删基础设施数据）
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

    // 3. 删除 wiki 页面本身
    {
        let store = state
            .wiki_pages
            .lock()
            .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
        store.delete(id)?;
    }

    Ok(())
}

/// 删除维基页面，**不再级联删除关联的 documents/chunks/vectors**。
///
/// 设计原则：documents 是摄入基础设施（被向量搜索、KB 编译、来源追溯共用），
/// 与 wiki 页面解耦。删除 wiki 不应销毁这些数据，否则：
/// - "删 wiki → 强制重编译"无法找回 document_id（lookup_document_id 返回 None）
/// - 向量索引/BM25 数据被一并删除，下次需要重新摄入（耗时）
///
/// 重新生成 wiki 页面：通过强制重编译按钮（清空 compile cache 后从 raw_sources 重建）。
/// 彻底删除源数据：通过 source 管理页的独立删除入口。
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

#[cfg(test)]
mod tests {
    use super::*;

    // ─── extract_filename 单元测试 ───

    #[test]
    fn extract_filename_full_windows_path() {
        assert_eq!(
            extract_filename(r"C:\Users\Neal\原始资料\需求.docx"),
            Some("需求.docx".to_string())
        );
    }

    #[test]
    fn extract_filename_unix_path() {
        assert_eq!(
            extract_filename("/home/user/docs/report.pdf"),
            Some("report.pdf".to_string())
        );
    }

    #[test]
    fn extract_filename_bare_filename() {
        assert_eq!(extract_filename("需求.docx"), Some("需求.docx".to_string()));
    }

    #[test]
    fn extract_filename_with_chinese_extension() {
        assert_eq!(
            extract_filename("D:/资料/01需求分析报告.docx"),
            Some("01需求分析报告.docx".to_string())
        );
    }

    #[test]
    fn extract_filename_empty_string() {
        assert_eq!(extract_filename(""), None);
    }

    // ─── resolve_source_identity_for_cache 单元测试（Issue 1 回归）───
    // 关键：cache key 必须用带扩展名的 identity（"需求.docx"），不能用无扩展名的 title（"需求"）

    #[test]
    fn resolve_source_identity_prefers_raw_source_identity() {
        // 正常路径：raw_source_identity 已写入 → 直接用
        assert_eq!(
            resolve_source_identity_for_cache(Some("需求.docx"), Some("C:/old/需求.docx"), "需求"),
            "需求.docx"
        );
    }

    #[test]
    fn resolve_source_identity_falls_back_to_source_path() {
        // 旧数据：raw_source_identity 为 None，但 source_path 存在
        assert_eq!(
            resolve_source_identity_for_cache(None, Some("C:/原始资料/需求.docx"), "需求"),
            "需求.docx"
        );
    }

    #[test]
    fn resolve_source_identity_falls_back_to_title() {
        // 最差情况：两个都 None → 只能 title 兜底（cache key 会失配，但至少不 panic）
        assert_eq!(
            resolve_source_identity_for_cache(None, None, "需求"),
            "需求"
        );
    }

    #[test]
    fn resolve_source_identity_title_inequivalent_to_raw() {
        // 关键回归测试：旧 bug 是用 title 代替 raw_source_identity
        // 这个测试现在会失败（如果有人再改回去）
        // 用 docx 场景，title 是"需求"，raw 是"需求.docx" → 必须返回 "需求.docx"
        let result = resolve_source_identity_for_cache(Some("需求.docx"), None, "需求");
        assert_ne!(result, "需求", "不能用 title 代替 raw_source_identity");
        assert_eq!(result, "需求.docx");
    }

    #[test]
    fn resolve_source_identity_xlsx_real_world() {
        // 真实场景：05需求跟踪矩阵_模板（for V10.0）.xlsx
        let raw = "05需求跟踪矩阵_模板（for V10.0）.xlsx";
        let title = "05需求跟踪矩阵_模板（for V10.0）";
        assert_eq!(
            resolve_source_identity_for_cache(Some(raw), Some("C:/原始/需求.xlsx"), title),
            raw
        );
    }
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
