//! Question recommendation engine — semantic matching + edition filter + context accumulation
//!
//! Recommends relevant research questions based on the current conversation topic.
//! Uses hybrid_search (vector + BM25) against the research_questions index, with
//! edition filtering and answered-question exclusion at the DB level.
//!
//! Graceful degradation: KB search failure -> return empty list (non-fatal).
//! LLM failure for follow-up questions -> return empty list (non-fatal).

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use crate::services::bm25_service::BM25Service;
use crate::services::embedding::EmbeddingService;
use crate::services::hybrid_search::{self, HybridSearchResult};
use crate::services::llm_service::{ChatMessage, LLMService};
use crate::services::metadata::MetadataStore;
use crate::services::research_indexer::ResearchIndexer;
use crate::services::research_outline::{Edition, FlatQuestion};
use crate::services::smart_completion::{KBSource, SmartFillResult};
use crate::services::verification::types::ScenarioType;
use crate::services::vector_index::VectorIndex;

// --- Types ---

/// A single recommended question with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendedQuestion {
    /// Database ID of the question (research_questions.id)
    pub question_id: i64,
    /// The question text
    pub question_text: String,
    /// Module name (e.g., "总账", "基础平台")
    pub module_name: String,
    /// Section name (e.g., "架构", "业务概况")
    pub section: String,
    /// Category name (e.g., "部署架构", "组织人员")
    pub category: String,
    /// Relevance score from hybrid search (higher = more relevant)
    pub score: f32,
    /// Which retriever contributed: "vector", "bm25", or "both"
    pub source: String,
    /// Whether this question has been answered (always false for recommended)
    pub is_answered: bool,
}

/// Request for question recommendation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendRequest {
    /// Current topic / user input for semantic matching
    pub query: String,
    /// IDs of already-answered questions (excluded from results)
    pub answered_question_ids: Vec<i64>,
    /// Text of already-answered questions (for context accumulation)
    pub answered_texts: Vec<String>,
    /// Optional project name for KB filtering
    pub project_name: Option<String>,
    /// Maximum number of results (default 10)
    pub top_k: Option<usize>,
}

/// Request for follow-up question generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowUpRequest {
    /// Already answered Q&A pairs (question, answer)
    pub answered_qa: Vec<(String, String)>,
    /// Optional project name for KB context
    pub project_name: Option<String>,
    /// Optional module name for context focus
    pub module_name: Option<String>,
}

/// Result of follow-up question generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowUpResult {
    /// Generated follow-up questions
    pub followup_questions: Vec<String>,
    /// KB sources used as context
    pub kb_sources: Vec<KBSource>,
}

// --- Constants ---

const DEFAULT_TOP_K: usize = 10;
const FOLLOWUP_KB_TOP_K: usize = 5;
const SMART_FILL_KB_TOP_K: usize = 8;
// ─── T1: recommend_questions ───

