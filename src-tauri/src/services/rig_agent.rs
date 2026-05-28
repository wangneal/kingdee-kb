//! rig Agent 核心实现 — 替代手写的 ReAct 循环
//!
//! 使用 rig 的流式 API 和原生 function calling。
//! 中间事件（Thinking、ToolCall、ToolResult、TextDelta）
//! 通过 ReActEvent 实时推送到前端。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use tokio::sync::mpsc;

use crate::services::bm25_service::BM25Service;
use crate::services::embedding::EmbeddingService;
use crate::services::llm_service::{ChatMessage, LLMProvider, LLMService};
use crate::services::metadata::MetadataStore;
use crate::services::product_store::ProductStore;
use crate::services::question_tool::PendingQuestions;
use crate::services::react_agent::ReActEvent;
use crate::services::risk_control::RiskControlStore;
use crate::services::rig_provider::{build_anthropic_client, build_openai_client};
use crate::services::rig_tool::{all_rig_tools, RigQuestionTool};
use crate::services::vector_index::VectorIndex;
use rig_core::client::CompletionClient;
use rig_core::streaming::StreamingPrompt;

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
        project_id: Option<&str>,
        embedding: Arc<Mutex<EmbeddingService>>,
        vector_index: Arc<Mutex<VectorIndex>>,
        bm25: Arc<Mutex<BM25Service>>,
        metadata: Arc<Mutex<MetadataStore>>,
        data_dir: std::path::PathBuf,
        products: Arc<Mutex<ProductStore>>,
        risk_store: Arc<tokio::sync::Mutex<RiskControlStore>>,
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
        let project_section = match project_id {
            Some(pid) => format!("\n【当前项目】{}\n所有工具调用（搜索知识库、生成文档等）都应限定在此项目范围内。\n", pid),
            None => String::new(),
        };
        let system_prompt = format!(
            "\
{extra}\
{project_section}\
你是一个金蝶ERP实施顾问AI助手。你可以调用工具来获取信息或执行操作。

【附件处理规则 — 优先遵守】
当用户消息中包含【本轮附件】时：
1. **直接使用附件内容**，不要要求用户提供已包含的信息
2. 附件中的文档内容是用户提供的原始资料，优先基于这些内容回答
3. 如果附件包含调研纪要、会议记录等，直接基于内容生成报告/分析
4. 只有当附件内容确实不足时，才调用 search-knowledge 补充
5. **禁止**要求用户提供附件中已有的信息（如项目名、调研记录等）

【文档生成规则 — 必须遵守】
当用户要求生成文档时（调研报告、蓝图、会议纪要、周报等）：
1. **必须调用 generate-doc 工具**，不要直接用文字回复
2. 正确的 template_id 值：
   - investigation_report: 调研报告
   - business_blueprint: 业务蓝图
   - meeting_minutes: 会议纪要
   - weekly_monthly_report: 周报/月报
   - pcr: 变更申请
3. 将附件内容或用户提供的信息放入 context 参数
4. 不要自己编写文档内容，让工具处理

【工作方式】
在每次回答前，先检查用户消息是否包含附件：
- 有附件 → 直接基于附件内容回答/生成
- 无附件 → 思考需要什么信息，调用工具获取

【规则】
- 一次只调用一个工具
- 观察工具结果后再决定下一步
- 最多允许 {max_turns} 次工具调用
- 如果你已经有足够信息，直接回答，不要额外调用工具",
            extra = system_extra,
            project_section = project_section,
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

                let mut tools = all_rig_tools(
                    project_id,
                    data_dir.clone(),
                    llm.clone(),
                    embedding.clone(),
                    vector_index.clone(),
                    bm25.clone(),
                    metadata.clone(),
                    products.clone(),
                    risk_store.clone(),
                );
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

                let mut tools = all_rig_tools(
                    project_id,
                    data_dir.clone(),
                    llm.clone(),
                    embedding.clone(),
                    vector_index.clone(),
                    bm25.clone(),
                    metadata.clone(),
                    products.clone(),
                    risk_store.clone(),
                );
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
        stream: &mut rig_core::agent::StreamingResult<R>,
        sender: &mpsc::UnboundedSender<ReActEvent>,
        sid: &str,
    ) {
        use rig_core::agent::MultiTurnStreamItem;
        use rig_core::streaming::{StreamedAssistantContent, StreamedUserContent};

        // 跟踪最近的工具调用 (name, args) 以检测死循环
        let mut recent_calls: VecDeque<(String, String)> =
            VecDeque::with_capacity(DOOM_LOOP_THRESHOLD);

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
                                && recent_calls
                                    .front()
                                    .map_or(false, |first| recent_calls.iter().all(|c| c == first))
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
                Ok(MultiTurnStreamItem::StreamUserItem(user_content)) => match user_content {
                    StreamedUserContent::ToolResult { tool_result, .. } => {
                        let result_text = tool_result
                            .content
                            .iter()
                            .filter_map(|c| match c {
                                rig_core::completion::message::ToolResultContent::Text(t) => {
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
                },
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
