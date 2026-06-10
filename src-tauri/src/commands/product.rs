use tauri::State;

use crate::app_state::AppState;
use crate::services::product_store::ProductMeta;

/// 列出产品，可按项目筛选。
#[tauri::command]
pub async fn list_products(
    state: State<'_, AppState>,
    project_id: Option<i64>,
) -> Result<Vec<ProductMeta>, String> {
    let store = state
        .products
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.list(project_id, None, None)
}

/// 根据 ID 获取单个产品。
#[tauri::command]
pub async fn get_product(state: State<'_, AppState>, id: i64) -> Result<ProductMeta, String> {
    let store = state
        .products
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store
        .get(id)?
        .ok_or_else(|| format!("Product not found: {}", id))
}

/// 删除产品及其所有版本。
#[tauri::command]
pub async fn delete_product(
    state: State<'_, AppState>,
    id: i64,
    project_id: Option<i64>,
) -> Result<(), String> {
    let store = state
        .products
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.delete(id, project_id)
}

/// 将产品的输出文件导出到目标目录。
#[tauri::command]
pub async fn export_product(
    state: State<'_, AppState>,
    id: i64,
    target_dir: String,
    project_id: Option<i64>,
) -> Result<String, String> {
    let store = state
        .products
        .lock()
        .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.export_product(id, &target_dir, project_id)
}