/// Recommend relevant research questions based on the current topic.
///
/// Pipeline:
/// 1. Build enhanced query (user query + answered question texts)
/// 2. Call hybrid_search to get semantically matching chunks
/// 3. Extract question IDs from matching chunk metadata
/// 4. Fetch candidate questions from DB (edition-filtered, excluding answered IDs)
/// 5. Score candidates by hybrid search overlap + text similarity
/// 6. Deduplicate by Jaccard similarity on bigrams
/// 7. Return top-K results
///
/// Graceful degradation: if hybrid_search fails, falls back to DB-only
/// recommendations (ordered by question_order).
pub fn recommend_questions(
    request: &RecommendRequest,
    indexer: &ResearchIndexer,
    edition: &Edition,
    embedding: &Mutex<EmbeddingService>,
    vector_index: &Mutex<VectorIndex>,
    bm25: &Mutex<BM25Service>,
    metadata: &Mutex<MetadataStore>,
) -> Result<Vec<RecommendedQuestion>, String> {
    let top_k = request.top_k.unwrap_or(DEFAULT_TOP_K);

    // Step 1: Build enhanced query from user input + answered texts
    let enhanced_query = build_enhanced_query(&request.query, &request.answered_texts);

    // Step 2: Hybrid search for relevant chunks
    let search_results = hybrid_search::hybrid_search(
        &enhanced_query,
        request.project_name.as_deref(),
        &[],
        top_k * 3, // over-fetch for better coverage
        embedding,
        vector_index,
        bm25,
        metadata,
    )
    .unwrap_or_default(); // KB search failure is non-fatal

    // Step 3: Get candidate questions from DB (edition-filtered, excluding answered)
    let exclude_ids: &[i64] = &request.answered_question_ids;
    let candidates = indexer.get_questions_by_edition_excluding(edition, exclude_ids, top_k * 5)?;

    // Step 4: If hybrid search returned results, score by overlap; else fallback
    if search_results.is_empty() {
        return fallback_recommend_by_edition(&candidates, top_k);
    }

    // Step 5: Score candidates by text similarity to search results
    let search_texts: Vec<String> = search_results
        .iter()
        .take(10)
        .map(|r| r.content.clone())
        .collect();
    let mut scored: Vec<(i64, FlatQuestion, f32)> = Vec::new();

    for (id, fq) in &candidates {
        let candidate_text = format!(
            "{} {} {} {}",
            fq.module_name, fq.section, fq.category, fq.question_text
        );
        let mut best_score: f32 = 0.0;
        for st in &search_texts {
            let sim = text_similarity(&candidate_text, st);
            if sim > best_score {
                best_score = sim;
            }
        }
        scored.push((*id, fq.clone(), best_score));
    }

    // Sort by score descending
    scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    // Step 6: Deduplicate
    let deduped = deduplicate_by_similarity(scored, 0.8);

    // Step 7: Build results
    let results: Vec<RecommendedQuestion> = deduped
        .into_iter()
        .take(top_k)
        .map(|(id, fq, score)| RecommendedQuestion {
            question_id: id,
            question_text: fq.question_text,
            module_name: fq.module_name,
            section: fq.section,
            category: fq.category,
            score,
            source: if score > 0.0 {
                "hybrid".to_string()
            } else {
                "db".to_string()
            },
            is_answered: false,
        })
        .collect();

    Ok(results)
}

// ─── Internal helpers for T1 ───

/// Build an enhanced search query by concatenating the user query with
/// answered question texts (max 5 texts, 200 chars each).
fn build_enhanced_query(query: &str, answered_texts: &[String]) -> String {
    let mut parts = Vec::new();
    parts.push(query.to_string());

    for text in answered_texts.iter().take(5) {
        let truncated = truncate_str(text, 200);
        if !truncated.is_empty() {
            parts.push(truncated);
        }
    }

    parts.join(" ")
}

/// Fallback: return questions ordered by DB order (question_order) when
/// hybrid search returns no results.
fn fallback_recommend_by_edition(
    candidates: &[(i64, FlatQuestion)],
    top_k: usize,
) -> Result<Vec<RecommendedQuestion>, String> {
    let results: Vec<RecommendedQuestion> = candidates
        .iter()
        .take(top_k)
        .map(|(id, fq)| RecommendedQuestion {
            question_id: *id,
            question_text: fq.question_text.clone(),
            module_name: fq.module_name.clone(),
            section: fq.section.clone(),
            category: fq.category.clone(),
            score: 0.0,
            source: "db".to_string(),
            is_answered: false,
        })
        .collect();

    Ok(results)
}

/// Deduplicate scored candidates by Jaccard similarity on character bigrams.
/// Two candidates with similarity > threshold are considered duplicates;
/// the one with higher score is kept.
fn deduplicate_by_similarity(
    scored: Vec<(i64, FlatQuestion, f32)>,
    threshold: f32,
) -> Vec<(i64, FlatQuestion, f32)> {
    let mut result: Vec<(i64, FlatQuestion, f32)> = Vec::new();

    for item in scored {
        let candidate_text = &item.1.question_text;
        let mut is_dup = false;

        for existing in &result {
            let sim = text_similarity(candidate_text, &existing.1.question_text);
            if sim > threshold {
                is_dup = true;
                break;
            }
        }

        if !is_dup {
            result.push(item);
        }
    }

    result
}

