import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"

// ── 与 Rust 结构体对应的类型 ───────────────────────────────────────────────

export interface AttachmentInfo {
  name: string
  path: string
  kind: string // "image" 或 "document"
}

export interface HybridSearchResult {
  chunk_id: number
  title: string
  content: string
  score: number
  source: string
  document_id: number
  section_path?: string
  project: string
}

export interface BM25SearchResult {
  chunk_id: number
  score: number
  content: string
}

export interface IngestionResult {
  document_id: number
  title: string
  sha256: string
  is_duplicate: boolean
  chunk_count: number
  vector_count: number
  kb_compilation_error?: string | null
  kb_analysis_engine?: string | null
}

export interface ExtractedFileText {
  file_path: string
  title: string
  text: string
  char_count: number
}

export interface FileError {
  path: string
  error: string
}

export interface DirectoryIngestionResult {
  imported: IngestionResult[]
  errors: FileError[]
}

export interface IngestionProgress {
  step: number
  step_name: string
  progress: number
  message?: string
}

export interface DocumentMeta {
  id: number
  title: string
  source_path?: string
  sha256?: string
  created_at: string
  project_id: number
  document_scope: string
  chat_session_id?: string | null
}

export interface ChunkMeta {
  id: number
  vector_key: number
  document_id: number
  content: string
  section_path?: string
  tags?: string
  line_no?: number
  created_at: string
}

export interface KnowledgeStats {
  document_count: number
  chunk_count: number
  db_path: string
}

// ── LLM / RAG 类型 ────────────────────────────────────────────────────────

export interface ChatMessage {
  role: string
  content: string
}

export interface RAGSource {
  title: string
  section_path?: string
  content_snippet: string
  score: number
}

// ── Tauri 命令封装 ─────────────────────────────────────────────────────────
// 注意：Tauri v2 #[tauri::command] 默认按 rename_all="camelCase" 处理。
// JS invoke() 必须使用 camelCase 参数名（例如 filePath，而不是 file_path）。
// Rust 函数参数保持 snake_case，由宏负责映射。

/** 检查是否存在已配置有效 API Key 的 LLM 供应商 */
export async function isLLMConfigured(): Promise<boolean> {
  return invoke("is_llm_configured")
}

export async function hybridSearch(
  query: string,
  projectId?: number | null,
  topK?: number,
): Promise<HybridSearchResult[]> {
  return invoke("hybrid_search", {
    query,
    projectId: projectId ?? null,
    topK: topK ?? 5,
  })
}

export async function bm25Search(
  query: string,
  projectId?: number | null,
  topK?: number,
): Promise<BM25SearchResult[]> {
  return invoke("bm25_search", {
    query,
    projectId: projectId ?? null,
    topK: topK ?? 10,
  })
}

export async function ingestText(
  text: string,
  title: string,
  projectId: number,
  enableKbCompilation?: boolean,
): Promise<IngestionResult> {
  return invoke("ingest_text", {
    text,
    title,
    projectId,
    enableKbCompilation: enableKbCompilation ?? null,
  })
}

export async function ingestFile(
  filePath: string,
  projectId: number,
  enableKbCompilation?: boolean,
): Promise<IngestionResult> {
  return invoke("ingest_file", {
    filePath,
    projectId,
    enableKbCompilation: enableKbCompilation ?? null,
  })
}

export async function extractFileText(filePath: string): Promise<ExtractedFileText> {
  return invoke("extract_file_text", { filePath })
}

export async function ingestDirectory(
  dirPath: string,
  projectId: number,
  enableKbCompilation?: boolean,
): Promise<DirectoryIngestionResult> {
  return invoke("ingest_directory", {
    dirPath,
    projectId,
    enableKbCompilation: enableKbCompilation ?? null,
  })
}

export async function getKbCompilationEnabled(): Promise<boolean> {
  return invoke("get_kb_compilation_enabled")
}

export async function setKbCompilationEnabled(enabled: boolean): Promise<void> {
  return invoke("set_kb_compilation_enabled", { enabled })
}

export interface RecompileFailedSourceError {
  source_id: number
  title: string
  error: string
}

export interface RecompileFailedSourcesResult {
  retried: number
  succeeded: number
  failed: RecompileFailedSourceError[]
}

export interface KbRecompileStatus {
  status: "idle" | "running" | "completed" | "failed"
  project_id: number | null
  force: boolean
  retried: number
  succeeded: number
  failed: RecompileFailedSourceError[]
  completed_source_keys: string[]
  message?: string | null
  started_at?: string | null
  finished_at?: string | null
}

export async function recompileFailedKbSources(
  projectId: number,
  force: boolean = false,
): Promise<RecompileFailedSourcesResult> {
  return invoke("recompile_failed_kb_sources", { projectId, force })
}

export async function startKbRecompile(
  projectId: number,
  force: boolean = false,
): Promise<KbRecompileStatus> {
  return invoke("start_kb_recompile", { projectId, force })
}

