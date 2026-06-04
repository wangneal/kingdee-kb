use std::path::PathBuf;
use tauri::State;

use crate::app_state::AppState;

fn resolve_product_project_id(state: &AppState, project_id: Option<i64>) -> Result<i64, String> {
    if let Some(project_id) = project_id {
        return Ok(project_id);
    }

    let store = state.project_store.lock().map_err(|e| e.to_string())?;
    store.ensure_default_project()
}
use crate::services::deliverable_recipes::DeliverableRecipe;
use crate::services::doc_generator::{
    GenerateDocRequest, GeneratedDoc, RecipeDocRequest, RecipeDocResult,
};
use crate::services::smart_completion::{SmartFillRequest, SmartFillResult};
use crate::services::template_docx::FieldInfo;
use crate::services::template_scanner::TemplateInfo;
use crate::services::template_schema::TemplateSchema;

/// 扫描模板目录并返回按阶段排序的所有模板。
#[tauri::command]
pub async fn scan_templates(template_dir: Option<String>) -> Result<Vec<TemplateInfo>, String> {
    let root = match template_dir {
        Some(dir) => PathBuf::from(dir),
        None => {
            let home = dirs::home_dir().ok_or("Cannot find home directory")?;
            home.join(".kingdee-kb").join("templates")
        }
    };
    crate::services::template_scanner::scan_templates(&root)
}

/// 从 .docx 或 .xlsx 模板文件中提取字段占位符。
#[tauri::command]
pub async fn extract_template_fields(file_path: String) -> Result<Vec<FieldInfo>, String> {
    let path = PathBuf::from(&file_path);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "docx" => crate::services::template_docx::extract_docx_fields(&path),
        "xlsx" => {
            let xlsx_fields = crate::services::template_xlsx::extract_xlsx_fields(&path)?;
            Ok(xlsx_fields
                .into_iter()
                .map(|f| FieldInfo {
                    name: f.name,
                    field_type: f.field_type,
                    context: f.cell_refs.join(", "),
                    count: f.count,
                    source: f.source,
                })
                .collect())
        }
        _ => Err(format!("Unsupported template format: .{}", ext)),
    }
}

