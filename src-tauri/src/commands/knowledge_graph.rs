//! 知识图谱 Tauri 命令
//!
//! ⚠️ 实验性能力 — 阶段五独立计划，wiki_pages 数据充足后再纳入主流程。
//! 当前不依赖也不影响主搜索链路，仅供探索性使用。

use tauri::State;

use crate::app_state::AppState;
use crate::services::knowledge_graph::{FullGraph, GraphNeighbor, GraphRecommendation, GraphStats};

/// 构建/重建项目知识图谱（4 信号：wikilink、tag 共现、source 共源、co_citation）。
/// 返回插入的边数。
///
/// **分两阶段**：
/// 1. Backfill（事务外）：修复历史空 wikilinks
/// 2. Build（事务内）：清空旧图 + 4 信号重建
///
/// 这样拆分避免大项目下长事务阻塞 wiki_pages 写入。
///
/// ⚠️ 实验性能力 — 不影响主流程，仅供探索性使用。
#[tauri::command]
pub async fn build_knowledge_graph(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<usize, String> {
    // 预检：扫描项目 wiki_pages 状态，让 0 边现象有据可查
    let (total_pages, pages_with_wikilinks, sample_wikilinks, sample_content_has_brackets) = {
        let wiki_store = state
            .wiki_pages
            .lock()
            .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
        wiki_store.diagnose_wikilinks(project_id)?
    };
    tracing::info!(
        "知识图谱构建预检: project={} pages={} 非空wikilinks={} 含[[..]]={}",
        project_id, total_pages, pages_with_wikilinks, sample_content_has_brackets
    );
    for s in &sample_wikilinks {
        tracing::info!("  - 样例 wikilinks: {}", s);
    }
    if total_pages == 0 {
        tracing::warn!("项目 {} 没有任何 wiki_pages，无法构建图谱", project_id);
    } else if pages_with_wikilinks == 0 && sample_content_has_brackets == 0 {
        tracing::warn!(
            "项目 {} 的 {} 个页面均无 `[[slug]]` 引用，LLM 可能没遵循提示词要求",
            project_id, total_pages
        );
    }

    let store = state
        .graph_store
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    // 阶段 1：backfill（独立事务，不阻塞 wiki_pages 写入）
    let backfilled = store.backfill_empty_wikilinks(project_id)?;
    // 阶段 2：4 信号构建（事务内，原子性）
    let inserted = store.build_knowledge_graph(project_id)?;
    tracing::info!(
        "知识图谱构建完成: project={} backfill={} insert={}",
        project_id, backfilled, inserted
    );
    Ok(inserted)
}

/// 获取某页面的直接邻居（1 跳）。
///
/// ⚠️ 实验性能力 — 不影响主流程，仅供探索性使用。
#[tauri::command]
pub async fn get_graph_neighbors(
    state: State<'_, AppState>,
    project_id: i64,
    slug: String,
) -> Result<Vec<GraphNeighbor>, String> {
    let store = state
        .graph_store
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.get_neighbors(project_id, &slug)
}

/// 获取项目知识图谱统计信息（边数、节点数、信号分布、平均度数）。
///
/// ⚠️ 实验性能力 — 不影响主流程，仅供探索性使用。
#[tauri::command]
pub async fn get_graph_stats(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<GraphStats, String> {
    let store = state
        .graph_store
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.get_graph_stats(project_id)
}

/// 获取项目完整图数据（所有节点和边），用于前端可视化。
///
/// ⚠️ 实验性能力 — 不影响主流程，仅供探索性使用。
#[tauri::command]
pub async fn get_full_graph(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<FullGraph, String> {
    let store = state
        .graph_store
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.get_full_graph(project_id)
}

/// 图扩展检索：给定页面，推荐相关页面。
/// 使用递归遍历获取多跳邻居，按组合权重排序，返回 top K。
///
/// ⚠️ 实验性能力 — 不影响主流程，仅供探索性使用。
#[tauri::command]
pub async fn graph_expand_search(
    state: State<'_, AppState>,
    project_id: i64,
    slug: String,
    max_depth: Option<i64>,
    max_results: Option<i64>,
    min_weight: Option<f64>,
) -> Result<Vec<GraphRecommendation>, String> {
    let store = state
        .graph_store
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.graph_expand_search(
        project_id,
        &slug,
        max_depth.unwrap_or(2),
        max_results.unwrap_or(10),
        min_weight.unwrap_or(0.3),
    )
}
