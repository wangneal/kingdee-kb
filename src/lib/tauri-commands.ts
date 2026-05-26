import { invoke } from "@tauri-apps/api/core";

// ── Types matching Rust structs ──────────────────────────────────────────────

export interface HybridSearchResult {
  chunk_id: number;
  title: string;
  content: string;
  score: number;
  source: string;
  document_id: number;
  section_path?: string;
  project: string;
}

export interface BM25SearchResult {
  chunk_id: number;
  score: number;
  content: string;
}

export interface IngestionResult {
  document_id: number;
  title: string;
  sha256: string;
  is_duplicate: boolean;
  chunk_count: number;
  vector_count: number;
}

export interface FileError {
  path: string;
  error: string;
}

export interface DirectoryIngestionResult {
  imported: IngestionResult[];
  errors: FileError[];
}

export interface IngestionProgress {
  step: number;
  step_name: string;
  progress: number;
  message?: string;
}

export interface DocumentMeta {
  id: number;
  title: string;
  source_path?: string;
  sha256?: string;
  created_at: string;
  project: string;
}

export interface ChunkMeta {
  id: number;
  vector_key: number;
  document_id: number;
  content: string;
  section_path?: string;
  tags?: string;
  line_no?: number;
  created_at: string;
}

export interface KnowledgeStats {
  document_count: number;
  chunk_count: number;
  db_path: string;
}

// ── LLM / RAG Types ────────────────────────────────────────────────────────

export interface LLMConfig {
  provider: 'openai' | 'anthropic' | 'local';
  api_key: string;
  base_url: string;
  model: string;
  temperature: number;
  max_tokens: number;
}

export interface ChatMessage {
  role: string;
  content: string;
}

export interface StreamChunk {
  content: string;
  done: boolean;
  thinking?: string;
}

export interface RAGSource {
  title: string;
  section_path?: string;
  content_snippet: string;
  score: number;
}

export interface RAGResponse {
  answer: string;
  sources: RAGSource[];
  llm_available: boolean;
}

// ── Tauri command wrappers ───────────────────────────────────────────────────
// NOTE: Tauri v2 invoke() does NOT auto-convert camelCase↔snake_case.
// All parameter keys MUST match the Rust function parameter names exactly (snake_case).

export async function hybridSearch(
  query: string,
  projectId?: string,
  topK?: number
): Promise<HybridSearchResult[]> {
  return invoke("hybrid_search", {
    query,
    projectId: projectId ?? null,
    topK: topK ?? 5,
  });
}

export async function bm25Search(
  query: string,
  projectId?: string,
  topK?: number
): Promise<BM25SearchResult[]> {
  return invoke("bm25_search", {
    query,
    projectId: projectId ?? null,
    topK: topK ?? 10,
  });
}

export async function ingestText(
  text: string,
  title: string,
  project: string
): Promise<IngestionResult> {
  return invoke("ingest_text", { text, title, project });
}

export async function ingestFile(
  filePath: string,
  project: string
): Promise<IngestionResult> {
  return invoke("ingest_file", { filePath, project });
}

export async function ingestDirectory(
  dirPath: string,
  project: string
): Promise<DirectoryIngestionResult> {
  return invoke("ingest_directory", { dirPath, project });
}

export async function listDocuments(
  project?: string
): Promise<DocumentMeta[]> {
  return invoke("list_documents", { project: project ?? null });
}

export async function getDocumentChunks(documentId: number): Promise<ChunkMeta[]> {
  return invoke("get_document_chunks", { documentId });
}

export async function getStats(): Promise<KnowledgeStats> {
  return invoke("get_stats");
}

export async function deleteDocument(documentId: number): Promise<void> {
  return invoke("delete_document", { documentId });
}

/** Batch-delete multiple documents (and their chunks) in a single transaction */
export async function deleteDocumentsBatch(documentIds: number[]): Promise<number> {
  return invoke("delete_documents_batch", { documentIds });
}

// ── Embedding model commands ──────────────────────────────────────────────────

export async function initModel(): Promise<boolean> {
  return invoke("init_model");
}

export async function getModelStatus(): Promise<boolean> {
  return invoke("get_model_status");
}

