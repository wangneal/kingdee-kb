pub mod citation;
pub mod consistency;
pub mod contradiction;
pub mod pipeline;
pub mod self_consistency;
pub mod types;
pub mod uncertainty;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, LazyLock};
use crate::services::hybrid_search::HybridSearchResult;

// 全局会话级别的 RAG 检索缓存，键为 session_id
pub static SESSION_RAG_CACHE: LazyLock<Arc<Mutex<HashMap<String, Vec<HybridSearchResult>>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

/// 将检索结果追加存入会话 RAG 缓存
pub fn append_session_rag_results(session_id: &str, results: &[HybridSearchResult]) {
    if let Ok(mut cache) = SESSION_RAG_CACHE.lock() {
        let entry = cache.entry(session_id.to_string()).or_default();
        for r in results {
            if !entry.iter().any(|existing| existing.chunk_id == r.chunk_id) {
                entry.push(r.clone());
            }
        }
    }
}

/// 获取当前会话缓存的所有检索结果
pub fn get_session_rag_results(session_id: &str) -> Vec<HybridSearchResult> {
    if let Ok(cache) = SESSION_RAG_CACHE.lock() {
        cache.get(session_id).cloned().unwrap_or_default()
    } else {
        Vec::new()
    }
}

/// 清空指定会话的 RAG 缓存
pub fn clear_session_rag_results(session_id: &str) {
    if let Ok(mut cache) = SESSION_RAG_CACHE.lock() {
        cache.remove(session_id);
    }
}
