//! 知识图谱 Tauri 命令

use tauri::State;

use crate::app_state::AppState;
use crate::services::knowledge_graph::{GraphNeighbor, GraphPath, GraphRecommendation, GraphStats};

/// 构建/重建项目知识图谱（4 信号：wikilink、tag 共现、source 共源、co_citation）。
/// 返回插入的边数。
#[tauri::command]
pub async fn build_knowledge_graph(
    state: State<'_, AppState>,
    project: String,
) -> Result<usize, String> {
    let store = state
        .graph_store
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.build_knowledge_graph(&project)
}

/// 递归图遍历：从 seed 页面出发，沿边展开 N 层。
#[tauri::command]
pub async fn traverse_graph(
    state: State<'_, AppState>,
    project: String,
    slug: String,
    max_depth: Option<i64>,
    min_weight: Option<f64>,
) -> Result<Vec<GraphPath>, String> {
    let store = state
        .graph_store
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.traverse_graph(&project, &slug, max_depth.unwrap_or(2), min_weight.unwrap_or(0.1))
}

/// 获取某页面的直接邻居（1 跳）。
#[tauri::command]
pub async fn get_graph_neighbors(
    state: State<'_, AppState>,
    project: String,
    slug: String,
) -> Result<Vec<GraphNeighbor>, String> {
    let store = state
        .graph_store
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.get_neighbors(&project, &slug)
}

/// 获取项目知识图谱统计信息（边数、节点数、信号分布、平均度数）。
#[tauri::command]
pub async fn get_graph_stats(
    state: State<'_, AppState>,
    project: String,
) -> Result<GraphStats, String> {
    let store = state
        .graph_store
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.get_graph_stats(&project)
}

/// 图扩展检索：给定页面，推荐相关页面。
/// 使用递归遍历获取多跳邻居，按组合权重排序，返回 top K。
#[tauri::command]
pub async fn graph_expand_search(
    state: State<'_, AppState>,
    project: String,
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
        &project,
        &slug,
        max_depth.unwrap_or(2),
        max_results.unwrap_or(10),
        min_weight.unwrap_or(0.3),
    )
}