/** Get embedding model download progress (0–100) */
export async function getDownloadProgress(): Promise<number> {
  return invoke("get_download_progress");
}

// ── LLM / RAG command wrappers ───────────────────────────────────────────────

export interface EmbeddingModelConfig {
  custom_model_dir?: string | null;
}

export async function getEmbeddingModelConfig(): Promise<EmbeddingModelConfig> {
  return invoke("get_embedding_model_config");
}

export async function setEmbeddingModelConfig(
  customModelDir?: string | null
): Promise<boolean> {
  return invoke("set_embedding_model_config", {
    custom_model_dir: customModelDir ?? null,
  });
}

export async function setLLMConfig(config: LLMConfig): Promise<void> {
  return invoke("set_llm_config", { config });
}

export async function getLLMConfig(): Promise<LLMConfig> {
  return invoke("get_llm_config");
}

export async function isLLMConfigured(): Promise<boolean> {
  return invoke("is_llm_configured");
}

export async function testLLMConnection(): Promise<string> {
  return invoke("test_llm_connection");
}

export async function ragQuery(
  query: string,
  projectId?: string,
  conversationHistory?: ChatMessage[]
): Promise<RAGResponse> {
  return invoke("rag_query", {
    query,
    projectId: projectId ?? null,
    conversationHistory: conversationHistory ?? null,
  });
}

export async function ragQueryStream(
  query: string,
  projectId?: string,
  conversationHistory?: ChatMessage[]
): Promise<StreamChunk[]> {
  return invoke("rag_query_stream", {
    query,
    projectId: projectId ?? null,
    conversationHistory: conversationHistory ?? null,
  });
}

export async function countTokens(text: string): Promise<number> {
  return invoke("count_tokens", { text });
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
  projectId?: string,
  conversationHistory?: ChatMessage[]
): Promise<void> {
    return invoke("start_chat_stream", {
    query,
    projectId: projectId ?? null,
    conversationHistory: conversationHistory ?? null,
  });
}

/** A single event from the chat stream */
export interface ChatStreamEvent {
  type: "text_delta" | "done" | "error" | "sources" | "thinking";
  content?: string;
  message?: string;
  sources?: RAGSource[];
}

/**
 * Listen for `chat_chunk` events from the backend streaming chat.
 * Returns an unlisten function to clean up.
 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export function listenChatEvents(
  handler: (event: ChatStreamEvent) => void
): Promise<UnlistenFn> {
  return listen<ChatStreamEvent>("chat_chunk", (e) => handler(e.payload));
}

// ── Chat Memory ───────────────────────────────────────────────────────────

/** Save chat conversation to memory: archive + extract → ingest into KB. */
export async function saveChatMemory(
  conversation: ChatMessage[]
): Promise<void> {
  return invoke("save_chat_memory", { conversation });
}

// ── Phase 9/10/11/12/13: Template & Wizard Types ────────────────────────────

export interface TemplateInfo {
  id: string;
  name: string;
  filename: string;
  phase: string;
  phase_index: number;
  format: string;
  file_path: string;
  relative_path: string;
  file_size: number;
}

export interface FieldInfo {
  name: string;
  field_type: string;
  context: string;
  count: number;
}

export interface SchemaField {
  name: string;
  type: string;
  fill_strategy: string;
  required: boolean;
  default?: string;
  description?: string;
  cell_refs?: string[];
}

export interface TemplateSchema {
  template: {
    id: string;
    name: string;
    format: string;
    phase: string;
  };
  fields: SchemaField[];
}

export interface SmartFillRequest {
  template_id: string;
  user_input: string;
  manual_fields: Record<string, string>;
  schema_fields: SchemaField[];
  project_name?: string;
}

export interface KBSource {
  title: string;
  section_path?: string;
  content_snippet: string;
  score: number;
}

export interface SmartFillResult {
  filled_fields: Record<string, string>;
  ai_fields: string[];
  missing_fields: string[];
  kb_sources: KBSource[];
}

export interface GenerateDocRequest {
  template_path: string;
  output_path: string;
  fields: Record<string, string>;
  schema_fields?: SchemaField[];
  project_name?: string;
  context?: string;
}

