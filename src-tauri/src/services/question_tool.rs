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
//!
//! ## Implementation
//!
//! The actual `RigQuestionTool` struct implementing `rig_core::tool::Tool` trait
//! is in `rig_tool.rs`. This file only contains shared types and the pending
//! questions registry.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

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

// ── Answer resolution ───────────────────────────────────────────────────────

/// Resolve a pending question with the user's answer.
///
/// Called by the `answer_question` Tauri command when the frontend sends back
/// the user's response.
pub async fn answer_question(
    pending: &PendingQuestions,
    question_id: &str,
    answer: &str,
) -> Result<(), String> {
    let mut map = pending.lock().await;
    match map.remove(question_id) {
        Some(tx) => {
            let _ = tx.send(answer.to_string());
            Ok(())
        }
        None => Err(format!("问题 {} 不存在或已回答", question_id)),
    }
}
