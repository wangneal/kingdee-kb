//! ReAct 事件定义
//!
//! ReActEvent 枚举用于前端 SSE 事件格式，
//! 被 rig_agent、question_tool 等模块使用。

use serde::{Deserialize, Serialize};

use crate::services::question_tool::ClarificationPayload;

/// ReAct 事件 — 通过 SSE 流式发送给前端
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReActEvent {
    #[serde(rename = "thinking")]
    Thinking { session_id: String, content: String },
    #[serde(rename = "tool_call")]
    ToolCall {
        session_id: String,
        name: String,
        args: String,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        session_id: String,
        name: String,
        result: String,
    },
    #[serde(rename = "text_delta")]
    TextDelta { session_id: String, content: String },
    #[serde(rename = "error")]
    Error { session_id: String, message: String },
    #[serde(rename = "done")]
    Done { session_id: String },
    /// Agent uses the `question` tool to ask the user a clarification question.
    /// `payload` contains question_id, prompt, mode, and options.
    #[serde(rename = "clarification")]
    Clarification {
        session_id: String,
        payload: ClarificationPayload,
    },
    #[serde(rename = "plan_generated")]
    PlanGenerated {
        session_id: String,
        steps: Vec<crate::services::planner::PlanStep>,
    },
    #[serde(rename = "step_start")]
    StepStart {
        session_id: String,
        step_index: usize,
        total_steps: usize,
        description: String,
    },
    #[serde(rename = "step_result")]
    StepResult {
        session_id: String,
        step_index: usize,
        result: String,
        success: bool,
    },
    #[serde(rename = "replan")]
    Replan {
        session_id: String,
        reason: String,
    },
    #[serde(rename = "planner_timeout")]
    PlannerTimeout {
        session_id: String,
        message: String,
    },
}
