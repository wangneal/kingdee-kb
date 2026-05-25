//! `question` tool — lets the ReAct agent ask the user clarification questions.
//!
//! The agent invokes this tool when it needs more information from the user
//! before it can give a good answer. Three modes are supported:
//!
//! - `single_choice` — user picks **one** option from a list
//! - `multi_choice`  — user picks **one or more** options from a list
//! - `free_input`    — user types a free-form answer
//!
//! ## Flow
//!
//! 1. Agent decides it needs clarification → calls `question` tool
//! 2. Tool creates a unique `question_id`, registers it in `PendingQuestions`,
//!    sends `ReActEvent::Clarification` to the frontend via the event channel
//! 3. Tool **awaits** on a `oneshot` channel for the user's reply
//! 4. Frontend renders the question UI (options / input box)
//! 5. User picks an option or types text → frontend calls `answer_question` Tauri command
//! 6. The command resolves the oneshot → tool returns the answer as `ToolResult`
//! 7. ReAct loop continues with the user's answer in context

use crate::services::react_agent::ReActEvent;
use crate::services::tool_registry::{Tool, ToolParam, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot, Mutex};

// ── Pending questions registry ──────────────────────────────────────────────

/// Global registry of questions waiting for user replies.
///
/// Key: `question_id` (unique per question)
/// Value: `oneshot::Sender<String>` — resolves when the frontend calls `answer_question`
pub type PendingQuestions = Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>;

/// Create a fresh pending-questions map.
pub fn create_pending_questions() -> PendingQuestions {
    Arc::new(Mutex::new(HashMap::new()))
}

// ── Clarification payload (sent to frontend via SSE) ────────────────────────

/// Full clarification payload sent inside `ReActEvent::Clarification`.
/// The frontend uses this to render the question UI and must include
/// `question_id` when calling `answer_question`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarificationPayload {
    /// Unique question ID — frontend must send this back with the answer.
    pub question_id: String,
    /// The question text to display to the user.
    pub prompt: String,
    /// Question mode: `"single_choice"` | `"multi_choice"` | `"free_input"`
    pub mode: String,
    /// Available options for choice modes. Empty for `free_input`.
    pub options: Vec<String>,
}

// ── QuestionTool ────────────────────────────────────────────────────────────

/// Tool that asks the user a question and waits for the reply.
///
/// This is special: unlike other tools that return immediately, `QuestionTool`
/// blocks until the user responds. This is achieved via a `oneshot` channel
/// registered in the global `PendingQuestions` map.
///
/// The `event_sender` and `session_id` are set per-session so each invocation
/// routes the SSE event to the correct frontend listener.
pub struct QuestionTool {
    pending: PendingQuestions,
    event_sender: mpsc::UnboundedSender<ReActEvent>,
    session_id: String,
}

impl QuestionTool {
    pub fn new(
        pending: PendingQuestions,
        event_sender: mpsc::UnboundedSender<ReActEvent>,
        session_id: String,
    ) -> Self {
        Self {
            pending,
            event_sender,
            session_id,
        }
    }
}

#[async_trait]
impl Tool for QuestionTool {
    fn name(&self) -> &str {
        "question"
    }

    fn description(&self) -> &str {
        "向用户提问以获取更多信息。当用户的问题模糊、需要选择方向、或需要补充细节时使用此工具。\
         参数：prompt（问题文本，必填）、mode（single_choice/multi_choice/free_input，默认single_choice）、\
         options（选项列表，仅 single_choice 和 multi_choice 模式需要，以JSON数组字符串形式传入）。\
         用户回复后，回复内容将作为工具返回值。"
    }

    fn parameters(&self) -> Vec<ToolParam> {
        vec![
            ToolParam {
                name: "prompt".into(),
                description: "要向用户提出的问题文本".into(),
                required: true,
                param_type: "string".into(),
            },
            ToolParam {
                name: "mode".into(),
                description: "提问模式：single_choice(单选)/multi_choice(多选)/free_input(自由输入)".into(),
                required: false,
                param_type: "string".into(),
            },
            ToolParam {
                name: "options".into(),
                description: "选项列表（JSON数组字符串，如 [\"选项1\",\"选项2\"]，仅single_choice/multi_choice模式需要）".into(),
                required: false,
                param_type: "string".into(),
            },
        ]
    }

    async fn call(&self, args: HashMap<String, String>) -> ToolResult {
        let prompt = args.get("prompt").cloned().unwrap_or_default();
        let mode = args.get("mode").cloned().unwrap_or_else(|| "single_choice".to_string());
        let options_str = args.get("options").cloned().unwrap_or_default();

        // Parse options: could be a JSON array or comma-separated
        let options: Vec<String> = if options_str.is_empty() {
            vec![]
        } else if options_str.starts_with('[') {
            serde_json::from_str(&options_str).unwrap_or_else(|_| {
                options_str.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
        } else {
            options_str.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };

        // Validate: choice modes must have options
        if (mode == "single_choice" || mode == "multi_choice") && options.is_empty() {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("question 工具的 {} 模式必须提供至少一个选项", mode)),
            };
        }

        // Generate unique question_id (timestamp-based, no uuid dependency needed)
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        // Add a small counter to handle same-millisecond calls
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

        let event = ReActEvent::Clarification {
            session_id: self.session_id.clone(),
            payload,
        };

        if let Err(e) = self.event_sender.send(event) {
            // Frontend disconnected — clean up
            let mut map = self.pending.lock().await;
            map.remove(&question_id);
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("无法发送问题到前端: {}", e)),
            };
        }

        eprintln!(
            "[Question] Sent question {} (mode={}) — waiting for user reply",
            question_id, mode
        );

        // Await the user's reply (resolved by `answer_question` Tauri command)
        match rx.await {
            Ok(answer) => {
                eprintln!("[Question] Got reply for {}: {}", question_id, answer);
                ToolResult {
                    success: true,
                    output: answer,
                    error: None,
                }
            }
            Err(_) => {
                // oneshot was dropped (session cancelled or timeout)
                let mut map = self.pending.lock().await;
                map.remove(&question_id);
                ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("用户取消了回复".to_string()),
                }
            }
        }
    }
}

// ── Resolve a pending question (called from Tauri command) ──────────────────

/// Resolve a pending question from the frontend.
///
/// Called by the Chat UI after the user picks an option or types a response.
/// This wakes up the blocked `QuestionTool::call()` oneshot receiver.
pub async fn answer_question(
    pending: &PendingQuestions,
    question_id: &str,
    answer: &str,
) -> Result<(), String> {
    let mut map = pending.lock().await;
    if let Some(sender) = map.remove(question_id) {
        sender.send(answer.to_string())
            .map_err(|_| format!("问题 {} 的等待已超时或被取消", question_id))?;
        Ok(())
    } else {
        Err(format!("未找到待回复的问题: {}", question_id))
    }
}