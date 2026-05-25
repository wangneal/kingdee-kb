//! rig Agent 核心实现 — 替代手写的 ReAct 循环
//!
//! 使用 rig 的流式 API 和原生 function calling。
//! 中间事件（Thinking、ToolCall、ToolResult、TextDelta）
//! 通过 ReActEvent 实时推送到前端。

use std::collections::VecDeque;

use futures::StreamExt;
use tokio::sync::mpsc;

use crate::services::llm_service::{ChatMessage, LLMProvider, LLMService};
use crate::services::question_tool::PendingQuestions;
use crate::services::react_agent::ReActEvent;
use crate::services::rig_provider::{build_anthropic_client, build_openai_client};
use crate::services::rig_tool::{all_rig_tools, RigQuestionTool};
use rig::client::CompletionClient;
use rig::streaming::StreamingPrompt;

/// 死循环阈值：如果最近 N 次工具调用的 name+args 完全相同，则提前中断。
/// 在 `drain_stream()` 中执行 — 独立于 rig 的 `default_max_turns` 硬限制。
const DOOM_LOOP_THRESHOLD: usize = 3;

/// Agent 循环的默认最大轮数
const DEFAULT_MAX_TURNS: usize = 10;

/// RigAgent — 使用 rig 实现替代 ReActAgent
///
/// 零大小类型；所有状态保存在 rig 的 agent builder 中。
pub struct RigAgent;