/// Compute Jaccard similarity between two strings based on character bigrams.
fn text_similarity(a: &str, b: &str) -> f32 {
    let bigrams_a = build_bigrams(a);
    let bigrams_b = build_bigrams(b);

    if bigrams_a.is_empty() && bigrams_b.is_empty() {
        return 1.0;
    }
    if bigrams_a.is_empty() || bigrams_b.is_empty() {
        return 0.0;
    }

    let intersection = bigrams_a.intersection(&bigrams_b).count() as f32;
    let union = bigrams_a.union(&bigrams_b).count() as f32;

    if union == 0.0 {
        return 0.0;
    }

    intersection / union
}

/// Build a HashSet of character bigrams from a string.
fn build_bigrams(text: &str) -> HashSet<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 2 {
        return HashSet::new();
    }

    chars
        .windows(2)
        .map(|w| w.iter().collect::<String>())
        .collect()
}

/// Truncate a string to a maximum character length.
fn truncate_str(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

// ─── T2: generate_followup_questions ───

/// Generate follow-up questions based on answered Q&A context + KB knowledge.
///
/// Pipeline:
/// 1. Build query from answered Q&A text
/// 2. Search KB for relevant context
/// 3. Build system + user prompts for LLM
/// 4. Call LLM to generate follow-up questions
/// 5. Parse LLM response into a list of questions
///
/// Graceful degradation: KB search failure -> proceed with Q&A context only.
/// LLM failure -> return empty FollowUpResult (logged, non-fatal).
pub async fn generate_followup_questions(
    request: &FollowUpRequest,
    llm: &LLMService,
    embedding: &Mutex<EmbeddingService>,
    vector_index: &Mutex<VectorIndex>,
    bm25: &Mutex<BM25Service>,
    metadata: &Mutex<MetadataStore>,
) -> Result<FollowUpResult, String> {
    // Step 1: Build search query from answered Q&A pairs
    let module_name_ref: Option<&str> = request.module_name.as_deref();
    let query = build_followup_query(&request.answered_qa, &module_name_ref);

    // Step 2: Search KB for relevant context
    let (kb_results, kb_sources) = search_kb_for_followup(
        &query,
        request.project_name.as_deref(),
        FOLLOWUP_KB_TOP_K,
        embedding,
        vector_index,
        bm25,
        metadata,
    );

    // Step 3: Build prompts
    let system_prompt = build_followup_system_prompt(&module_name_ref);
    let user_prompt = build_followup_user_prompt(&request.answered_qa, &kb_results);

    // Step 4: Call LLM
    let config = llm.get_active_config()?;
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system_prompt,
        },
        ChatMessage {
            role: "user".to_string(),
            content: user_prompt,
        },
    ];

    let (response_text, _report) = llm.verified_chat_completion(&messages, &config, ScenarioType::Research).await?;
    let followup_questions = parse_followup_response(&response_text);
    Ok(FollowUpResult {
        followup_questions,
        kb_sources,
    })
}

