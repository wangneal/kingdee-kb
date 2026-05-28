use rig_core::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot};

use crate::services::bm25_service::BM25Service;
use crate::services::doc_generator::RecipeDocRequest;
use crate::services::embedding::EmbeddingService;
use crate::services::hybrid_search;
use crate::services::llm_service::LLMService;
use crate::services::metadata::MetadataStore;
use crate::services::product_store::ProductStore;
use crate::services::question_tool::{ClarificationPayload, PendingQuestions};
use crate::services::react_agent::ReActEvent;
use crate::services::risk_control::RiskControlStore;
use crate::services::template_scanner::TemplateInfo;
use crate::services::vector_index::VectorIndex;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ToolError(String);

impl ToolError {
    fn msg(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

// ─── 1. SearchKnowledgeTool ───

pub struct SearchKnowledgeTool {
    pub embedding: Arc<Mutex<EmbeddingService>>,
    pub vector_index: Arc<Mutex<VectorIndex>>,
    pub bm25: Arc<Mutex<BM25Service>>,
    pub metadata: Arc<Mutex<MetadataStore>>,
    pub project_id: Option<String>,
}

#[derive(Deserialize)]
pub struct SearchKnowledgeToolArgs {
    pub query: String,
}

impl SearchKnowledgeTool {
    pub fn new(
        project_id: Option<String>,
        embedding: Arc<Mutex<EmbeddingService>>,
        vector_index: Arc<Mutex<VectorIndex>>,
        bm25: Arc<Mutex<BM25Service>>,
        metadata: Arc<Mutex<MetadataStore>>,
    ) -> Self {
        Self {
            project_id,
            embedding,
            vector_index,
            bm25,
            metadata,
        }
    }
}

impl Tool for SearchKnowledgeTool {
    const NAME: &'static str = "search-knowledge";
    type Error = ToolError;
    type Args = SearchKnowledgeToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "搜索知识库，根据查询返回匹配的文档片段和来源。适用于回答用户问题时查找相关参考信息。".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "搜索查询语句" }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let results = hybrid_search::hybrid_search(
            &args.query,
            self.project_id.as_deref(),
            5,
            &self.embedding,
            &self.vector_index,
            &self.bm25,
            &self.metadata,
        )
        .map_err(ToolError::msg)?;

        if results.is_empty() {
            return Ok(
                "知识库中未找到与查询相关的文档片段。请尝试换一种表述方式搜索。".to_string(),
            );
        }

        // 格式化搜索结果为 Agent 可消费的文本
        let mut output = String::new();
        output.push_str(&format!("找到 {} 条相关结果：\n\n", results.len()));
        for (i, r) in results.iter().enumerate() {
            output.push_str(&format!(
                "【{}】{} (相关度: {:.3}, 来源: {})\n{}\n\n",
                i + 1,
                r.title,
                r.score,
                r.source,
                truncate_content(&r.content, 500),
            ));
        }
        Ok(output)
    }
}

/// 截断文本到指定字符数，超出部分用省略号代替
fn truncate_content(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_chars).collect();
        format!("{}...", truncated)
    }
}

// ─── 2. GenerateDocTool ───

pub struct GenerateDocTool {
    pub data_dir: PathBuf,
    pub llm: LLMService,
    pub embedding: Arc<Mutex<EmbeddingService>>,
    pub vector_index: Arc<Mutex<VectorIndex>>,
    pub bm25: Arc<Mutex<BM25Service>>,
    pub metadata: Arc<Mutex<MetadataStore>>,
    pub products: Arc<Mutex<ProductStore>>,
    pub project_id: Option<String>,
}

#[derive(Deserialize)]
pub struct GenerateDocToolArgs {
    pub template_id: String,
    pub project_name: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
}

