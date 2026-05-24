//! Document generation service
//!
//! High-level API that orchestrates template filling:
//! 1. Routes to docx_filler or xlsx_filler based on file extension
//! 2. Calls LLM for `fill_strategy: "ai"` fields
//! 3. Validates required fields
//! 4. Merges user-provided + LLM-generated + default field values
//!
//! Recipe-aware generation (`generate_recipe_doc`) additionally:
//! 5. Looks up a DeliverableRecipe by template_id
//! 6. Applies recipe field_overrides to schema
//! 7. Searches KB for `fill_strategy: "kb"` fields
//! 8. Uses recipe-specific system_prompt for LLM generation

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::bm25_service::BM25Service;
use super::deliverable_recipes;
use super::docx_filler;
use super::embedding::EmbeddingService;
use super::hybrid_search;
use super::llm_service::LLMService;
use super::metadata::MetadataStore;
use super::template_schema::SchemaField;
use super::vector_index::VectorIndex;
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

/// A knowledge base source citation for recipe-aware generation.
///
/// Tracks which KB documents contributed context for which fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KbSource {
    /// The field name that was filled using this KB content
    pub field_name: String,
    /// Document titles that contributed context
    pub sources: Vec<String>,
}

/// Request to generate a document using a deliverable recipe.
///
/// This is the primary API for Phase 10 document generation. It combines
/// recipe lookup, KB search, and LLM generation into a single call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeDocRequest {
    /// Deliverable recipe ID (e.g., "investigation_report", "meeting_minutes")
    pub recipe_id: String,
    /// Absolute path to the template file
    pub template_path: String,
    /// Absolute path for the output file
    pub output_path: String,
    /// User-provided field values (key: field_name, value: field_value)
    pub fields: HashMap<String, String>,
    /// Schema fields extracted from the template (before recipe overrides)
    pub schema_fields: Vec<SchemaField>,
    /// Project name for KB filtering and LLM context
    pub project_name: Option<String>,
    /// Additional context for LLM generation
    pub context: Option<String>,
    /// Project ID for KB filtering (optional, uses project_name if not set)
    pub project_id: Option<String>,
}

/// Result of recipe-aware document generation.
///
/// Extends GeneratedDoc with recipe metadata and KB source citations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeDocResult {
    /// The generated document metadata
    pub doc: GeneratedDoc,
    /// The recipe name that was applied (e.g., "调研报告")
    pub recipe_name: String,
    /// Knowledge base sources used for field filling
    pub kb_sources: Vec<KbSource>,
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

    let mut system_prompt = "你是一个金蝶ERP文档字段填充助手。\n\
        【核心规则 — 不得违反】\n\
        1. 根据提供的项目信息和上下文生成字段值，不得编造不存在的数据。\n\
        2. 不确定的内容写「待确认」，不要用模糊套话填充。\n\
        3. 涉及系统配置时给出具体路径（如：系统管理→基础资料→科目→新建）。\n\
        4. 禁止使用「实现高效管理」「优化流程」「加强协同」等无具体操作的表述。\n\
        \n\
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

// ─── Recipe-Aware Document Generation ───

/// KB context token budget for recipe-aware generation
const RECIPE_KB_MAX_TOKENS: u32 = 2048;

/// Number of KB search results to retrieve per query
const RECIPE_KB_TOP_K: usize = 8;

