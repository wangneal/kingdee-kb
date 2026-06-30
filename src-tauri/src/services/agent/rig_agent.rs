//! rig Agent 核心实现 — 替代手写的 ReAct 循环
//!
//! 使用 rig 的流式 API 和原生 function calling。
//! 中间事件（Thinking、ToolCall、ToolResult、TextDelta）
//! 通过 AgentEvent 实时推送到前端。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use futures::StreamExt;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::services::harness::agents_log::AgentsLog;
use crate::services::verification::pipeline::VerificationPipeline;
use crate::services::verification::types::{ScenarioType, VerificationInput};
use crate::services::harness::constraints::ToolConstraintChecker;
use crate::services::harness::verifier::{ResultVerifier, VerificationStatus};

use crate::services::agent_router;
use crate::services::agent_timeout::{
    AGENT_SESSION_TIMEOUT_SECS, LLM_CALL_TIMEOUT_SECS, MAX_RETRIES, PLANNER_TIMEOUT_SECS,
};
use crate::services::bm25_service::BM25Service;
use crate::services::embedding::EmbeddingService;
use crate::services::llm_providers::{LLMProtocol, LLMProviderConfig};
use crate::services::llm_service::{ChatMessage, LLMService};
use crate::services::metadata::MetadataStore;
use crate::services::planner::{self, PlanState, PlanStateMachine};
use crate::services::product_store::ProductStore;
use crate::services::project_store::ProjectStore;
use crate::services::question_tool::PendingQuestions;
use crate::services::agent_event::{is_llm_auth_error, AgentEvent};
use crate::services::rig_provider::{
    build_anthropic_client, build_ollama_client, build_openai_client,
};
use crate::services::rig_tool::{
    all_rig_tools, disabled_tool_policy_text, filter_disabled_rig_tools, load_rig_tool_config,
    runtime_rig_tools, tool_output_policy_text,
};
use crate::services::risk_control::RiskControlStore;
use crate::services::types::{AgentMode, AttachmentInfo};
use crate::services::vector_index::VectorIndex;
use crate::services::wiki_page::WikiPageStore;
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

struct PlanStepExecution {
    result: String,
    success: bool,
    replan_reason: Option<String>,
}

/// 核心系统提示词（对应 resources/prompts/system_prompt.md）
/// 包含：角色定义 + 反二开规则 + 回答质量要求 + 来源标注格式
const CORE_SYSTEM_PROMPT: &str = "\
你是一个金蝶ERP实施顾问知识助手。你的核心职责是提供严谨、可落地的实施建议。\n\n\
【反二开蔓延规则 — 严格遵守】\n\
1. 你的角色是严谨的质量审计员，不是推销员。\n\
2. 当用户提出需求时，**默认立场是拒绝不合理的二次开发**。\n\
3. 在推荐任何二开方案前，必须先检查标准功能(Best Practices)是否有替代方案。\n\
4. 如果标准功能确实无法满足，明确标记为 [Gap]，并说明是配置项差异还是需评估范围变更。\n\
5. 禁止编造不存在的系统功能、BAPI、配置路径或单据类型。\n\
6. 不得为了讨好用户而顺着不切实际的需求编造方案。\n\n\
【回答质量要求】\n\
1. 基于知识库中的本地文档回答，标注具体来源。\n\
2. 当知识库中无相关信息时，明确说明「知识库中暂无相关内容」。\n\
3. 回答结构：先说结论 → 再给依据 → 最后给出操作建议。\n\
4. 涉及配置时，写全路径（如：系统管理→基础资料→科目→新建）。\n\
5. 禁止使用「实现高效管理」「优化业务流程」等无具体操作的空话。\n\n\
【来源标注格式】\n\
每个事实性陈述后必须标注引用，格式为 [chunk:N]，其中 N 是知识库段落编号。\n\
例如：「金蝶云星空支持多组织架构[chunk:1]。」\n\
回答末尾标注：(来源：[chunk:N] - [文档名称].md)";

/// RigAgent — 使用 rig 实现替代 ReActAgent
///
/// 零大小类型；所有状态保存在 rig 的 agent builder 中。
pub struct RigAgent;

/// 检测用户消息是否包含交付物生成意图
///
/// 使用关键词匹配（零延迟），不需要 LLM 调用。
fn detect_deliverable_intent(message: &str) -> bool {
    let keywords = [
        "生成", "创建", "写", "制作", "输出", "导出",
        "文档", "报告", "方案", "蓝图", "PPT", "演示文稿",
        "清单", "表格", "模板", "周报", "月报", "任命书",
        "启动会", "验收", "上线方案", "调研纪要", "调研报告",
        "项目看板", "幻灯片", "交付物", "材料",
    ];
    let msg = message.to_lowercase();
    keywords.iter().filter(|k| msg.contains(&k.to_lowercase())).count() >= 2
}

