import { invoke } from "@tauri-apps/api/core"

// ── Types matching Rust structs ──────────────────────────────────────────────

export interface AttachmentInfo {
  name: string
  path: string
  kind: string // "image" | "document"
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

// ── LLM / RAG Types ────────────────────────────────────────────────────────

export interface ChatMessage {
  role: string
  content: string
}

export interface StreamChunk {
  content: string
  done: boolean
  thinking?: string
}

export interface RAGSource {
  title: string
  section_path?: string
  content_snippet: string
  score: number
}

export interface RAGResponse {
  answer: string
  sources: RAGSource[]
  llm_available: boolean
}

// ── Tauri command wrappers ───────────────────────────────────────────────────
// NOTE: Tauri v2 #[tauri::command] defaults to rename_all="camelCase".
// JS invoke() must use camelCase keys (e.g. filePath, not file_path).
// Rust function params stay snake_case; the macro handles the mapping.

/** Check if any LLM provider is configured with a valid API key */
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

export async function recompileFailedKbSources(
  projectId: number,
): Promise<RecompileFailedSourcesResult> {
  return invoke("recompile_failed_kb_sources", { projectId })
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

/** Batch-delete multiple documents (and their chunks) in a single transaction */
export async function deleteDocumentsBatch(
  documentIds: number[],
  projectId?: number | null,
): Promise<number> {
  return invoke("delete_documents_batch", { documentIds, projectId: projectId ?? null })
}

// ── Embedding model commands ──────────────────────────────────────────────────

export async function initModel(): Promise<boolean> {
  return invoke("init_model")
}

export async function getModelStatus(): Promise<boolean> {
  return invoke("get_model_status")
}

/** Get embedding model download progress (0–100) */
export async function getDownloadProgress(): Promise<number> {
  return invoke("get_download_progress")
}

// ── LLM / RAG command wrappers ───────────────────────────────────────────────

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

/** Online embedding provider configuration (stored in frontend localStorage) */
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

export async function ragQuery(
  query: string,
  projectId?: number | null,
  conversationHistory?: ChatMessage[],
): Promise<RAGResponse> {
  return invoke("rag_query", {
    query,
    projectId: projectId ?? null,
    conversationHistory: conversationHistory ?? null,
  })
}

export async function ragQueryStream(
  query: string,
  projectId?: number | null,
  conversationHistory?: ChatMessage[],
): Promise<StreamChunk[]> {
  return invoke("rag_query_stream", {
    query,
    projectId: projectId ?? null,
    conversationHistory: conversationHistory ?? null,
  })
}

export async function countTokens(text: string): Promise<number> {
  return invoke("count_tokens", { text })
}

// ── Real-time streaming chat (EventSource pattern, like EchoBird) ────────────

/**
 * Start a streaming chat session.
 *
 * The backend spawns a background task and emits `chat_chunk` Tauri events.
 * Frontend should call `listenChatEvents()` before this to receive chunks.
 */
export async function startChatStream(
  query: string,
  projectId?: number | null,
  conversationHistory?: ChatMessage[],
): Promise<void> {
  return invoke("start_chat_stream", {
    query,
    projectId: projectId ?? null,
    conversationHistory: conversationHistory ?? null,
  })
}

/** A single event from the chat stream */
export interface ChatStreamEvent {
  type: "text_delta" | "done" | "error" | "sources" | "thinking"
  content?: string
  message?: string
  sources?: RAGSource[]
}

/**
 * Listen for `chat_chunk` events from the backend streaming chat.
 * Returns an unlisten function to clean up.
 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

export function listenChatEvents(handler: (event: ChatStreamEvent) => void): Promise<UnlistenFn> {
  return listen<ChatStreamEvent>("chat_chunk", (e) => handler(e.payload))
}

// ── Chat Memory ───────────────────────────────────────────────────────────

/** Save chat conversation to memory: archive + extract → ingest into KB. */
export async function saveChatMemory(
  conversation: ChatMessage[],
  projectId?: number | null,
): Promise<void> {
  return invoke("save_chat_memory", { conversation, projectId: projectId ?? null })
}

// ── Phase 9/10/11/12/13: Template & Wizard Types ────────────────────────────

export interface TemplateInfo {
  id: string
  name: string
  filename: string
  phase: string
  phase_index: number
  format: string
  file_path: string
  relative_path: string
  file_size: number
}

export interface FieldInfo {
  name: string
  field_type: string
  context: string
  count: number
}

export interface SchemaField {
  name: string
  type: string
  fill_strategy: string
  required: boolean
  default?: string
  description?: string
  cell_refs?: string[]
}

export interface TemplateSchema {
  template: {
    id: string
    name: string
    format: string
    phase: string
  }
  fields: SchemaField[]
}

export interface SmartFillRequest {
  template_id: string
  user_input: string
  manual_fields: Record<string, string>
  schema_fields: SchemaField[]
  project_name?: string
  project_id?: number
}

export interface KBSource {
  title: string
  section_path?: string
  content_snippet: string
  score: number
}

export interface SmartFillResult {
  filled_fields: Record<string, string>
  ai_fields: string[]
  missing_fields: string[]
  kb_sources: KBSource[]
}

export interface GenerateDocRequest {
  template_path: string
  output_path: string
  fields: Record<string, string>
  schema_fields?: SchemaField[]
  project_name?: string
  project_id?: number
  context?: string
}

export interface MissingField {
  name: string
  description: string
  reason: string
}

export interface GeneratedDoc {
  output_path: string
  fields_filled: number
  user_fields: string[]
  ai_fields: string[]
  missing_fields: string[]
  missing_fields_detail: MissingField[]
}

export interface DeliverableRecipe {
  name: string
  template_id: string
  phase: string
  description: string
  field_overrides: Record<string, { strategy: string; hint?: string }>
  system_prompt: string
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

// ── Phase 9+ command wrappers ────────────────────────────────────────────────

export async function scanTemplates(templateDir?: string): Promise<TemplateInfo[]> {
  return invoke("scan_templates", { templateDir: templateDir ?? null })
}

export async function extractTemplateFields(filePath: string): Promise<FieldInfo[]> {
  return invoke("extract_template_fields", { filePath })
}

export async function getTemplateSchema(
  templateId: string,
  templateName: string,
  filePath: string,
  phase: string,
  writeSidecar?: boolean,
): Promise<TemplateSchema> {
  return invoke("get_template_schema", {
    templateId,
    templateName,
    filePath,
    phase,
    writeSidecar: writeSidecar ?? false,
  })
}

export async function smartFill(request: SmartFillRequest): Promise<SmartFillResult> {
  return invoke("smart_fill", { request })
}

export async function generateDoc(request: GenerateDocRequest): Promise<GeneratedDoc> {
  return invoke("generate_doc", { request })
}

export async function getDeliverableRecipe(templateId: string): Promise<DeliverableRecipe> {
  return invoke("get_deliverable_recipe", { templateId })
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

// ── Phase 13: Research Session Management ─────────────────────────────────

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

// ── Phase 12: Whisper Voice Recognition ───────────────────────────────────

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

export async function loadWhisperModel(modelSize: string): Promise<void> {
  return invoke("load_whisper_model", { modelSize })
}

export async function getWhisperStatus(): Promise<WhisperStatus> {
  return invoke("get_whisper_status")
}

export async function startWhisperRecording(): Promise<void> {
  return invoke("start_whisper_recording")
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

/** Clarification payload sent from backend when agent uses the question tool */
export interface ClarificationPayload {
  question_id: string
  prompt: string
  mode: "single_choice" | "multi_choice" | "free_input"
  options: string[]
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

function nextSessionId(): string {
  return crypto.randomUUID()
}

/** agentChat 请求超时时间（毫秒） */
const AGENT_CHAT_TIMEOUT_MS = 180_000 // 3 minutes
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
        attachments: attachments ?? [],
      })

      const timeoutPromise = new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error("__AGENT_CHAT_TIMEOUT__")), AGENT_CHAT_TIMEOUT_MS),
      )

