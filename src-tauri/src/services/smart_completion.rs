//! Smart completion service — KB-assisted field filling
//!
//! Combines knowledge base search with LLM generation to auto-fill template fields.
//! For each `ai`/`llm` strategy field, searches the knowledge base for relevant context,
//! then passes assembled context + field schema to the LLM for value generation.
//!
//! Returns filled fields along with KB source citations for transparency.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::services::bm25_service::BM25Service;
use crate::services::embedding::EmbeddingService;
use crate::services::hybrid_search::{self, HybridSearchResult};
use crate::services::llm_service::{ChatMessage, LLMService};
use crate::services::metadata::MetadataStore;
use crate::services::template_schema::SchemaField;
use crate::services::vector_index::VectorIndex;

// ─── Types ───

/// Request for smart field completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartFillRequest {
    /// Template ID (for context)
    pub template_id: String,
    /// User-provided input/description for the document (e.g., project name, scope)
    pub user_input: String,
    /// Fields the user has already filled manually (key: field_name, value: field_value)
    pub manual_fields: HashMap<String, String>,
    /// Schema fields with fill_strategy info
    pub schema_fields: Vec<SchemaField>,
    /// Optional project name for KB filtering
    pub project_name: Option<String>,
}

/// Result of smart completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartFillResult {
    /// All field values after completion (user + AI + default)
    pub filled_fields: HashMap<String, String>,
    /// Fields filled by the AI (LLM-generated)
    pub ai_fields: Vec<String>,
    /// Fields that remain empty after completion (required but unfilled)
    pub missing_fields: Vec<String>,
    /// Knowledge base sources used for context (for citation display)
    pub kb_sources: Vec<KBSource>,
}

/// A knowledge base source citation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KBSource {
    /// Document title
    pub title: String,
    /// Section path within the document
    pub section_path: Option<String>,
    /// Content snippet used as context
    pub content_snippet: String,
    /// Relevance score from hybrid search
    pub score: f32,
}

// ─── Constants ───

/// Maximum tokens for KB context in the prompt
const KB_CONTEXT_MAX_TOKENS: u32 = 2048;

/// Maximum number of KB search results to use
const KB_TOP_K: usize = 8;

// ─── Smart Fill ───

/// Perform smart completion: search KB → assemble context → LLM fill fields.
///
/// For each field with `fill_strategy` == `"ai"` or `"llm"` that the user hasn't
/// manually filled, this function:
/// 1. Searches the knowledge base using `user_input` as query
/// 2. Assembles relevant context snippets
/// 3. Sends field schema + KB context to the LLM
/// 4. Parses the JSON response to extract field values
///
/// Returns the completed fields, AI-filled field names, missing fields, and KB source citations.
pub async fn smart_fill(
    request: SmartFillRequest,
    llm: &LLMService,
    embedding: &Mutex<EmbeddingService>,
    vector_index: &Mutex<VectorIndex>,
    bm25: &Mutex<BM25Service>,
    metadata: &Mutex<MetadataStore>,
) -> Result<SmartFillResult, String> {
    // ── Step 1: Identify fields to fill ──
    let llm_fields: Vec<&SchemaField> = request
        .schema_fields
        .iter()
        .filter(|f| f.fill_strategy == "ai" || f.fill_strategy == "llm")
        .filter(|f| !request.manual_fields.contains_key(&f.name))
        .collect();

    // If no fields need AI filling, return early
    if llm_fields.is_empty() {
        let missing = find_missing_fields(&request.schema_fields, &request.manual_fields);
        return Ok(SmartFillResult {
            filled_fields: request.manual_fields.clone(),
            ai_fields: Vec::new(),
            missing_fields: missing,
            kb_sources: Vec::new(),
        });
    }

    // ── Step 2: Search KB for context ──
    let search_query = build_search_query(&request.user_input, &request.project_name, &llm_fields);
    let search_results = hybrid_search::hybrid_search(
        &search_query,
        request.project_name.as_deref(),
        KB_TOP_K,
        embedding,
        vector_index,
        bm25,
        metadata,
    )
    .unwrap_or_default(); // KB search failure is non-fatal

    // ── Step 3: Assemble KB context ──
    let kb_context = assemble_kb_context(&search_results, KB_CONTEXT_MAX_TOKENS);
    let kb_sources: Vec<KBSource> = search_results
        .iter()
        .map(|r| KBSource {
            title: r.title.clone(),
            section_path: r.section_path.clone(),
            content_snippet: truncate_snippet(&r.content, 200),
            score: r.score,
        })
        .collect();

    // ── Step 4: Call LLM to fill fields ──
    let ai_values = generate_fields_with_kb_context(
        llm,
        &llm_fields,
        &kb_context,
        &request.user_input,
        &request.project_name,
    )
    .await?;

    // ── Step 5: Merge results ──
    let mut filled_fields = request.manual_fields.clone();
    let mut ai_fields = Vec::new();

    for (name, value) in ai_values {
        if !value.trim().is_empty() {
            filled_fields.insert(name.clone(), value);
            ai_fields.push(name);
        }
    }

    // Apply defaults for remaining unfilled fields
    for field in &request.schema_fields {
        if !filled_fields.contains_key(&field.name) {
            if let Some(ref default_val) = field.default {
                filled_fields.insert(field.name.clone(), default_val.clone());
            }
        }
    }

    let missing_fields = find_missing_fields(&request.schema_fields, &filled_fields);

    Ok(SmartFillResult {
        filled_fields,
        ai_fields,
        missing_fields,
        kb_sources,
    })
}