impl RigAgent {
    /// 运行基于 rig 的 agent 流式循环。
    ///
    /// 使用 `stream_prompt()` 接收中间事件：
    /// - `Text` → `ReActEvent::TextDelta`
    /// - `ToolCall` → `ReActEvent::ToolCall`
    /// - `StreamedUserContent::ToolResult` → `ReActEvent::ToolResult`
    /// - `Reasoning` → `ReActEvent::Thinking`
    /// - `FinalResponse` → `ReActEvent::Done`
    pub async fn run(
        llm: &LLMService,
        user_message: &str,
        system_extra: &str,
        _history: &[ChatMessage],
        sender: mpsc::UnboundedSender<ReActEvent>,
        session_id: &str,
        pending: PendingQuestions,
    ) {
        let sid = session_id.to_string();

        // 1. 获取 LLM 配置
        let config = match llm.get_config() {
            Ok(c) => c,
            Err(e) => {
                let _ = sender.send(ReActEvent::Error {
                    session_id: sid,
                    message: e,
                });
                return;
            }
        };

        // 2. 构建系统提示词
        let system_prompt = format!(
            "\
{extra}\
你是一个金蝶ERP实施顾问AI助手。你可以调用工具来获取信息或执行操作。

【工作方式】
在每次回答前，先思考你需要什么信息、需要调用什么工具。
如果你已经有足够信息，直接回答。
如果你需要更多信息，调用工具，观察结果后再决定下一步。

【规则】
- 一次只调用一个工具
- 观察工具结果后再决定下一步
- 最多允许 {max_turns} 次工具调用
- 如果你已经有足够信息，直接回答，不要额外调用工具",
            extra = system_extra,
            max_turns = DEFAULT_MAX_TURNS,
        );

        let model = &config.model;
        let temperature = config.temperature as f64;
        let max_tokens = config.max_tokens as u64;

        // 3. 按 provider 分支，流式推送 agent 事件
        match config.provider {
            LLMProvider::OpenAI | LLMProvider::Local => {
                let client = match build_openai_client(&config) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = sender.send(ReActEvent::Error {
                            session_id: sid,
                            message: e,
                        });
                        return;
                    }
                };

                // 使用传统的 Chat Completions API（/v1/chat/completions）
                // 而不是新的 Responses API（/v1/responses）
                let completions_client = client.completions_api();

                let mut tools = all_rig_tools();
                tools.push(Box::new(RigQuestionTool::new(
                    pending.clone(),
                    sender.clone(),
                    sid.clone(),
                )));

                let mut stream = completions_client
                    .agent(model)
                    .preamble(&system_prompt)
                    .tools(tools)
                    .temperature(temperature)
                    .max_tokens(max_tokens)
                    .default_max_turns(DEFAULT_MAX_TURNS)
                    .build()
                    .stream_prompt(user_message)
                    .await;

                Self::drain_stream(&mut stream, &sender, &sid).await;
            }
            LLMProvider::Anthropic => {
                let client = match build_anthropic_client(&config) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = sender.send(ReActEvent::Error {
                            session_id: sid,
                            message: e,
                        });
                        return;
                    }
                };

                let mut tools = all_rig_tools();
                tools.push(Box::new(RigQuestionTool::new(
                    pending.clone(),
                    sender.clone(),
                    sid.clone(),
                )));

                let mut stream = client
                    .agent(model)
                    .preamble(&system_prompt)
                    .tools(tools)
                    .temperature(temperature)
                    .max_tokens(max_tokens)
                    .default_max_turns(DEFAULT_MAX_TURNS)
                    .build()
                    .stream_prompt(user_message)
                    .await;

                Self::drain_stream(&mut stream, &sender, &sid).await;
            }
        }
    }

    /// 消费 rig 流式响应，将每个 item 映射为 ReActEvent。
    /// 同时跟踪最近的工具调用以检测死循环。
    async fn drain_stream<R>(
        stream: &mut rig::agent::StreamingResult<R>,
        sender: &mpsc::UnboundedSender<ReActEvent>,
        sid: &str,
    ) {
        use rig::agent::MultiTurnStreamItem;
        use rig::streaming::{StreamedAssistantContent, StreamedUserContent};

        // 跟踪最近的工具调用 (name, args) 以检测死循环
        let mut recent_calls: VecDeque<(String, String)> = VecDeque::with_capacity(DOOM_LOOP_THRESHOLD);

        while let Some(item) = stream.next().await {
            match item {
                Ok(MultiTurnStreamItem::StreamAssistantItem(content)) => {
                    match content {
                        StreamedAssistantContent::Text(text) => {
                            let _ = sender.send(ReActEvent::TextDelta {
                                session_id: sid.to_string(),
                                content: text.text,
                            });
                        }
                        StreamedAssistantContent::ToolCall { tool_call, .. } => {
                            let name = tool_call.function.name.clone();
                            let args = tool_call.function.arguments.to_string();

                            let _ = sender.send(ReActEvent::ToolCall {
                                session_id: sid.to_string(),
                                name: name.clone(),
                                args: args.clone(),
                            });

                            // 死循环检测
                            recent_calls.push_back((name, args));
                            if recent_calls.len() > DOOM_LOOP_THRESHOLD {
                                recent_calls.pop_front();
                            }
                            if recent_calls.len() == DOOM_LOOP_THRESHOLD
                                && recent_calls.front().map_or(false, |first| recent_calls.iter().all(|c| c == first))
                            {
                                let _ = sender.send(ReActEvent::Error {
                                    session_id: sid.to_string(),
                                    message: format!(
                                        "检测到死循环：连续 {} 次相同的工具调用，已中断。",
                                        DOOM_LOOP_THRESHOLD
                                    ),
                                });
                                return;
                            }
                        }
                        StreamedAssistantContent::Reasoning(reasoning) => {
                            let text = reasoning.display_text();
                            if !text.is_empty() {
                                let _ = sender.send(ReActEvent::Thinking {
                                    session_id: sid.to_string(),
                                    content: text.to_string(),
                                });
                            }
                        }
                        // Delta 变体 — 增量更新，忽略
                        StreamedAssistantContent::ToolCallDelta { .. } => {}
                        StreamedAssistantContent::ReasoningDelta { .. } => {}
                        // Provider 特定的最终响应 — 忽略（使用 FinalResponse）
                        StreamedAssistantContent::Final(_) => {}
                    }
                }
                Ok(MultiTurnStreamItem::StreamUserItem(user_content)) => {
                    match user_content {
                        StreamedUserContent::ToolResult { tool_result, .. } => {
                            let result_text = tool_result
                                .content
                                .iter()
                                .filter_map(|c| match c {
                                    rig::completion::message::ToolResultContent::Text(t) => {
                                        Some(t.text.clone())
                                    }
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                                .join("");

                            let _ = sender.send(ReActEvent::ToolResult {
                                session_id: sid.to_string(),
                                name: String::new(),
                                result: result_text,
                            });
                        }
                    }
                }
                Ok(MultiTurnStreamItem::FinalResponse(_)) => {
                    let _ = sender.send(ReActEvent::Done {
                        session_id: sid.to_string(),
                    });
                }
                // non_exhaustive 回退处理新变体
                Ok(_) => {}
                Err(e) => {
                    let _ = sender.send(ReActEvent::Error {
                        session_id: sid.to_string(),
                        message: format!("流式错误: {}", e),
                    });
                }
            }
        }
    }
}
