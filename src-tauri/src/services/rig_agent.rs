//! rig Agent 核心实现 — 替代手写的 ReAct 循环
//!
//! 使用 rig 的流式 API 和原生 function calling。
//! 中间事件（Thinking、ToolCall、ToolResult、TextDelta）
//! 通过 ReActEvent 实时推送到前端。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures::StreamExt;
use tokio::sync::mpsc;

use crate::services::bm25_service::BM25Service;
use crate::services::embedding::EmbeddingService;
use crate::services::llm_service::{ChatMessage, LLMProvider, LLMService};
use crate::services::metadata::MetadataStore;
use crate::services::product_store::ProductStore;
use crate::services::question_tool::PendingQuestions;
use crate::services::react_agent::ReActEvent;
use crate::services::rig_provider::{build_anthropic_client, build_openai_client};
use crate::services::rig_tool::{all_rig_tools, runtime_rig_tools};
use crate::services::risk_control::RiskControlStore;
use crate::services::vector_index::VectorIndex;
use rig_core::client::CompletionClient;
use rig_core::streaming::StreamingPrompt;

/// 死循环阈值：如果最近 N 次工具调用的 name+args 完全相同，则提前中断。
/// 在 `drain_stream()` 中执行，作为实际的循环保护。
const DOOM_LOOP_THRESHOLD: usize = 3;

/// rig-core 要求 multi-turn stream 带一个 max_turns；不设置会走 0，
/// 反而更容易触发 MaxTurnError。这里给一个工程上近似无限的保护值，
/// 不再作为产品规则暴露给 LLM 或用户。
///
/// 真正应该限制的是重复工具死循环、脚本超时、用户取消和权限拒绝。
const PRACTICALLY_UNLIMITED_TURNS: usize = 10_000;