// ─── Internal Helpers ───

/// Build a search query from user input + field names that need filling.
fn build_search_query(
    user_input: &str,
    project_name: &Option<String>,
    fields: &[&SchemaField],
) -> String {
    let mut query = String::new();

    if let Some(project) = project_name {
        query.push_str(project);
        query.push(' ');
    }

    query.push_str(user_input);

    // Add field context to make search more relevant
    let field_names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
    if !field_names.is_empty() {
        query.push_str(&format!(" {}", field_names.join(" ")));
    }

    query
}

/// Assemble KB context string from hybrid search results.
///
/// Format: `[来源：title | section]\ncontent\n\n`
fn assemble_kb_context(results: &[HybridSearchResult], max_tokens: u32) -> String {
    let mut context = String::new();

    for result in results {
        let section = result.section_path.as_deref().unwrap_or("（无章节信息）");

        let entry = format!(
            "[来源：{} | {}]\n{}\n\n",
            result.title, section, result.content
        );
        context.push_str(&entry);
    }

    // Truncate to fit token budget
    super::llm_service::truncate_to_tokens(&context, max_tokens)
}

/// Call LLM to generate field values using KB context.
///
/// Builds a system prompt describing the task, includes KB context and field schema,
/// then asks the LLM to return a JSON object mapping field names to values.
async fn generate_fields_with_kb_context(
    llm: &LLMService,
    fields: &[&SchemaField],
    kb_context: &str,
    user_input: &str,
    project_name: &Option<String>,
) -> Result<HashMap<String, String>, String> {
    // Build field descriptions
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

    // System prompt
    let mut system_prompt = "你是一个金蝶ERP实施文档字段填充助手。\n\
        根据提供的知识库内容和用户输入，为文档模板中的字段生成合适的值。\n\
        优先使用知识库中的真实数据，如果知识库中没有相关信息，\n\
        可以根据你的金蝶ERP实施经验生成合理的默认值。\n\
        \n\
        请严格以JSON格式返回，格式为 {\"字段名\": \"字段值\", ...}。\n\
        只返回JSON，不要添加任何其他文字。"
        .to_string();

    if let Some(project) = project_name {
        system_prompt.push_str(&format!("\n\n当前项目: {}", project));
    }

    // User prompt with KB context + fields
    let mut user_prompt = String::new();

    if !kb_context.is_empty() {
        user_prompt.push_str(&format!("知识库检索到的相关内容：\n{}\n\n", kb_context));
    }

    user_prompt.push_str(&format!("用户输入说明: {}\n\n", user_input));
    user_prompt.push_str("请为以下文档字段生成合适的值：\n\n");
    user_prompt.push_str(&field_descriptions.join("\n"));

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

    // Call LLM
    let config = llm.get_active_config()?;
    let response = llm.chat_completion(&messages, &config).await?;

    // Parse JSON response
    let json_str = extract_json_from_response(&response);
    let generated: HashMap<String, String> = serde_json::from_str(&json_str).map_err(|e| {
        format!(
            "Failed to parse LLM response as JSON: {}. Response: {}",
            e, response
        )
    })?;

    Ok(generated)
}