export async function getKbRecompileStatus(): Promise<KbRecompileStatus> {
  return invoke("get_kb_recompile_status")
}

/// 强制重编译指定的源（用于"删 wiki 后原地重生成"场景）
/// 流程：按 source_id 取出 raw_source → 重读文件 → 清 ingest/analysis cache →
/// 调 process_with_kb_compilation(force_recompile=true) 跳过 Step 0 cache 命中
export interface ForceRecompileResult {
  analysis?: unknown
  engine: string
  cache_hit: boolean
  generated_pages: string[]
  compilation_done: boolean
}

export async function forceRecompileKbSource(
  projectId: number,
  sourceId: number,
): Promise<ForceRecompileResult> {
  return invoke<ForceRecompileResult>("force_recompile_kb_source", { projectId, sourceId })
}

export async function listDocuments(projectId?: number | null): Promise<DocumentMeta[]> {
  return invoke("list_documents", { projectId: projectId ?? null })
}

export async function getDocumentChunks(documentId: number): Promise<ChunkMeta[]> {
  return invoke("get_document_chunks", { documentId })
}

export async function getStats(projectId?: number | null): Promise<KnowledgeStats> {
  return invoke("get_stats", { projectId: projectId ?? null })
}

export async function deleteDocument(documentId: number, projectId?: number | null): Promise<void> {
  return invoke("delete_document", { documentId, projectId: projectId ?? null })
}

/** 在单个事务中批量删除多个文档及其片段 */
export async function deleteDocumentsBatch(
  documentIds: number[],
  projectId?: number | null,
): Promise<number> {
  return invoke("delete_documents_batch", { documentIds, projectId: projectId ?? null })
}

// ── Embedding 模型命令 ─────────────────────────────────────────────────────

export async function initModel(): Promise<boolean> {
  return invoke("init_model")
}

export async function getModelStatus(): Promise<boolean> {
  return invoke("get_model_status")
}

/** 获取 Embedding 模型下载进度（0-100） */
export async function getDownloadProgress(): Promise<number> {
  return invoke("get_download_progress")
}

// ── LLM / RAG 命令封装 ────────────────────────────────────────────────────

export type EmbeddingProviderType =
  | "local"
  | "openai"
  | "siliconflow"
  | "zhipu"
  | "dashscope"
  | "cohere"
  | "custom"

export interface EmbeddingModelConfig {
  custom_model_dir?: string | null
}

/** 在线 Embedding 供应商配置（存储在前端 localStorage） */
export interface EmbeddingProviderConfig {
  provider: EmbeddingProviderType
  api_key: string
  base_url: string
  model_name: string
}

export async function getEmbeddingModelConfig(): Promise<EmbeddingModelConfig> {
  return invoke("get_embedding_model_config")
}

export async function setEmbeddingModelConfig(customModelDir?: string | null): Promise<boolean> {
  return invoke("set_embedding_model_config", {
    customModelDir: customModelDir ?? null,
  })
}

export async function countTokens(text: string): Promise<number> {
  return invoke("count_tokens", { text })
}

// ── 聊天记忆 ───────────────────────────────────────────────────────────────

/** 保存聊天记忆：归档、提取并写入知识库。 */
export async function saveChatMemory(
  conversation: ChatMessage[],
  projectId?: number | null,
): Promise<void> {
  return invoke("save_chat_memory", { conversation, projectId: projectId ?? null })
}

export interface ProductMeta {
  id: number
  template_id: string
  template_name: string
  project_id: number
  status: string
  output_path: string
  field_count: number
  llm_fields_count: number
  created_at: string
}

export async function listProducts(projectId?: number | null): Promise<ProductMeta[]> {
  return invoke("list_products", { projectId: projectId ?? null })
}

export async function exportProduct(
  id: number,
  targetDir: string,
  projectId?: number | null,
): Promise<string> {
  return invoke("export_product", { id, targetDir, projectId: projectId ?? null })
}

export async function deleteProduct(id: number, projectId?: number | null): Promise<void> {
  return invoke("delete_product", { id, projectId: projectId ?? null })
}

// ── 阶段 13：调研会话管理 ────────────────────────────────────────────────

export interface ResearchSession {
  id: number
  title: string
  edition: string
  module_code: string
  interviewee: string
  session_date: string
  status: string
  project_id: number
  created_at: string
  updated_at: string
}

export interface QARecord {
  id: number
  session_id: number
  question_id: number | null
  question_text: string
  answer_text: string
  notes: string
  sort_order: number
  created_at: string
}

export interface SessionDetail {
  session: ResearchSession
  records: QARecord[]
}

export async function createResearchSession(
  title: string,
  edition: string,
  moduleCode: string,
  interviewee: string,
  sessionDate: string,
  projectId?: number | null,
): Promise<number> {
  return invoke("create_research_session", {
    title,
    edition,
    moduleCode,
    interviewee,
    sessionDate,
    projectId: projectId ?? null,
  })
}

