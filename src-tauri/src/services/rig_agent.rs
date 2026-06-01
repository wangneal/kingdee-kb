//! rig Agent 核心实现 — 替代手写的 ReAct 循环
//!
//! 使用 rig 的流式 API 和原生 function calling。
//! 中间事件（Thinking、ToolCall、ToolResult、TextDelta）
//! 通过 ReActEvent 实时推送到前端。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::StreamExt;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::services::agent_router;
use crate::services::agent_timeout::AGENT_SESSION_TIMEOUT_SECS;
use crate::services::bm25_service::BM25Service;
use crate::services::embedding::EmbeddingService;
use crate::services::llm_providers::LLMProtocol;
use crate::services::llm_service::{ChatMessage, LLMService};
use crate::services::metadata::MetadataStore;
use crate::services::planner::{self, PlanState, PlanStateMachine};
use crate::services::product_store::ProductStore;
use crate::services::question_tool::PendingQuestions;
use crate::services::react_agent::ReActEvent;
use crate::services::rig_provider::{build_anthropic_client, build_openai_client};
use crate::services::rig_tool::{all_rig_tools, runtime_rig_tools};
use crate::services::risk_control::RiskControlStore;
use crate::services::types::AgentMode;
use crate::services::vector_index::VectorIndex;
use rig_core::client::CompletionClient;
use rig_core::streaming::StreamingPrompt;

/// Agent 会话取消标志
///
/// 通过 `AtomicBool` 实现线程安全的取消信号。
/// `new()` 返回标志句柄和共享的原子布尔值，
/// 前者用于外部取消，后者传入 agent 循环检测。
pub struct AgentCancelFlag {
    cancelled: Arc<AtomicBool>,
}