impl RigAgent {
    /// 运行基于 rig 的 agent 流式循环。
    ///
    /// 使用 `stream_prompt()` 接收中间事件：
    /// - `Text` → `AgentEvent::TextDelta`
    /// - `ToolCall` → `AgentEvent::ToolCall`
    /// - `StreamedUserContent::ToolResult` → `AgentEvent::ToolResult`
    /// - `Reasoning` → `AgentEvent::Thinking`
    /// - `FinalResponse` → `AgentEvent::Done`
    pub async fn run(
        llm: &LLMService,
        user_message: &str,
        system_extra: &str,
        history: &[ChatMessage],
        sender: mpsc::UnboundedSender<AgentEvent>,
        session_id: &str,
        pending: PendingQuestions,
        project_id: Option<i64>,
        _risk_project_id: Option<i64>,
        embedding: Arc<RwLock<EmbeddingService>>,
        vector_index: Arc<RwLock<VectorIndex>>,
        bm25: Arc<RwLock<BM25Service>>,
        metadata: Arc<Mutex<MetadataStore>>,
        data_dir: std::path::PathBuf,
        products: Arc<Mutex<ProductStore>>,
        project_store: Arc<Mutex<ProjectStore>>,
        risk_store: Arc<tokio::sync::Mutex<RiskControlStore>>,
        skill_manager: Arc<tokio::sync::Mutex<crate::services::skill_manager::SkillManager>>,
        cancel_flag: Option<Arc<AtomicBool>>,
        provider_id: Option<&str>,
        model_id: Option<&str>,
        attachments: Option<Vec<AttachmentInfo>>,
        image_processor: Arc<RwLock<crate::services::image_processor::ImageProcessor>>,
        llm_providers: Arc<RwLock<crate::services::llm_providers::LLMProviderManager>>,
        wiki_pages: Option<Arc<Mutex<WikiPageStore>>>,
        meeting_store: Option<Arc<Mutex<crate::services::meeting_store::MeetingStore>>>,
        raw_sources: Option<Arc<Mutex<crate::services::raw_source::RawSourceStore>>>,
    ) {
        let sid = session_id.to_string();
        let started_at = Instant::now();
        let active_project_id = match project_id {
            Some(pid) => pid,
            None => match project_store.lock() {
                Ok(store) => match store.ensure_default_project() {
                    Ok(pid) => pid,
                    Err(e) => {
                        let _ = sender.send(AgentEvent::error(
                            sid.clone(),
                            format!("获取默认项目失败: {}", e),
                        ));
                        return;
                    }
                },
                Err(e) => {
                    let _ = sender.send(AgentEvent::error(
                        sid.clone(),
                        format!("获取项目锁失败: {}", e),
                    ));
                    return;
                }
            },
        };
        let attachment_project_id = active_project_id.to_string();

        // 查询项目信息，让 LLM 能感知当前项目上下文
        let active_project = project_store
            .lock()
            .ok()
            .and_then(|store| store.get_project(active_project_id).ok().flatten());
        let active_project_name = active_project
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| active_project_id.to_string());
        // 获取项目产品列表
        let active_project_products: Vec<crate::services::project_store::ProjectProduct> =
            project_store
                .lock()
                .ok()
                .and_then(|store| store.list_project_products(active_project_id).ok())
                .unwrap_or_default();
        let attachment_search_projects = attachments
            .as_ref()
            .filter(|list| !list.is_empty())
            .map(|_| vec![attachment_project_id.clone()])
            .unwrap_or_default();

        // ── 后端静默附件解析/入库处理 ──
        let mut attachment_prompts = Vec::new();

        if let Some(ref list) = attachments {
            // 前置处理：获取 OCR 和多模态模型候选
            let ocr_config = {
                if let Ok(proc) = image_processor.read() {
                    proc.get_ocr_config_cloned()
                } else {
                    None
                }
            };
            let candidates = {
                if let Ok(mgr) = llm_providers.read() {
                    mgr.get_vision_candidates()
                } else {
                    Vec::new()
                }
            };

            for attachment in list {
                match attachment.kind.as_str() {
                    "image" => {
                        let mut success_text = None;
                        let mut last_err = String::new();

                        // 逐个尝试候选模型
                        for (api_key, base_url, model_name, provider_id, _, protocol) in &candidates
                        {
                            if api_key.is_empty() && protocol != &LLMProtocol::Local {
                                continue;
                            }

                            let mut processor =
                                crate::services::image_processor::ImageProcessor::new(
                                    api_key.clone(),
                                    base_url.clone(),
                                    model_name.clone(),
                                );
                            processor.set_protocol(protocol.clone());
                            if let Some(ref ocr) = ocr_config {
                                processor.set_ocr_config(ocr.clone());
                            }

                            match tokio::time::timeout(
                                Duration::from_secs(8),
                                processor.process_image(&attachment.path),
                            )
                            .await
                            {
                                Ok(Ok(res)) => {
                                    if processor.is_llm_multimodal() {
                                        if let Ok(mut global) = image_processor.write() {
                                            global.set_llm_multimodal(true);
                                        }
                                    }
                                    success_text = Some(res.text);
                                    break;
                                }
                                Ok(Err(e)) => {
                                    last_err = format!("{} ({} > {})", e, provider_id, model_name);
                                }
                                Err(_) => {
                                    last_err = format!("超时 ({} > {})", provider_id, model_name);
                                }
                            }
                        }

                        // LLM 失败则 OCR 回退
                        if success_text.is_none() {
                            if let Some(ref ocr) = ocr_config {
                                let (api_key, base_url, model_name, protocol) =
                                    if let Some((k, u, m, _, _, p)) = candidates.first() {
                                        (k.clone(), u.clone(), m.clone(), p.clone())
                                    } else {
                                        (
                                            String::new(),
                                            String::new(),
                                            String::new(),
                                            LLMProtocol::OpenAI,
                                        )
                                    };

                                let mut processor =
                                    crate::services::image_processor::ImageProcessor::new(
                                        api_key, base_url, model_name,
                                    );
                                processor.set_protocol(protocol);
                                processor.set_ocr_config(ocr.clone());

                                match processor.ocr_only(&attachment.path).await {
                                    Ok(res) => {
                                        success_text = Some(res.text);
                                    }
                                    Err(e) => {
                                        last_err = format!("OCR 回退也失败: {}", e);
                                    }
                                }
                            }
                        }

                        match success_text {
                            Some(text) => {
                                attachment_prompts.push(format!(
                                    "--- 图片：{} ---\n内容/OCR提取：\n{}",
                                    attachment.name, text
                                ));
                            }
                            None => {
                                attachment_prompts.push(format!(
                                    "--- 图片：{} ---\n[图片解析失败，原因为：{}]",
                                    attachment.name, last_err
                                ));
                            }
                        }
                    }
                    "document" => {
                        // 文档入库后台化，不阻塞首 token
                        let ingest_path = std::path::PathBuf::from(&attachment.path);
                        let ingest_project_id = active_project_id;
                        let doc_name = attachment.name.clone();
                        let emb = embedding.clone();
                        let vidx = vector_index.clone();
                        let meta = metadata.clone();
                        let bm = bm25.clone();
                        tokio::task::spawn_blocking(move || {
                            let res = crate::services::ingestion::ingest_file(
                                &ingest_path,
                                ingest_project_id,
                                &emb,
                                &vidx,
                                &meta,
                                &bm,
                                None,
                                None,
                                None,
                            );
                            if let Err(e) = res {
                                tracing::warn!("后台文档入库失败 {}: {}", doc_name, e);
                            }
                        });
                        attachment_prompts.push(format!(
                            "--- 文档：{} ---\n[文档正在后台入库，稍后可通过 search-knowledge 工具检索]",
                            attachment.name
                        ));
                    }
                    _ => {}
                }
            }
        }