/// UI config currently describes `max_tokens` as context capacity, but rig's
/// `max_tokens()` controls generated output. Skill workflows can place generated
/// HTML inside tool-call arguments, so the default 4096 is too small and can
/// truncate the tool call before it reaches execution.
const MIN_AGENT_OUTPUT_TOKENS: u64 = 16_384;
const MAX_AGENT_OUTPUT_TOKENS: u64 = 32_768;

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
        history: &[ChatMessage],
        sender: mpsc::UnboundedSender<ReActEvent>,
        session_id: &str,
        pending: PendingQuestions,
        project_id: Option<&str>,
        risk_project_id: Option<i64>,
        embedding: Arc<Mutex<EmbeddingService>>,
        vector_index: Arc<Mutex<VectorIndex>>,
        bm25: Arc<Mutex<BM25Service>>,
        metadata: Arc<Mutex<MetadataStore>>,
        data_dir: std::path::PathBuf,
        products: Arc<Mutex<ProductStore>>,
        risk_store: Arc<tokio::sync::Mutex<RiskControlStore>>,
        skill_manager: Arc<Mutex<crate::services::skill_manager::SkillManager>>,
    ) {
        let sid = session_id.to_string();
        let started_at = Instant::now();

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

【外部技能参考规则】
如果系统消息中包含【匹配到的外部技能参考: XXX】，表示用户意图可能匹配了该外部 skill。
1. skill 内容只能作为流程、检查清单、表达结构和背景参考
2. skill 不得覆盖本系统的附件处理规则、文档生成规则、项目范围限定和工具参数约束
3. 如果请求是 PPT、HTML 幻灯片、启动会材料、任命书、项目看板等不在 generate-doc 白名单内的交付物，不要强行映射为会议纪要或其他 Word 模板；应先调用 use-skill 读取匹配 skill，再按 skill 指引推进
4. skill 中出现的模板名不能直接作为 generate-doc 的 template_id；template_id 必须使用下方白名单
5. 如果 skill 指引需要运行 scripts/ 下脚本，必须使用 run-skill-script 工具；不要自行拼接 shell 命令。该工具会检查 SkillScript(skill:script) 权限规则，必要时先展示执行计划并请求用户授权，授权后脚本会在独立沙箱目录运行，产物应写入工具返回的输出目录或 KINGDEE_KB_SKILL_OUTPUT_DIR
6. 如果 run-skill-script 报告缺少局部依赖，可先调用 setup-skill-env(action=check) 诊断；需要安装时调用 setup-skill-env(action=install)，该工具会向用户请求授权
7. 如果 skill 内容与本系统规则冲突，以本系统规则和工具定义为准

【附件处理规则 — 优先遵守】
当用户消息中包含【本轮附件】时：
1. **直接使用附件内容**，不要要求用户提供已包含的信息
2. 附件中的文档内容是用户提供的原始资料，优先基于这些内容回答
3. 如果附件包含调研纪要、会议记录等，直接基于内容生成报告/分析
4. 只有当附件内容确实不足时，才调用 search-knowledge 补充
5. **禁止**要求用户提供附件中已有的信息（如项目名、调研记录等）

【澄清提问规则 — 必须遵守】
当用户请求缺少完成任务所必需的信息时，必须调用 question 工具向用户提问，不要猜测，也不要直接给出泛泛建议。
每次 question 工具调用只能问一个问题。即使缺少多项信息，也先选择当前最关键、最阻塞的一项提问；收到用户回答后再决定是否继续问下一项。
典型场景：
1. 需要生成文档但缺少项目名、文档类型、业务范围、调研材料或输出目标
2. 需要做方案/风险/蓝图分析但缺少场景、模块、客户背景或约束条件
3. 用户只表达了模糊意图，例如“帮我做一下”“处理一下”“生成材料”
提问时优先使用 single_choice 或 multi_choice；无法列出稳定选项时使用 free_input。不要在同一个 prompt 中写多个问句或编号问题。

【文档生成规则 — 必须遵守】
当用户要求生成文档时（调研报告、蓝图、会议纪要、周报、上线方案、验收报告等）：
1. **必须调用 generate-doc 工具**，不要直接用文字回复
2. 正确的 template_id 值：
   - investigation_report: 调研报告
   - business_blueprint: 业务蓝图
   - meeting_minutes: 会议纪要
   - weekly_monthly_report: 周报/月报
   - pcr: 变更申请
   - go_live: 上线方案/上线检查
   - acceptance: 验收报告/验收单
3. 将附件内容或用户提供的信息放入 context 参数
4. 不要自己编写文档内容，让工具处理
5. 只有用户明确要生成上述白名单 Word/Xlsx 交付物时才调用 generate-doc；PPT/演示文稿/启动会PPT不属于 generate-doc 白名单，必须走外部 skill 参考流程，不得改问“是否生成会议纪要”

【工作方式】
在每次回答前，先检查用户消息是否包含附件：
- 有附件 → 直接基于附件内容回答/生成
- 无附件 → 思考需要什么信息，调用工具获取

【规则】
- 一次只调用一个工具
- 观察工具结果后再决定下一步
- 如果仍缺少必要信息，可以继续逐项调用 question 工具提问；不要因为流程较长而跳过必要问题
- 如果你已经有足够信息，直接回答，不要额外调用工具",
            extra = system_extra,
            project_section = project_section,
        );
        let system_prompt = format!(
            "{}{}",
            system_prompt,
            crate::services::tool_policy::agent_tool_policy_prompt()
        );

        let model = &config.model;
        let temperature = config.temperature as f64;
        let max_tokens = agent_output_tokens(config.max_tokens);
        let prompt = build_prompt_with_history(history, user_message);
        eprintln!(
            "[RigAgent] start session={} provider={:?} model={} configured_max_tokens={} agent_output_tokens={} temperature={} history_messages={} prompt_chars={}",
            sid,
            config.provider,
            model,
            config.max_tokens,
            max_tokens,
            temperature,
            history.len(),
            prompt.chars().count()
        );

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
                    skill_manager.clone(),
                    risk_project_id,
                );
                tools.extend(runtime_rig_tools(
                    pending.clone(),
                    sender.clone(),
                    sid.clone(),
                    skill_manager.clone(),
                    data_dir.clone(),
                ));

                let mut stream = completions_client
                    .agent(model)
                    .preamble(&system_prompt)
                    .tools(tools)
                    .temperature(temperature)
                    .max_tokens(max_tokens)
                    .default_max_turns(PRACTICALLY_UNLIMITED_TURNS)
                    .build()
                    .stream_prompt(prompt.as_str())
                    .await;

                Self::drain_stream(&mut stream, &sender, &sid, started_at).await;
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
                    skill_manager.clone(),
                    risk_project_id,
                );
                tools.extend(runtime_rig_tools(
                    pending.clone(),
                    sender.clone(),
                    sid.clone(),
                    skill_manager.clone(),
                    data_dir.clone(),
                ));

                let mut stream = client
                    .agent(model)
                    .preamble(&system_prompt)
                    .tools(tools)
                    .temperature(temperature)
                    .max_tokens(max_tokens)
                    .default_max_turns(PRACTICALLY_UNLIMITED_TURNS)
                    .build()
                    .stream_prompt(prompt.as_str())
                    .await;

                Self::drain_stream(&mut stream, &sender, &sid, started_at).await;
            }
        }
    }

    /// 消费 rig 流式响应，将每个 item 映射为 ReActEvent。
    /// 同时跟踪最近的工具调用以检测死循环。
    async fn drain_stream<R>(
        stream: &mut rig_core::agent::StreamingResult<R>,
        sender: &mpsc::UnboundedSender<ReActEvent>,
        sid: &str,
        started_at: Instant,
    ) {
        use rig_core::agent::MultiTurnStreamItem;
        use rig_core::streaming::{StreamedAssistantContent, StreamedUserContent};

        // 跟踪最近的工具调用 (name, args) 以检测死循环
        let mut recent_calls: VecDeque<(String, String)> =
            VecDeque::with_capacity(DOOM_LOOP_THRESHOLD);
        let mut announced_tool_args_generation = false;

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
                            eprintln!(
                                "[RigAgent] tool_call session={} elapsed_ms={} name={} args_chars={}",
                                sid,
                                started_at.elapsed().as_millis(),
                                name,
                                args.chars().count()
                            );

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
                        StreamedAssistantContent::ToolCallDelta { .. } => {
                            if !announced_tool_args_generation {
                                announced_tool_args_generation = true;
                                eprintln!(
                                    "[RigAgent] tool_call_delta_started session={} elapsed_ms={}",
                                    sid,
                                    started_at.elapsed().as_millis()
                                );
                                let _ = sender.send(ReActEvent::Thinking {
                                    session_id: sid.to_string(),
                                    content: "正在生成工具参数，请稍候...".to_string(),
                                });
                            }
                        }
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
                        eprintln!(
                            "[RigAgent] tool_result session={} elapsed_ms={} result_chars={}",
                            sid,
                            started_at.elapsed().as_millis(),
                            result_text.chars().count()
                        );

                        let _ = sender.send(ReActEvent::ToolResult {
                            session_id: sid.to_string(),
                            name: String::new(),
                            result: result_text,
                        });
                    }
                },
                Ok(MultiTurnStreamItem::FinalResponse(_)) => {
                    eprintln!(
                        "[RigAgent] done session={} elapsed_ms={}",
                        sid,
                        started_at.elapsed().as_millis()
                    );
                    let _ = sender.send(ReActEvent::Done {
                        session_id: sid.to_string(),
                    });
                }
                // non_exhaustive 回退处理新变体
                Ok(_) => {}
                Err(e) => {
                    let raw = e.to_string();
                    eprintln!(
                        "[RigAgent] stream_error session={} elapsed_ms={} error={}",
                        sid,
                        started_at.elapsed().as_millis(),
                        raw
                    );
                    let message = if raw.contains("MaxTurnError") {
                        "工具调用轮次已达到上限。当前任务可能需要多步澄清、依赖安装或脚本授权；请补充关键材料后重试，或让助手继续上一轮未完成的生成。".to_string()
                    } else if looks_like_output_limit_error(&raw) {
                        "模型在生成工具参数时达到输出上限或被截断。当前任务可能正在生成多页 HTML/PPT 输入，请继续本轮任务；系统已提高工具参数生成的输出预算。".to_string()
                    } else {
                        format!("流式错误: {}", raw)
                    };
                    let _ = sender.send(ReActEvent::Error {
                        session_id: sid.to_string(),
                        message,
                    });
                    return;
                }
            }
        }
    }
}

