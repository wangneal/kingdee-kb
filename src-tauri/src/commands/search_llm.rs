use tauri::State;

use crate::app_state::AppState;
use crate::services::bm25_service::BM25SearchResult;
use crate::services::embedding::EmbeddingService;
use crate::services::hybrid_search::HybridSearchResult;
use crate::services::llm_service::{ChatMessage, LLMConfig};
use crate::services::memory;

/// 确保 embedding 模型已加载（懒加载）。
/// 如果模型未加载，尝试从 ModelManager 初始化。
fn ensure_embedding_ready(
    embedding: &std::sync::Mutex<EmbeddingService>,
    model_manager: &std::sync::Mutex<crate::services::embedding::ModelManager>,
) {
    let emb = embedding.lock().unwrap();
    if emb.is_ready() {
        return; // 已加载
    }
    drop(emb);

    // 尝试从 ModelManager 获取模型
    let mut mm = match model_manager.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    if !mm.is_ready() {
        if let Err(e) = mm.init() {
            eprintln!("[LazyLoad] Model init failed: {}", e);
            return;
        }
    }
    if let Some(model) = mm.take_model() {
        let mut emb = embedding.lock().unwrap();
        emb.set_model(model);
        println!("[LazyLoad] Embedding model loaded on first use!");
    }
}

/// 使用 BM25 按关键词搜索分块（jieba 分词 + tantivy 评分）
#[tauri::command]
pub async fn bm25_search(
    state: State<'_, AppState>,
    query: String,
    project_id: Option<String>,
    top_k: Option<u32>,
) -> Result<Vec<BM25SearchResult>, String> {
    let bm25 = state.bm25.lock().map_err(|e| e.to_string())?;
    bm25.search(&query, project_id.as_deref(), top_k.unwrap_or(10))
}

/// 混合搜索：向量 + BM25 通过 RRFR 融合（k=60, final top_k=5）
///
/// 首次调用时会自动加载 embedding 模型（懒加载）。
#[tauri::command]
pub async fn hybrid_search(
    state: State<'_, AppState>,
    query: String,
    project_id: Option<String>,
    top_k: Option<usize>,
) -> Result<Vec<HybridSearchResult>, String> {
    // 懒加载 embedding 模型
    ensure_embedding_ready(&state.embedding, &state.model_manager);

    crate::services::hybrid_search::hybrid_search(
        &query,
        project_id.as_deref(),
        top_k.unwrap_or(5),
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
}

/// 配置 LLM 提供商（API 密钥、基础 URL、模型等）
#[tauri::command]
pub async fn set_llm_config(state: State<'_, AppState>, config: LLMConfig) -> Result<(), String> {
    state.llm.set_config(config)
}

/// 获取当前 LLM 配置（API 密钥已脱敏）
#[tauri::command]
pub async fn get_llm_config(state: State<'_, AppState>) -> Result<LLMConfig, String> {
    let mut config = state.llm.get_config()?;
    let key_len = config.api_key.len();
    if key_len > 10 {
        config.api_key = format!(
            "{}...{}",
            &config.api_key[..3],
            &config.api_key[key_len - 3..]
        );
    } else if key_len > 0 {
        config.api_key = "****".to_string();
    }
    Ok(config)
}

/// 检查 LLM 是否已配置（有 API 密钥）
#[tauri::command]
pub async fn is_llm_configured(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.llm.is_configured())
}

/// 测试 LLM API 连通性
#[tauri::command]
pub async fn test_llm_connection(state: State<'_, AppState>) -> Result<String, String> {
    state.llm.test_connection().await
}

/// 保存聊天记忆：归档对话 + LLM 提取 → 摄入知识库。
#[tauri::command]
pub async fn save_chat_memory(
    state: State<'_, AppState>,
    conversation: Vec<ChatMessage>,
    project: Option<String>,
) -> Result<(), String> {
    let data_dir = dirs::home_dir()
        .ok_or("Cannot find home directory")?
        .join(".kingdee-kb");

    let llm = state.llm.clone();
    let embedding = state.embedding.clone();
    let vector_index = state.vector_index.clone();
    let metadata = state.metadata.clone();

    let bm25 = state.bm25.clone();

    tokio::spawn(async move {
        memory::save_chat_memory(
            &conversation,
            &data_dir,
            &llm,
            &embedding,
            &vector_index,
            &metadata,
            &bm25,
            project.as_deref(),
        )
        .await;
    });

    Ok(())
}

/// 统计文本中的 token 数量
#[tauri::command]
pub async fn count_tokens(text: String) -> Result<u32, String> {
    Ok(crate::services::llm_service::count_tokens(&text))
}