export interface MissingField {
  name: string;
  description: string;
  reason: string;
}

export interface GeneratedDoc {
  output_path: string;
  fields_filled: number;
  user_fields: string[];
  ai_fields: string[];
  missing_fields: string[];
  missing_fields_detail: MissingField[];
}

export interface DeliverableRecipe {
  name: string;
  template_id: string;
  phase: string;
  description: string;
  field_overrides: Record<string, { strategy: string; hint?: string }>;
  system_prompt: string;
}

export interface ProductMeta {
  id: number;
  template_id: string;
  template_name: string;
  project: string;
  status: string;
  output_path: string;
  field_count: number;
  llm_fields_count: number;
  created_at: string;
}

// ── Phase 9+ command wrappers ────────────────────────────────────────────────

export async function scanTemplates(templateDir?: string): Promise<TemplateInfo[]> {
  return invoke("scan_templates", { template_dir: templateDir ?? null });
}

export async function extractTemplateFields(filePath: string): Promise<FieldInfo[]> {
  return invoke("extract_template_fields", { filePath });
}

export async function getTemplateSchema(
  templateId: string,
  templateName: string,
  filePath: string,
  phase: string,
  writeSidecar?: boolean
): Promise<TemplateSchema> {
  return invoke("get_template_schema", {
    templateId,
    templateName,
    filePath,
    phase,
    writeSidecar: writeSidecar ?? false,
  });
}

export async function smartFill(request: SmartFillRequest): Promise<SmartFillResult> {
  return invoke("smart_fill", { request });
}

export async function generateDoc(request: GenerateDocRequest): Promise<GeneratedDoc> {
  return invoke("generate_doc", { request });
}

export async function getDeliverableRecipe(templateId: string): Promise<DeliverableRecipe> {
  return invoke("get_deliverable_recipe", { templateId });
}

export async function listProducts(project?: string): Promise<ProductMeta[]> {
  return invoke("list_products", { project: project ?? null });
}

export async function exportProduct(id: number, targetDir: string): Promise<string> {
  return invoke("export_product", { id, targetDir });
}

export async function deleteProduct(id: number): Promise<void> {
  return invoke("delete_product", { id });
}

// ── Phase 13: Research Session Management ─────────────────────────────────

export interface ResearchSession {
  id: number;
  title: string;
  edition: string;
  module_code: string;
  interviewee: string;
  session_date: string;
  status: string;
  created_at: string;
  updated_at: string;
}

export interface QARecord {
  id: number;
  session_id: number;
  question_id: number | null;
  question_text: string;
  answer_text: string;
  notes: string;
  sort_order: number;
  created_at: string;
}

export interface SessionDetail {
  session: ResearchSession;
  records: QARecord[];
}

export async function createResearchSession(
  title: string,
  edition: string,
  moduleCode: string,
  interviewee: string,
  sessionDate: string,
): Promise<number> {
  return invoke("create_research_session", {
    title,
    edition,
    moduleCode,
    interviewee,
    sessionDate,
  });
}

export async function listResearchSessions(): Promise<ResearchSession[]> {
  return invoke("list_research_sessions");
}

export async function getResearchSession(sessionId: number): Promise<SessionDetail | null> {
  return invoke("get_research_session", { sessionId });
}

export async function updateResearchSession(
  sessionId: number,
  title: string,
  interviewee: string,
  sessionDate: string,
  status: string,
): Promise<void> {
  return invoke("update_research_session", { sessionId, title, interviewee, sessionDate, status });
}

export async function deleteResearchSession(sessionId: number): Promise<void> {
  return invoke("delete_research_session", { sessionId });
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
  });
}

export async function updateQARecord(
  recordId: number,
  answerText: string,
  notes: string,
): Promise<void> {
  return invoke("update_qa_record", { recordId, answerText, notes });
}

export async function deleteQARecord(recordId: number): Promise<void> {
  return invoke("delete_qa_record", { recordId });
}

export async function getSessionRecords(sessionId: number): Promise<QARecord[]> {
  return invoke("get_session_records", { sessionId });
}

export async function exportSessionCsv(sessionId: number): Promise<string> {
  return invoke("export_session_csv", { sessionId });
}