// ─── T3: smart_fill_for_question ───
/// Smart-fill a single question using KB knowledge + LLM.
///
/// Pipeline:
/// 1. Find matching question from indexer
/// 2. Assemble KB context via hybrid search
/// 3. Build prompt with question + KB context
/// 4. Call LLM to generate an answer
/// 5. Return SmartFillResult with sources
///
/// Graceful degradation: KB search failure -> proceed without KB context.
/// LLM failure -> return SmartFillResult with missing_fields.
pub async fn smart_fill_for_question(
    question_text: &str,
    edition: &Edition,
    project_name: Option<&str>,
    indexer: &ResearchIndexer,
    llm: &LLMService,
    embedding: &Mutex<EmbeddingService>,
    vector_index: &Mutex<VectorIndex>,
    bm25: &Mutex<BM25Service>,
    metadata: &Mutex<MetadataStore>,
) -> Result<SmartFillResult, String> {
    // Step 1: Find matching question in indexer (for context enrichment)
    let matching_question = find_matching_question(question_text, edition, indexer);

    // Step 2: Assemble KB context
    let (kb_results, kb_sources) = assemble_kb_context_for_fill(
        question_text,
        project_name.as_deref(),
        SMART_FILL_KB_TOP_K,
        embedding,
        vector_index,
        bm25,
        metadata,
    );

    // Step 3: Build prompt
    let mut filled_fields = HashMap::new();
    let mut ai_fields = Vec::new();
    let mut missing_fields = Vec::new();

    // Build system prompt for question answering
    let system_prompt = "你是一个金蝶ERP实施调研助手。。
根据提供的知识库内容，回答调研问题。
请基于知识库中的真实信息回答，如果知识库中没有相关信息，
可以根据金蝶ERP实施经验给出合理的参考回答。
回答应当简洁专业,直接给出答案内容。"
        .to_string();

    // Build user prompt with question + KB context
    let mut user_prompt = format!("问题: {}\n\n", question_text);

    if let Some(ref q) = &matching_question {
        user_prompt.push_str(&format!(
            "上下文信息: 模块={}, 章节={}, 分类={}\n\n",
            q.module_name, q.section, q.category
        ));
    }

    if kb_results.is_empty() {
        user_prompt.push_str("（知识库中未找到相关信息）\n");
    } else {
        user_prompt.push_str("知识库参考资料:\n");
        for result in kb_results.iter().take(5) {
            let section = result.section_path.as_deref().unwrap_or("无章节");
            user_prompt.push_str(&format!(
                "[来源: {} | {}]\n{}\n\n",
                result.title,
                section,
                truncate_str(&result.content, 300)
            ));
        }
    }

    user_prompt.push_str("\n请直接回答上述问题。");

    // Step 4: Call LLM
    let config = llm.get_active_config()?;
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system_prompt,
        },
        ChatMessage {
            role: "user".to_string(),
            content: user_prompt,
        },
    ];

    let llm_response = llm.verified_chat_completion(&messages, &config, ScenarioType::Research).await;

    match llm_response {
        Ok((answer, _report)) => {
            if answer.trim().is_empty() {
                missing_fields.push(question_text.to_string());
            } else {
                filled_fields.insert(question_text.to_string(), answer.trim().to_string());
                ai_fields.push(question_text.to_string());
            }
        }
        Err(e) => {
            eprintln!(
                "[QuestionRecommend] Smart fill LLM call failed (non-fatal) for question '{}': {}",
                question_text, e
            );
            missing_fields.push(question_text.to_string());
        }
    }

    Ok(SmartFillResult {
        filled_fields,
        ai_fields,
        missing_fields,
        kb_sources,
    })
}

// ─── Internal helpers for T2 ───

/// Build a search query from answered Q&A pairs for follow-up question generation.
fn build_followup_query(answered_qa: &[(String, String)], module_name: &Option<&str>) -> String {
    let mut parts = Vec::new();

    // Include module focus if provided
    if let Some(module) = module_name {
        parts.push(module.to_string());
    }

    // Combine answered question texts for context
    for (question, answer) in answered_qa.iter().take(3) {
        parts.push(truncate_str(question, 100));
        if !answer.is_empty() {
            parts.push(truncate_str(answer, 150));
        }
    }

    // Join with separator for semantic search
    parts.join(" ")
}

/// Search KB for context relevant to follow-up question generation.
fn search_kb_for_followup(
    query: &str,
    project_name: Option<&str>,
    top_k: usize,
    embedding: &Mutex<EmbeddingService>,
    vector_index: &Mutex<VectorIndex>,
    bm25: &Mutex<BM25Service>,
    metadata: &Mutex<MetadataStore>,
) -> (String, Vec<KBSource>) {
    let search_results = hybrid_search::hybrid_search(
        query,
        project_name,
        &[],
        top_k,
        embedding,
        vector_index,
        bm25,
        metadata,
    )
    .unwrap_or_default();

    // Assemble KB context string
    let mut context = String::new();
    for result in &search_results {
        let section = result.section_path.as_deref().unwrap_or("（无章节信息）");
        context.push_str(&format!(
            "[来源：{} | {}]\n{}\n\n",
            result.title, section, result.content
        ));
    }

    let kb_sources: Vec<KBSource> = search_results
        .iter()
        .map(|r| KBSource {
            title: r.title.clone(),
            section_path: r.section_path.clone(),
            content_snippet: truncate_str(&r.content, 200),
            score: r.score,
        })
        .collect();

    (context, kb_sources)
}