impl GenerateDocTool {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        data_dir: PathBuf,
        llm: LLMService,
        project_id: Option<String>,
        embedding: Arc<Mutex<EmbeddingService>>,
        vector_index: Arc<Mutex<VectorIndex>>,
        bm25: Arc<Mutex<BM25Service>>,
        metadata: Arc<Mutex<MetadataStore>>,
        products: Arc<Mutex<ProductStore>>,
    ) -> Self {
        Self {
            data_dir,
            llm,
            embedding,
            vector_index,
            bm25,
            metadata,
            products,
            project_id,
        }
    }
}

impl Tool for GenerateDocTool {
    const NAME: &'static str = "generate-doc";
    type Error = ToolError;
    type Args = GenerateDocToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "根据模板生成标准化实施文档。\
                          支持的文档类型：\
                          - investigation_report: 调研报告（企业现状、问题分析、建议方案）\
                          - business_blueprint: 业务蓝图\
                          - meeting_minutes: 会议纪要\
                          - weekly_monthly_report: 周报/月报\
                          - pcr: 变更申请\
                          当用户要求生成调研报告、蓝图、会议纪要等文档时，必须调用此工具。\
                          不要直接用文字回复，必须调用工具生成标准化文档。".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "template_id": { "type": "string", "description": "文档类型ID: investigation_report(调研报告), business_blueprint(蓝图), meeting_minutes(会议纪要), weekly_monthly_report(周报月报), pcr(变更申请)" },
                    "project_name": { "type": "string", "description": "项目名称" },
                    "context": { "type": "string", "description": "生成文档所需的背景信息、调研记录或用户补充说明" }
                },
                "required": ["template_id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let project = args
            .project_name
            .clone()
            .or_else(|| self.project_id.clone())
            .unwrap_or_else(|| "default".to_string());

        let recipe =
            crate::services::deliverable_recipes::get_recipe_by_template_id(&args.template_id)
                .ok_or_else(|| ToolError::msg(format!("未找到交付物配方: {}", args.template_id)))?;

        let template_root = resolve_template_root(&self.data_dir);
        let templates = crate::services::template_scanner::scan_templates(&template_root)
            .map_err(ToolError::msg)?;
        let template = find_template_for_recipe(&templates, &args.template_id, &recipe.name)
            .ok_or_else(|| {
                ToolError::msg(format!(
                    "未找到 template_id=[{}] 对应的模板文件。已扫描目录: {}",
                    args.template_id,
                    template_root.display()
                ))
            })?;

        let schema = build_template_schema(&template).map_err(ToolError::msg)?;
        let mut fields = HashMap::new();
        seed_project_fields(&mut fields, &schema.fields, &project);

        let schema_fields = schema
            .fields
            .into_iter()
            .map(|mut field| {
                if field.fill_strategy == "user" && !fields.contains_key(&field.name) {
                    field.fill_strategy = "ai".to_string();
                }
                field
            })
            .collect::<Vec<_>>();

        let output_path =
            build_output_path(&self.data_dir, &template, &project).map_err(ToolError::msg)?;
        let context = args.context.or_else(|| {
            Some(format!(
                "项目名称：{}。请生成 {}，不确定的信息写“待确认”。",
                project, recipe.name
            ))
        });

        let request = RecipeDocRequest {
            recipe_id: args.template_id.clone(),
            template_path: template.file_path.clone(),
            output_path: output_path.to_string_lossy().to_string(),
            fields,
            schema_fields,
            project_name: Some(project.clone()),
            context,
            project_id: self.project_id.clone().or_else(|| Some(project.clone())),
        };

        let user_field_count = request.fields.len() as i64;
        let schema_field_count = request.schema_fields.len() as i64;
        let input_json = serde_json::to_string(&request).unwrap_or_else(|_| "{}".to_string());

        let result = crate::services::doc_generator::generate_recipe_doc(
            request,
            &self.llm,
            &self.embedding,
            &self.vector_index,
            &self.bm25,
            &self.metadata,
        )
        .await
        .map_err(ToolError::msg)?;

        let product_id = {
            let store = self
                .products
                .lock()
                .map_err(|e| ToolError::msg(e.to_string()))?;
            store
                .create(
                    &args.template_id,
                    &result.recipe_name,
                    &project,
                    &result.doc.output_path,
                    user_field_count.max(schema_field_count),
                    result.doc.ai_fields.len() as i64,
                    &input_json,
                )
                .map_err(ToolError::msg)?
        };

        Ok(format!(
            "文档已生成并写入产物管理。\n模板：{}\n项目：{}\n产物ID：{}\n输出路径：{}\n填充字段：{}\nAI字段：{}\n未填字段：{}",
            result.recipe_name,
            project,
            product_id,
            result.doc.output_path,
            result.doc.fields_filled,
            result.doc.ai_fields.len(),
            result.doc.missing_fields.len()
        ))
    }
}

