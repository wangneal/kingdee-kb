use std::path::PathBuf;
use tauri::State;

use crate::app_state::AppState;
use crate::services::doc_generator::GenerateDocRequest;
use crate::services::product_store::ProductMeta;

/// 列出产品，可按项目筛选。
#[tauri::command]
pub async fn list_products(
    state: State<'_, AppState>,
    project: Option<String>,
) -> Result<Vec<ProductMeta>, String> {
    let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.list(project.as_deref(), None, None)
}

/// 根据 ID 获取单个产品。
#[tauri::command]
pub async fn get_product(
    state: State<'_, AppState>,
    id: i64,
) -> Result<ProductMeta, String> {
    let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store
        .get(id)?
        .ok_or_else(|| format!("Product not found: {}", id))
}

/// 删除产品及其所有版本。
#[tauri::command]
pub async fn delete_product(
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.delete(id)
}

/// 将产品的输出文件导出到目标目录。
#[tauri::command]
pub async fn export_product(
    state: State<'_, AppState>,
    id: i64,
    target_dir: String,
) -> Result<String, String> {
    let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.export_product(id, &target_dir)
}

/// 使用更新的字段值重新生成产品。
#[tauri::command]
pub async fn regenerate_product(
    state: State<'_, AppState>,
    id: i64,
    updated_fields: std::collections::HashMap<String, String>,
) -> Result<ProductMeta, String> {
    let (product_output_path, original_input, _latest_input_data) = {
        let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;

        let product = store
            .get(id)?
            .ok_or_else(|| format!("Product not found: {}", id))?;

        let latest = store
            .get_latest_version(id)?
            .ok_or_else(|| format!("No versions found for product: {}", id))?;

        let original_input: serde_json::Value =
            serde_json::from_str(&latest.input_data)
                .unwrap_or_else(|_| serde_json::json!({}));

        (product.output_path.clone(), original_input, latest.input_data.clone())
    };

    let template_path = original_input
        .get("template_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Original input missing template_path".to_string())?
        .to_string();

    let schema_fields = original_input
        .get("schema_fields")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let project_name = original_input
        .get("project_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let context = original_input
        .get("context")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let timestamp = {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("{}", now)
    };
    let output_dir = PathBuf::from(&product_output_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let stem = PathBuf::from(&product_output_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output")
        .to_string();
    let ext = PathBuf::from(&product_output_path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("docx")
        .to_string();
    let new_output_path = output_dir
        .join(format!("{}_v{}.{}", stem, timestamp, ext))
        .to_string_lossy()
        .to_string();

    let request = GenerateDocRequest {
        template_path,
        output_path: new_output_path.clone(),
        fields: updated_fields.clone(),
        schema_fields,
        project_name,
        context,
    };

    let result = crate::services::doc_generator::generate_document(request, &state.llm).await?;

    let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    let input_json = serde_json::to_string(&serde_json::json!({
        "template_path": original_input.get("template_path"),
        "fields": updated_fields,
        "schema_fields": original_input.get("schema_fields"),
        "project_name": original_input.get("project_name"),
        "context": original_input.get("context"),
    }))
    .unwrap_or_else(|_| "{}".to_string());

    store.add_version(id, &input_json, &result.output_path)?;

    store
        .get(id)?
        .ok_or_else(|| format!("Product not found after regeneration: {}", id))
}
