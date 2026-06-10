//! `question` 工具用于让 Agent 在执行过程中向用户追问。
//!
//! 工具协议参考 opencode：模型一次传入 `questions[]`，每个问题包含
//! `question/header/options/multiple/custom`。后端把整组问题作为一次
//! clarification 事件发给前端并阻塞等待回答，最后按顺序返回答案。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

// ── 待回答问题注册表 ───────────────────────────────────────────────────────

/// 前端对待回答问题的处理结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingQuestionReply {
    Answer(String),
    Rejected,
}

/// 全局待回答问题表。
/// Key 是单个前端问题 ID，Value 在前端调用 answer_question/reject_question 时被解析。
pub type PendingQuestions = Arc<Mutex<HashMap<String, oneshot::Sender<PendingQuestionReply>>>>;

/// 创建新的待回答问题表。
pub fn create_pending_questions() -> PendingQuestions {
    Arc::new(Mutex::new(HashMap::new()))
}

// ── 发送给前端的澄清问题载荷 ───────────────────────────────────────────────

/// 单个可选项，兼容 opencode 的 Question.Option。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

impl QuestionOption {
    pub fn new(label: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            description: description.into(),
        }
    }
}

/// 同一次 question 工具调用中的单个问题。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarificationQuestion {
    pub prompt: String,
    pub header: String,
    pub mode: String,
    pub options: Vec<QuestionOption>,
    pub multiple: bool,
    pub custom: bool,
}

/// 前端渲染单个澄清问题所需的完整载荷。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarificationPayload {
    pub question_id: String,
    pub prompt: String,
    pub header: String,
    pub mode: String,
    pub options: Vec<QuestionOption>,
    pub multiple: bool,
    pub custom: bool,
    pub questions: Vec<ClarificationQuestion>,
}

// ── 回答解析 ───────────────────────────────────────────────────────────────

/// 用用户回答解析一个待回答问题。
pub async fn answer_question(
    pending: &PendingQuestions,
    question_id: &str,
    answer: &str,
) -> Result<(), String> {
    let mut map = pending.lock().await;
    match map.remove(question_id) {
        Some(tx) => {
            let _ = tx.send(PendingQuestionReply::Answer(answer.to_string()));
            Ok(())
        }
        None => Err(format!("问题 {} 不存在或已回答", question_id)),
    }
}

/// 取消一个待回答问题。
pub async fn reject_question(pending: &PendingQuestions, question_id: &str) -> Result<(), String> {
    let mut map = pending.lock().await;
    match map.remove(question_id) {
        Some(tx) => {
            let _ = tx.send(PendingQuestionReply::Rejected);
            Ok(())
        }
        None => Err(format!("问题 {} 不存在或已回答", question_id)),
    }
}
