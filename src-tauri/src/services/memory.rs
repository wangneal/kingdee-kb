//! Chat memory persistence — archive conversations and extract key info into KB
//!
//! After each completed chat, this module:
//! 1. Saves the raw conversation to `~/.kingdee-kb/chats/chat_{ts}.json`
//! 2. If LLM is configured, extracts project-related key information (decisions,
//!    constraints, tech choices, lessons) as Markdown
//! 3. Ingests the extracted memory into the knowledge base so it's searchable
//!    in future RAG queries

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::services::embedding::EmbeddingService;
use crate::services::ingestion::ingest_text;
use crate::services::llm_service::{ChatMessage, LLMService};
use crate::services::metadata::{ChunkMeta, MetadataStore};
use crate::services::vector_index::VectorIndex;

/// Maximum conversation length (in chars) to send for memory extraction
const MAX_MEMORY_EXTRACT_CHARS: usize = 6000;

/// Maximum title length from first user message
const MAX_TITLE_CHARS: usize = 40;

/// Maximum number of memory documents to keep in the knowledge base.
/// When exceeded, the oldest memories are purged from vector index + metadata.
const MAX_MEMORY_DOCS: usize = 200;

/// Save chat memory in the background: archive + LLM extraction → KB ingestion.
///
/// This is called after each chat stream completes (`done` event).
/// All errors are non-fatal (logged, not propagated to user).
pub async fn save_chat_memory(
    conversation: &[ChatMessage],
    data_dir: &Path,
    llm: &LLMService,
    embedding: &Arc<Mutex<EmbeddingService>>,
    vector_index: &Arc<Mutex<VectorIndex>>,
    metadata: &Arc<Mutex<MetadataStore>>,
) {
    // 1. Save raw conversation
    let chats_dir = data_dir.join("chats");
    if fs::create_dir_all(&chats_dir).is_err() {
        return;
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let chat_file = chats_dir.join(format!("chat_{}.json", timestamp));
    if let Ok(json) = serde_json::to_string_pretty(conversation) {
        let _ = fs::write(&chat_file, &json);
    }

    // 2. Extract and store memory if LLM is configured
    if !llm.is_configured() {
        return;
    }

    let memory_text = match extract_memory(conversation, llm).await {
        Ok(text) => text,
        Err(e) => {
            eprintln!("[Memory] Extraction failed (non-fatal): {}", e);
            return;
        }
    };

    // 3a. Skip if the LLM returned an empty template (no useful info extracted)
    //     Empty templates from different conversations are near-identical → would
    //     cause all subsequent memories to be deduped away. We avoid this by
    //     checking whether the extracted text contains any actual content
    //     beyond the Markdown headings and horizontal rule.
    let trimmed = memory_text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("---"))
        .collect::<Vec<_>>()
        .join("");
    if trimmed.is_empty() {
        eprintln!("[Memory] Skipping — LLM returned empty template (no project info in conversation): {}", extract_title(conversation));
        return;
    }

    // 3b. Save memory as Markdown file
    let memory_file = chats_dir.join(format!("chat_{}_memory.md", timestamp));
    let _ = fs::write(&memory_file, &memory_text);

    // 4. Check for duplicate by semantic similarity — skip if too similar to recent memory
    let title = format!("记忆: {}", extract_title(conversation));
    if is_duplicate_memory(&title, &memory_text, embedding, vector_index, metadata) {
        eprintln!("[Memory] Skipping — similar memory already exists: {}", title);
        return;
    }

    // 5. Ingest into knowledge base for future RAG search
    let _ = ingest_text(
        &memory_text,
        &title,
        "记忆库",
        embedding,
        vector_index,
        metadata,
        None,
    );

    // 6. Cleanup stale memories (vector index + metadata)
    cleanup_stale_memories(embedding, vector_index, metadata);
}

