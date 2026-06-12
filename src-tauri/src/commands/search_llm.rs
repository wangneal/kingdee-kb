use tauri::State;

use crate::app_state::AppState;
use crate::services::bm25_service::BM25SearchResult;
use crate::services::hybrid_search::HybridSearchResult;
use crate::services::llm_service::ChatMessage;
use crate::services::memory;

/// 使用 BM25 按关键词搜索分块（jieba 分词 + tantivy 评分）
#[tauri::command]
pub async fn bm25_search(
    state: State<'_, AppState>,
    query: String,
    project_id: Option<i64>,
    top_k: Option<u32>,
) -> Result<Vec<BM25SearchResult>, String> {
    state.get_or_init_bm25()?;
    let bm25 = state.bm25.read().map_err(|e| e.to_string())?;
    let project_id = project_id.map(|id| id.to_string());
    bm25.search(&query, project_id.as_deref(), &[], top_k.unwrap_or(10), &[])
}

/// 混合搜索：向量 + BM25 通过 RRFR 融合（k=60, final top_k=5）
///
/// 首次调用时会自动加载 embedding 模型（懒加载）。
#[tauri::command]
pub async fn hybrid_search(
    state: State<'_, AppState>,
    query: String,
    project_id: Option<i64>,
    top_k: Option<usize>,
) -> Result<Vec<HybridSearchResult>, String> {
    state.ensure_embedding_ready();
    state.get_or_init_bm25()?;
    let project_id = project_id.map(|id| id.to_string());

    crate::services::hybrid_search::hybrid_search(
        &query,
        project_id.as_deref(),
        &[],
        top_k.unwrap_or(5),
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
        state.get_or_init_reranker().as_deref(),
        Some(&state.wiki_pages),
    )
}

/// 保存聊天记忆：归档对话 + LLM 提取 → 摄入知识库。
#[tauri::command]
pub async fn save_chat_memory(
    state: State<'_, AppState>,
    conversation: Vec<ChatMessage>,
    project_id: Option<i64>,
) -> Result<(), String> {
    state.get_or_init_bm25()?;
    let data_dir = dirs::home_dir()
        .ok_or("Cannot find home directory")?
        .join(".kingdee-kb");

    let llm = state.llm.clone();
    let embedding = state.embedding.clone();
    let vector_index = state.vector_index.clone();
    let metadata = state.metadata.clone();

    let bm25 = state.bm25.clone();
    let resolved_project_id = match project_id {
        Some(id) => id,
        None => {
            let store = state.project_store.lock().map_err(|e| e.to_string())?;
            store.ensure_default_project()?
        }
    };

    tokio::spawn(async move {
        memory::save_chat_memory(
            &conversation,
            &data_dir,
            &llm,
            &embedding,
            &vector_index,
            &metadata,
            &bm25,
            resolved_project_id,
        )
        .await;
    });

    Ok(())
}

/// 统计文本中的 token 数量
#[tauri::command]
pub async fn count_tokens(text: String) -> Result<u32, String> {
    Ok(crate::services::token::count_tokens_with_fallback(&text))
}