fn resolve_template_root(data_dir: &Path) -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let candidates = [
        data_dir.join("templates"),
        cwd.join("templates"),
        cwd.join("..").join("templates"),
    ];

    candidates
        .into_iter()
        .find(|path| path.exists() && template_dir_has_files(path))
        .unwrap_or_else(|| data_dir.join("templates"))
}

fn template_dir_has_files(path: &Path) -> bool {
    walkdir::WalkDir::new(path)
        .min_depth(1)
        .into_iter()
        .filter_map(Result::ok)
        .any(|entry| {
            entry.file_type().is_file()
                && entry
                    .path()
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "docx" | "xlsx"))
                    .unwrap_or(false)
        })
}

fn find_template_for_recipe(
    templates: &[TemplateInfo],
    template_id: &str,
    recipe_name: &str,
) -> Option<TemplateInfo> {
    let keywords = recipe_keywords(template_id, recipe_name);
    templates
        .iter()
        .find(|t| t.id == template_id)
        .or_else(|| {
            templates.iter().find(|t| {
                keywords
                    .iter()
                    .any(|kw| t.name.contains(kw) || t.filename.contains(kw))
            })
        })
        .cloned()
}

fn recipe_keywords(template_id: &str, recipe_name: &str) -> Vec<String> {
    let mut keywords = vec![recipe_name.to_string()];
    match template_id {
        "investigation_report" => keywords.extend(["调研报告", "调研"].map(str::to_string)),
        "business_blueprint" => keywords.extend(["蓝图", "业务蓝图"].map(str::to_string)),
        "meeting_minutes" => keywords.extend(["会议纪要", "会议"].map(str::to_string)),
        "weekly_monthly_report" => keywords.extend(["周报", "月报", "进度"].map(str::to_string)),
        "pcr" => keywords.extend(["PCR", "变更"].map(str::to_string)),
        "go_live" => keywords.extend(["上线", "上线检查"].map(str::to_string)),
        "acceptance" => keywords.extend(["验收", "验收单"].map(str::to_string)),
        _ => {}
    }
    keywords
}

fn build_template_schema(
    template: &TemplateInfo,
) -> Result<crate::services::template_schema::TemplateSchema, String> {
    let path = PathBuf::from(&template.file_path);
    if let Some(schema) = crate::services::template_schema::load_schema_sidecar(&path)? {
        return Ok(schema);
    }

    match template.format.as_str() {
        "docx" => {
            let fields = crate::services::template_docx::extract_docx_fields(&path)?;
            Ok(crate::services::template_schema::generate_schema_from_docx(
                &template.id,
                &template.name,
                &template.phase,
                &fields,
            ))
        }
        "xlsx" => {
            let fields = crate::services::template_xlsx::extract_xlsx_fields(&path)?;
            Ok(crate::services::template_schema::generate_schema_from_xlsx(
                &template.id,
                &template.name,
                &template.phase,
                &fields,
            ))
        }
        _ => Err(format!("Unsupported template format: {}", template.format)),
    }
}