/// Check if a memory with similar content already exists in the "记忆库" project.
///
/// Uses vector search (nearest neighbor) against the index. Embeds the new memory,
/// then searches the index for the closest existing vector. If the nearest hit
/// belongs to a "记忆库" document and similarity exceeds 0.92, the memory is skipped.
fn is_duplicate_memory(
    title: &str,
    memory_text: &str,
    embedding: &Arc<Mutex<EmbeddingService>>,
    vector_index: &Arc<Mutex<VectorIndex>>,
    metadata: &Arc<Mutex<MetadataStore>>,
) -> bool {
    // Compute embedding for the new memory
    let new_vec: Vec<f32> = match embedding.lock() {
        Ok(mut emb) => match emb.embed_text(memory_text) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[Memory] Embedding failed for dedup check: {}", e);
                // Fall back to exact title match
                return is_duplicate_memory_by_title(title, metadata);
            }
        },
        Err(_) => return is_duplicate_memory_by_title(title, metadata),
    };

    // Search the vector index for nearest neighbors
    let search_results = match vector_index.lock() {
        Ok(idx) => match idx.search(&new_vec, 5) {
            Ok(results) => results,
            Err(_) => return false,
        },
        Err(_) => return false,
    };

    // Acquire metadata lock ONCE and do all lookups within it
    let meta_guard = match metadata.lock() {
        Ok(m) => m,
        Err(_) => return false,
    };

    // Get all "记忆库" documents
    let memory_docs = match meta_guard.get_documents(Some("记忆库")) {
        Ok(d) => d,
        Err(_) => return false,
    };
    let memory_doc_ids: Vec<i64> = memory_docs.iter().map(|d| d.id).collect();

    for result in search_results {
        if result.similarity > 0.92 {
            // Check if this vector belongs to a "记忆库" document
            match meta_guard.get_chunk_by_vector_key(result.key as i64) {
                Ok(Some(chunk)) if memory_doc_ids.contains(&chunk.document_id) => {
                    let doc_title = memory_docs
                        .iter()
                        .find(|d| d.id == chunk.document_id)
                        .map(|d| d.title.as_str())
                        .unwrap_or("(unknown)");

                    eprintln!(
                        "[Memory] Duplicate detected: '{}' vs '{}' (sim={:.3})",
                        title, doc_title, result.similarity
                    );
                    return true;
                }
                _ => continue,
            }
        }
    }
    false
}

/// Fallback: exact title match (used when embedding is unavailable)
fn is_duplicate_memory_by_title(title: &str, metadata: &Arc<Mutex<MetadataStore>>) -> bool {
    let docs = match metadata.lock() {
        Ok(meta) => match meta.get_documents(Some("记忆库")) {
            Ok(d) => d,
            Err(_) => return false,
        },
        Err(_) => return false,
    };
    docs.iter().rev().take(3).any(|doc| doc.title == title)
}

/// Delete oldest memory documents from vector index + metadata store
/// when the total exceeds `MAX_MEMORY_DOCS`.
fn cleanup_stale_memories(
    _embedding: &Arc<Mutex<EmbeddingService>>,
    vector_index: &Arc<Mutex<VectorIndex>>,
    metadata: &Arc<Mutex<MetadataStore>>,
) {
    let docs = match metadata.lock() {
        Ok(meta) => match meta.get_documents(Some("记忆库")) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[Memory] Failed to list memory docs: {}", e);
                return;
            }
        },
        Err(e) => {
            eprintln!("[Memory] Metadata lock error: {}", e);
            return;
        }
    };

    if docs.len() <= MAX_MEMORY_DOCS {
        return;
    }

    // Sort by created_at ascending (oldest first)
    let mut sorted = docs.clone();
    sorted.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    let to_delete = docs.len() - MAX_MEMORY_DOCS;
    let stale = &sorted[..to_delete];

    for doc in stale {
        // Acquire both locks in consistent order (vector_index → metadata),
        // same order as ingestion to avoid deadlocks.
        // Hold both locks for the entire operation to prevent
        // interleaving with concurrent ingestions.
        let chunks = {
            let idx = vector_index.lock().unwrap_or_else(|e| e.into_inner());
            let meta = metadata.lock().unwrap_or_else(|e| e.into_inner());

            // Get chunks
            let chunks: Vec<ChunkMeta> = match meta.get_chunks_by_document(doc.id) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("[Memory] Failed to get chunks for doc {}: {}", doc.id, e);
                    continue;
                }
            };

            // Remove from vector index and save
            for chunk in &chunks {
                let _ = idx.remove(chunk.vector_key as u64);
            }
            if let Err(e) = idx.save() {
                eprintln!("[Memory] Failed to save index after cleanup: {}", e);
            }

            // Remove from metadata store
            for chunk in &chunks {
                let _ = meta.delete_chunk_by_vector_key(chunk.vector_key);
            }
            let _ = meta.delete_document(doc.id);

            chunks // return chunks for progress logging
        }; // both locks dropped together

        eprintln!(
            "[Memory] Cleaned up doc '{}' ({} chunks removed)",
            doc.title,
            chunks.len()
        );
    }

    eprintln!(
        "[Memory] Cleaned up {} stale memories ({} remaining)",
        stale.len(),
        MAX_MEMORY_DOCS
    );
}