        let effective_user_message = if !attachment_prompts.is_empty() {
            format!(
                "{}\n\n【本轮附件】\n{}",
                user_message,
                attachment_prompts.join("\n\n")
            )
        } else {
            user_message.to_string()
        };
        let user_message = &effective_user_message;

        // 1. 自动路由：检测是否需要多模态模型
        let has_images = attachments
            .as_ref()
            .map(|list| list.iter().any(|a| a.kind == "image"))
            .unwrap_or(false);
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
                &active_project_name,
                None,
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
                model_id,
            )
            .await
            {
                Ok(()) => return, // Plan-Execute completed
                Err(e) => {
                    tracing::warn!("Plan-Execute 失败，降级到 ReAct: {}", e);
                    let _ = sender.send(AgentEvent::PlannerTimeout {
                        session_id: session_id.to_string(),
                        message: format!("规划失败，已降级到快速模式: {}", e),
                    });
                    // Fall through to ReAct below
                }
            }
        }

        // 2. 获取 LLM 配置（支持指定供应商或自动路由）
        let config = match llm.get_config_for_provider_model(effective_provider_id, model_id) {
            Ok(c) => c,
            Err(e) => {
                // 配置阶段还没有 provider_id（错误来自 active runtime provider），
                // 用占位 id 让前端知道是 LLM 相关错误；用户进设置页查看具体供应商
                let _ = sender.send(AgentEvent::llm_error(sid, e, "default"));
                return;
            }
        };

        // 2. 构建系统提示词
        let project_section = if let Some(ref project) = active_project {
            let mut section = format!(
                "\n【当前项目】{}\n客户：{}\n项目描述：{}\n当前阶段：{}",
                project.name,
                if project.client_name.is_empty() { "未填写" } else { &project.client_name },
                if project.description.is_empty() { "未填写" } else { &project.description },
                project.current_phase,
            );
            if !active_project_products.is_empty() {
                let products_info: Vec<String> = active_project_products
                    .iter()
                    .map(|p| {
                        if p.product_version.is_empty() {
                            p.product_name.clone()
                        } else {
                            format!("{} {}", p.product_name, p.product_version)
                        }
                    })
                    .collect();
                section.push_str(&format!("\n产品版本：{}", products_info.join("、")));
            }
            section.push_str("\n所有工具调用（搜索知识库、生成交付物等）都应限定在此项目范围内。\n");
            section
        } else {
            format!(
                "\n【当前项目】{}\n所有工具调用（搜索知识库、生成交付物等）都应限定在此项目范围内。\n",
                active_project_name
            )
        };

        // 活文档机制：注入历史失败教训（驾驭工程 Harness Engineering）
        // agents_log 在会话结束后保持可变，用于记录本次会话的失败模式
        let mut agents_log = AgentsLog::new(&data_dir);
        let learned_section = agents_log.get_learned_constraints().unwrap_or_default();
        let tool_config = match load_rig_tool_config(&data_dir) {
            Ok(config) => config,
            Err(e) => {
                let _ = sender.send(AgentEvent::error(sid, e));
                return;
            }
        };

        // ── 三层提示词架构（Anthropic 2025 最佳实践）──
        // 第1层：始终注入 — 核心身份 + 项目上下文 + 活文档
        let has_attachment = !attachment_prompts.is_empty();
        let has_skill = !system_extra.is_empty();
        let has_deliverable_intent = detect_deliverable_intent(user_message);

        let system_prompt = format!(
            "{CORE}\n\n\
{skill_section}\
{project_section}\
{learned_section}\
\
你是一个金蝶ERP实施顾问AI助手。你可以调用工具来获取信息或执行操作。\n\n\
【工作方式 — 必须遵守】\n\
1. 检查用户消息是否包含【本轮附件】：
   - 有附件 → 直接基于附件内容回答/生成
   - 无附件且属于需要查证的事实性问题 → **必须先调用 search-knowledge 工具搜索知识库**
2. **强制搜索规则**：当用户提到以下内容时，必须先搜索知识库再回答：
   - 项目名称、客户名称
   - 金蝶产品功能、模块、配置
   - 实施方法论、最佳实践
   - 任何需要查证的事实性问题
3. **跳过搜索规则**：
   - 仅当用户问日常问候寒暄（如\"哈喽\"\"你好\"\"Hi\"）或与金蝶业务无关的常规问题时，才可跳过搜索。
   - **禁止对日常问候与闲聊语句调用任何检索或文档生成工具**。
4. 搜索结果作为回答依据，必须引用来源。

\
{attachment_section}\
{deliverable_section}\
\
【规则】
- 一次只调用一个工具
- 观察工具结果后再决定下一步
- 如果仍缺少必要信息，可以继续逐项调用 question 工具提问
- 如果你已经有足够信息，直接回答，不要额外调用工具

\
【输出格式规则 — 必须遵守】\n- 回答必须使用 Markdown 格式，禁止使用 HTML 标签\n- 禁止使用 <br> <ul> <li> <b> <i> <strong> <em> <code> <pre> <table> <tr> <td> 等 HTML 标签\n- 使用 Markdown 语法：**粗体**、*斜体*、`代码`、- 列表、1. 编号列表、表格",
            CORE = CORE_SYSTEM_PROMPT,
            skill_section = if has_skill {
                system_extra
            } else {
                ""
            },
            project_section = project_section,
            learned_section = learned_section,
            attachment_section = if has_attachment {
                "【附件处理规则 — 本轮有效】\n当用户消息中包含【本轮附件】时：\n1. **直接使用附件内容**，不要要求用户提供已包含的信息\n2. 附件中的文档内容是用户提供的原始资料，优先基于这些内容回答\n3. 如果附件包含调研纪要、会议记录等，直接基于内容生成报告/分析\n4. 只有当附件内容确实不足时，才调用 search-knowledge 补充\n5. **禁止**要求用户提供附件中已有的信息\n"
            } else {
                ""
            },
            deliverable_section = if has_deliverable_intent {
                "【交付物生成规则 — 本轮有效】\n当用户要求生成文档、PPT、清单、报告或其他交付物时，必须使用技能系统：\n1. 先检查匹配技能：调研报告→survey-assistant、业务蓝图→blueprint-tools、启动会→kickoff-pack、上线→golive-pack、验收→acceptance-pack、周报→weekly-report、PPT→kingdee-ppt、通用文档→doc-tools\n2. 如匹配到技能，调用 use-skill 读取指引再推进\n3. 如技能需运行脚本，使用 run-skill-script 工具\n4. 如无匹配技能，调用 question 工具说明并询问用户\n"
            } else {
                ""
            },
        );
        let system_prompt = format!(
            "{}{}{}{}",
            system_prompt,
            crate::services::tool_policy::agent_tool_policy_prompt(),
            disabled_tool_policy_text(&tool_config),
            tool_output_policy_text(&tool_config)
        );

        let model = config.get_default_model_name();
        let temperature = config.temperature as f64;
        let max_tokens = agent_output_tokens(config.effective_max_output_tokens());
        let context_window = config.effective_context_window();
        let prompt = build_prompt_with_history(history, user_message, context_window);
        info!(
            session = %sid,
            provider = ?config.protocol,
            model = %model,
            configured_max_tokens = config.effective_max_output_tokens(),
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
                LLMProtocol::OpenAI => {
                    let client = match build_openai_client(&config) {
                        Ok(c) => c,
                        Err(e) => {
                            let _ = sender.send(AgentEvent::llm_error(sid, e, &config.id));
                            return;
                        }
                    };

                    // 使用传统的 Chat Completions API（/v1/chat/completions）
                    // 而不是新的 Responses API（/v1/responses）
                    let completions_client = client.completions_api();

                    let mut tools = all_rig_tools(
                        Some(active_project_id),
                        data_dir.clone(),
                        tool_config.output_limits,
                        llm.clone(),
                        embedding.clone(),
                        vector_index.clone(),
                        bm25.clone(),
                        metadata.clone(),
                        products.clone(),
                        project_store.clone(),
                        risk_store.clone(),
                        skill_manager.clone(),
                        None,
                        attachment_search_projects.clone(),
                        wiki_pages.clone(),
                        Some(sid.to_string()), // 传入会话 ID 用于 RAG 缓存
                        meeting_store.clone(),
                        raw_sources.clone(),
                    );

                    tools.extend(runtime_rig_tools(
                        pending.clone(),
                        sender.clone(),
                        sid.clone(),
                        skill_manager.clone(),
                        data_dir.clone(),
                        tool_config.output_limits,
                        products.clone(),
                        active_project_id,
                    ));
                    let tools = filter_disabled_rig_tools(tools, &tool_config);

                    let agent = completions_client
                        .agent(&model)
                        .preamble(&system_prompt)
                        .tools(tools)
                        .temperature(temperature)
                        .max_tokens(max_tokens)
                        .default_max_turns(PRACTICALLY_UNLIMITED_TURNS)
                        .build();

                    let stream_future =
                        std::future::IntoFuture::into_future(agent.stream_prompt(prompt.as_str()));
                    tokio::pin!(stream_future);
                    let mut stream = loop {
                        if is_cancelled(&cancel_flag) {
                            send_cancelled(&sender, &sid);
                            return;
                        }
                        tokio::select! {
                            stream = &mut stream_future => break stream,
                            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
                        }
                    };

                    Self::drain_stream(
                        &mut stream,
                        &sender,
                        &sid,
                        started_at,
                        &mut rate_limiter,
                        cancel_flag.clone(),
                        &mut agents_log,
                        &config.id,
                        llm.verifier.clone(),
                        user_message.to_string(),
                    )
                    .await;
                }
                LLMProtocol::Local => {
                    let client = match build_ollama_client(&config) {
                        Ok(c) => c,
                        Err(e) => {
                            let _ = sender.send(AgentEvent::llm_error(sid, e, &config.id));
                            return;
                        }
                    };

                    let mut tools = all_rig_tools(
                        Some(active_project_id),
                        data_dir.clone(),
                        tool_config.output_limits,
                        llm.clone(),
                        embedding.clone(),
                        vector_index.clone(),
                        bm25.clone(),
                        metadata.clone(),
                        products.clone(),
                        project_store.clone(),
                        risk_store.clone(),
                        skill_manager.clone(),
                        None,
                        attachment_search_projects.clone(),
                        wiki_pages.clone(),
                        Some(sid.to_string()), // 传入会话 ID 用于 RAG 缓存
                        meeting_store.clone(),
                        raw_sources.clone(),
                    );

                    tools.extend(runtime_rig_tools(
                        pending.clone(),
                        sender.clone(),
                        sid.clone(),
                        skill_manager.clone(),
                        data_dir.clone(),
                        tool_config.output_limits,
                        products.clone(),
                        active_project_id,
                    ));
                    let tools = filter_disabled_rig_tools(tools, &tool_config);

                    let agent = client
                        .agent(&model)
                        .preamble(&system_prompt)
                        .tools(tools)
                        .temperature(temperature)
                        .max_tokens(max_tokens)
                        .default_max_turns(PRACTICALLY_UNLIMITED_TURNS)
                        .build();

                    let stream_future =
                        std::future::IntoFuture::into_future(agent.stream_prompt(prompt.as_str()));
                    tokio::pin!(stream_future);
                    let mut stream = loop {
                        if is_cancelled(&cancel_flag) {
                            send_cancelled(&sender, &sid);
                            return;
                        }
                        tokio::select! {
                            stream = &mut stream_future => break stream,
                            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
                        }
                    };

                    Self::drain_stream(
                        &mut stream,
                        &sender,
                        &sid,
                        started_at,
                        &mut rate_limiter,
                        cancel_flag.clone(),
                        &mut agents_log,
                        &config.id,
                        llm.verifier.clone(),
                        user_message.to_string(),
                    )
                    .await;
                }
                LLMProtocol::Anthropic => {
                    let client = match build_anthropic_client(&config) {
                        Ok(c) => c,
                        Err(e) => {
                            let _ = sender.send(AgentEvent::llm_error(sid, e, &config.id));
                            return;
                        }
                    };

                    let mut tools = all_rig_tools(
                        Some(active_project_id),
                        data_dir.clone(),
                        tool_config.output_limits,
                        llm.clone(),
                        embedding.clone(),
                        vector_index.clone(),
                        bm25.clone(),
                        metadata.clone(),
                        products.clone(),
                        project_store.clone(),
                        risk_store.clone(),
                        skill_manager.clone(),
                        None,
                        attachment_search_projects.clone(),
                        wiki_pages.clone(),
                        Some(sid.to_string()), // 传入会话 ID 用于 RAG 缓存
                        meeting_store.clone(),
                        raw_sources.clone(),
                    );

                    tools.extend(runtime_rig_tools(
                        pending.clone(),
                        sender.clone(),
                        sid.clone(),
                        skill_manager.clone(),
                        data_dir.clone(),
                        tool_config.output_limits,
                        products.clone(),
                        active_project_id,
                    ));
                    let tools = filter_disabled_rig_tools(tools, &tool_config);

                    let agent = client
                        .agent(&model)
                        .preamble(&system_prompt)
                        .tools(tools)
                        .temperature(temperature)
                        .max_tokens(max_tokens)
                        .default_max_turns(PRACTICALLY_UNLIMITED_TURNS)
                        .build();

                    let stream_future =
                        std::future::IntoFuture::into_future(agent.stream_prompt(prompt.as_str()));
                    tokio::pin!(stream_future);
                    let mut stream = loop {
                        if is_cancelled(&cancel_flag) {
                            send_cancelled(&sender, &sid);
                            return;
                        }
                        tokio::select! {
                            stream = &mut stream_future => break stream,
                            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
                        }
                    };

                    Self::drain_stream(
                        &mut stream,
                        &sender,
                        &sid,
                        started_at,
                        &mut rate_limiter,
                        cancel_flag,
                        &mut agents_log,
                        &config.id,
                        llm.verifier.clone(),
                        user_message.to_string(),
                    )
                    .await;
                }
            }
        })
        .await;

        if result.is_err() {
            let _ = timeout_sender.send(AgentEvent::error(
                timeout_sid.clone(),
                "会话超时（超过10分钟），请重新开始对话",
            ));
            let _ = timeout_sender.send(AgentEvent::Done {
                session_id: timeout_sid,
                verification_report: None,
            });
        }
    }

    /// 消费 rig 流式响应，将每个 item 映射为 AgentEvent。
    /// 同时跟踪最近的工具调用以检测死循环。
    async fn drain_stream<R>(
        stream: &mut rig_core::agent::StreamingResult<R>,
        sender: &mpsc::UnboundedSender<AgentEvent>,
        sid: &str,
        started_at: Instant,
        rate_limiter: &mut ToolRateLimiter,
        cancel_flag: Option<Arc<AtomicBool>>,
        agents_log: &mut AgentsLog,
        provider_id: &str,
        verifier: Option<Arc<VerificationPipeline>>,
        user_query: String,
    ) {
        use rig_core::agent::MultiTurnStreamItem;
        use rig_core::streaming::{StreamedAssistantContent, StreamedUserContent};

        // 跟踪最近的工具调用 (name, args) 以检测死循环
        let mut recent_calls: VecDeque<(String, String)> =
            VecDeque::with_capacity(DOOM_LOOP_THRESHOLD);
        let mut announced_tool_args_generation = false;
        let mut constraint_checker = ToolConstraintChecker::new();
        let mut result_verifier = ResultVerifier::new(3); // max 3 consecutive failures
        let current_step_id = String::from("react");

        // 验证管线集成：收集完整响应文本和搜索工具结果
        let mut full_response = String::new();
        let mut last_tool_name: Option<String> = None;
        let mut search_chunks: Vec<String> = Vec::new();
        let mut search_titles: Vec<String> = Vec::new();

        loop {
            if is_cancelled(&cancel_flag) {
                let _ = sender.send(AgentEvent::error(
                    sid.to_string(),
                    "用户已取消操作",
                ));
                let _ = sender.send(AgentEvent::Done {
                    session_id: sid.to_string(),
                    verification_report: None,
                });
                return;
            }

            let item = tokio::select! {
                item = stream.next() => item,
                _ = tokio::time::sleep(Duration::from_millis(200)) => {
                    continue;
                }
            };

            let Some(item) = item else {
                break;
            };

            match item {
                Ok(MultiTurnStreamItem::StreamAssistantItem(content)) => {
                    match content {
                        StreamedAssistantContent::Text(text) => {
                            full_response.push_str(&text.text);
                            let _ = sender.send(AgentEvent::TextDelta {
                                session_id: sid.to_string(),
                                content: text.text,
                            });
                        }
                        StreamedAssistantContent::ToolCall { tool_call, .. } => {
                            let name = tool_call.function.name.clone();
                            let args = tool_call.function.arguments.to_string();
                            last_tool_name = Some(name.clone());
                            info!(
                                session = %sid,
                                elapsed_ms = started_at.elapsed().as_millis(),
                                name = %name,
                                args_chars = args.chars().count(),
                                "tool call"
                            );

                            // 速率限制检查
                            if !rate_limiter.check_and_record() {
                                let _ = sender.send(AgentEvent::error(
                                    sid.to_string(),
                                    "工具调用过于频繁（每分钟上限30次），请稍后重试",
                                ));
                                return;
                            }

                            let _ = sender.send(AgentEvent::ToolCall {
                                session_id: sid.to_string(),
                                name: name.clone(),
                                args: args.clone(),
                            });

                            // 死循环检测
                            recent_calls.push_back((name.clone(), args.clone()));
                            if recent_calls.len() > DOOM_LOOP_THRESHOLD {
                                recent_calls.pop_front();
                            }
                            if recent_calls.len() == DOOM_LOOP_THRESHOLD
                                && recent_calls
                                    .front()
                                    .map_or(false, |first| recent_calls.iter().all(|c| c == first))
                            {
                                let _ = sender.send(AgentEvent::error(
                                    sid.to_string(),
                                    format!(
                                        "检测到死循环：连续 {} 次相同的工具调用，已中断。",
                                        DOOM_LOOP_THRESHOLD
                                    ),
                                ));
                                return;
                            }

                            // Harness constraint check
                            if let Some(violation) =
                                constraint_checker.check_call(&current_step_id, &name, &args)
                            {
                                let _ = sender.send(AgentEvent::error(
                                    sid.to_string(),
                                    format!("工具约束违规: {}", violation),
                                ));
                                return;
                            }
                        }
                        StreamedAssistantContent::Reasoning(reasoning) => {
                            let text = reasoning.display_text();
                            if !text.is_empty() {
                                let _ = sender.send(AgentEvent::Thinking {
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
                                let _ = sender.send(AgentEvent::Thinking {
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

                        let result_text_for_verify = result_text.clone();
                        let _ = sender.send(AgentEvent::ToolResult {
                            session_id: sid.to_string(),
                            name: String::new(),
                            result: result_text,
                        });

                        // 收集搜索工具结果用于最终验证
                        if let Some(ref tool_name) = last_tool_name {
                            if tool_name.contains("search") {
                                search_chunks.push(result_text_for_verify.clone());
                                // 尝试解析搜索结果中的标题
                                for line in result_text_for_verify.lines() {
                                    if let Some(title) = line.strip_prefix("标题: ") {
                                        search_titles.push(title.trim().to_string());
                                    }
                                }
                            }
                        }

                        // Verify tool result
                        let step_label = format!("react-turn-{}", started_at.elapsed().as_millis());
                        let verify_status = result_verifier.verify(
                            &step_label,
                            &result_text_for_verify,
                            "", // no expected output in ReAct mode
                        );
                        match verify_status {
                            VerificationStatus::NeedsReplan(reason) => {
                                warn!(session = %sid, reason = %reason, "result verifier triggered replan");
                                agents_log.record_failure("replan", &reason, &sid);
                            }
                            VerificationStatus::Fail(reason) => {
                                debug!(session = %sid, reason = %reason, "tool result verification failed");
                            }
                            VerificationStatus::Exhausted(reason) => {
                                warn!(session = %sid, reason = %reason, "result verifier exhausted");
                                agents_log.record_failure("exhausted", &reason, &sid);
                                let _ = sender.send(AgentEvent::error(
                                    sid.to_string(),
                                    format!("连续验证失败: {}", reason),
                                ));
                                return;
                            }
                            VerificationStatus::Pass => {}
                        }
                    }
                },
                Ok(MultiTurnStreamItem::FinalResponse(_)) => {
                    info!(
                        session = %sid,
                        elapsed_ms = started_at.elapsed().as_millis(),
                        response_chars = full_response.chars().count(),
                        search_chunks = search_chunks.len(),
                        "agent session completed"
                    );

                    // 验证管线：对最终响应运行引用检查和事实一致性检查
                    let verification_report = if let Some(ref pipeline) = verifier {
                        let input = VerificationInput {
                            generated_text: full_response.clone(),
                            retrieved_chunks: search_chunks.clone(),
                            chunk_titles: search_titles.clone(),
                            available_chunk_ids: Vec::new(),
                            query: user_query.clone(),
                            scenario: ScenarioType::Chat,
                        };
                        let report = pipeline.verify(&input).await;
                        if report.level != crate::services::verification::types::VerificationLevel::Confirmed {
                            warn!(
                                session = %sid,
                                level = ?report.level,
                                confidence = report.overall_confidence,
                                labels = ?report.suggested_labels,
                                "verification flagged issues"
                            );
                            for label in &report.suggested_labels {
                                agents_log.record_failure("verification", label, sid);
                            }
                        }
                        Some(report)
                    } else {
                        None
                    };

                    let _ = sender.send(AgentEvent::Done {
                        session_id: sid.to_string(),
                        verification_report,
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
                    let is_auth = is_llm_auth_error(&raw);
                    let message = if raw.contains("MaxTurnError") {
                        "工具调用轮次已达到上限。当前任务可能需要多步澄清、依赖安装或脚本授权；请补充关键材料后重试，或让助手继续上一轮未完成的生成。".to_string()
                    } else if looks_like_output_limit_error(&raw) {
                        "模型在生成工具参数时达到输出上限或被截断。当前任务可能正在生成多页 HTML/PPT 输入，请继续本轮任务；系统已提高工具参数生成的输出预算。".to_string()
                    } else {
                        format!("流式错误: {}", raw)
                    };
                    // P0-5：流式 401 时附带 error_code，前端弹"配置 API Key"对话框
                    let _ = sender.send(if is_auth {
                        AgentEvent::llm_error(sid.to_string(), message, provider_id)
                    } else {
                        AgentEvent::error(sid.to_string(), message)
                    });
                    return;
                }
            }
        }
    }

    async fn execute_plan_step_with_feedback(
        llm: &LLMService,
        config: &LLMProviderConfig,
        plan: &planner::ExecutionPlan,
        step_index: usize,
        step_context: &planner::StepContext,
        sender: &mpsc::UnboundedSender<AgentEvent>,
        session_id: &str,
        cancel_flag: &Option<Arc<AtomicBool>>,
    ) -> Result<PlanStepExecution, String> {
        let step = step_context
            .current_step
            .as_ref()
            .ok_or_else(|| "缺少当前计划步骤".to_string())?;
        let max_attempts = MAX_RETRIES.max(1);
        let mut verifier = ResultVerifier::new(max_attempts as usize);
        let mut last_result = String::new();
        let mut last_reason: Option<String> = None;

        for attempt in 1..=max_attempts {
            if is_cancelled(cancel_flag) {
                return Err("用户已取消操作".to_string());
            }

            if attempt > 1 {
                let _ = sender.send(AgentEvent::Thinking {
                    session_id: session_id.to_string(),
                    content: format!(
                        "步骤结果未通过验证，正在第 {}/{} 次修正: {}",
                        attempt, max_attempts, step.description
                    ),
                });
            }

            let prompt =
                build_plan_step_prompt(step_context, attempt, last_reason.as_deref(), &last_result);
            let step_messages = vec![ChatMessage {
                role: "user".to_string(),
                content: prompt,
            }];

            let result = match tokio::time::timeout(
                Duration::from_secs(LLM_CALL_TIMEOUT_SECS),
                llm.chat_completion(&step_messages, config),
            )
            .await
            {
                Ok(Ok(result)) => result,
                Ok(Err(e)) => format!("步骤执行失败: {}", e),
                Err(_) => format!("步骤执行超时: {} 秒", LLM_CALL_TIMEOUT_SECS),
            };

            if is_cancelled(cancel_flag) {
                return Err("用户已取消操作".to_string());
            }

            let verify_status = verifier.verify(&step.description, &result, &step.expected_output);
            let planner_needs_replan = planner::Planner::should_replan(
                plan,
                step_index,
                &result,
                &step.expected_output,
                None,
            );

            match verify_status {
                VerificationStatus::Pass if !planner_needs_replan => {
                    verifier.reset_consecutive();
                    return Ok(PlanStepExecution {
                        result,
                        success: true,
                        replan_reason: None,
                    });
                }
                VerificationStatus::Pass => {
                    last_reason = Some("步骤结果包含失败或阻塞信号".to_string());
                    last_result = result;
                }
                VerificationStatus::Fail(reason) => {
                    last_reason = Some(reason);
                    last_result = result;
                }
                VerificationStatus::NeedsReplan(reason) | VerificationStatus::Exhausted(reason) => {
                    return Ok(PlanStepExecution {
                        result,
                        success: false,
                        replan_reason: Some(reason),
                    });
                }
            }
        }

        Ok(PlanStepExecution {
            result: last_result,
            success: false,
            replan_reason: last_reason.or_else(|| Some("步骤执行未通过验证".to_string())),
        })
    }

    /// 尝试 Plan-Execute 模式执行
    /// 10 秒规划超时 → 自动降级到 ReAct
    async fn try_plan_execute(
        llm: &LLMService,
        user_message: &str,
        system_extra: &str,
        _history: &[ChatMessage],
        sender: &mpsc::UnboundedSender<AgentEvent>,
        session_id: &str,
        _pending: PendingQuestions,
        project_name: &str,
        _risk_project_id: Option<i64>,
        _embedding: Arc<RwLock<EmbeddingService>>,
        _vector_index: Arc<RwLock<VectorIndex>>,
        _bm25: Arc<RwLock<BM25Service>>,
        _metadata: Arc<Mutex<MetadataStore>>,
        _data_dir: std::path::PathBuf,
        _products: Arc<Mutex<ProductStore>>,
        _risk_store: Arc<tokio::sync::Mutex<RiskControlStore>>,
        _skill_manager: Arc<tokio::sync::Mutex<crate::services::skill_manager::SkillManager>>,
        cancel_flag: Option<Arc<AtomicBool>>,
        provider_id: Option<&str>,
        model_id: Option<&str>,
    ) -> Result<(), String> {
        let sid = session_id.to_string();

        if is_cancelled(&cancel_flag) {
            send_cancelled(sender, &sid);
            return Ok(());
        }

        // Step 1: Generate execution plan with 10s timeout
        let config = llm
            .get_config_for_provider_model(provider_id, model_id)
            .map_err(|e| format!("获取配置失败: {}", e))?;

        let plan_budget = config.effective_context_window() / 4; // Plan 获得 25% 的上下文窗口预算
        let plan = tokio::time::timeout(
            std::time::Duration::from_secs(PLANNER_TIMEOUT_SECS),
            planner::Planner::plan(
                user_message,
                system_extra,
                project_name,
                llm,
                &config,
                plan_budget,
            ),
        )
        .await
        .map_err(|_| format!("Planner 超时 ({}s)", PLANNER_TIMEOUT_SECS))?
        .map_err(|e| format!("Planner 失败: {}", e))?;

        // Step 2: Emit plan event
        let total_steps = plan.steps.len();
        let _ = sender.send(AgentEvent::PlanGenerated {
            session_id: sid.clone(),
            steps: plan.steps.clone(),
        });

        // Step 3: Initialize state machine and execute steps
        let mut state_machine = PlanStateMachine::new(plan);

        while *state_machine.state() != PlanState::Completed {
            if is_cancelled(&cancel_flag) {
                send_cancelled(sender, &sid);
                return Ok(());
            }

            match state_machine.state().clone() {
                PlanState::Ready => {
                    if let Some(step) = state_machine.current_step().cloned() {
                        let idx = state_machine.current_step_index();
                        let _ = sender.send(AgentEvent::StepStart {
                            session_id: sid.clone(),
                            step_index: idx,
                            total_steps,
                            description: step.description.clone(),
                        });
                        state_machine.begin_step();

                        // 为当前步骤构建最小上下文
                        let step_ctx = state_machine.build_step_context(user_message);

                        let execution = Self::execute_plan_step_with_feedback(
                            llm,
                            &config,
                            state_machine.plan(),
                            idx,
                            &step_ctx,
                            sender,
                            &sid,
                            &cancel_flag,
                        )
                        .await?;

                        let _ = sender.send(AgentEvent::StepResult {
                            session_id: sid.clone(),
                            step_index: idx,
                            result: execution.result.clone(),
                            success: execution.success,
                        });

                        if execution.success {
                            state_machine.record_result(execution.result);
                            state_machine.advance();
                        } else {
                            // 连续修正仍失败后才触发重规划
                            let remaining = state_machine.remaining_steps().to_vec();
                            let executed = state_machine.executed().to_vec();
                            let reason = execution
                                .replan_reason
                                .unwrap_or_else(|| "步骤执行未通过验证".to_string());

                            match planner::Planner::replan(
                                user_message,
                                &executed,
                                &remaining,
                                llm,
                                &config,
                            )
                            .await
                            {
                                Ok(new_steps) => {
                                    if is_cancelled(&cancel_flag) {
                                        send_cancelled(sender, &sid);
                                        return Ok(());
                                    }
                                    let _ = sender.send(AgentEvent::Replan {
                                        session_id: sid.clone(),
                                        reason,
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

        let _ = sender.send(AgentEvent::Done {
            session_id: sid,
            verification_report: None,
        });
        Ok(())
    }
}

fn is_cancelled(cancel_flag: &Option<Arc<AtomicBool>>) -> bool {
    cancel_flag
        .as_ref()
        .map_or(false, |flag| flag.load(Ordering::SeqCst))
}

fn build_plan_step_prompt(
    step_context: &planner::StepContext,
    attempt: u32,
    last_reason: Option<&str>,
    last_result: &str,
) -> String {
    let mut prompt = step_context.to_prompt();

    if let Some(step) = &step_context.current_step {
        let tool = step.tool.as_deref().unwrap_or("无指定工具");
        prompt.push_str(&format!(
            "\n\n## 当前步骤验收要求\n\n\
             **建议工具**: {tool}\n\n\
             **预期输出**: {expected}\n\n\
             请按预期输出交付可直接使用的结果。若确实无法完成，必须明确说明阻塞原因和缺失条件。",
            expected = step.expected_output
        ));
    }

    if attempt > 1 {
        let reason = last_reason.unwrap_or("上一次结果未通过验证");
        prompt.push_str(&format!(
            "\n\n## 修正要求\n\n\
             上一次执行未通过验证：{reason}\n\n\
             上一次结果摘要：\n{preview}\n\n\
             请只修正当前步骤，不要扩展到后续步骤。",
            preview = preview_text(last_result, 800)
        ));
    }

    prompt
}

fn preview_text(text: &str, max_chars: usize) -> String {
    let mut preview = String::new();
    for ch in text.chars().take(max_chars) {
        preview.push(ch);
    }
    if text.chars().count() > max_chars {
        preview.push_str("...");
    }
    preview
}

fn send_cancelled(sender: &mpsc::UnboundedSender<AgentEvent>, session_id: &str) {
    let _ = sender.send(AgentEvent::error(
        session_id.to_string(),
        "用户已取消操作",
    ));
    let _ = sender.send(AgentEvent::Done {
        session_id: session_id.to_string(),
        verification_report: None,
    });
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

fn build_prompt_with_history(history: &[ChatMessage], user_message: &str, context_window: u32) -> String {
    // 按用户配置的 context_window 动态计算历史预算：
    //   - 50% 留给对话历史
    //   - 另 50% 涵盖 system prompt + RAG 检索上下文 + max_output_tokens（LLM 输出）
    //     （max_output 已含在这 50% 内，不再单独扣减；偏保守，不会溢出）
    //   - 上下文窗口按 ~3.5 字符/token 折算（中英混合保守估计）
    //   - 下限 8_000 字符、上限 96_000 字符（防止极端配置）
    let max_history_chars = ((context_window as f64 * 0.5 * 3.5) as usize)
        .clamp(8_000, 96_000);

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
        if chars + line_chars > max_history_chars {
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

