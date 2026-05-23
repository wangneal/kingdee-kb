//! Document generation service
//!
//! High-level API that orchestrates template filling:
//! 1. Routes to docx_filler or xlsx_filler based on file extension
//! 2. Calls LLM for `fill_strategy: "ai"` fields
//! 3. Validates required fields
//! 4. Merges user-provided + LLM-generated + default field values

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::docx_filler;
use super::llm_service::LLMService;
use super::template_schema::SchemaField;
use super::xlsx_filler;

/// Information about a missing required field after document generation.
///
/// Provides diagnostic detail for the frontend to display to the user,
/// explaining why a field was not filled and what it represents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingField {
    /// Field name (matches `{field_name}` placeholder)
    pub name: String,
    /// Human-readable description of what this field is for
    pub description: String,
    /// Reason why the field was not filled
    pub reason: String,
}

/// Result of document generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedDoc {
    /// Absolute path to the generated file
    pub output_path: String,
    /// Number of field replacements made
    pub fields_filled: usize,
    /// Fields that were filled by the user
    pub user_fields: Vec<String>,
    /// Fields that were filled by LLM
    pub ai_fields: Vec<String>,
    /// Required fields that were not filled (warnings) — simple names for backward compat
    pub missing_fields: Vec<String>,
    /// Detailed missing field information with reasons (new in Phase 11)
    #[serde(default)]
    pub missing_fields_detail: Vec<MissingField>,
}

/// Request to generate a document from a template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateDocRequest {
    /// Absolute path to the template file (.docx or .xlsx)
    pub template_path: String,
    /// Absolute path for the output file
    pub output_path: String,
    /// User-provided field values (key: field_name, value: field_value)
    pub fields: HashMap<String, String>,
    /// Schema fields with fill_strategy info (optional; if provided, enables LLM filling)
    pub schema_fields: Option<Vec<SchemaField>>,
    /// Project name for LLM context (optional)
    pub project_name: Option<String>,
    /// Additional context for LLM field generation (optional)
    pub context: Option<String>,
}

/// Generate a document by filling a template with field values.
///
/// This is the main entry point for document generation. It:
/// 1. Determines template type from file extension
/// 2. Fills `user` strategy fields from `request.fields`
/// 3. If `schema_fields` provided, calls LLM for `ai`/`llm` strategy fields
/// 4. Validates required fields
/// 5. Delegates to `fill_docx` or `fill_xlsx` for actual template filling
///
/// Returns a `GeneratedDoc` with metadata about the generation.
pub async fn generate_document(
    request: GenerateDocRequest,
    llm: &LLMService,
) -> Result<GeneratedDoc, String> {
    let template_path = PathBuf::from(&request.template_path);
    let output_path = PathBuf::from(&request.output_path);

    // Validate template exists
    if !template_path.exists() {
        return Err(format!(
            "Template file not found: {}",
            template_path.display()
        ));
    }

    // Determine format from extension
    let ext = template_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Start with user-provided fields
    let mut all_fields: HashMap<String, String> = request.fields.clone();

    // Track which fields came from where
    let user_fields: Vec<String> = request.fields.keys().cloned().collect();
    let mut ai_fields: Vec<String> = Vec::new();

    // If schema provided, call LLM for ai/llm strategy fields
    if let Some(ref schema_fields) = request.schema_fields {
        let llm_fields: Vec<&SchemaField> = schema_fields
            .iter()
            .filter(|f| f.fill_strategy == "ai" || f.fill_strategy == "llm")
            .filter(|f| !all_fields.contains_key(&f.name)) // Skip if user already provided
            .collect();

        if !llm_fields.is_empty() {
            match generate_llm_fields(
                llm,
                &llm_fields,
                &request.project_name,
                &request.context,
            )
            .await
            {
                Ok(generated) => {
                    for (name, value) in generated {
                        all_fields.insert(name.clone(), value);
                        ai_fields.push(name);
                    }
                }
                Err(e) => {
                    // LLM failure is non-fatal; log and continue with available fields
                    eprintln!("LLM field generation failed (non-fatal): {}", e);
                }
            }
        }

        // Apply defaults for fields with default values
        for field in schema_fields {
            if !all_fields.contains_key(&field.name) {
                if let Some(ref default_val) = field.default {
                    all_fields.insert(field.name.clone(), default_val.clone());
                }
            }
        }
    }

    // Check required fields — build detailed missing field info
    let mut missing_fields: Vec<String> = Vec::new();
    let mut missing_fields_detail: Vec<MissingField> = Vec::new();
    if let Some(ref schema_fields) = request.schema_fields {
        for field in schema_fields {
            if field.required && !all_fields.contains_key(&field.name) {
                missing_fields.push(field.name.clone());

                // Determine reason based on fill_strategy
                let reason = match field.fill_strategy.as_str() {
                    "ai" | "llm" => "LLM 生成失败或返回空值".to_string(),
                    "kb" => "知识库中未找到相关信息".to_string(),
                    "user" => "用户未填写".to_string(),
                    "default" => "未配置默认值".to_string(),
                    _ => "未知原因".to_string(),
                };

                let description = field
                    .description
                    .clone()
                    .unwrap_or_else(|| format!("字段 {}", field.name));

                missing_fields_detail.push(MissingField {
                    name: field.name.clone(),
                    description,
                    reason,
                });
            }
        }
    }

    // Fill the template
    let fields_filled = match ext.as_str() {
        "docx" => docx_filler::fill_docx(&template_path, &all_fields, &output_path)?,
        "xlsx" => xlsx_filler::fill_xlsx(&template_path, &all_fields, &output_path)?,
        _ => return Err(format!("Unsupported template format: .{}", ext)),
    };

    Ok(GeneratedDoc {
        output_path: output_path.to_string_lossy().to_string(),
        fields_filled,
        user_fields,
        ai_fields,
        missing_fields,
        missing_fields_detail,
    })
}