fn seed_project_fields(
    fields: &mut HashMap<String, String>,
    schema_fields: &[crate::services::template_schema::SchemaField],
    project: &str,
) {
    for field in schema_fields {
        if field.name.contains("项目名称") || field.name.contains("项目名") {
            fields.insert(field.name.clone(), project.to_string());
        }
    }
}

fn build_output_path(
    data_dir: &Path,
    template: &TemplateInfo,
    project: &str,
) -> Result<PathBuf, String> {
    let output_dir = data_dir.join("generated");
    std::fs::create_dir_all(&output_dir).map_err(|e| {
        format!(
            "Failed to create output dir {}: {}",
            output_dir.display(),
            e
        )
    })?;

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let ext = Path::new(&template.file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or(template.format.as_str());
    let filename = format!(
        "{}_{}_{}.{}",
        sanitize_filename(project),
        sanitize_filename(&template.name),
        ts,
        ext
    );
    Ok(output_dir.join(filename))
}

fn sanitize_filename(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>();
    let trimmed = sanitized.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "document".to_string()
    } else {
        trimmed.chars().take(80).collect()
    }
}

// ─── 3. CheckScopeCreepTool ───

pub struct CheckScopeCreepTool {
    pub project_id: i64,
    pub llm: LLMService,
    pub risk_store: Arc<tokio::sync::Mutex<RiskControlStore>>,
}

#[derive(Deserialize)]
pub struct CheckScopeCreepToolArgs {
    pub requirement: String,
}

impl CheckScopeCreepTool {
    pub fn new(
        project_id: i64,
        llm: LLMService,
        risk_store: Arc<tokio::sync::Mutex<RiskControlStore>>,
    ) -> Self {
        Self { project_id, llm, risk_store }
    }
}

impl Tool for CheckScopeCreepTool {
    const NAME: &'static str = "check-scope-creep";
    type Error = ToolError;
    type Args = CheckScopeCreepToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "检查新需求是否超出合同范围。适用于客户提出了新需求，需要判断是否在合同范围内并给出风险评级。".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "requirement": { "type": "string", "description": "新需求描述" }
                },
                "required": ["requirement"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let store = self.risk_store.lock().await;
        let result = store
            .check_scope_creep(self.project_id, &self.llm, &args.requirement)
            .await
            .map_err(ToolError::msg)?;

        Ok(format!(
            "需求蔓延检查结果：\n风险等级：{} ({})\n分析：{}\n匹配条款：{}\n建议：{}",
            result.risk_level,
            result.risk_label,
            result.explanation,
            result.matched_items.join("、"),
            result.suggestion
        ))
    }
}

// ─── 4. AnalyzeFitGapTool ───

pub struct AnalyzeFitGapTool {
    pub llm: LLMService,
}

#[derive(Deserialize)]
pub struct AnalyzeFitGapToolArgs {
    pub requirements: String,
}

impl AnalyzeFitGapTool {
    pub fn new(llm: LLMService) -> Self {
        Self { llm }
    }
}

impl Tool for AnalyzeFitGapTool {
    const NAME: &'static str = "analyze-fit-gap";
    type Error = ToolError;
    type Args = AnalyzeFitGapToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "对需求列表进行差异分析，判断每项需求是标准配置(Fit)还是需要二次开发(Gap)。适用于评估客户需求与ERP标准功能的匹配度。".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "requirements": { "type": "string", "description": "需求列表，每行一条" }
                },
                "required": ["requirements"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        use crate::services::llm_service::ChatMessage;

        let prompt = format!(
            "你是一个金蝶ERP差异分析专家。请分析以下需求，判断每项是标准配置(Fit)还是需要二次开发(Gap)。\n\n\
             需求列表：\n{}\n\n\
             请以Markdown表格格式返回，包含列：需求项、Fit/Gap、说明、建议。\n\
             如果是Gap，说明需要评估的内容。",
            args.requirements
        );

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "你是金蝶ERP差异分析专家，熟悉标准功能和常见二开场景。".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: prompt,
            },
        ];

        let config = self.llm.get_config().map_err(ToolError::msg)?;
        let response = self.llm.chat_completion(&messages, &config).await.map_err(ToolError::msg)?;
        Ok(response)
    }
}

