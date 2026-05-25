//! ReAct 推理引擎 — 思考→行动→观察→循环
//!
//! Agent 流程:
//! 1. 组装 System Prompt（角色 + 工具描述 + 规则）
//! 2. LLM 返回决策（工具调用 或 最终回答）
//! 3. 工具调用 → 执行 → 结果喂回 LLM → 回到 2
//! 4. 最终回答 → 流式发送给前端
//!
//! 事件通过 mpsc channel 发出，由 Tauri 命令转发为 SSE 事件

use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::services::llm_service::{ChatMessage, LLMService};
use crate::services::question_tool::{ClarificationPayload, PendingQuestions, QuestionTool};
use crate::services::tool_registry::{Tool, ToolRegistry};

/// ReAct 事件 — 通过 SSE 流式发送给前端
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReActEvent {
    #[serde(rename = "thinking")]
    Thinking { session_id: String, content: String },
    #[serde(rename = "tool_call")]
    ToolCall { session_id: String, name: String, args: String },
    #[serde(rename = "tool_result")]
    ToolResult { session_id: String, name: String, result: String },
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
}

/// LLM 返回的决策
enum ReActDecision {
    ToolCall {
        thought: String,
        tool: String,
        args: HashMap<String, String>,
    },
    Answer {
        thought: String,
        content: String,
    },
}

/// ReAct 推理引擎
pub struct ReActAgent {
    tools: Arc<ToolRegistry>,
    max_iterations: usize,
}

impl ReActAgent {
    pub fn new(tools: Arc<ToolRegistry>) -> Self {
        Self {
            tools,
            max_iterations: 10,
        }
    }