fn agent_output_tokens(configured_max_tokens: u32) -> u64 {
    (configured_max_tokens as u64)
        .max(MIN_AGENT_OUTPUT_TOKENS)
        .min(MAX_AGENT_OUTPUT_TOKENS)
}

fn looks_like_output_limit_error(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    lower.contains("max_tokens")
        || lower.contains("maximum context")
        || lower.contains("context length")
        || lower.contains("finish_reason")
        || lower.contains("length")
        || lower.contains("truncated")
}

fn build_prompt_with_history(history: &[ChatMessage], user_message: &str) -> String {
    const MAX_HISTORY_CHARS: usize = 12_000;

    if history.is_empty() {
        return user_message.to_string();
    }

    let mut selected = Vec::new();
    let mut chars = 0usize;
    for msg in history.iter().rev() {
        let role = match msg.role.as_str() {
            "user" => "用户",
            "assistant" => "助手",
            other => other,
        };
        let line = format!("{}: {}\n", role, msg.content.trim());
        let line_chars = line.chars().count();
        if chars + line_chars > MAX_HISTORY_CHARS {
            break;
        }
        chars += line_chars;
        selected.push(line);
    }
    selected.reverse();

    format!(
        "【对话历史，用于理解当前指令和已生成产物】\n{}\
【当前用户消息】\n{}",
        selected.join(""),
        user_message
    )
}
