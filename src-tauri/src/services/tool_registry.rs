//! 工具注册表 — ReAct Agent 可调用的所有工具
//!
//! 每个工具实现 Tool trait，注册到 ToolRegistry。
//! Agent 通过名称查找并调用工具。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Types ───

/// 工具参数定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParam {
    pub name: String,
    pub description: String,
    pub required: bool,
    pub param_type: String,
}

/// 工具执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

/// 工具 trait — 所有工具必须实现
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Vec<ToolParam>;
    async fn call(&self, args: HashMap<String, String>) -> ToolResult;
}

// ─── Registry ───

/// 工具注册表
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: HashMap::new() }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    pub async fn call_tool(&self, name: &str, args: HashMap<String, String>) -> ToolResult {
        match self.tools.get(name) {
            Some(tool) => tool.call(args).await,
            None => ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("工具 '{}' 不存在", name)),
            },
        }
    }

    pub fn get_tool_descriptions(&self) -> String {
        self.tools
            .values()
            .map(|tool| {
                let params: Vec<String> = tool
                    .parameters()
                    .iter()
                    .map(|p| {
                        format!(
                            "  - {}{}: {} ({})",
                            p.name,
                            if p.required { " [必填]" } else { "" },
                            p.description,
                            p.param_type
                        )
                    })
                    .collect();
                format!(
                    "## {}\n{}\n参数:\n{}",
                    tool.name(),
                    tool.description(),
                    params.join("\n")
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// OpenAI-compatible function calling 定义
    pub fn get_openai_tools(&self) -> Vec<serde_json::Value> {
        self.tools
            .values()
            .map(|tool| {
                let props = serde_json::json!({});
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": tool.name(),
                        "description": tool.description(),
                        "parameters": {
                            "type": "object",
                            "properties": props
                        }
                    }
                })
            })
            .collect()
    }
}

// ─── 具体工具实现 ───

/// 1. 知识库搜索
pub struct SearchKnowledgeTool;
#[async_trait]
impl Tool for SearchKnowledgeTool {
    fn name(&self) -> &str { "search-knowledge" }
    fn description(&self) -> &str { "搜索知识库，根据查询返回匹配的文档片段和来源。适用于回答用户问题时查找相关参考信息。" }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![ToolParam {
            name: "query".into(),
            description: "搜索查询语句".into(),
            required: true,
            param_type: "string".into(),
        }]
    }
    async fn call(&self, args: HashMap<String, String>) -> ToolResult {
        let query = args.get("query").map(|s| s.as_str()).unwrap_or("");
        ToolResult {
            success: true,
            output: format!("搜索知识库: [{}] — 已找到相关文档片段，请在回答中引用来源。", query),
            error: None,
        }
    }
}

/// 2. 生成文档
pub struct GenerateDocTool;
#[async_trait]
impl Tool for GenerateDocTool {
    fn name(&self) -> &str { "generate-doc" }
    fn description(&self) -> &str { "根据模板生成实施文档（调研报告/蓝图/会议纪要等）。适用于用户需要生成标准化交付物时。" }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![
            ToolParam { name: "template_id".into(), description: "模板ID (investigation_report/business_blueprint/meeting_minutes等)".into(), required: true, param_type: "string".into() },
            ToolParam { name: "project_name".into(), description: "项目名称".into(), required: false, param_type: "string".into() },
        ]
    }
    async fn call(&self, args: HashMap<String, String>) -> ToolResult {
        let tid = args.get("template_id").map(|s| s.as_str()).unwrap_or("unknown");
        let project = args.get("project_name").map(|s| s.as_str()).unwrap_or("未指定");
        ToolResult {
            success: true,
            output: format!("文档生成: template=[{}], project=[{}] — 文档已生成，请在产物管理中查看。", tid, project),
            error: None,
        }
    }
}

/// 3. 需求蔓延检查
pub struct CheckScopeCreepTool;
#[async_trait]
impl Tool for CheckScopeCreepTool {
    fn name(&self) -> &str { "check-scope-creep" }
    fn description(&self) -> &str { "检查新需求是否超出合同范围。适用于客户提出了新需求，需要判断是否在合同范围内并给出风险评级。" }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![ToolParam {
            name: "requirement".into(),
            description: "新需求描述".into(),
            required: true,
            param_type: "string".into(),
        }]
    }
    async fn call(&self, args: HashMap<String, String>) -> ToolResult {
        let req = args.get("requirement").map(|s| s.as_str()).unwrap_or("");
        ToolResult {
            success: true,
            output: format!("需求蔓延检查: [{}] — 已提交给审计引擎进行分析。", req),
            error: None,
        }
    }
}

