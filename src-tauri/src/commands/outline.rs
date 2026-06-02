//! 研究大纲节点的 Tauri 命令包装
//!
//! 所有写操作完成后会通过 Tauri 事件系统发送 "outline:changed" 通知前端。

use tauri::{AppHandle, Emitter, State};

use crate::app_state::AppState;
use crate::services::outline::OutlineNode;

/// 创建大纲节点。
#[tauri::command]
pub async fn create_outline_node(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: i64,
    parent_id: Option<i64>,
    content: String,
) -> Result<OutlineNode, String> {
    let store = state
        .outline_store
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    let node = store.create_node(session_id, parent_id, &content)?;
    app.emit("outline:changed", session_id).ok();
    Ok(node)
}

/// 更新大纲节点（部分更新）。
#[tauri::command]
pub async fn update_outline_node(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    content: Option<String>,
    notes: Option<String>,
    tags: Option<String>,
    collapsed: Option<bool>,
    completed: Option<bool>,
    marker: Option<String>,
    priority: Option<String>,
    note: Option<String>,
) -> Result<OutlineNode, String> {
    let store = state
        .outline_store
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    let node = store.update_node(
        id,
        content.as_deref(),
        notes.as_deref(),
        tags.as_deref(),
        collapsed,
        completed,
        marker.as_deref(),
        priority.as_deref(),
        note.as_deref(),
    )?;
    app.emit("outline:changed", node.session_id).ok();
    Ok(node)
}

/// 删除大纲节点及其所有后代（子树删除）。
#[tauri::command]
pub async fn delete_outline_node(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    session_id: i64,
) -> Result<(), String> {
    let store = state
        .outline_store
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.delete_node_subtree(id)?;
    app.emit("outline:changed", session_id).ok();
    Ok(())
}

/// 移动大纲节点到新的父节点下。
#[tauri::command]
pub async fn move_outline_node(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    new_parent_id: Option<i64>,
    new_sort_order: f64,
) -> Result<OutlineNode, String> {
    let store = state
        .outline_store
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    let node = store.move_node(id, new_parent_id, new_sort_order)?;
    app.emit("outline:changed", node.session_id).ok();
    Ok(node)
}

/// 获取指定 session 的完整大纲树（平铺列表）。
#[tauri::command]
pub async fn get_outline_tree(
    state: State<'_, AppState>,
    session_id: i64,
) -> Result<Vec<OutlineNode>, String> {
    let store = state
        .outline_store
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.get_tree(session_id)
}

/// 导出大纲为指定格式的字符串。
#[tauri::command]
pub async fn export_outline(
    state: State<'_, AppState>,
    session_id: i64,
    format: String,
) -> Result<String, String> {
    let store = state
        .outline_store
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.export_outline(session_id, &format)
}

/// 获取大纲统计信息。
#[tauri::command]
pub async fn get_outline_stats(
    state: State<'_, AppState>,
    session_id: i64,
) -> Result<serde_json::Value, String> {
    let store = state
        .outline_store
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.get_outline_stats(session_id)
}