export async function exportSessionMarkdown(sessionId: number): Promise<string> {
  return invoke("export_session_markdown", { sessionId });
}

export async function reorderQARecords(sessionId: number, recordIds: number[]): Promise<void> {
  return invoke("reorder_qa_records", { sessionId, recordIds });
}

// ── Phase 12: Whisper Voice Recognition ───────────────────────────────────

export interface TranscriptionResult {
  text: string;
  segments: TranscriptionSegment[];
  confidence: number;
  processing_time_ms: number;
}

export interface TranscriptionSegment {
  start_ms: number;
  end_ms: number;
  text: string;
}

export interface WhisperStatus {
  model_loaded: boolean;
  model_size: string;
  language: string;
}

export async function loadWhisperModel(modelSize: string): Promise<void> {
  return invoke("load_whisper_model", { modelSize });
}

export async function getWhisperStatus(): Promise<WhisperStatus> {
  return invoke("get_whisper_status");
}

export async function startWhisperRecording(): Promise<void> {
  return invoke("start_whisper_recording");
}

export async function stopWhisperRecording(): Promise<TranscriptionResult> {
  return invoke("stop_whisper_recording");
}

// ── P1: 双轨风险把控舱 ──────────────────────────────────────────────────

export interface ContractScopeItem {
  id: number;
  category: string;
  description: string;
  is_in_scope: boolean;
  detail: string;
  created_at: string;
}

export interface ScopeCreepResult {
  risk_level: string;
  risk_label: string;
  explanation: string;
  matched_items: string[];
  suggestion: string;
}

export interface HealthDimension {
  name: string;
  score: number;
  weight: number;
  detail: string;
}

export interface ProjectHealthScore {
  overall_score: number;
  risk_level: string;
  dimensions: HealthDimension[];
  trend: string;
  alert_count: number;
}

export interface DefenseScriptRequest {
  scenario: string;
  context: string;
  tone: string;
}

export interface ScriptItem {
  phase: string;
  content: string;
  tip: string;
}

export interface DefenseScriptResult {
  scenario_label: string;
  scripts: ScriptItem[];
}

export async function addScopeItem(
  category: string,
  description: string,
  isInScope: boolean,
  detail: string,
): Promise<number> {
  return invoke("add_scope_item", { category, description, isInScope, detail });
}

export async function listScopeItems(): Promise<ContractScopeItem[]> {
  return invoke("list_scope_items");
}

export async function deleteScopeItem(itemId: number): Promise<void> {
  return invoke("delete_scope_item", { itemId });
}

export async function checkScopeCreep(requirement: string): Promise<ScopeCreepResult> {
  return invoke("check_scope_creep", { requirement });
}

export async function recordHealthMetric(
  indicatorType: string,
  value: number,
  notes: string,
): Promise<number> {
  return invoke("record_health_metric", { indicatorType, value, notes });
}

export async function getProjectHealth(): Promise<ProjectHealthScore> {
  return invoke("get_project_health");
}

export async function generateRiskReport(context: string): Promise<string> {
  return invoke("generate_risk_report", { context });
}

export async function generateDefenseScript(
  request: DefenseScriptRequest,
): Promise<DefenseScriptResult> {
  return invoke("generate_defense_script", { request });
}

// ── P2: 蓝图提炼 / Fit-Gap / 脱敏 ──────────────────────────────────────

export async function extractBlueprint(researchContext: string): Promise<string> {
  return invoke("extract_blueprint", { research_context: researchContext });
}

export async function analyzeFitGap(requirements: string): Promise<string> {
  return invoke("analyze_fit_gap", { requirements });
}

export async function desensitizeText(text: string): Promise<{ safe_text: string; mapping: Record<string, string> }> {
  return invoke("desensitize_text", { text });
}

export async function addSensitiveKeyword(keyword: string): Promise<void> {
  return invoke("add_sensitive_keyword", { keyword });
}

export async function listSensitiveKeywords(): Promise<string[]> {
  return invoke("list_sensitive_keywords");
}

export async function removeSensitiveKeyword(keyword: string): Promise<boolean> {
  return invoke("remove_sensitive_keyword", { keyword });
}