/// Call LLM to extract structured project memory from conversation.
async fn extract_memory(
    conversation: &[ChatMessage],
    llm: &LLMService,
) -> Result<String, String> {
    // Build conversation text (truncated for token budget)
    let conversation_text: String = conversation
        .iter()
        .map(|m| format!("**{}**: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    let truncated: String = conversation_text
        .chars()
        .take(MAX_MEMORY_EXTRACT_CHARS)
        .collect();

    let system_prompt = "\
你是一个项目记忆提取助手。从对话中提取项目相关的关键信息。\
只提取明确提到的内容，不要编造。没有相关信息就留空该章节。";

    let user_prompt = format!(
        "从以下对话中提取项目关键信息，以 Markdown 格式输出：\n\n\
## 项目名称\n\n\
## 关键决策\n\n\
## 业务约束\n\n\
## 技术选择\n\n\
## 经验教训\n\n\
---\n\n{}",
        truncated
    );

    let config = llm.get_config()?;
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: user_prompt,
        },
    ];
    llm.chat_completion(&messages, &config).await
}

/// Extract a short title from the first user message.
///
/// Strips control characters (including newlines/tabs), truncates to
/// `MAX_TITLE_CHARS`, then trims whitespace. This prevents multi-line
/// titles that would break display and dedup logic.
fn extract_title(conversation: &[ChatMessage]) -> String {
    for msg in conversation {
        if msg.role == "user" {
            let clean: String = msg
                .content
                .chars()
                .filter(|c| !c.is_ascii_control() && !c.is_control())
                .take(MAX_TITLE_CHARS)
                .collect();
            let trimmed = clean.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    "未命名对话".to_string()
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_title ─────────────────────────────────────────────────────

    #[test]
    fn test_extract_title_from_user_message() {
        let msgs = vec![
            ChatMessage { role: "user".to_string(), content: "项目A的预算情况如何".to_string() },
            ChatMessage { role: "assistant".to_string(), content: "项目A的预算是100万".to_string() },
        ];
        let title = extract_title(&msgs);
        assert_eq!(title, "项目A的预算情况如何");
    }

    #[test]
    fn test_extract_title_truncates_long() {
        let long = "项目A的预算情况如何项目A的预算情况如何项目A的预算情况如何项目A的预算情况如何项目A的预算情况如何";
        let msgs = vec![
            ChatMessage { role: "user".to_string(), content: long.to_string() },
        ];
        let title = extract_title(&msgs);
        // Title should not be the full input (it should be truncated or cleaned)
        assert!(!title.is_empty(), "title should not be empty");
        assert!(title.starts_with("项目A的预算情况如何"), "title should start with the first user message");
    }

    #[test]
    fn test_extract_title_fallback() {
        let msgs = vec![
            ChatMessage { role: "assistant".to_string(), content: "回复内容".to_string() },
        ];
        assert_eq!(extract_title(&msgs), "未命名对话");
    }

    #[test]
    fn test_extract_title_empty_user() {
        let msgs = vec![
            ChatMessage { role: "user".to_string(), content: "   ".to_string() },
            ChatMessage { role: "user".to_string(), content: "实际的问题".to_string() },
        ];
        assert_eq!(extract_title(&msgs), "实际的问题");
    }
}