/// Find required fields that are not yet filled.
fn find_missing_fields(
    schema_fields: &[SchemaField],
    filled: &HashMap<String, String>,
) -> Vec<String> {
    schema_fields
        .iter()
        .filter(|f| f.required)
        .filter(|f| {
            !filled.contains_key(&f.name)
                || filled
                    .get(&f.name)
                    .map(|v| v.trim().is_empty())
                    .unwrap_or(true)
        })
        .map(|f| f.name.clone())
        .collect()
}

/// Truncate a text snippet to a maximum number of characters.
fn truncate_snippet(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{}...", truncated)
}

/// Extract JSON from LLM response (handles markdown code blocks).
fn extract_json_from_response(response: &str) -> String {
    let trimmed = response.trim();

    // Try ```json ... ``` code block
    if let Some(start) = trimmed.find("```json") {
        let json_start = start + 7;
        if let Some(end) = trimmed[json_start..].find("```") {
            return trimmed[json_start..json_start + end].trim().to_string();
        }
    }

    // Try ``` ... ``` code block (no language tag)
    if let Some(start) = trimmed.find("```") {
        let json_start = start + 3;
        let content_start = if let Some(newline) = trimmed[json_start..].find('\n') {
            json_start + newline + 1
        } else {
            json_start
        };
        if let Some(end) = trimmed[content_start..].find("```") {
            return trimmed[content_start..content_start + end]
                .trim()
                .to_string();
        }
    }

    // Try raw JSON object
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if end > start {
                return trimmed[start..=end].to_string();
            }
        }
    }

    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_search_query_with_project() {
        let fields = vec![SchemaField {
            name: "实施范围".to_string(),
            field_type: "text".to_string(),
            fill_strategy: "ai".to_string(),
            required: true,
            default: None,
            description: None,
            cell_refs: None,
        }];
        let field_refs: Vec<&SchemaField> = fields.iter().collect();
        let query = build_search_query("星达铜业", &Some("星达铜业".to_string()), &field_refs);
        assert!(query.contains("星达铜业"));
        assert!(query.contains("实施范围"));
    }

    #[test]
    fn test_find_missing_fields() {
        let schema = vec![
            SchemaField {
                name: "项目名称".to_string(),
                field_type: "text".to_string(),
                fill_strategy: "user".to_string(),
                required: true,
                default: None,
                description: None,
                cell_refs: None,
            },
            SchemaField {
                name: "备注".to_string(),
                field_type: "text".to_string(),
                fill_strategy: "ai".to_string(),
                required: false,
                default: None,
                description: None,
                cell_refs: None,
            },
        ];

        let mut filled = HashMap::new();
        filled.insert("备注".to_string(), "一些备注".to_string());

        let missing = find_missing_fields(&schema, &filled);
        assert_eq!(missing, vec!["项目名称"]);
    }

    #[test]
    fn test_truncate_snippet() {
        let text = "这是一段很长的文字内容";
        let result = truncate_snippet(text, 5);
        assert_eq!(result, "这是一段很...");
    }

    #[test]
    fn test_extract_json_from_code_block() {
        let response = "```json\n{\"项目名称\": \"测试\"}\n```";
        assert_eq!(
            extract_json_from_response(response),
            "{\"项目名称\": \"测试\"}"
        );
    }
}
