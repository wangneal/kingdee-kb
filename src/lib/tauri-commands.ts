import { invoke } from "@tauri-apps/api/core";

// ── Types matching Rust structs ──────────────────────────────────────────────

export interface HybridSearchResult {
  chunk_id: string;
  title: string;
  content: string;
  score: number;
  source: string;
  document_id: string;
  section_path?: string;
  project: string;
}

export interface BM25SearchResult {
  chunk_id: string;
  score: number;
  content: string;
}

export interface IngestionResult {
  document_id: string;
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
  id: string;
  title: string;
  source_path?: string;
  sha256?: string;
  created_at: string;
  project: string;
}

export interface ChunkMeta {
  id: string;
  vector_key: number;
  document_id: string;
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
  project?: string,
  limit?: number
): Promise<HybridSearchResult[]> {
  return invoke("hybrid_search", {
    request: {
      query,
      project: project ?? null,
      limit: limit ?? 20,
    },
  });
}

export async function bm25Search(
  query: string,
  limit?: number
): Promise<BM25SearchResult[]> {
  return invoke("bm25_search", {
    query,
    limit: limit ?? 20,
  });
}

export async function ingestText(
  text: string,
  title: string,
  tags: string[]
): Promise<IngestionResult> {
  return invoke("ingest_text", {
    request: { text, title, tags },
  });
}

export async function ingestFile(path: string): Promise<IngestionResult> {
  return invoke("ingest_file", { path });
}

export async function ingestDirectory(path: string): Promise<IngestionResult[]> {
  return invoke("ingest_directory", { path });
}

export async function listDocuments(): Promise<DocumentMeta[]> {
  return invoke("list_documents");
}

export async function getDocumentChunks(documentId: string): Promise<ChunkMeta[]> {
  return invoke("get_document_chunks", { documentId });
}

export async function getStats(): Promise<KnowledgeStats> {
  return invoke("get_stats");
}

export async function deleteDocument(documentId: string): Promise<void> {
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