/// Build system prompt for follow-up question generation.
fn build_followup_system_prompt(module_name: &Option<&str>) -> String {
    let base = "你是一个金蝶ERP实施调研助手。。
根据已回答的问题和知识库内容，生成3-5个后续调研问题。
这些问题应该：
1. 与已回答的问题在主题上相关但有延伸性
2. 能够帮助更深入了解金蝶ERP在该领域的实施细节
3. 避免与已回答的问题重复";

    match module_name {
        Some(module) => format!("{}\n4. 重点关注模块「{}」", base, module),
        None => base.to_string(),
    }
}

/// Build user prompt for follow-up question generation.
fn build_followup_user_prompt(answered_qa: &[(String, String)], kb_context: &str) -> String {
    let mut prompt = "已回答的问题:\n".to_string();

    for (i, (question, answer)) in answered_qa.iter().enumerate() {
        prompt.push_str(&format!(
            "{}. Q: {}\n   A: {}\n",
            i + 1,
            truncate_str(question, 200),
            truncate_str(answer, 200)
        ));
    }

    if !kb_context.is_empty() {
        prompt.push_str(&format!("\n知识库参考:\n{}", kb_context));
    }

    prompt.push_str("\n请生成后续调研问题，以JSON数组格式返回: [\"问题1\", \"问题2\", ...]");
    prompt
}

/// Parse LLM response for follow-up questions.
///
/// Expected format: JSON array of strings, e.g. ["问题1", "问题2", ...]
/// Also accepts plain text with numbered items (1. 问题 / - 问题).
fn parse_followup_response(response: &str) -> Vec<String> {
    let trimmed = response.trim();

    // Try JSON array first
    if trimmed.starts_with('[') {
        if let Ok(parsed) = serde_json::from_str::<Vec<String>>(trimmed) {
            return parsed;
        }
        // Try stripping markdown fences
        let cleaned = trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        if let Ok(parsed) = serde_json::from_str::<Vec<String>>(cleaned) {
            return parsed;
        }
    }

    // Fallback: parse numbered/bullet list
    let mut questions = Vec::new();
    for line in trimmed.lines() {
        let line = line.trim();
        // Strip "1. " or "- " or "* " prefixes
        let cleaned = line
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .trim_start_matches('.')
            .trim_start_matches('-')
            .trim_start_matches('*')
            .trim();
        if !cleaned.is_empty() && cleaned.len() > 5 {
            questions.push(cleaned.to_string());
        }
    }

    questions
}

// ─── Internal helpers for T3 ───
/// Find a matching question from the indexer for context enrichment.
fn find_matching_question(
    question_text: &str,
    edition: &Edition,
    indexer: &ResearchIndexer,
) -> Option<FlatQuestion> {
    // Get all questions for this edition, then find best text match
    let all_questions = indexer
        .get_questions_by_edition(edition)
        .unwrap_or_default();

    let mut best_match: Option<FlatQuestion> = None;
    let mut best_score: f32 = 0.0;

    for fq in &all_questions {
        let sim = text_similarity(question_text, &fq.question_text);
        if sim > best_score && sim > 0.3 {
            best_score = sim;
            best_match = Some(fq.clone());
        }
    }

    best_match
}

/// Search KB and assemble context for smart-fill.
/// Returns (search_results, kb_sources) — search_results for prompt building,
/// kb_sources for citation display.
fn assemble_kb_context_for_fill(
    question_text: &str,
    project_name: Option<&str>,
    top_k: usize,
    embedding: &Mutex<EmbeddingService>,
    vector_index: &Mutex<VectorIndex>,
    bm25: &Mutex<BM25Service>,
    metadata: &Mutex<MetadataStore>,
) -> (Vec<HybridSearchResult>, Vec<KBSource>) {
    let search_results = hybrid_search::hybrid_search(
        question_text,
        project_name,
        &[],
        top_k,
        embedding,
        vector_index,
        bm25,
        metadata,
    )
    .unwrap_or_default(); // KB search failure is non-fatal

    let kb_sources: Vec<KBSource> = search_results
        .iter()
        .map(|r| KBSource {
            title: r.title.clone(),
            section_path: r.section_path.clone(),
            content_snippet: truncate_str(&r.content, 200),
            score: r.score,
        })
        .collect();

    (search_results, kb_sources)
}

/// Truncate a snippet to a maximum character length（迁移期间预留）.
#[allow(dead_code)]
fn truncate_snippet(text: &str, max_chars: usize) -> String {
    truncate_str(text, max_chars)
}
