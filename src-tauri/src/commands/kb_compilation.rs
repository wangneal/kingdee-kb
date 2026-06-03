//! 知识编译开关的 Tauri Command

use crate::app_state::AppState;

/// 获取知识编译开关状态
#[tauri::command]
pub async fn get_kb_compilation_enabled(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    let store = state.metadata.lock().map_err(|e| format!("获取 metadata 锁失败: {}", e))?;
    store.get_kb_compilation_enabled()
}

/// 设置知识编译开关状态
#[tauri::command]
pub async fn set_kb_compilation_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    let store = state.metadata.lock().map_err(|e| format!("获取 metadata 锁失败: {}", e))?;
    store.set_kb_compilation_enabled(enabled)
}