// ─── 5. GetProjectHealthTool ───

pub struct GetProjectHealthTool {
    pub project_id: i64,
    pub risk_store: Arc<tokio::sync::Mutex<RiskControlStore>>,
}

#[derive(Deserialize)]
pub struct GetProjectHealthToolArgs {}

impl GetProjectHealthTool {
    pub fn new(project_id: i64, risk_store: Arc<tokio::sync::Mutex<RiskControlStore>>) -> Self {
        Self { project_id, risk_store }
    }
}

impl Tool for GetProjectHealthTool {
    const NAME: &'static str = "get-project-health";
    type Error = ToolError;
    type Args = GetProjectHealthToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description:
                "获取当前项目的健康状态评分，包括缺席率、数据延迟、问题积压、配合度四个维度的评估。"
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        let store = self.risk_store.lock().await;
        let score = store
            .calculate_health_score(self.project_id)
            .map_err(ToolError::msg)?;

        let dimensions: Vec<String> = score.dimensions
            .iter()
            .map(|d| format!("- {}: {:.1}/100 ({})", d.name, d.score, d.detail))
            .collect();

        Ok(format!(
            "项目健康评分：{:.1}/100\n风险等级：{}\n趋势：{}\n告警数：{}\n\n各维度：\n{}",
            score.overall_score,
            score.risk_level,
            score.trend,
            score.alert_count,
            dimensions.join("\n")
        ))
    }
}

// ─── 6. GenerateDefenseScriptTool ───

pub struct GenerateDefenseScriptTool {
    pub llm: LLMService,
    pub risk_store: Arc<tokio::sync::Mutex<RiskControlStore>>,
}

#[derive(Deserialize)]
pub struct GenerateDefenseScriptToolArgs {
    pub scenario: String,
    pub tone: Option<String>,
}

impl GenerateDefenseScriptTool {
    pub fn new(llm: LLMService, risk_store: Arc<tokio::sync::Mutex<RiskControlStore>>) -> Self {
        Self { llm, risk_store }
    }
}

impl Tool for GenerateDefenseScriptTool {
    const NAME: &'static str = "generate-defense-script";
    type Error = ToolError;
    type Args = GenerateDefenseScriptToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "根据场景生成专业沟通话术。适用于顾问需要应对客户不合理需求或沟通困境时。"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "scenario": { "type": "string", "description": "场景描述" },
                    "tone": { "type": "string", "description": "基调 (push_back/guide/escalate)" }
                },
                "required": ["scenario"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let request = crate::services::risk_control::DefenseScriptRequest {
            scenario: args.scenario,
            context: String::new(),
            tone: args.tone.unwrap_or_else(|| "guide".to_string()),
        };
        let store = self.risk_store.lock().await;
        let result = store
            .generate_defense_script(&self.llm, &request)
            .await
            .map_err(ToolError::msg)?;

        let scripts: Vec<String> = result.scripts
            .iter()
            .map(|s| format!("[{}] {}\n  提示：{}", s.phase, s.content, s.tip))
            .collect();

        Ok(format!(
            "场景：{}\n\n{}",
            result.scenario_label,
            scripts.join("\n\n")
        ))
    }
}

// ─── 7. ExtractBlueprintTool ───

pub struct ExtractBlueprintTool {
    pub llm: LLMService,
}

#[derive(Deserialize)]
pub struct ExtractBlueprintToolArgs {
    pub context: String,
}

impl ExtractBlueprintTool {
    pub fn new(llm: LLMService) -> Self {
        Self { llm }
    }
}

