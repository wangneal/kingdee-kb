//! Hybrid search engine — RRFR fusion of vector + BM25 results
//!
//! Combines semantic (vector) and keyword (BM25) search via Reciprocal Rank
//! Fusion (RRF) with k=60. Project-level isolation prevents cross-project
//! leakage. Stateless module — all functions operate on borrowed service refs.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::services::bm25_service::BM25Service;
use crate::services::embedding::EmbeddingService;
use crate::services::metadata::MetadataStore;
use crate::services::rerank::RerankerService;
use crate::services::vector_index::VectorIndex;
use crate::services::wiki_page::WikiPageStore;

/// RRF constant — higher k favors more uniform weighting across retrievers
const RRF_K: f32 = 60.0;

/// Per-retriever weights for weighted RRF fusion.
///
/// Vector search captures semantic similarity (higher weight for KB QA),
/// BM25 captures exact keyword matches (lower weight as supplement).
const VECTOR_WEIGHT: f32 = 2.0;
const BM25_WEIGHT: f32 = 1.0;

/// Number of candidates to fetch from each retriever before fusion.
/// Increased from 30 to absorb `chat-attachments:` exclusion headroom.
const TOP_N: usize = 200;
const CHAT_ATTACHMENT_PROJECT_PREFIX: &str = "chat-attachments:";

fn is_chat_attachment_project(project: &str) -> bool {
    project.starts_with(CHAT_ATTACHMENT_PROJECT_PREFIX)
}

fn project_allowed(project: &str, project_id: Option<&str>, extra_project_ids: &[String]) -> bool {
    if extra_project_ids.iter().any(|p| p == project) {
        return true;
    }
    if is_chat_attachment_project(project) {
        return false;
    }
    project_id.map_or(true, |pid| project == pid)
}

/// A single fused result returned to the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSearchResult {
    /// Chunk ID (maps to chunks table primary key)
    pub chunk_id: i64,
    /// Document title
    pub title: String,
    /// Chunk content text
    pub content: String,
    /// Fused RRF relevance score (higher = more relevant)
    pub score: f32,
    /// Which retriever(s) contributed: "vector", "bm25", or "both"
    pub source: String,
    /// Document ID for navigation
    pub document_id: i64,
    /// Section path within the document
    pub section_path: Option<String>,
    /// Project name this chunk belongs to
    pub project: String,
}

// ─── Internal helper: resolved chunk info for fusion ───

/// Resolved chunk info used during fusion (avoids re-querying metadata in the hot loop)
#[derive(Debug, Clone)]
struct ResolvedChunk {
    chunk_id: i64,
    title: String,
    content: String,
    document_id: i64,
    section_path: Option<String>,
    project: String,
}

// ─── RRFR Fusion ───

/// Compute Weighted Reciprocal Rank Fusion scores from two ranked result sets.
///
/// Each retriever returns `ResolvedChunk`s keyed by `chunk_id`. The weighted RRF
/// score for a document is `sum(weight / (k + rank))` across all retrievers where
/// it appears. Vector results get `VECTOR_WEIGHT=2.0`, BM25 gets `BM25_WEIGHT=1.0`.
///
/// Returns a vec of `(chunk_id, fused_score)` sorted descending by score.
fn rrf_fuse(vector_results: &[ResolvedChunk], bm25_results: &[ResolvedChunk]) -> Vec<(i64, f32)> {
    let mut scores: HashMap<i64, f32> = HashMap::new();

    // Vector results (weighted: VECTOR_WEIGHT / (RRF_K + rank))
    for (rank, r) in vector_results.iter().enumerate() {
        let entry = scores.entry(r.chunk_id).or_insert(0.0);
        *entry += VECTOR_WEIGHT / (RRF_K + (rank + 1) as f32);
    }

    // BM25 results (weighted: BM25_WEIGHT / (RRF_K + rank))
    for (rank, r) in bm25_results.iter().enumerate() {
        let entry = scores.entry(r.chunk_id).or_insert(0.0);
        *entry += BM25_WEIGHT / (RRF_K + (rank + 1) as f32);
    }

    let mut fused: Vec<(i64, f32)> = scores.into_iter().collect();
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    fused
}

// ─── Hybrid Search ───