export async function listResearchSessions(projectId?: number | null): Promise<ResearchSession[]> {
  return invoke("list_research_sessions", { projectId: projectId ?? null })
}

export async function getResearchSession(sessionId: number): Promise<SessionDetail | null> {
  return invoke("get_research_session", { sessionId })
}

export async function updateResearchSession(
  sessionId: number,
  title: string,
  interviewee: string,
  sessionDate: string,
  status: string,
): Promise<void> {
  return invoke("update_research_session", { sessionId, title, interviewee, sessionDate, status })
}

export async function deleteResearchSession(sessionId: number): Promise<void> {
  return invoke("delete_research_session", { sessionId })
}

export async function addQARecord(
  sessionId: number,
  questionId: number | null,
  questionText: string,
  answerText: string,
  notes: string,
  sortOrder: number,
): Promise<number> {
  return invoke("add_qa_record", {
    sessionId,
    questionId: questionId ?? null,
    questionText,
    answerText,
    notes,
    sortOrder,
  })
}

export async function updateQARecord(
  recordId: number,
  answerText: string,
  notes: string,
): Promise<void> {
  return invoke("update_qa_record", { recordId, answerText, notes })
}

export async function deleteQARecord(recordId: number): Promise<void> {
  return invoke("delete_qa_record", { recordId })
}

export async function getSessionRecords(sessionId: number): Promise<QARecord[]> {
  return invoke("get_session_records", { sessionId })
}

export async function exportSessionCsv(sessionId: number): Promise<string> {
  return invoke("export_session_csv", { sessionId })
}

export async function exportSessionMarkdown(sessionId: number): Promise<string> {
  return invoke("export_session_markdown", { sessionId })
}

export async function reorderQARecords(sessionId: number, recordIds: number[]): Promise<void> {
  return invoke("reorder_qa_records", { sessionId, recordIds })
}

// ── 阶段 12：Whisper 语音识别 ───────────────────────────────────────────

export interface TranscriptionResult {
  text: string
  segments: TranscriptionSegment[]
  confidence: number
  processing_time_ms: number
}

export interface TranscriptionSegment {
  start_ms: number
  end_ms: number
  text: string
}

export interface WhisperStatus {
  model_loaded: boolean
  model_size: string
  language: string
}

export interface AudioInputDeviceInfo {
  id: string
  name: string
  host: string
  is_default: boolean
}

export interface RecordingPreviewResult {
  text: string
  sample_count: number
  processing_time_ms: number
}

export async function loadWhisperModel(modelSize: string): Promise<void> {
  return invoke("load_whisper_model", { modelSize })
}

export async function getWhisperStatus(): Promise<WhisperStatus> {
  return invoke("get_whisper_status")
}

export async function listAudioInputDevices(): Promise<AudioInputDeviceInfo[]> {
  return invoke("list_audio_input_devices")
}

export async function startWhisperRecording(deviceName?: string): Promise<void> {
  return invoke("start_whisper_recording", { deviceName: deviceName ?? null })
}

export async function transcribeWhisperRecordingChunk(
  fromSample: number,
): Promise<RecordingPreviewResult> {
  return invoke("transcribe_whisper_recording_chunk", { fromSample })
}

export async function reviewTranscriptionText(text: string): Promise<string> {
  return invoke("review_transcription_text", { text })
}

export async function stopWhisperRecording(provider?: string): Promise<TranscriptionResult> {
  return invoke("stop_whisper_recording", { provider: provider ?? null })
}

// ── P1: 双轨风险把控舱 ──────────────────────────────────────────────────

export interface ContractScopeItem {
  id: number
  category: string
  description: string
  is_in_scope: boolean
  detail: string
  created_at: string
}

export interface ScopeCreepResult {
  risk_level: string
  risk_label: string
  explanation: string
  matched_items: string[]
  suggestion: string
}

export interface HealthDimension {
  name: string
  score: number
  weight: number
  detail: string
  has_data: boolean
}

export interface ProjectHealthScore {
  overall_score: number
  risk_level: string
  dimensions: HealthDimension[]
  trend: string
  alert_count: number
  metric_count: number
  data_completeness: number
}

// ── P2: 蓝图提炼 / Fit-Gap / 脱敏 ──────────────────────────────────────

export async function extractBlueprint(researchContext: string): Promise<string> {
  return invoke("extract_blueprint", { researchContext })
}

export async function analyzeFitGap(projectId: number, requirements: string): Promise<string> {
  return invoke("analyze_fit_gap", { projectId, requirements })
}

export async function desensitizeText(
  text: string,
): Promise<{ safe_text: string; mapping: Record<string, string> }> {
  return invoke("desensitize_text", { text })
}

export async function addSensitiveKeyword(keyword: string): Promise<void> {
  return invoke("add_sensitive_keyword", { keyword })
}

