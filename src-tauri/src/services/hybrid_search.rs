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
use crate::services::vector_index::VectorIndex;

/// RRF constant — higher k favors more uniform weighting across retrievers
const RRF_K: f32 = 60.0;

/// Number of candidates to fetch from each retriever before fusion
const TOP_N: usize = 30;

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

/// Compute Reciprocal Rank Fusion scores from two ranked result sets.
///
/// Each retriever returns `ResolvedChunk`s keyed by `chunk_id`. The RRF score
/// for a document is `sum(1 / (k + rank))` across all retrievers where it appears.
///
/// Returns a vec of `(chunk_id, fused_score)` sorted descending by score.
fn rrf_fuse(
    vector_results: &[ResolvedChunk],
    bm25_results: &[ResolvedChunk],
) -> Vec<(i64, f32)> {
    let mut scores: HashMap<i64, f32> = HashMap::new();

    // Vector results (rank is 0-based, so +1 for 1-based rank)
    for (rank, r) in vector_results.iter().enumerate() {
        let entry = scores.entry(r.chunk_id).or_insert(0.0);
        *entry += 1.0 / (RRF_K + (rank + 1) as f32);
    }

    // BM25 results
    for (rank, r) in bm25_results.iter().enumerate() {
        let entry = scores.entry(r.chunk_id).or_insert(0.0);
        *entry += 1.0 / (RRF_K + (rank + 1) as f32);
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
    top_k: usize,
    embedding: &std::sync::Mutex<EmbeddingService>,
    vector_index: &std::sync::Mutex<VectorIndex>,
    bm25: &std::sync::Mutex<BM25Service>,
    metadata: &std::sync::Mutex<MetadataStore>,
) -> Result<Vec<HybridSearchResult>, String> {
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

            chunks
                .into_iter()
                .filter_map(|c| {
                    let (title, project) = meta
                        .get_document(c.document_id)
                        .ok()
                        .flatten()
                        .map(|d| (d.title, d.project))
                        .unwrap_or_else(|| (String::new(), "default".to_string()));

                    if let Some(pid) = project_id {
                        if project != pid {
                            return None;
                        }
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

    // ── Step 4: BM25 search ──
    // BM25SearchResult already contains title, content, section_path, project
    let bm25_raw = {
        let service = bm25.lock().map_err(|e| e.to_string())?;
        service.search(query, project_id, TOP_N as u32)?
    }; // drop bm25 lock

    // Convert BM25SearchResult → ResolvedChunk (metadata already embedded in BM25 results)
    let bm25_resolved: Vec<ResolvedChunk> = bm25_raw
        .into_iter()
        .map(|r| ResolvedChunk {
            chunk_id: r.chunk_id,
            title: r.title,
            content: r.content,
            document_id: 0, // BM25 doesn't return document_id; will resolve if needed
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

    // For BM25-only results missing document_id, resolve via metadata
    let needs_doc_resolve: Vec<i64> = fused
        .iter()
        .filter(|(cid, _)| bm25_map.contains_key(cid) && !vector_map.contains_key(cid))
        .map(|(cid, _)| *cid)
        .collect();

    let doc_id_map: HashMap<i64, i64> = if !needs_doc_resolve.is_empty() {
        let meta = metadata.lock().map_err(|e| e.to_string())?;
        let chunks = meta.get_chunks_by_vector_keys(&needs_doc_resolve)?;
        chunks.into_iter().map(|c| (c.id, c.document_id)).collect()
    } else {
        HashMap::new()
    };

    // ── Step 6: Build final results ──
    let mut results = Vec::with_capacity(top_k);

    for (chunk_id, score) in &fused {
        if results.len() >= top_k {
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
        let resolved = vector_map
            .get(chunk_id)
            .or_else(|| bm25_map.get(chunk_id));

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

    Ok(results)
}