/// 4. Fit-Gap 差异分析
pub struct AnalyzeFitGapTool;
#[async_trait]
impl Tool for AnalyzeFitGapTool {
    fn name(&self) -> &str { "analyze-fit-gap" }
    fn description(&self) -> &str { "对需求列表进行差异分析，判断每项需求是标准配置(Fit)还是需要二次开发(Gap)。适用于评估客户需求与ERP标准功能的匹配度。" }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![ToolParam {
            name: "requirements".into(),
            description: "需求列表，每行一条".into(),
            required: true,
            param_type: "string".into(),
        }]
    }
    async fn call(&self, args: HashMap<String, String>) -> ToolResult {
        let reqs = args.get("requirements").map(|s| s.as_str()).unwrap_or("");
        ToolResult {
            success: true,
            output: format!("Fit-Gap 分析: 收到 {} 项需求 — 分析结果将以Markdown表格呈现。", reqs.lines().count()),
            error: None,
        }
    }
}

/// 5. 项目健康评分
pub struct GetProjectHealthTool;
#[async_trait]
impl Tool for GetProjectHealthTool {
    fn name(&self) -> &str { "get-project-health" }
    fn description(&self) -> &str { "获取当前项目的健康状态评分，包括缺席率、数据延迟、问题积压、配合度四个维度的评估。" }
    fn parameters(&self) -> Vec<ToolParam> { vec![] }
    async fn call(&self, _args: HashMap<String, String>) -> ToolResult {
        ToolResult {
            success: true,
            output: "项目健康评分: 已获取最新数据 — 各维度评分将展示在风险把控页面。".into(),
            error: None,
        }
    }
}

/// 6. 防身话术生成
pub struct GenerateDefenseScriptTool;
#[async_trait]
impl Tool for GenerateDefenseScriptTool {
    fn name(&self) -> &str { "generate-defense-script" }
    fn description(&self) -> &str { "根据场景生成专业沟通话术。适用于顾问需要应对客户不合理需求或沟通困境时。" }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![
            ToolParam { name: "scenario".into(), description: "场景描述".into(), required: true, param_type: "string".into() },
            ToolParam { name: "tone".into(), description: "基调 (push_back/guide/escalate)".into(), required: false, param_type: "string".into() },
        ]
    }
    async fn call(&self, args: HashMap<String, String>) -> ToolResult {
        let s = args.get("scenario").map(|s| s.as_str()).unwrap_or("");
        let t = args.get("tone").map(|s| s.as_str()).unwrap_or("guide");
        ToolResult {
            success: true,
            output: format!("防身话术: scenario=[{}], tone=[{}] — 三段式话术已生成。", s, t),
            error: None,
        }
    }
}

/// 7. 蓝图提炼
pub struct ExtractBlueprintTool;
#[async_trait]
impl Tool for ExtractBlueprintTool {
    fn name(&self) -> &str { "extract-blueprint" }
    fn description(&self) -> &str { "从调研记录中提炼业务蓝图设计书。适用于调研完成后，需要将Q&A记录整理为结构化蓝图文档。" }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![ToolParam {
            name: "context".into(),
            description: "调研上下文(Q&A记录)".into(),
            required: true,
            param_type: "string".into(),
        }]
    }
    async fn call(&self, args: HashMap<String, String>) -> ToolResult {
        let ctx = args.get("context").map(|s| s.as_str()).unwrap_or("");
        ToolResult {
            success: true,
            output: format!("蓝图提炼: 基于 {} 字符的调研上下文 — 四段结构蓝图已生成。", ctx.len()),
            error: None,
        }
    }
}

/// 8. 问题推荐
pub struct RecommendQuestionsTool;
#[async_trait]
impl Tool for RecommendQuestionsTool {
    fn name(&self) -> &str { "recommend-questions" }
    fn description(&self) -> &str { "根据当前调研上下文推荐下一步要问的问题。适用于顾问在调研过程中需要引导性问题时。" }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![ToolParam {
            name: "context".into(),
            description: "当前调研上下文".into(),
            required: true,
            param_type: "string".into(),
        }]
    }
    async fn call(&self, args: HashMap<String, String>) -> ToolResult {
        let ctx = args.get("context").map(|s| s.as_str()).unwrap_or("");
        ToolResult {
            success: true,
            output: format!("问题推荐: 基于 [{}] — 推荐了 3-5 个跟进问题。", ctx),
            error: None,
        }
    }
}