impl Tool for ExtractBlueprintTool {
    const NAME: &'static str = "extract-blueprint";
    type Error = ToolError;
    type Args = ExtractBlueprintToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "从调研记录中提炼业务蓝图设计书。适用于调研完成后，需要将Q&A记录整理为结构化蓝图文档。".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "context": { "type": "string", "description": "调研上下文(Q&A记录)" }
                },
                "required": ["context"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        use crate::services::llm_service::ChatMessage;

        let prompt = format!(
            "你是一个金蝶ERP业务架构师。请根据以下调研记录提炼业务蓝图设计书。\n\n\
             调研记录：\n{}\n\n\
             请严格按照以下四段结构输出：\n\
             1.【现有线下流程 As-Is】— 描述客户当前的业务操作模式\n\
             2.【系统标准流程 To-Be】— 描述金蝶系统中的标准解决方案\n\
             3.【差异配置点】— 按「配置路径: 配置值」格式列出具体的系统配置项\n\
             4.【对应系统单据类型】— 涉及的单据名称及单据编号规则\n\n\
             每段必须有具体的系统操作路径、配置参数或单据示例。",
            args.context
        );

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "你是金蝶ERP业务架构师，擅长从业务需求提炼系统方案。".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: prompt,
            },
        ];

        let config = self.llm.get_config().map_err(ToolError::msg)?;
        let response = self.llm.chat_completion(&messages, &config).await.map_err(ToolError::msg)?;
        Ok(response)
    }
}

// ─── 8. RecommendQuestionsTool ───

pub struct RecommendQuestionsTool {
    pub llm: LLMService,
}

#[derive(Deserialize)]
pub struct RecommendQuestionsToolArgs {
    pub context: String,
}

impl RecommendQuestionsTool {
    pub fn new(llm: LLMService) -> Self {
        Self { llm }
    }
}

impl Tool for RecommendQuestionsTool {
    const NAME: &'static str = "recommend-questions";
    type Error = ToolError;
    type Args = RecommendQuestionsToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description:
                "根据当前调研上下文推荐下一步要问的问题。适用于顾问在调研过程中需要引导性问题时。"
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "context": { "type": "string", "description": "当前调研上下文" }
                },
                "required": ["context"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        use crate::services::llm_service::ChatMessage;

        let prompt = format!(
            "你是一个金蝶ERP实施调研助手。根据当前调研上下文，推荐3-5个后续调研问题。\n\n\
             当前上下文：\n{}\n\n\
             要求：\n\
             1. 问题应与当前主题相关但有延伸性\n\
             2. 能够帮助更深入了解金蝶ERP在该领域的实施细节\n\
             3. 避免与已问过的问题重复\n\
             4. 每个问题单独一行，带编号",
            args.context
        );

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "你是金蝶ERP实施调研助手，擅长设计引导性问题。".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: prompt,
            },
        ];

        let config = self.llm.get_config().map_err(ToolError::msg)?;
        let response = self.llm.chat_completion(&messages, &config).await.map_err(ToolError::msg)?;
        Ok(response)
    }
}

// ─── 9. RigQuestionTool（运行时注入，不在 all_rig_tools() 中）───

/// rig Tool implementation for asking the user a clarification question.
///
/// Unlike other tools that return immediately, this blocks until the user
/// replies via a `oneshot` channel registered in `PendingQuestions`.
/// The `Clarification` event is sent to the frontend for UI rendering.
pub struct RigQuestionTool {
    pending: PendingQuestions,
    sender: mpsc::UnboundedSender<ReActEvent>,
    session_id: String,
}

impl RigQuestionTool {
    pub fn new(
        pending: PendingQuestions,
        sender: mpsc::UnboundedSender<ReActEvent>,
        session_id: String,
    ) -> Self {
        Self {
            pending,
            sender,
            session_id,
        }
    }
}

#[derive(Deserialize)]
pub struct QuestionArgs {
    pub prompt: String,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub options: Option<Vec<String>>,
}