/// Generate a YAML schema for a template.
#[tauri::command]
pub async fn get_template_schema(
    template_id: String,
    template_name: String,
    file_path: String,
    phase: String,
    write_sidecar: Option<bool>,
) -> Result<TemplateSchema, String> {
    let path = PathBuf::from(&file_path);

    if let Some(schema) = crate::services::template_schema::load_schema_sidecar(&path)? {
        return Ok(schema);
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let schema = match ext.as_str() {
        "docx" => {
            let fields = crate::services::template_docx::extract_docx_fields(&path)?;
            crate::services::template_schema::generate_schema_from_docx(
                &template_id,
                &template_name,
                &phase,
                &fields,
            )
        }
        "xlsx" => {
            let fields = crate::services::template_xlsx::extract_xlsx_fields(&path)?;
            crate::services::template_schema::generate_schema_from_xlsx(
                &template_id,
                &template_name,
                &phase,
                &fields,
            )
        }
        _ => return Err(format!("Unsupported template format: .{}", ext)),
    };

    if write_sidecar.unwrap_or(false) {
        crate::services::template_schema::write_schema_sidecar(&path, &schema)?;
    }

    Ok(schema)
}

/// Generate templates.json index file listing all templates with categories.
#[tauri::command]
pub async fn generate_templates_index(template_dir: Option<String>) -> Result<String, String> {
    let root = match template_dir {
        Some(dir) => PathBuf::from(dir),
        None => {
            let home = dirs::home_dir().ok_or("Cannot find home directory")?;
            home.join(".kingdee-kb").join("templates")
        }
    };
    let output_path = root.join("templates.json");
    crate::services::template_scanner::write_templates_json(&root, &output_path)
}

/// 使用字段值填充模板（无 LLM，简单替换）。
#[tauri::command]
pub async fn fill_template(
    template_path: String,
    fields: std::collections::HashMap<String, String>,
    output_path: String,
) -> Result<GeneratedDoc, String> {
    crate::services::doc_generator::fill_template(
        std::path::Path::new(&template_path),
        &fields,
        std::path::Path::new(&output_path),
    )
}

/// 通过填充模板生成文档，可选 LLM 字段生成。
#[tauri::command]
pub async fn generate_doc(
    state: State<'_, AppState>,
    request: GenerateDocRequest,
) -> Result<GeneratedDoc, String> {
    let template_path = request.template_path.clone();
    let product_project_id = resolve_product_project_id(state.inner(), request.project_id)?;
    let user_field_count = request.fields.len() as i64;
    let schema_field_count = request
        .schema_fields
        .as_ref()
        .map(|fields| fields.len() as i64)
        .unwrap_or(0);
    let input_json = serde_json::to_string(&request).unwrap_or_else(|_| "{}".to_string());

    let result = crate::services::doc_generator::generate_document(request, &state.llm).await?;

    let template_name = std::path::Path::new(&template_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("generated_document");
    {
        let store = state
            .products
            .lock()
            .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
        store.create(
            &template_path,
            template_name,
            product_project_id,
            &result.output_path,
            user_field_count.max(schema_field_count),
            result.ai_fields.len() as i64,
            &input_json,
        )?;
    }

    Ok(result)
}

/// 使用交付物配方生成文档（配方感知生成）。
#[tauri::command]
pub async fn generate_recipe_doc_cmd(
    state: State<'_, AppState>,
    request: RecipeDocRequest,
) -> Result<RecipeDocResult, String> {
    let recipe_id = request.recipe_id.clone();
    let project = request.project_name.clone().unwrap_or_default();
    let product_project_id = resolve_product_project_id(state.inner(), request.project_id)?;
    let user_field_count = request.fields.len() as i64;
    let schema_fields_json: String =
        serde_json::to_string(&request.schema_fields).unwrap_or_else(|_| "[]".to_string());

    let result = crate::services::doc_generator::generate_recipe_doc(
        request,
        &state.llm,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
    .await?;

    let input_json = serde_json::to_string(&serde_json::json!({
        "recipe_id": recipe_id,
        "schema_fields": schema_fields_json,
        "project_name": project,
        "project_id": product_project_id,
    }))
    .unwrap_or_else(|_| "{}".to_string());

    {
        let store = state
            .products
            .lock()
            .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
        let _ = store.create(
            &recipe_id,
            &result.recipe_name,
            product_project_id,
            &result.doc.output_path,
            user_field_count,
            result.doc.ai_fields.len() as i64,
            &input_json,
        );
    }

    Ok(result)
}

/// 便捷命令：从调研笔记生成文档。
#[tauri::command]
pub async fn generate_from_research(
    state: State<'_, AppState>,
    recipe_id: String,
    template_path: String,
    output_path: String,
    fields: std::collections::HashMap<String, String>,
    schema_fields: Option<Vec<crate::services::template_schema::SchemaField>>,
    project_name: Option<String>,
    research_notes: String,
    project_id: Option<i64>,
) -> Result<RecipeDocResult, String> {
    let request = RecipeDocRequest {
        recipe_id,
        template_path,
        output_path,
        fields,
        schema_fields: schema_fields.unwrap_or_default(),
        project_name,
        context: Some(research_notes),
        project_id,
    };
    crate::services::doc_generator::generate_recipe_doc(
        request,
        &state.llm,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
    .await
}

/// 便捷命令：从会议记录生成文档。
#[tauri::command]
pub async fn generate_from_meeting(
    state: State<'_, AppState>,
    recipe_id: String,
    template_path: String,
    output_path: String,
    fields: std::collections::HashMap<String, String>,
    schema_fields: Option<Vec<crate::services::template_schema::SchemaField>>,
    project_name: Option<String>,
    meeting_transcript: String,
    project_id: Option<i64>,
) -> Result<RecipeDocResult, String> {
    let request = RecipeDocRequest {
        recipe_id,
        template_path,
        output_path,
        fields,
        schema_fields: schema_fields.unwrap_or_default(),
        project_name,
        context: Some(meeting_transcript),
        project_id,
    };
    crate::services::doc_generator::generate_recipe_doc(
        request,
        &state.llm,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
    .await
}

/// 智能填充：使用 hybrid_search + LLM 进行 KB 辅助字段填充
#[tauri::command]
pub async fn smart_fill(
    state: State<'_, AppState>,
    request: SmartFillRequest,
) -> Result<SmartFillResult, String> {
    crate::services::smart_completion::smart_fill(
        request,
        &state.llm,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
    .await
}

/// 探测缺失字段
#[tauri::command]
pub async fn probe_missing_fields(
    state: State<'_, AppState>,
    request: GenerateDocRequest,
) -> Result<GeneratedDoc, String> {
    crate::services::doc_generator::generate_document(request, &state.llm).await
}

/// 根据 template_id 获取交付物配方
#[tauri::command]
pub fn get_deliverable_recipe(template_id: String) -> Result<DeliverableRecipe, String> {
    crate::services::deliverable_recipes::get_recipe_by_template_id(&template_id)
        .ok_or_else(|| format!("No recipe found for template_id: {}", template_id))
}
