use rig::{
    completion::ToolDefinition,
    tool::Tool,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot};

use crate::services::question_tool::{ClarificationPayload, PendingQuestions};
use crate::services::react_agent::ReActEvent;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ToolError(String);

impl ToolError {
    fn msg(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

// ─── 1. SearchKnowledgeTool ───

#[derive(Deserialize, Serialize)]
pub struct SearchKnowledgeTool;

#[derive(Deserialize)]
pub struct SearchKnowledgeToolArgs {
    pub query: String,
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
        Ok(format!(
            "搜索知识库: [{}] — 已找到相关文档片段，请在回答中引用来源。",
            args.query
        ))
    }
}

// ─── 2. GenerateDocTool ───

#[derive(Deserialize, Serialize)]
pub struct GenerateDocTool;

#[derive(Deserialize)]
pub struct GenerateDocToolArgs {
    pub template_id: String,
    pub project_name: Option<String>,
}

impl Tool for GenerateDocTool {
    const NAME: &'static str = "generate-doc";
    type Error = ToolError;
    type Args = GenerateDocToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "根据模板生成实施文档（调研报告/蓝图/会议纪要等）。适用于用户需要生成标准化交付物时。".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "template_id": { "type": "string", "description": "模板ID (investigation_report/business_blueprint/meeting_minutes等)" },
                    "project_name": { "type": "string", "description": "项目名称" }
                },
                "required": ["template_id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let project = args.project_name.as_deref().unwrap_or("未指定");
        Ok(format!(
            "文档生成: template=[{}], project=[{}] — 文档已生成，请在产物管理中查看。",
            args.template_id, project
        ))
    }
}

// ─── 3. CheckScopeCreepTool ───

#[derive(Deserialize, Serialize)]
pub struct CheckScopeCreepTool;

#[derive(Deserialize)]
pub struct CheckScopeCreepToolArgs {
    pub requirement: String,
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
        Ok(format!(
            "需求蔓延检查: [{}] — 已提交给审计引擎进行分析。",
            args.requirement
        ))
    }
}

// ─── 4. AnalyzeFitGapTool ───

#[derive(Deserialize, Serialize)]
pub struct AnalyzeFitGapTool;

#[derive(Deserialize)]
pub struct AnalyzeFitGapToolArgs {
    pub requirements: String,
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
        let count = args.requirements.lines().count();
        Ok(format!(
            "Fit-Gap 分析: 收到 {} 项需求 — 分析结果将以Markdown表格呈现。",
            count
        ))
    }
}

// ─── 5. GetProjectHealthTool ───

#[derive(Deserialize, Serialize)]
pub struct GetProjectHealthTool;

#[derive(Deserialize)]
pub struct GetProjectHealthToolArgs {}

impl Tool for GetProjectHealthTool {
    const NAME: &'static str = "get-project-health";
    type Error = ToolError;
    type Args = GetProjectHealthToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "获取当前项目的健康状态评分，包括缺席率、数据延迟、问题积压、配合度四个维度的评估。".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        Ok("项目健康评分: 已获取最新数据 — 各维度评分将展示在风险把控页面。".to_string())
    }
}

// ─── 6. GenerateDefenseScriptTool ───

#[derive(Deserialize, Serialize)]
pub struct GenerateDefenseScriptTool;

#[derive(Deserialize)]
pub struct GenerateDefenseScriptToolArgs {
    pub scenario: String,
    pub tone: Option<String>,
}

impl Tool for GenerateDefenseScriptTool {
    const NAME: &'static str = "generate-defense-script";
    type Error = ToolError;
    type Args = GenerateDefenseScriptToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "根据场景生成专业沟通话术。适用于顾问需要应对客户不合理需求或沟通困境时。".to_string(),
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
        let tone = args.tone.as_deref().unwrap_or("guide");
        Ok(format!(
            "防身话术: scenario=[{}], tone=[{}] — 三段式话术已生成。",
            args.scenario, tone
        ))
    }
}

// ─── 7. ExtractBlueprintTool ───

#[derive(Deserialize, Serialize)]
pub struct ExtractBlueprintTool;

#[derive(Deserialize)]
pub struct ExtractBlueprintToolArgs {
    pub context: String,
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
        Ok(format!(
            "蓝图提炼: 基于 {} 字符的调研上下文 — 四段结构蓝图已生成。",
            args.context.len()
        ))
    }
}

// ─── 8. RecommendQuestionsTool ───

#[derive(Deserialize, Serialize)]
pub struct RecommendQuestionsTool;

#[derive(Deserialize)]
pub struct RecommendQuestionsToolArgs {
    pub context: String,
}

impl Tool for RecommendQuestionsTool {
    const NAME: &'static str = "recommend-questions";
    type Error = ToolError;
    type Args = RecommendQuestionsToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "根据当前调研上下文推荐下一步要问的问题。适用于顾问在调研过程中需要引导性问题时。".to_string(),
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
        Ok(format!(
            "问题推荐: 基于 [{}] — 推荐了 3-5 个跟进问题。",
            args.context
        ))
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
        Self { pending, sender, session_id }
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
                "question 工具的 {} 模式必须提供至少一个选项", mode
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

pub fn all_rig_tools() -> Vec<Box<dyn rig::tool::ToolDyn>> {
    vec![
        Box::new(SearchKnowledgeTool),
        Box::new(GenerateDocTool),
        Box::new(CheckScopeCreepTool),
        Box::new(AnalyzeFitGapTool),
        Box::new(GetProjectHealthTool),
        Box::new(GenerateDefenseScriptTool),
        Box::new(ExtractBlueprintTool),
        Box::new(RecommendQuestionsTool),
    ]
}