export async function listSensitiveKeywords(): Promise<string[]> {
  return invoke("list_sensitive_keywords")
}

export async function removeSensitiveKeyword(keyword: string): Promise<boolean> {
  return invoke("remove_sensitive_keyword", { keyword })
}

// ── ReAct Agent ──────────────────────────────────────────────────────────

export type AgentToolEffect =
  | "read_only"
  | "user_interaction"
  | "skill_reference"
  | "skill_environment"
  | "skill_execution"

export type AgentToolRetry = "none" | "exponential"

export interface AgentToolProfile {
  id: string
  effect: AgentToolEffect
  retry: AgentToolRetry
  schema_guard: boolean
  audit: boolean
  disable_allowed: boolean
}

export interface AgentToolAuditRecord {
  session_id?: string | null
  assistant_message_id?: string | null
  tool_call_id?: string | null
  started_at_ms: number
  tool: string
  effect: AgentToolEffect
  retry: AgentToolRetry
  schema_guard: boolean
  status: "ok" | "error" | string
  duration_ms: number
  args_bytes: number
  output_chars: number | null
  returned_chars: number | null
  truncated: boolean | null
  empty_output: boolean | null
  output_path: string | null
  error_kind: string | null
  error: string | null
}

export interface AgentToolAuditToolSummary {
  tool: string
  calls: number
  ok: number
  error: number
  truncated: number
  empty_output: number
  avg_duration_ms: number
  max_duration_ms: number
  last_started_at_ms: number
}

export interface AgentToolAuditErrorKindSummary {
  kind: string
  count: number
}

export interface AgentToolAuditRecentError {
  started_at_ms: number
  tool: string
  kind: string
  error: string
}

export interface AgentToolAuditSummary {
  sampled: number
  ok: number
  error: number
  truncated: number
  empty_output: number
  avg_duration_ms: number
  max_duration_ms: number
  tools: AgentToolAuditToolSummary[]
  error_kinds: AgentToolAuditErrorKindSummary[]
  recent_errors: AgentToolAuditRecentError[]
}

export interface AgentToolOutputContent {
  path: string
  content: string
  bytes: number
  offset_bytes: number
  returned_bytes: number
  truncated: boolean
  next_offset_bytes: number | null
}

export interface AgentToolOutputLimits {
  max_chars: number
  max_bytes: number
  max_lines: number
}

export interface AgentToolConfig {
  disabled_tools: string[]
  output_limits: AgentToolOutputLimits
}

export interface SkillPermissionRuleInfo {
  rule: string
  effect: "allow" | "deny" | string
  skill_name: string
  script: string
  created_at_ms: number
}

/** Agent 使用提问工具时后端发送的澄清载荷 */
export interface QuestionOption {
  label: string
  description: string
}

export interface ClarificationQuestion {
  prompt: string
  header: string
  mode: "single_choice" | "multi_choice" | "free_input"
  options: QuestionOption[]
  multiple: boolean
  custom: boolean
}

export interface ClarificationPayload {
  question_id: string
  prompt: string
  header: string
  mode: "single_choice" | "multi_choice" | "free_input"
  options: QuestionOption[]
  multiple: boolean
  custom: boolean
  questions: ClarificationQuestion[]
}

export interface PlanStep {
  id: number
  description: string
  tool: string | null
  expected_output: string
  depends_on: number[]
}

export type ReActEvent =
  | { type: "thinking"; session_id: string; sessionId?: string; content: string }
  | { type: "tool_call"; session_id: string; sessionId?: string; name: string; args: string }
  | { type: "tool_result"; session_id: string; sessionId?: string; name: string; result: string }
  | { type: "text_delta"; session_id: string; sessionId?: string; content: string }
  | { type: "error"; session_id: string; sessionId?: string; message: string }
  | { type: "done"; session_id: string; sessionId?: string }
  | { type: "plan_generated"; session_id: string; sessionId?: string; steps: PlanStep[] }
  | {
      type: "step_start"
      session_id: string
      sessionId?: string
      step_index: number
      total_steps: number
      description: string
    }
  | {
      type: "step_result"
      session_id: string
      sessionId?: string
      step_index: number
      result: string
      success: boolean
    }
  | { type: "replan"; session_id: string; sessionId?: string; reason: string }
  | { type: "planner_timeout"; session_id: string; sessionId?: string; message: string }
  | { type: "clarification"; session_id: string; sessionId?: string; payload: ClarificationPayload }

export interface AgentSessionRecord {
  id: string
  project_id: number
  slot: string
  status: string
  provider_id?: string | null
  model_id?: string | null
  started_at: string
  updated_at: string
  ended_at?: string | null
}

export interface AgentMessageRecord {
  id: string
  session_id: string
  role: "user" | "assistant" | string
  content: string
  status: string
  parent_message_id?: string | null
  created_at: string
}