/// Perform hybrid search: vector + BM25 → RRFR fusion → project filter → top-K.
///
/// Locking strategy (sequential, no nesting to avoid deadlocks):
/// 1. Lock embedding → embed query → **drop lock**
/// 2. Lock vector_index → search → **drop lock**
/// 3. Lock metadata → resolve vector chunks → **drop lock**
/// 4. Lock bm25 → search → **drop lock** (BM25 already returns enriched results)
/// 5. RRFR fusion + project filter (no locks needed)
pub fn hybrid_search(
    query: &str,
    project_id: Option<&str>,
    extra_project_ids: &[String],
    top_k: usize,
    embedding: &std::sync::Mutex<EmbeddingService>,
    vector_index: &std::sync::Mutex<VectorIndex>,
    bm25: &std::sync::Mutex<BM25Service>,
    metadata: &std::sync::Mutex<MetadataStore>,
    reranker: Option<&RerankerService>,
    wiki_pages: Option<&std::sync::Mutex<WikiPageStore>>,
) -> Result<Vec<HybridSearchResult>, String> {
    // ── Step 0: wiki_pages 优先搜索 ──
    if let Some(wiki_store) = wiki_pages {
        if let Ok(store) = wiki_store.lock() {
            if let Ok(mut wiki_results) = store.search_pages(project_id, query, top_k) {
                wiki_results.retain(|r| project_allowed(&r.project, project_id, extra_project_ids));
                if wiki_results.first().map(|r| r.score).unwrap_or(0.0) > 0.5 {
                    return Ok(wiki_results);
                }
            }
        }
    }

    // ── Step 0.5: 前置过滤——收集 chat-attachments 的 chunk_id 供排除 ──
    let exclude_chunk_ids: Vec<i64> = {
        let meta = metadata.lock().map_err(|e| e.to_string())?;
        meta.get_chat_attachment_chunk_ids()?
    };

    // ── Step 1: Embed query (graceful degradation if model not initialized) ──
    let vector_resolved: Vec<ResolvedChunk> = {
        let emb = embedding.lock().map_err(|e| e.to_string())?;
        if emb.is_ready() {
            // Embedding available → full hybrid search
            drop(emb); // release lock before mutable borrow
            let query_vec = {
                let mut emb_mut = embedding.lock().map_err(|e| e.to_string())?;
                emb_mut.embed_text(query)?
            }; // drop emb_mut lock

            // Vector search
            let vector_raw = {
                let index = vector_index.lock().map_err(|e| e.to_string())?;
                index.search(&query_vec, TOP_N)?
            }; // drop index lock

            // Resolve vector results to full metadata
            let meta = metadata.lock().map_err(|e| e.to_string())?;
            let vector_keys: Vec<i64> = vector_raw.iter().map(|r| r.key as i64).collect();
            let chunks = meta.get_chunks_by_vector_keys(&vector_keys)?;

            // fetch all documents (eliminates N+1 query)
            let doc_ids: Vec<i64> = chunks.iter().map(|c| c.document_id).collect();
            let doc_map = meta.get_documents_by_ids(&doc_ids)?;

            chunks
                .into_iter()
                .filter_map(|c| {
                    // 前置过滤：排除 chat-attachments 的 chunk
                    if exclude_chunk_ids.contains(&c.id) {
                        return None;
                    }
                    let (title, project) = doc_map
                        .get(&c.document_id)
                        .map(|d| (d.title.clone(), d.project.clone()))
                        .unwrap_or_else(|| (String::new(), "default".to_string()));

                    if !project_allowed(&project, project_id, extra_project_ids) {
                        return None;
                    }

                    Some(ResolvedChunk {
                        chunk_id: c.id,
                        title,
                        content: c.content,
                        document_id: c.document_id,
                        section_path: c.section_path,
                        project,
                    })
                })
                .collect()
        } else {
            // Embedding not initialized → skip vector search, BM25 only
            Vec::new()
        }
    }; // drop all locks

    // ── Step 4: BM25 search（前置过滤已排除 chat-attachments chunk）──
    let bm25_raw = {
        let service = bm25.lock().map_err(|e| e.to_string())?;
        service.search(
            query,
            project_id,
            extra_project_ids,
            TOP_N as u32,
            &exclude_chunk_ids,
        )?
    };

    // 将 BM25 结果转为 ResolvedChunk
    let bm25_resolved: Vec<ResolvedChunk> = bm25_raw
        .into_iter()
        .filter(|r| project_allowed(&r.project, project_id, extra_project_ids))
        .map(|r| ResolvedChunk {
            chunk_id: r.chunk_id,
            title: r.title,
            content: r.content,
            document_id: 0,
            section_path: r.section_path,
            project: r.project,
        })
        .collect();

    // ── Step 5: RRFR fusion ──
    let fused = rrf_fuse(&vector_resolved, &bm25_resolved);

    // Build lookup maps for source annotation + metadata
    let vector_map: HashMap<i64, &ResolvedChunk> =
        vector_resolved.iter().map(|r| (r.chunk_id, r)).collect();
    let bm25_map: HashMap<i64, &ResolvedChunk> =
        bm25_resolved.iter().map(|r| (r.chunk_id, r)).collect();

    // ── Step 6: Build initial results ──
    let mut results = build_results(&fused, &vector_map, &bm25_map, &metadata, top_k * 2);

    // ── Step 7: MMR diversity re-ranking ──
    results = diversify_by_title(results, top_k);

    // ── Step 8: Cross-Encoder Rerank（可选）──
    if let Some(reranker) = reranker {
        if let Ok(reranked) = reranker.rerank(query, &results) {
            return Ok(reranked.into_iter().map(|r| HybridSearchResult {
                chunk_id: r.chunk_id,
                title: r.title,
                content: r.content,
                score: r.score,
                source: r.source,
                document_id: r.document_id,
                section_path: r.section_path,
                project: r.project,
            }).collect());
        }
    }

    Ok(results)
}