      await Promise.race([invokePromise, timeoutPromise])
      return sid // Success
    } catch (err) {
      lastError = err
      const isTimeout = err instanceof Error && err.message === "__AGENT_CHAT_TIMEOUT__"

      if (isTimeout && attempt < MAX_RETRIES) {
        // Exponential backoff: 1s, 2s
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

  // Should not reach here, but satisfy TypeScript
  throw lastError
}

/** Answer a pending clarification question (resolves the blocked question tool) */
export async function answerQuestion(
  questionId: string,
  answer: string,
  projectId?: number | null,
): Promise<void> {
  return invoke("answer_question", {
    questionId,
    answer,
    projectId: projectId ?? null,
  })
}

/** Cancel a running agent stream session */
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
    // Check session_id in both snake_case and camelCase (Tauri v2 may convert)
    const eventSessionId = event.payload.session_id || event.payload.sessionId
    if (sessionId && eventSessionId !== sessionId) return
    handler(event.payload)
  })
  return unlisten
}

// ── Utility — Export ───────────────────────────────────────────────────────

/** Export content to a file with UTF-8 BOM encoding (uses PowerShell to avoid Chinese encoding issues) */
export async function exportReport(content: string, filePath: string): Promise<string> {
  return invoke("export_report", { content, filePath })
}

// ── Phase 14: Video Transcription ───────────────────────────────────────

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

/** Transcribe audio from a video file via Whisper. Requires loaded Whisper model. */
export async function transcribeVideoFile(videoPath: string): Promise<VideoTranscriptionResult> {
  return invoke("transcribe_video_file", { videoPath })
}

/** Full video pipeline: transcribe → ingest into KB → optional meeting minutes. */
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

/** Generate meeting minutes from an existing transcript. */
export async function generateMeetingMinutesFromTranscript(
  transcript: string,
): Promise<MeetingMinutesResult> {
  return invoke("generate_meeting_minutes_from_transcript", { transcript })
}

/** Video processing progress event payload */
export interface VideoProgressEvent {
  step: "extracting" | "transcribing" | "ingesting" | "generating_minutes" | "done"
  progress: number
  message: string
}

/** Listen for video processing progress events. Returns unlisten function. */
export function listenVideoProgress(
  handler: (event: VideoProgressEvent) => void,
): Promise<() => void> {
  return listen<VideoProgressEvent>("video_progress", (e) => handler(e.payload))
}

// ─── Risk Control Types (P1: 双轨风险把控舱) ───

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

// ─── Risk Control API ───

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

// ─── Phase 12b: 在线 ASR Provider ───

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
export async function getGraphNeighbors(
  projectId: number,
  slug: string,
): Promise<GraphNeighbor[]> {
  return invoke("get_graph_neighbors", { projectId, slug })
}

/** 构建/重建知识图谱（返回插入边数） */
export async function buildKnowledgeGraph(projectId: number): Promise<number> {
  return invoke("build_knowledge_graph", { projectId })
}