export interface AgentToolCallRecord {
  id: string
  session_id: string
  assistant_message_id?: string | null
  tool_name: string
  tool_revision: string
  effect: string
  args_json: string
  status: string
  started_at: string
  ended_at?: string | null
}

export interface AgentToolResultRecord {
  id: string
  tool_call_id: string
  result_json: string
  preview_text: string
  output_ref?: string | null
  status: string
  created_at: string
}

export interface AgentEventRecord {
  id: string
  session_id: string
  event_type: string
  payload_json: string
  created_at: string
}

export interface AgentSessionSnapshot {
  session: AgentSessionRecord
  messages: AgentMessageRecord[]
  tool_calls: AgentToolCallRecord[]
  tool_results: AgentToolResultRecord[]
  events: AgentEventRecord[]
}

/** 获取 Agent 工具注册元数据，用于诊断、设置和工具策略展示 */
export async function listAgentToolProfiles(): Promise<AgentToolProfile[]> {
  return invoke("list_agent_tool_profiles")
}

/** 获取最近 Agent 工具调用审计记录，按新到旧排序 */
export async function listAgentToolAudit(limit: number = 50): Promise<AgentToolAuditRecord[]> {
  return invoke("list_agent_tool_audit", { limit })
}

/** 获取最近 Agent 工具调用审计摘要，用于观察稳定性和异常分布 */
export async function listAgentToolAuditSummary(
  limit: number = 200,
): Promise<AgentToolAuditSummary> {
  return invoke("list_agent_tool_audit_summary", { limit })
}

/** 获取 Agent 工具可用性配置 */
export async function getAgentToolConfig(): Promise<AgentToolConfig> {
  return invoke("get_agent_tool_config")
}

/** 保存 Agent 工具可用性配置 */
export async function setAgentToolConfig(config: AgentToolConfig): Promise<AgentToolConfig> {
  return invoke("set_agent_tool_config", { config })
}

/** 安全读取被截断后保存的 Agent 工具完整输出预览 */
export async function readAgentToolOutput(
  outputPath: string,
  maxBytes: number = 512 * 1024,
  offsetBytes: number = 0,
): Promise<AgentToolOutputContent> {
  return invoke("read_agent_tool_output", { outputPath, maxBytes, offsetBytes })
}

/** 获取已保存的 skill 脚本授权规则 */
export async function listSkillPermissionRules(): Promise<SkillPermissionRuleInfo[]> {
  return invoke("list_skill_permission_rules")
}

/** 撤销一条已保存的 skill 脚本授权规则 */
export async function revokeSkillPermissionRule(rule: string): Promise<SkillPermissionRuleInfo[]> {
  return invoke("revoke_skill_permission_rule", { rule })
}

/** 获取指定项目最近一次 Agent 会话账本 */
export async function getLatestAgentSession(
  projectId: number,
  slot: string,
): Promise<AgentSessionSnapshot | null> {
  return invoke("get_latest_agent_session", { projectId, slot })
}

/** 获取指定 Agent 会话账本 */
export async function getAgentSession(sessionId: string): Promise<AgentSessionSnapshot | null> {
  return invoke("get_agent_session", { sessionId })
}

function nextSessionId(): string {
  return crypto.randomUUID()
}

/** agentChat 请求超时时间（毫秒） */
const AGENT_CHAT_TIMEOUT_MS = 180_000 // 3 分钟
/** agentChat 最大重试次数 */
const MAX_RETRIES = 2

/**
 * Agent 对话入口：发送消息给 rig agent，通过 SSE 事件流返回结果。
 * 前端应先调用 listenReActEvents() 监听事件，再调用此函数。
 *
 * 包含 3 分钟超时和最多 2 次指数退避重试（仅对超时错误重试）。
 *
 * 注意：Rust 端还有 _system_extra（未使用）参数，Tauri 默认 camelCase 转换后
 * 前端应传 _systemExtra。此处省略因为该参数在 Rust 中未使用。
 */
export async function agentChat(
  message: string,
  sessionId?: string,
  projectId?: number | null,
  history?: ChatMessage[],
  providerId?: string,
  modelId?: string,
  attachments?: AttachmentInfo[],
): Promise<string> {
  const sid = sessionId || nextSessionId()

  let lastError: unknown
  for (let attempt = 0; attempt <= MAX_RETRIES; attempt++) {
    try {
      const invokePromise = invoke("agent_chat", {
        message,
        sessionId: sid,
        projectId: projectId ?? null,
        riskProjectId: null,
        history: history ?? [],
        providerId: providerId ?? null,
        modelId: modelId ?? null,
        attachments: attachments ?? [],
      })

      const timeoutPromise = new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error("__AGENT_CHAT_TIMEOUT__")), AGENT_CHAT_TIMEOUT_MS),
      )

      await Promise.race([invokePromise, timeoutPromise])
      return sid // 成功
    } catch (err) {
      lastError = err
      const isTimeout = err instanceof Error && err.message === "__AGENT_CHAT_TIMEOUT__"

      if (isTimeout && attempt < MAX_RETRIES) {
        // 指数退避：1 秒、2 秒
        const delay = 1000 * 2 ** attempt
        await new Promise((resolve) => setTimeout(resolve, delay))
        continue
      }

      if (isTimeout) {
        throw new Error("请求超时，请检查网络连接后重试")
      }
      throw err
    }
  }

  // 理论上不会到达这里，仅用于满足 TypeScript
  throw lastError
}