// ── ReAct Agent ──────────────────────────────────────────────────────────

/** Clarification payload sent from backend when agent uses the question tool */
export interface ClarificationPayload {
  question_id: string;
  prompt: string;
  mode: "single_choice" | "multi_choice" | "free_input";
  options: string[];
}

export type ReActEvent =
  | { type: "thinking"; session_id: string; content: string }
  | { type: "tool_call"; session_id: string; name: string; args: string }
  | { type: "tool_result"; session_id: string; name: string; result: string }
  | { type: "text_delta"; session_id: string; content: string }
  | { type: "error"; session_id: string; message: string }
  | { type: "done"; session_id: string }
  | { type: "clarification"; session_id: string; payload: ClarificationPayload };

function nextSessionId(): string {
  return crypto.randomUUID();
}

export async function reactChat(
  message: string,
  systemExtra?: string,
  sessionId?: string,
): Promise<string> {
  const sid = sessionId || nextSessionId();
  await invoke("react_chat", {
    message,
    systemExtra: systemExtra ?? "",
    sessionId: sid,
  });
  return sid;
}

/** Answer a pending clarification question (resolves the blocked question tool) */
export async function answerQuestion(
  questionId: string,
  answer: string,
): Promise<void> {
  return invoke("answer_question", {
    questionId,
    answer,
  });
}

/**
 * Listen for ReAct agent events, optionally filtered by session_id.
 * Returns an unsubscribe function.
 */
export async function listenReActEvents(
  handler: (event: ReActEvent) => void,
  sessionId?: string,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<ReActEvent>("react-event", (event) => {
    // Check session_id in both snake_case and camelCase (Tauri v2 may convert)
    const eventSessionId = event.payload.session_id || (event.payload as any).sessionId;
    if (sessionId && eventSessionId !== sessionId) return;
    handler(event.payload);
  });
  return unlisten;
}

// ── Utility — Export ───────────────────────────────────────────────────────

/** Export content to a file with UTF-8 BOM encoding (uses PowerShell to avoid Chinese encoding issues) */
export async function exportReport(content: string, filePath: string): Promise<string> {
  return invoke("export_report", { content, filePath });
}

// ── Phase 14: Video Transcription ───────────────────────────────────────

export interface VideoTranscriptionSegment {
  start_ms: number;
  end_ms: number;
  text: string;
}

export interface VideoTranscriptionResult {
  video_path: string;
  text: string;
  segments: VideoTranscriptionSegment[];
  confidence: number;
  extraction_time_ms: number;
  transcription_time_ms: number;
  duration_secs: number;
}

export interface MeetingMinutesResult {
  minutes: string;
  generation_time_ms: number;
}

export interface VideoPipelineResult {
  transcription: VideoTranscriptionResult;
  ingestion_document_id: number | null;
  meeting_minutes: MeetingMinutesResult | null;
}

/** Transcribe audio from a video file via Whisper. Requires loaded Whisper model. */
export async function transcribeVideoFile(
  videoPath: string,
): Promise<VideoTranscriptionResult> {
  return invoke("transcribe_video_file", { video_path: videoPath });
}

/** Full video pipeline: transcribe → ingest into KB → optional meeting minutes. */
export async function transcribeAndIngestVideo(
  videoPath: string,
  project: string,
  generateMinutes: boolean,
): Promise<VideoPipelineResult> {
  return invoke("transcribe_and_ingest_video", {
    video_path: videoPath,
    project,
    generate_minutes: generateMinutes,
  });
}

/** Generate meeting minutes from an existing transcript. */
export async function generateMeetingMinutesFromTranscript(
  transcript: string,
): Promise<MeetingMinutesResult> {
  return invoke("generate_meeting_minutes_from_transcript", { transcript });
}

/** Video processing progress event payload */
export interface VideoProgressEvent {
  step: "extracting" | "transcribing" | "ingesting" | "generating_minutes" | "done";
  progress: number;
  message: string;
}

/** Listen for video processing progress events. Returns unlisten function. */
export function listenVideoProgress(
  handler: (event: VideoProgressEvent) => void,
): Promise<() => void> {
  return listen<VideoProgressEvent>("video_progress", (e) => handler(e.payload));
}