/// Build `HybridSearchResult` vec from fused scores up to `max_count`.
fn build_results(
    fused: &[(i64, f32)],
    vector_map: &HashMap<i64, &ResolvedChunk>,
    bm25_map: &HashMap<i64, &ResolvedChunk>,
    metadata: &std::sync::Mutex<MetadataStore>,
    max_count: usize,
) -> Vec<HybridSearchResult> {
    // For BM25-only results missing document_id, resolve via metadata
    let needs_doc_resolve: Vec<i64> = fused
        .iter()
        .filter(|(cid, _)| bm25_map.contains_key(cid) && !vector_map.contains_key(cid))
        .map(|(cid, _)| *cid)
        .collect();

    let doc_id_map: HashMap<i64, i64> = if !needs_doc_resolve.is_empty() {
        if let Ok(meta) = metadata.lock() {
            if let Ok(chunks) = meta.get_chunks_by_vector_keys(&needs_doc_resolve) {
                chunks.into_iter().map(|c| (c.id, c.document_id)).collect()
            } else {
                HashMap::new()
            }
        } else {
            HashMap::new()
        }
    } else {
        HashMap::new()
    };

    let mut results = Vec::with_capacity(max_count);

    for (chunk_id, score) in fused {
        if results.len() >= max_count {
            break;
        }

        // Determine source annotation
        let source = match (
            vector_map.contains_key(chunk_id),
            bm25_map.contains_key(chunk_id),
        ) {
            (true, true) => "both",
            (true, false) => "vector",
            (false, true) => "bm25",
            _ => "unknown",
        };

        // Pick metadata from whichever retriever has it
        let resolved = vector_map.get(chunk_id).or_else(|| bm25_map.get(chunk_id));

        if let Some(r) = resolved {
            let document_id = if r.document_id != 0 {
                r.document_id
            } else {
                doc_id_map.get(chunk_id).copied().unwrap_or(0)
            };

            results.push(HybridSearchResult {
                chunk_id: *chunk_id,
                title: r.title.clone(),
                content: r.content.clone(),
                score: *score,
                source: source.to_string(),
                document_id,
                section_path: r.section_path.clone(),
                project: r.project.clone(),
            });
        }
    }

    results
}

/// Enforce result diversity: at most 2 results per document title.
///
/// OpenClaw-inspired MMR-lite: prevents the search from returning chunks
/// all from the same document, ensuring broader coverage across the KB.
fn diversify_by_title(results: Vec<HybridSearchResult>, top_k: usize) -> Vec<HybridSearchResult> {
    let mut per_title: std::collections::HashMap<String, Vec<HybridSearchResult>> =
        std::collections::HashMap::new();
    for r in results {
        per_title.entry(r.title.clone()).or_default().push(r);
    }

    // Sort each title group by score descending, then interleave
    for list in per_title.values_mut() {
        list.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    let mut diversified = Vec::with_capacity(top_k);
    let max_from_title = 2;

    for round in 0..max_from_title {
        let mut remaining_titles: Vec<String> = per_title.keys().cloned().collect();
        remaining_titles.sort();
        for title in &remaining_titles {
            if diversified.len() >= top_k {
                return diversified;
            }
            if let Some(list) = per_title.get_mut(title) {
                if round < list.len() {
                    diversified.push(list[round].clone());
                }
            }
        }
    }

    // Fill remaining slots with any leftover results
    for title in per_title.keys().cloned().collect::<Vec<_>>() {
        if diversified.len() >= top_k {
            break;
        }
        if let Some(list) = per_title.get(&title) {
            for r in list.iter() {
                if !diversified.iter().any(|d| d.chunk_id == r.chunk_id) {
                    diversified.push(r.clone());
                    if diversified.len() >= top_k {
                        return diversified;
                    }
                }
            }
        }
    }

    diversified
}
