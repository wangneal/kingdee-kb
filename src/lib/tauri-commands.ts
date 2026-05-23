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
  chunk_count: number;
  vector_count: number;
  project: string;
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
  api_key: string;
  base_url: string;
  model: string;
  max_tokens: number;
  temperature: number;
}

export interface ChatMessage {
  role: string;
  content: string;
}

export interface StreamChunk {
  content: string;
  done: boolean;
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
): Promise<IngestionResult[]> {
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

// ── LLM / RAG command wrappers ───────────────────────────────────────────────

export async function setLLMConfig(config: LLMConfig): Promise<void> {
  return invoke("set_llm_config", { config });
}

export async function getLLMConfig(): Promise<LLMConfig> {
  return invoke("get_llm_config");
}

export async function isLLMConfigured(): Promise<boolean> {
  return invoke("is_llm_configured");
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
  return invoke("scan_templates", { templateDir: templateDir ?? null });
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