/// Fill a template directly (synchronous, no LLM).
///
/// Simple version that just replaces fields without LLM generation or validation.
/// Use `generate_document` for the full pipeline with LLM support.
pub fn fill_template(
    template_path: &Path,
    fields: &HashMap<String, String>,
    output_path: &Path,
) -> Result<GeneratedDoc, String> {
    if !template_path.exists() {
        return Err(format!(
            "Template file not found: {}",
            template_path.display()
        ));
    }

    let ext = template_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let fields_filled = match ext.as_str() {
        "docx" => docx_filler::fill_docx(template_path, fields, output_path)?,
        "xlsx" => xlsx_filler::fill_xlsx(template_path, fields, output_path)?,
        _ => return Err(format!("Unsupported template format: .{}", ext)),
    };

    let user_fields: Vec<String> = fields.keys().cloned().collect();

    Ok(GeneratedDoc {
        output_path: output_path.to_string_lossy().to_string(),
        fields_filled,
        user_fields,
        ai_fields: Vec::new(),
        missing_fields: Vec::new(),
        missing_fields_detail: Vec::new(),
    })
}

/// Call LLM to generate values for AI-strategy fields.
///
/// Builds a prompt with field names, types, and descriptions, then asks the LLM
/// to return a JSON object mapping field names to values.
async fn generate_llm_fields(
    llm: &LLMService,
    fields: &[&SchemaField],
    project_name: &Option<String>,
    context: &Option<String>,
) -> Result<HashMap<String, String>, String> {
    use super::llm_service::ChatMessage;

    // Build field descriptions for the prompt
    let field_descriptions: Vec<String> = fields
        .iter()
        .map(|f| {
            let mut desc = format!("- {} (类型: {})", f.name, f.field_type);
            if let Some(ref d) = f.description {
                desc.push_str(&format!(": {}", d));
            }
            if f.required {
                desc.push_str(" [必填]");
            }
            desc
        })
        .collect();

    let mut system_prompt = "你是一个文档字段填充助手。根据提供的项目信息和上下文，为文档模板中的字段生成合适的值。\n\
        请严格以JSON格式返回，格式为 {\"字段名\": \"字段值\", ...}。\n\
        只返回JSON，不要添加任何其他文字。"
        .to_string();

    if let Some(ref project) = project_name {
        system_prompt.push_str(&format!("\n\n项目名称: {}", project));
    }

    let mut user_prompt = "请为以下文档字段生成合适的值：\n\n".to_string();
    user_prompt.push_str(&field_descriptions.join("\n"));

    if let Some(ref ctx) = context {
        user_prompt.push_str(&format!("\n\n补充信息:\n{}", ctx));
    }

    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system_prompt,
        },
        ChatMessage {
            role: "user".to_string(),
            content: user_prompt,
        },
    ];

    // Use LLM service's configured settings
    let config = llm.get_config()?;
    let response = llm.chat_completion(&messages, &config).await?;

    // Parse JSON response
    let json_str = extract_json_from_response(&response);
    let generated: HashMap<String, String> =
        serde_json::from_str(&json_str).map_err(|e| {
            format!(
                "Failed to parse LLM response as JSON: {}. Response: {}",
                e, response
            )
        })?;

    Ok(generated)
}

/// Extract JSON object from LLM response (handles markdown code blocks).
fn extract_json_from_response(response: &str) -> String {
    let trimmed = response.trim();

    // Try to extract from ```json ... ``` code block
    if let Some(start) = trimmed.find("```json") {
        let json_start = start + 7;
        if let Some(end) = trimmed[json_start..].find("```") {
            return trimmed[json_start..json_start + end].trim().to_string();
        }
    }

    // Try to extract from ``` ... ``` code block (no language tag)
    if let Some(start) = trimmed.find("```") {
        let json_start = start + 3;
        // Skip optional language tag on the same line
        let content_start = if let Some(newline) = trimmed[json_start..].find('\n') {
            json_start + newline + 1
        } else {
            json_start
        };
        if let Some(end) = trimmed[content_start..].find("```") {
            return trimmed[content_start..content_start + end].trim().to_string();
        }
    }

    // Try to find raw JSON object
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if end > start {
                return trimmed[start..=end].to_string();
            }
        }
    }

    // Return as-is if no JSON found
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_from_code_block() {
        let response = "```json\n{\"项目名称\": \"测试项目\"}\n```";
        let result = extract_json_from_response(response);
        assert_eq!(result, "{\"项目名称\": \"测试项目\"}");
    }

    #[test]
    fn test_extract_json_raw() {
        let response = "这是结果：{\"项目名称\": \"测试项目\"} 以上。";
        let result = extract_json_from_response(response);
        assert_eq!(result, "{\"项目名称\": \"测试项目\"}");
    }

    #[test]
    fn test_extract_json_no_json() {
        let response = "no json here";
        let result = extract_json_from_response(response);
        assert_eq!(result, "no json here");
    }

    #[test]
    fn test_extract_json_unnamed_code_block() {
        let response = "```\n{\"key\": \"value\"}\n```";
        let result = extract_json_from_response(response);
        assert_eq!(result, "{\"key\": \"value\"}");
    }
}