/** 回答待处理的澄清问题（解除被阻塞的提问工具） */
export async function answerQuestion(
  questionId: string,
  answer: string,
  sessionId?: string | null,
  projectId?: number | null,
): Promise<void> {
  return invoke("answer_question", {
    questionId,
    answer,
    sessionId: sessionId ?? null,
    projectId: projectId ?? null,
  })
}

/** 取消待处理的澄清问题（解除被阻塞的提问工具） */
export async function rejectQuestion(
  questionId: string,
  sessionId?: string | null,
  projectId?: number | null,
): Promise<void> {
  return invoke("reject_question", {
    questionId,
    sessionId: sessionId ?? null,
    projectId: projectId ?? null,
  })
}

/** 取消正在运行的 Agent 流会话 */
export async function cancelAgentStream(sessionId: string): Promise<void> {
  return invoke("cancel_agent_stream", { sessionId })
}

/**
 * Listen for ReAct agent events, optionally filtered by session_id.
 * Returns an unsubscribe function.
 */
export async function listenReActEvents(
  handler: (event: ReActEvent) => void,
  sessionId?: string,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event")
  const unlisten = await listen<ReActEvent>("react-event", (event) => {
    // 同时检查 snake_case 和 camelCase 的 session_id（Tauri v2 可能转换）
    const eventSessionId = event.payload.session_id || event.payload.sessionId
    if (sessionId && eventSessionId !== sessionId) return
    handler(event.payload)
  })
  return unlisten
}

// ── 工具：导出 ────────────────────────────────────────────────────────────

/** 使用 UTF-8 BOM 编码导出内容到文件（避免中文编码问题） */
export async function exportReport(content: string, filePath: string): Promise<string> {
  return invoke("export_report", { content, filePath })
}

// ── 阶段 14：视频转写 ────────────────────────────────────────────────────

export interface VideoTranscriptionSegment {
  start_ms: number
  end_ms: number
  text: string
}

export interface VideoTranscriptionResult {
  video_path: string
  text: string
  segments: VideoTranscriptionSegment[]
  confidence: number
  extraction_time_ms: number
  transcription_time_ms: number
  duration_secs: number
}

export interface MeetingMinutesResult {
  minutes: string
  generation_time_ms: number
}

export interface VideoPipelineResult {
  transcription: VideoTranscriptionResult
  ingestion_document_id: number | null
  meeting_minutes: MeetingMinutesResult | null
}

/** 通过 Whisper 转写视频文件中的音频，需要先加载 Whisper 模型。 */
export async function transcribeVideoFile(videoPath: string): Promise<VideoTranscriptionResult> {
  return invoke("transcribe_video_file", { videoPath })
}

/** 完整视频流程：转写、入库、可选生成会议纪要。 */
export async function transcribeAndIngestVideo(
  videoPath: string,
  projectId: number,
  generateMinutes: boolean,
): Promise<VideoPipelineResult> {
  return invoke("transcribe_and_ingest_video", {
    videoPath,
    projectId,
    generateMinutes,
  })
}

/** 根据已有转写稿生成会议纪要。 */
export async function generateMeetingMinutesFromTranscript(
  transcript: string,
): Promise<MeetingMinutesResult> {
  return invoke("generate_meeting_minutes_from_transcript", { transcript })
}

/** 视频处理进度事件载荷 */
export interface VideoProgressEvent {
  step: "extracting" | "transcribing" | "ingesting" | "generating_minutes" | "done"
  progress: number
  message: string
}

/** 监听视频处理进度事件，返回取消监听函数。 */
export function listenVideoProgress(
  handler: (event: VideoProgressEvent) => void,
): Promise<() => void> {
  return listen<VideoProgressEvent>("video_progress", (e) => handler(e.payload))
}

// ─── 风险控制类型（P1：双轨风险把控舱） ───

export interface CandidateScopeItem {
  category: string
  description: string
  is_in_scope: boolean
  detail: string
  confidence: number
}

export interface DefenseScriptRequest {
  scenario: string
  context?: string
  tone?: string
}

export interface DefenseScriptResult {
  scenario_label: string
  scripts: ScriptItem[]
}

export interface ScriptItem {
  phase: string
  content: string
  tip: string
}

export interface ImportDbResult {
  db_size_bytes: number
  document_count: number
  chunk_count: number
}

// ─── 风险控制 API ───