impl AgentCancelFlag {
    pub fn new() -> (Self, Arc<AtomicBool>) {
        let flag = Arc::new(AtomicBool::new(false));
        (
            Self {
                cancelled: flag.clone(),
            },
            flag,
        )
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

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

/// 每会话工具调用速率限制器
struct ToolRateLimiter {
    max_calls_per_minute: u32,
    recent_calls: Vec<Instant>,
}

impl ToolRateLimiter {
    fn new(max_calls_per_minute: u32) -> Self {
        Self {
            max_calls_per_minute,
            recent_calls: Vec::new(),
        }
    }

    fn check_and_record(&mut self) -> bool {
        let now = Instant::now();
        let one_minute_ago = now - Duration::from_secs(60);
        self.recent_calls.retain(|t| *t > one_minute_ago);
        if self.recent_calls.len() >= self.max_calls_per_minute as usize {
            false
        } else {
            self.recent_calls.push(now);
            true
        }
    }
}

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
        cancel_flag: Option<Arc<AtomicBool>>,
        provider_id: Option<&str>,
    ) {
        let sid = session_id.to_string();
        let started_at = Instant::now();

        // 1. 自动路由：检测是否需要多模态模型
        let has_images = detect_images_in_message(user_message);
        let effective_provider_id = if has_images && provider_id.is_none() {
            // 有图片且未指定供应商 → 尝试自动切换到多模态模型
            tracing::info!("检测到图片附件，尝试自动切换到多模态模型");
            None // 让 LLMService 自动选择
        } else {
            provider_id
        };

        // 1.5 Agent 模式路由：根据复杂度选择 ReAct 或 Plan-Execute
        let agent_mode = agent_router::route_mode(user_message, history);

        // 如果路由到 Plan-Execute 模式，尝试规划，超时则降级为 ReAct
        if agent_mode.contains(AgentMode::PlanExecute) {
            match Self::try_plan_execute(
                llm,
                user_message,
                system_extra,
                history,
                &sender,
                session_id,
                pending.clone(),
                project_id,
                risk_project_id,
                embedding.clone(),
                vector_index.clone(),
                bm25.clone(),
                metadata.clone(),
                data_dir.clone(),
                products.clone(),
                risk_store.clone(),
                skill_manager.clone(),
                cancel_flag.clone(),
                effective_provider_id,
            ).await {
                Ok(()) => return, // Plan-Execute completed
                Err(e) => {
                    tracing::warn!("Plan-Execute 失败，降级到 ReAct: {}", e);
                    let _ = sender.send(ReActEvent::PlannerTimeout {
                        session_id: session_id.to_string(),
                        message: format!("规划失败，已降级到快速模式: {}", e),
                    });
                    // Fall through to ReAct below
                }
            }
        }

        // 2. 获取 LLM 配置（支持指定供应商或自动路由）
        let config = match llm.get_config_for_provider(effective_provider_id) {
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
当用户要求生成文档时，按以下优先级处理：

**优先使用技能系统（推荐）：**
1. 首先检查是否有匹配的外部技能：
   - 调研报告/调研纪要 → survey-assistant 技能
   - 业务蓝图/蓝图文档 → blueprint-tools 技能
   - 启动会材料/任命书/项目看板 → kickoff-pack 技能
   - 上线方案/上线检查 → golive-pack 技能
   - 验收报告/验收单 → acceptance-pack 技能
   - 周报/月报 → weekly-report 技能
   - PPT/演示文稿 → kingdee-ppt 技能
   - 通用文档编辑/模板填充 → doc-tools 技能
2. 如果匹配到技能，调用 use-skill 读取技能内容，按技能指引推进
3. 如果技能需要运行脚本，使用 run-skill-script 工具执行

**回退到 generate-doc（兼容）：**
如果用户明确要求生成以下白名单 Word/Xlsx 交付物且无匹配技能，可调用 generate-doc：
1. 正确的 template_id 值：
   - investigation_report: 调研报告
   - business_blueprint: 业务蓝图
   - meeting_minutes: 会议纪要
   - weekly_monthly_report: 周报/月报
   - pcr: 变更申请
   - go_live: 上线方案/上线检查
   - acceptance: 验收报告/验收单
2. 将附件内容或用户提供的信息放入 context 参数
3. 不要自己编写文档内容，让工具处理

**重要提示：**
- 不要将非白名单交付物强行映射为 generate-doc 模板
- PPT/演示文稿/启动会PPT 必须走技能系统，不得改问\"是否生成会议纪要\"
- 优先尝试技能系统，generate-doc 仅作为兼容性回退

【工作方式 — 必须遵守】
在每次回答前：
1. 检查用户消息是否包含附件：
   - 有附件 → 直接基于附件内容回答/生成
   - 无附件 → **必须先调用 search-knowledge 工具搜索知识库**
2. **强制搜索规则**：当用户提到以下内容时，必须先搜索知识库再回答：
   - 项目名称、客户名称
   - 金蝶产品功能、模块、配置
   - 实施方法论、最佳实践
   - 具体技术问题、报错信息
   - 任何需要查证的事实性问题
3. 只有当用户问的是完全通用的问题（如 你好、今天天气）时，才可以跳过搜索直接回答
4. 搜索结果作为回答的依据，必须引用来源

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

        let model = config.get_default_model_name();
        let temperature = config.temperature as f64;
        let max_tokens = agent_output_tokens(config.max_tokens);
        let prompt = build_prompt_with_history(history, user_message);
        info!(
            session = %sid,
            provider = ?config.protocol,
            model = %model,
            configured_max_tokens = config.max_tokens,
            agent_output_tokens = max_tokens,
            temperature = temperature,
            history_messages = history.len(),
            prompt_chars = prompt.chars().count(),
            "agent session started"
        );

        // 3. 按 provider 分支，流式推送 agent 事件（带会话超时保护）
        let timeout_sender = sender.clone();
        let timeout_sid = sid.clone();

        let result = tokio::time::timeout(Duration::from_secs(AGENT_SESSION_TIMEOUT_SECS), async {
            let mut rate_limiter = ToolRateLimiter::new(30);
            match config.protocol {
                LLMProtocol::OpenAI | LLMProtocol::Local => {
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
                        .agent(&model)
                        .preamble(&system_prompt)
                        .tools(tools)
                        .temperature(temperature)
                        .max_tokens(max_tokens)
                        .default_max_turns(PRACTICALLY_UNLIMITED_TURNS)
                        .build()
                        .stream_prompt(prompt.as_str())
                        .await;

                    Self::drain_stream(
                        &mut stream,
                        &sender,
                        &sid,
                        started_at,
                        &mut rate_limiter,
                        cancel_flag.clone(),
                    )
                    .await;
                }
                LLMProtocol::Anthropic => {
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
                        .agent(&model)
                        .preamble(&system_prompt)
                        .tools(tools)
                        .temperature(temperature)
                        .max_tokens(max_tokens)
                        .default_max_turns(PRACTICALLY_UNLIMITED_TURNS)
                        .build()
                        .stream_prompt(prompt.as_str())
                        .await;

                    Self::drain_stream(
                        &mut stream,
                        &sender,
                        &sid,
                        started_at,
                        &mut rate_limiter,
                        cancel_flag,
                    )
                    .await;
                }
            }
        })
        .await;

        if result.is_err() {
            let _ = timeout_sender.send(ReActEvent::Error {
                session_id: timeout_sid.clone(),
                message: "会话超时（超过10分钟），请重新开始对话".to_string(),
            });
            let _ = timeout_sender.send(ReActEvent::Done {
                session_id: timeout_sid,
            });
        }
    }

    /// 消费 rig 流式响应，将每个 item 映射为 ReActEvent。
    /// 同时跟踪最近的工具调用以检测死循环。
    async fn drain_stream<R>(
        stream: &mut rig_core::agent::StreamingResult<R>,
        sender: &mpsc::UnboundedSender<ReActEvent>,
        sid: &str,
        started_at: Instant,
        rate_limiter: &mut ToolRateLimiter,
        cancel_flag: Option<Arc<AtomicBool>>,
    ) {
        use rig_core::agent::MultiTurnStreamItem;
        use rig_core::streaming::{StreamedAssistantContent, StreamedUserContent};

        // 跟踪最近的工具调用 (name, args) 以检测死循环
        let mut recent_calls: VecDeque<(String, String)> =
            VecDeque::with_capacity(DOOM_LOOP_THRESHOLD);
        let mut announced_tool_args_generation = false;

        while let Some(item) = stream.next().await {
            // 检查取消标志
            if cancel_flag
                .as_ref()
                .map_or(false, |f| f.load(Ordering::SeqCst))
            {
                let _ = sender.send(ReActEvent::Error {
                    session_id: sid.to_string(),
                    message: "用户已取消操作".to_string(),
                });
                let _ = sender.send(ReActEvent::Done {
                    session_id: sid.to_string(),
                });
                return;
            }

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
                            info!(
                                session = %sid,
                                elapsed_ms = started_at.elapsed().as_millis(),
                                name = %name,
                                args_chars = args.chars().count(),
                                "tool call"
                            );

                            // 速率限制检查
                            if !rate_limiter.check_and_record() {
                                let _ = sender.send(ReActEvent::Error {
                                    session_id: sid.to_string(),
                                    message: "工具调用过于频繁（每分钟上限30次），请稍后重试"
                                        .to_string(),
                                });
                                return;
                            }

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
                                debug!(
                                    session = %sid,
                                    elapsed_ms = started_at.elapsed().as_millis(),
                                    "tool call delta started"
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
                        debug!(
                            session = %sid,
                            elapsed_ms = started_at.elapsed().as_millis(),
                            result_chars = result_text.chars().count(),
                            "tool result"
                        );

                        let _ = sender.send(ReActEvent::ToolResult {
                            session_id: sid.to_string(),
                            name: String::new(),
                            result: result_text,
                        });
                    }
                },
                Ok(MultiTurnStreamItem::FinalResponse(_)) => {
                    info!(
                        session = %sid,
                        elapsed_ms = started_at.elapsed().as_millis(),
                        "agent session completed"
                    );
                    let _ = sender.send(ReActEvent::Done {
                        session_id: sid.to_string(),
                    });
                }
                // non_exhaustive 回退处理新变体
                Ok(_) => {}
                Err(e) => {
                    let raw = e.to_string();
                    error!(
                        session = %sid,
                        elapsed_ms = started_at.elapsed().as_millis(),
                        error = %raw,
                        "stream error"
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

    /// 尝试 Plan-Execute 模式执行
    /// 10 秒规划超时 → 自动降级到 ReAct
    async fn try_plan_execute(
        llm: &LLMService,
        user_message: &str,
        system_extra: &str,
        history: &[ChatMessage],
        sender: &mpsc::UnboundedSender<ReActEvent>,
        session_id: &str,
        _pending: PendingQuestions,
        project_id: Option<&str>,
        _risk_project_id: Option<i64>,
        _embedding: Arc<Mutex<EmbeddingService>>,
        _vector_index: Arc<Mutex<VectorIndex>>,
        _bm25: Arc<Mutex<BM25Service>>,
        _metadata: Arc<Mutex<MetadataStore>>,
        _data_dir: std::path::PathBuf,
        _products: Arc<Mutex<ProductStore>>,
        _risk_store: Arc<tokio::sync::Mutex<RiskControlStore>>,
        _skill_manager: Arc<Mutex<crate::services::skill_manager::SkillManager>>,
        _cancel_flag: Option<Arc<AtomicBool>>,
        provider_id: Option<&str>,
    ) -> Result<(), String> {
        let sid = session_id.to_string();

        // Step 1: Generate execution plan with 10s timeout
        let config = llm
            .get_config_for_provider(provider_id)
            .map_err(|e| format!("获取配置失败: {}", e))?;

        let plan_budget = config.max_tokens / 4; // Plan gets 25% of context budget
        let plan = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            planner::Planner::plan(
                user_message,
                system_extra,
                project_id.unwrap_or(""),
                llm,
                plan_budget,
            ),
        )
        .await
        .map_err(|_| "Planner 超时 (10s)".to_string())?
        .map_err(|e| format!("Planner 失败: {}", e))?;

        // Step 2: Emit plan event
        let total_steps = plan.steps.len();
        let _ = sender.send(ReActEvent::PlanGenerated {
            session_id: sid.clone(),
            steps: plan.steps.clone(),
        });

        // Step 3: Initialize state machine and execute steps
        let mut state_machine = PlanStateMachine::new(plan);

        while *state_machine.state() != PlanState::Completed {
            match state_machine.state().clone() {
                PlanState::Ready => {
                    if let Some(step) = state_machine.current_step() {
                        let idx = state_machine.current_step_index();
                        let _ = sender.send(ReActEvent::StepStart {
                            session_id: sid.clone(),
                            step_index: idx,
                            total_steps,
                            description: step.description.clone(),
                        });
                        state_machine.begin_step();

                        // Build step context for the LLM
                        let step_ctx = state_machine.build_step_context(user_message);

                        // Execute this step using the LLM
                        let step_messages = vec![ChatMessage {
                            role: "user".to_string(),
                            content: step_ctx.to_prompt(),
                        }];
                        let result = llm
                            .chat_completion(&step_messages, &config)
                            .await
                            .unwrap_or_else(|e| format!("步骤执行失败: {}", e));

                        let success = !planner::Planner::should_replan(
                            state_machine.plan(),
                            idx,
                            &result,
                            "",
                            None,
                        );

                        let _ = sender.send(ReActEvent::StepResult {
                            session_id: sid.clone(),
                            step_index: idx,
                            result: result.clone(),
                            success,
                        });

                        if success {
                            state_machine.record_result(result);
                            state_machine.advance();
                        } else {
                            // Check if we should replan
                            let remaining = state_machine.remaining_steps().to_vec();
                            let executed = state_machine.executed().to_vec();

                            match planner::Planner::replan(
                                user_message,
                                &executed,
                                &remaining,
                                llm,
                            )
                            .await
                            {
                                Ok(new_steps) => {
                                    let _ = sender.send(ReActEvent::Replan {
                                        session_id: sid.clone(),
                                        reason: "步骤失败，触发重新规划".into(),
                                    });
                                    state_machine.request_replan(new_steps);
                                }
                                Err(e) => {
                                    return Err(format!("重新规划失败: {}", e));
                                }
                            }
                        }
                    }
                }
                PlanState::Failed(msg) => {
                    return Err(msg);
                }
                PlanState::Completed => break,
                _ => {}
            }
        }

        let _ = sender.send(ReActEvent::Done { session_id: sid });
        Ok(())
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

/// 检测用户消息中是否包含图片引用
fn detect_images_in_message(message: &str) -> bool {
    let lower = message.to_lowercase();

    // 检测图片文件路径
    let image_extensions = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg"];
    for ext in &image_extensions {
        if lower.contains(ext) {
            return true;
        }
    }

    // 检测 base64 图片数据
    if lower.contains("data:image/") || lower.contains("base64,") {
        return true;
    }

    // 检测图片相关关键词
    let image_keywords = ["图片", "图像", "截图", "附件", "上传", "图片附件"];
    for keyword in &image_keywords {
        if message.contains(keyword) {
            return true;
        }
    }

    false
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