/// Generate a document using a deliverable recipe.
///
/// This is the primary Phase 10 API. It combines:
/// 1. Recipe lookup by `recipe_id` → get field_overrides + system_prompt
/// 2. Apply recipe overrides to schema_fields
/// 3. Search KB for `fill_strategy: "kb"` fields
/// 4. Call LLM for `fill_strategy: "ai"` or `"kb"` fields using recipe system_prompt
/// 5. Fill template with merged user + KB + AI + default values
/// 6. Return RecipeDocResult with KB source citations
///
/// If no recipe matches `recipe_id`, falls back to standard `generate_document`.
pub async fn generate_recipe_doc(
    request: RecipeDocRequest,
    llm: &LLMService,
    embedding: &Mutex<EmbeddingService>,
    vector_index: &Mutex<VectorIndex>,
    bm25: &Mutex<BM25Service>,
    metadata: &Mutex<MetadataStore>,
) -> Result<RecipeDocResult, String> {
    // ── Step 1: Look up recipe ──
    let recipe = deliverable_recipes::get_recipe_by_template_id(&request.recipe_id)
        .ok_or_else(|| format!("No recipe found for recipe_id: {}", request.recipe_id))?;

    let recipe_name = recipe.name.clone();
    let system_prompt = recipe.system_prompt.clone();

    // ── Step 2: Apply recipe overrides to schema ──
    let schema_fields = deliverable_recipes::apply_recipe_overrides(&request.schema_fields, &recipe);

    // ── Step 3: Validate template exists ──
    let template_path = PathBuf::from(&request.template_path);
    let output_path = PathBuf::from(&request.output_path);

    if !template_path.exists() {
        return Err(format!("Template file not found: {}", template_path.display()));
    }

    let ext = template_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // ── Step 4: Start with user-provided fields ──
    let mut all_fields: HashMap<String, String> = request.fields.clone();
    let user_fields: Vec<String> = request.fields.keys().cloned().collect();
    let mut ai_fields: Vec<String> = Vec::new();
    let mut kb_sources: Vec<KbSource> = Vec::new();

    // ── Step 5: Identify KB and AI fields that need filling ──
    let kb_fields: Vec<&SchemaField> = schema_fields
        .iter()
        .filter(|f| f.fill_strategy == "kb")
        .filter(|f| !all_fields.contains_key(&f.name))
        .collect();

    let ai_fields_to_fill: Vec<&SchemaField> = schema_fields
        .iter()
        .filter(|f| f.fill_strategy == "ai" || f.fill_strategy == "llm")
        .filter(|f| !all_fields.contains_key(&f.name))
        .collect();

    // ── Step 6: Search KB for kb-strategy fields ──
    let kb_context = if !kb_fields.is_empty() {
        // Build search query from field names, descriptions, and project context
        let search_query = build_kb_search_query(
            &kb_fields,
            &request.project_name,
            &request.context,
        );

        let search_results = hybrid_search::hybrid_search(
            &search_query,
            request.project_id.as_deref().or(request.project_name.as_deref()),
            RECIPE_KB_TOP_K,
            embedding,
            vector_index,
            bm25,
            metadata,
        )
        .unwrap_or_default(); // KB search failure is non-fatal

        // Record KB sources per field
        for field in &kb_fields {
            let field_sources: Vec<String> = search_results
                .iter()
                .filter(|r| {
                    // Simple relevance check: does the result content or title
                    // contain the field name or its description keywords?
                    let name_match = r.content.contains(&field.name)
                        || r.title.contains(&field.name);
                    let desc_match = field
                        .description
                        .as_ref()
                        .map(|d| {
                            d.chars()
                                .take(10)
                                .collect::<String>()
                                .split_whitespace()
                                .any(|kw| r.content.contains(kw))
                        })
                        .unwrap_or(false);
                    name_match || desc_match
                })
                .map(|r| {
                    format!(
                        "{}{}",
                        r.title,
                        r.section_path
                            .as_ref()
                            .map(|s| format!(" / {}", s))
                            .unwrap_or_default()
                    )
                })
                .collect();

            // If no specific matches, attribute all results as general KB context
            let sources = if field_sources.is_empty() && !search_results.is_empty() {
                search_results
                    .iter()
                    .take(3)
                    .map(|r| {
                        format!(
                            "{}{}",
                            r.title,
                            r.section_path
                                .as_ref()
                                .map(|s| format!(" / {}", s))
                                .unwrap_or_default()
                        )
                    })
                    .collect()
            } else {
                field_sources
            };

            kb_sources.push(KbSource {
                field_name: field.name.clone(),
                sources,
            });
        }

        // Assemble KB context for LLM
        assemble_kb_context(&search_results, RECIPE_KB_MAX_TOKENS)
    } else {
        String::new()
    };

    // ── Step 7: Call LLM for AI + KB fields using recipe system_prompt ──
    // Combine AI fields and KB fields — KB fields also need LLM to synthesize
    let mut fields_for_llm: Vec<&SchemaField> = ai_fields_to_fill;
    fields_for_llm.extend(kb_fields.iter().copied());
    // Deduplicate by field name
    let mut seen_names = std::collections::HashSet::new();
    fields_for_llm.retain(|f| seen_names.insert(f.name.clone()));

    if !fields_for_llm.is_empty() {
        match generate_llm_fields_with_recipe(
            llm,
            &fields_for_llm,
            &system_prompt,
            &kb_context,
            &request.project_name,
            &request.context,
        )
        .await
        {
            Ok(generated) => {
                for (name, value) in generated {
                    if !value.trim().is_empty() {
                        all_fields.insert(name.clone(), value);
                        ai_fields.push(name);
                    }
                }
            }
            Err(e) => {
                // LLM failure is non-fatal; log and continue with available fields
                eprintln!("Recipe LLM field generation failed (non-fatal): {}", e);
            }
        }
    }

    // ── Step 8: Apply defaults for remaining unfilled fields ──
    for field in &schema_fields {
        if !all_fields.contains_key(&field.name) {
            if let Some(ref default_val) = field.default {
                all_fields.insert(field.name.clone(), default_val.clone());
            }
        }
    }

    // ── Step 9: Check required fields ──
    let mut missing_fields: Vec<String> = Vec::new();
    let mut missing_fields_detail: Vec<MissingField> = Vec::new();
    for field in &schema_fields {
        if field.required && !all_fields.contains_key(&field.name) {
            missing_fields.push(field.name.clone());
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

    // ── Step 10: Fill the template ──
    let fields_filled = match ext.as_str() {
        "docx" => docx_filler::fill_docx(&template_path, &all_fields, &output_path)?,
        "xlsx" => xlsx_filler::fill_xlsx(&template_path, &all_fields, &output_path)?,
        _ => return Err(format!("Unsupported template format: .{}", ext)),
    };

    let doc = GeneratedDoc {
        output_path: output_path.to_string_lossy().to_string(),
        fields_filled,
        user_fields,
        ai_fields,
        missing_fields,
        missing_fields_detail,
    };

    Ok(RecipeDocResult {
        doc,
        recipe_name,
        kb_sources,
    })
}

/// Build a search query for KB-strategy fields.
///
/// Combines field names, their descriptions, project name, and user context
/// into a single search query that captures the most relevant KB content.
fn build_kb_search_query(
    kb_fields: &[&SchemaField],
    project_name: &Option<String>,
    context: &Option<String>,
) -> String {
    let mut query = String::new();

    if let Some(project) = project_name {
        query.push_str(project);
        query.push(' ');
    }

    // Add field names and descriptions as search keywords
    let field_keywords: Vec<String> = kb_fields
        .iter()
        .map(|f| {
            let mut kw = f.name.clone();
            if let Some(ref desc) = f.description {
                // Take first 10 chars of description as additional keywords
                let desc_prefix: String = desc.chars().take(10).collect();
                kw.push(' ');
                kw.push_str(&desc_prefix);
            }
            kw
        })
        .collect();
    query.push_str(&field_keywords.join(" "));

    if let Some(ctx) = context {
        query.push(' ');
        // Take first 50 chars of context to avoid overly long queries
        let ctx_prefix: String = ctx.chars().take(50).collect();
        query.push_str(&ctx_prefix);
    }

    query
}

/// Assemble KB context string from hybrid search results.
///
/// Format: `[来源：title | section]\ncontent\n\n`
/// Truncated to fit within `max_tokens`.
fn assemble_kb_context(
    results: &[hybrid_search::HybridSearchResult],
    max_tokens: u32,
) -> String {
    let mut context = String::new();

    for result in results {
        let section = result
            .section_path
            .as_deref()
            .unwrap_or("（无章节信息）");

        let entry = format!(
            "[来源：{} | {}]\n{}\n\n",
            result.title, section, result.content
        );
        context.push_str(&entry);
    }

    // Truncate to fit token budget
    super::llm_service::truncate_to_tokens(&context, max_tokens)
}

/// Call LLM to generate values for fields using a recipe-specific system prompt.
///
/// Similar to `generate_llm_fields()` but:
/// - Uses the recipe's domain-specific system_prompt instead of the generic one
/// - Includes KB context for fields that were filled from knowledge base
/// - Provides more structured output guidance
async fn generate_llm_fields_with_recipe(
    llm: &LLMService,
    fields: &[&SchemaField],
    system_prompt: &str,
    kb_context: &str,
    project_name: &Option<String>,
    context: &Option<String>,
) -> Result<HashMap<String, String>, String> {
    use super::llm_service::ChatMessage;

    // Build field descriptions for the prompt
    let field_descriptions: Vec<String> = fields
        .iter()
        .map(|f| {
            let mut desc = format!("- {} (类型: {}, 填充策略: {})", f.name, f.field_type, f.fill_strategy);
            if let Some(ref d) = f.description {
                desc.push_str(&format!(": {}", d));
            }
            if f.required {
                desc.push_str(" [必填]");
            }
            desc
        })
        .collect();

    let mut full_system_prompt = system_prompt.to_string();
    full_system_prompt.push_str(
        "\n\n【反空话规则】\n\
         1. 禁止使用「实现高效管理」「优化业务流程」「加强协同」等无具体操作的套话。\n\
         2. 每个字段值必须有具体内容：配置路径、操作步骤、单据示例等。\n\
         3. 不确定的字段写「待确认」或「[需调研]」，不要用模糊表述填充。\n\
         4. 涉及二开需求的字段，标注 [Gap:待评估]，不得编造技术方案。\n\
         \n\
         请严格以JSON格式返回，格式为 {\"字段名\": \"字段值\", ...}。\n\
         只返回JSON，不要添加任何其他文字。",
    );

    if let Some(ref project) = project_name {
        full_system_prompt.push_str(&format!("\n\n项目名称: {}", project));
    }

    // Build user prompt with KB context + field schema
    let mut user_prompt = String::new();

    if !kb_context.is_empty() {
        user_prompt.push_str(&format!(
            "知识库检索到的相关内容：\n{}\n\n",
            kb_context
        ));
    }

    user_prompt.push_str("请为以下文档字段生成合适的值：\n\n");
    user_prompt.push_str(&field_descriptions.join("\n"));

    if let Some(ref ctx) = context {
        user_prompt.push_str(&format!("\n\n补充信息:\n{}", ctx));
    }

    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: full_system_prompt,
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

    #[test]
    fn test_recipe_doc_request_serialization() {
        let request = RecipeDocRequest {
            recipe_id: "investigation_report".to_string(),
            template_path: "/tmp/template.docx".to_string(),
            output_path: "/tmp/output.docx".to_string(),
            fields: {
                let mut m = HashMap::new();
                m.insert("项目名称".to_string(), "测试项目".to_string());
                m
            },
            schema_fields: vec![],
            project_name: Some("测试项目".to_string()),
            context: Some("调研背景信息".to_string()),
            project_id: Some("proj_001".to_string()),
        };

        // Round-trip through serde JSON
        let json = serde_json::to_string(&request).expect("serialize failed");
        let deserialized: RecipeDocRequest =
            serde_json::from_str(&json).expect("deserialize failed");

        assert_eq!(deserialized.recipe_id, "investigation_report");
        assert_eq!(deserialized.template_path, "/tmp/template.docx");
        assert_eq!(deserialized.fields.get("项目名称").unwrap(), "测试项目");
        assert_eq!(deserialized.project_name, Some("测试项目".to_string()));
        assert_eq!(deserialized.project_id, Some("proj_001".to_string()));
    }

    #[test]
    fn test_kb_source_format() {
        let kb_source = KbSource {
            field_name: "调研结论".to_string(),
            sources: vec!["调研报告A".to_string(), "行业分析B".to_string()],
        };

        let json = serde_json::to_string(&kb_source).expect("serialize failed");
        assert!(json.contains("调研结论"));
        assert!(json.contains("调研报告A"));

        let deserialized: KbSource =
            serde_json::from_str(&json).expect("deserialize failed");
        assert_eq!(deserialized.field_name, "调研结论");
        assert_eq!(deserialized.sources.len(), 2);
    }

    #[test]
    fn test_recipe_doc_result_serialization() {
        let result = RecipeDocResult {
            doc: GeneratedDoc {
                output_path: "/tmp/output.docx".to_string(),
                fields_filled: 5,
                user_fields: vec!["项目名称".to_string()],
                ai_fields: vec!["调研结论".to_string()],
                missing_fields: vec!["负责人".to_string()],
                missing_fields_detail: vec![],
            },
            recipe_name: "调研报告".to_string(),
            kb_sources: vec![KbSource {
                field_name: "调研结论".to_string(),
                sources: vec!["文档1".to_string()],
            }],
        };

        let json = serde_json::to_string(&result).expect("serialize failed");
        let deserialized: RecipeDocResult =
            serde_json::from_str(&json).expect("deserialize failed");

        assert_eq!(deserialized.doc.fields_filled, 5);
        assert_eq!(deserialized.recipe_name, "调研报告");
        assert_eq!(deserialized.kb_sources.len(), 1);
    }
}