// 合同范围
export async function listScopeItems(projectId: number): Promise<ContractScopeItem[]> {
  return invoke("list_scope_items", { projectId })
}

export async function addScopeItem(
  projectId: number,
  category: string,
  description: string,
  isInScope: boolean,
  detail: string,
): Promise<number> {
  return invoke("add_scope_item", {
    projectId,
    category,
    description,
    isInScope,
    detail,
  })
}

export async function deleteScopeItem(projectId: number, itemId: number): Promise<void> {
  return invoke("delete_scope_item", { projectId, itemId })
}

// 需求蔓延检查
export async function checkScopeCreep(
  projectId: number,
  requirement: string,
): Promise<ScopeCreepResult> {
  return invoke("check_scope_creep", { projectId, requirement })
}

// 项目健康度
export async function getProjectHealth(projectId: number): Promise<ProjectHealthScore> {
  return invoke("get_project_health", { projectId })
}

export async function recordHealthMetric(
  projectId: number,
  indicatorType: string,
  value: number,
  notes: string,
): Promise<number> {
  return invoke("record_health_metric", {
    projectId,
    indicatorType,
    value,
    notes,
  })
}

// 健康风险报告
export async function generateRiskReport(projectId: number, context: string): Promise<string> {
  return invoke("generate_risk_report", { projectId, context })
}

// 防身话术
export async function generateDefenseScript(
  projectId: number,
  request: DefenseScriptRequest,
): Promise<DefenseScriptResult> {
  return invoke("generate_defense_script", { projectId, request })
}

// 文档范围提取
export async function extractScopeFromDocument(
  projectId: number,
  docId: number,
): Promise<CandidateScopeItem[]> {
  return invoke("extract_scope_from_document", {
    projectId,
    docId,
  })
}

export async function confirmScopeItems(
  projectId: number,
  items: CandidateScopeItem[],
): Promise<number> {
  return invoke("confirm_scope_items", { projectId, items })
}

// 整库备份
export async function exportDatabase(targetPath: string): Promise<void> {
  return invoke("export_database", { targetPath })
}

export async function importDatabase(backupPath: string): Promise<ImportDbResult> {
  return invoke("import_database", { backupPath })
}

// ─── 阶段 12b：在线 ASR 供应商 ───

export interface AsrProviderInfo {
  type: string
  name: string
  description: string
  supports_streaming: boolean
  supports_file: boolean
}

export async function listAsrProviders(): Promise<AsrProviderInfo[]> {
  return invoke("list_asr_providers")
}

export interface AsrConfigStatus {
  tencent_configured: boolean
}

/** 保存在线 ASR 配置（腾讯云） */
export async function saveAsrConfig(config: {
  tencent_secret_id?: string
  tencent_secret_key?: string
}): Promise<void> {
  return invoke("save_asr_config", config)
}

/** 获取当前 ASR 配置状态 */
export async function getAsrConfigStatus(): Promise<AsrConfigStatus> {
  return invoke("get_asr_config_status")
}

// ─── 腾讯会议 MCP ───

export interface TencentMeetingConfigStatus {
  configured: boolean
}

export interface TencentMeetingToolResult {
  tool_name: string
  content_text: string
  raw: unknown
}

export interface TencentMeetingTranscriptResult {
  record_file_id: string
  transcript: string
  minutes?: string | null
  records_raw?: unknown
  transcript_raw: unknown
  minutes_raw?: unknown
}

export async function saveTencentMeetingToken(token?: string): Promise<void> {
  return invoke("save_tencent_meeting_token", { token: token ?? null })
}

export async function getTencentMeetingConfigStatus(): Promise<TencentMeetingConfigStatus> {
  return invoke("get_tencent_meeting_config_status")
}

export async function listTencentMeetingTools(): Promise<unknown> {
  return invoke("list_tencent_meeting_tools")
}

export async function callTencentMeetingTool(
  name: string,
  argumentsValue: Record<string, unknown>,
): Promise<TencentMeetingToolResult> {
  return invoke("call_tencent_meeting_tool", { name, arguments: argumentsValue })
}

export async function fetchTencentMeetingTranscript(input: {
  meetingId?: string
  meetingCode?: string
  recordFileId?: string
  includeMinutes?: boolean
}): Promise<TencentMeetingTranscriptResult> {
  return invoke("fetch_tencent_meeting_transcript", {
    meetingId: input.meetingId ?? null,
    meetingCode: input.meetingCode ?? null,
    recordFileId: input.recordFileId ?? null,
    includeMinutes: input.includeMinutes ?? false,
  })
}

// ── Wiki 页面 ────────────────────────────────────────────────────────────────

/** Wiki 页面 */
export interface WikiPage {
  id: number
  project_id: number
  slug: string
  title: string
  page_type: string
  content: string
  content_candidate: string | null
  candidate_status: string | null
  sources_candidate: string | null
  frontmatter: string
  sources: string
  wikilinks: string
  tags: string
  page_metadata: string
  candidate_version: number | null
  page_status: string
  version: number
  created_at: string
  updated_at: string
}