impl Tool for RigQuestionTool {
    const NAME: &'static str = "question";

    type Args = QuestionArgs;
    type Output = String;
    type Error = ToolError;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "question".to_string(),
            description: "向用户提问以获取更多信息。当问题模糊、需要选择方向、或需要补充细节时使用。\
                          参数：prompt（问题文本，必填）、mode（single_choice/multi_choice/free_input，默认single_choice）、\
                          options（选项列表，仅choice模式需要）".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "要向用户提出的问题文本"
                    },
                    "mode": {
                        "type": "string",
                        "enum": ["single_choice", "multi_choice", "free_input"],
                        "description": "提问模式"
                    },
                    "options": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "选项列表（仅single_choice/multi_choice需要）"
                    }
                },
                "required": ["prompt"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let prompt = args.prompt;
        let mode = args.mode.as_deref().unwrap_or("single_choice").to_string();
        let options = args.options.unwrap_or_default();

        // Validate: choice modes must have options
        if (mode == "single_choice" || mode == "multi_choice") && options.is_empty() {
            return Err(ToolError(format!(
                "question 工具的 {} 模式必须提供至少一个选项",
                mode
            )));
        }

        // Generate unique question_id
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let question_id = format!("q_{ts}");

        // Create oneshot channel for the reply
        let (tx, rx) = oneshot::channel::<String>();

        // Register the pending question
        {
            let mut map = self.pending.lock().await;
            map.insert(question_id.clone(), tx);
        }

        // Send clarification event to frontend
        let payload = ClarificationPayload {
            question_id: question_id.clone(),
            prompt: prompt.clone(),
            mode: mode.clone(),
            options: options.clone(),
        };
        let _ = self.sender.send(ReActEvent::Clarification {
            session_id: self.session_id.clone(),
            payload,
        });

        // Wait for the user's answer
        let answer = rx.await.unwrap_or_default();

        // Cleanup
        {
            let mut map = self.pending.lock().await;
            map.remove(&question_id);
        }

        Ok(answer)
    }
}

/// 创建所有 rig 工具实例。
///
/// 所有工具都连接到真正的后端服务，返回真实结果。
pub fn all_rig_tools(
    project_id: Option<&str>,
    data_dir: PathBuf,
    llm: LLMService,
    embedding: Arc<Mutex<EmbeddingService>>,
    vector_index: Arc<Mutex<VectorIndex>>,
    bm25: Arc<Mutex<BM25Service>>,
    metadata: Arc<Mutex<MetadataStore>>,
    products: Arc<Mutex<ProductStore>>,
    risk_store: Arc<tokio::sync::Mutex<RiskControlStore>>,
) -> Vec<Box<dyn rig_core::tool::ToolDyn>> {
    // 默认 project_id 为 0（全局），如果有具体项目则使用项目 ID
    let risk_project_id = 0i64;

    vec![
        Box::new(SearchKnowledgeTool::new(
            project_id.map(|s| s.to_string()),
            embedding.clone(),
            vector_index.clone(),
            bm25.clone(),
            metadata.clone(),
        )),
        Box::new(GenerateDocTool::new(
            data_dir,
            llm.clone(),
            project_id.map(|s| s.to_string()),
            embedding,
            vector_index,
            bm25,
            metadata,
            products,
        )),
        Box::new(CheckScopeCreepTool::new(
            risk_project_id,
            llm.clone(),
            risk_store.clone(),
        )),
        Box::new(AnalyzeFitGapTool::new(llm.clone())),
        Box::new(GetProjectHealthTool::new(
            risk_project_id,
            risk_store.clone(),
        )),
        Box::new(GenerateDefenseScriptTool::new(
            llm.clone(),
            risk_store,
        )),
        Box::new(ExtractBlueprintTool::new(llm.clone())),
        Box::new(RecommendQuestionsTool::new(llm)),
    ]
}