    /// 运行 ReAct 循环，通过 sender 发送事件
    /// `pending` — 全局待回复问题注册表，用于 `question` 工具的跨进程通信
    pub async fn run(
        &self,
        llm: &LLMService,
        user_message: &str,
        system_extra: &str,
        history: &[ChatMessage],
        sender: mpsc::UnboundedSender<ReActEvent>,
        session_id: &str,
        pending: PendingQuestions,
    ) {
        let sid = session_id.to_string();
        let tool_descriptions = self.tools.get_tool_descriptions();
        // Append question tool description so LLM knows it can call it
        let question_desc = "## question\n向用户提问以获取更多信息。当用户的问题模糊、需要选择方向、或需要补充细节时使用此工具。\n参数:\n  - prompt [必填]: 要向用户提出的问题文本 (string)\n  - mode: 提问模式：single_choice(单选)/multi_choice(多选)/free_input(自由输入) (string)\n  - options: 选项列表（JSON数组字符串，如 [\"选项1\",\"选项2\"]，仅single_choice/multi_choice模式需要）(string)";
        let all_tools_desc = format!("{}\n\n{}", tool_descriptions, question_desc);
        let system_prompt = format!(
            "{}你是一个金蝶ERP实施顾问AI助手。你有权调用以下工具来帮助用户。\n\
             \n\
             【可用工具】\n\
             {}\n\
             \n\
             【工作方式】\n\
             在每次回答前，先思考你需要什么信息、需要调用什么工具。\n\
             如果你确定可以直接回答，用 answer 类型输出。\n\
             如果你需要更多信息，用 tool_call 类型调用工具，观察结果后再决定下一步。\n\
             \n\
             请严格按照以下JSON格式输出你的决策（不要添加其他文字）：\n\
             - 如果要调用工具：{{\"type\":\"tool_call\",\"thought\":\"...\",\"tool\":\"工具名\",\"args\":{{\"参数名\":\"值\"}}}}\n\
             - 如果要回答：{{\"type\":\"answer\",\"thought\":\"...\",\"content\":\"回答内容\"}}\n\
             \n\
             【规则】\n\
             - 一次只调用一个工具\n\
             - 观察工具结果后再决定下一步\n\
             - 最多允许 {} 次工具调用\n\
             - 如果你已经有足够信息，直接回答，不要额外调用工具",
            system_extra, all_tools_desc, self.max_iterations
        );

        let mut messages = vec![ChatMessage {
            role: "system".to_string(),
            content: system_prompt,
        }];
        for msg in history {
            messages.push(msg.clone());
        }
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: user_message.to_string(),
        });

        for _iteration in 0..self.max_iterations {
            let config = match llm.get_config() {
                Ok(c) => c,
                Err(e) => {
                    let _ = sender.send(ReActEvent::Error { session_id: sid.clone(), message: e });
                    break;
                }
            };

            let response = llm
                .chat_completion(&messages, &config)
                .await;

            match response {
                Ok(text) => {
                    let text: String = text;
                    if let Ok(decision) = serde_json::from_str::<ReActDecisionJson>(&text) {
                        match decision.type_field.as_str() {
                            "tool_call" => {
                                let thought = decision.thought.clone().unwrap_or_default();
                                let _ = sender.send(ReActEvent::Thinking {
                                    session_id: sid.clone(),
                                    content: thought,
                                });
                                let tool_name = decision.tool.clone().unwrap_or_default();
                                let args_str = serde_json::to_string(&decision.args).unwrap_or_default();
                                let _ = sender.send(ReActEvent::ToolCall {
                                    session_id: sid.clone(),
                                    name: tool_name.clone(),
                                    args: args_str,
                                });

                                // Execute tool — intercept `question` tool for per-session handling
                                let tool_args = decision.args.unwrap_or_default();
                                let result = if tool_name == "question" {
                                    // QuestionTool needs per-session state (pending + sender),
                                    // so we construct and call it directly instead of going through registry
                                    let qt = QuestionTool::new(
                                        pending.clone(),
                                        sender.clone(),
                                        sid.clone(),
                                    );
                                    qt.call(tool_args).await
                                } else {
                                    self.tools.call_tool(&tool_name, tool_args).await
                                };
                                let result_str = if result.success {
                                    result.output
                                } else {
                                    format!("错误: {}", result.error.unwrap_or_default())
                                };
                                let _ = sender.send(ReActEvent::ToolResult {
                                    session_id: sid.clone(),
                                    name: tool_name.clone(),
                                    result: result_str.clone(),
                                });

                                messages.push(ChatMessage {
                                    role: "assistant".to_string(),
                                    content: format!("调用工具: {}\n结果: {}", tool_name, result_str),
                                });
                            }
                            "answer" => {
                                let thought = decision.thought.unwrap_or_default();
                                let _ = sender.send(ReActEvent::Thinking {
                                    session_id: sid.clone(),
                                    content: thought,
                                });
                                let content = decision.content.unwrap_or_default();
                                // Send in 10-char chunks for performance
                                let chars: Vec<char> = content.chars().collect();
                                for chunk in chars.chunks(10) {
                                    let s: String = chunk.iter().collect();
                                    let _ = sender.send(ReActEvent::TextDelta {
                                        session_id: sid.clone(),
                                        content: s,
                                    });
                                }
                                let _ = sender.send(ReActEvent::Done { session_id: sid.clone() });
                                return;
                            }
                            _ => {
                                let _ = sender.send(ReActEvent::TextDelta { session_id: sid.clone(), content: text });
                                let _ = sender.send(ReActEvent::Done { session_id: sid.clone() });
                                return;
                            }
                        }
                    } else {
                        let _ = sender.send(ReActEvent::TextDelta { session_id: sid.clone(), content: text });
                        let _ = sender.send(ReActEvent::Done { session_id: sid.clone() });
                        return;
                    }
                }
                Err(e) => {
                    let _ = sender.send(ReActEvent::Error { session_id: sid.clone(), message: e });
                    break;
                }
            }
        }
        let _ = sender.send(ReActEvent::Error {
            session_id: sid.clone(),
            message: "超出最大迭代次数，请简化问题或提供更详细的信息。".to_string(),
        });
    }
}

/// JSON 解析用的中间结构
#[derive(Debug, Deserialize)]
struct ReActDecisionJson {
    #[serde(rename = "type")]
    type_field: String,
    thought: Option<String>,
    tool: Option<String>,
    args: Option<HashMap<String, String>>,
    content: Option<String>,
}