/** 创建 Wiki 页面参数 */
export interface CreateWikiPage {
  project_id: number
  slug: string
  title: string
  page_type: string
  content: string
  frontmatter?: string
  sources?: string
  wikilinks?: string
  tags?: string
  page_metadata?: string
  page_status?: string
}

/** Wiki 页面简略信息 */
export interface WikiPageBrief {
  id: number
  slug: string
  title: string
  page_type: string
}

/** Wikilink 目标 */
export interface WikiLinkTarget {
  slug: string
  title: string
  page_type: string
  page_status: string
}

export interface ApproveAutoWikiPagesResult {
  approved: number
  skipped: number
  failed: string[]
}

/** 列出所有 Wiki 页面 */
export async function listWikiPages(projectId: number): Promise<WikiPageBrief[]> {
  return invoke("list_wiki_pages", { projectId })
}

/** 获取 Wiki 页面详情 */
export async function getWikiPage(id: number): Promise<WikiPage> {
  return invoke("get_wiki_page", { id })
}

/** 根据 slug 获取 Wiki 页面 */
export async function getWikiPageBySlug(projectId: number, slug: string): Promise<WikiPage | null> {
  return invoke("get_wiki_page_by_slug", { projectId, slug })
}

/** 创建 Wiki 页面 */
export async function createWikiPage(data: CreateWikiPage): Promise<WikiPage> {
  return invoke("create_wiki_page", { data })
}

/** 更新 Wiki 页面 */
export async function updateWikiPage(id: number, data: Partial<CreateWikiPage>): Promise<WikiPage> {
  return invoke("update_wiki_page", { id, data })
}

/** 删除 Wiki 页面 */
export async function deleteWikiPage(id: number): Promise<void> {
  return invoke("delete_wiki_page", { id })
}

/** 批量删除 Wiki 页面 */
export async function batchDeleteWikiPages(ids: number[]): Promise<number> {
  return invoke("batch_delete_wiki_pages", { ids })
}

/** 批准 Wiki 页面候选内容 */
export async function approveWikiPage(id: number): Promise<WikiPage> {
  return invoke("approve_wiki_page", { id })
}

/** 自动批准低风险候选内容 */
export async function approveAutoWikiPages(projectId: number): Promise<ApproveAutoWikiPagesResult> {
  return invoke("approve_auto_wiki_pages", { projectId })
}

/** 拒绝 Wiki 页面候选内容（清空候选字段，保留 content） */
export async function rejectWikiPage(id: number): Promise<WikiPage> {
  return invoke("reject_wiki_page", { id })
}

/** 搜索 Wiki 页面（用于 wikilink 候选） */
export async function searchWikilinkCandidates(
  projectId: number,
  query: string,
): Promise<WikiPageBrief[]> {
  return invoke("search_wikilink_candidates", { projectId, query })
}

/** 获取反向链接 */
export async function getBacklinks(
  slug: string,
  projectId: number,
): Promise<{ slug: string; title: string; page_type: string }[]> {
  return invoke("get_backlinks", { slug, projectId })
}

/** 验证报告（与 backend VerificationReport 对应） */
export interface VerificationReport {
  level: "Confirmed" | "NeedsReview" | "Suspected" | "Failed"
  overall_confidence: number
  checks: {
    check_name: string
    passed: boolean
    confidence: number
    detail: string
    evidence: string[]
  }[]
  suggested_labels: string[]
}

// ── 验证报告 ──────────────────────────────────────────────────────

/** 对已生成的文本执行验证（Chat 完成后调用） */
export async function runVerification(
  generatedText: string,
  scenario: string,
  sessionId?: string,
): Promise<{ report: VerificationReport }> {
  return invoke("run_verification", {
    request: {
      generated_text: generatedText,
      scenario,
      session_id: sessionId,
    },
  })
}

// ── 知识图谱 ──────────────────────────────────────────────────────

/** 图统计信息（匹配 Rust GraphStats） */
export interface GraphStats {
  total_edges: number
  total_nodes: number
  signal_breakdown: Record<string, number>
  avg_degree: number
}

/** 图邻居（匹配 Rust GraphNeighbor） */
export interface GraphNeighbor {
  slug: string
  title: string
  signal: string
  weight: number
}

/** 知识图谱统计 */
export async function getGraphStats(projectId: number): Promise<GraphStats> {
  return invoke("get_graph_stats", { projectId })
}

/** 获取节点邻居（关联页面） */
export async function getGraphNeighbors(projectId: number, slug: string): Promise<GraphNeighbor[]> {
  return invoke("get_graph_neighbors", { projectId, slug })
}

/** 构建/重建知识图谱（返回插入边数） */
export async function buildKnowledgeGraph(projectId: number): Promise<number> {
  return invoke("build_knowledge_graph", { projectId })
}
