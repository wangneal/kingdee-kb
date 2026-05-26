use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::services::bm25_service::BM25SearchResult;
use crate::services::hybrid_search::HybridSearchResult;
use crate::services::llm_service::{ChatMessage, LLMConfig, RAGResponse, RAGSource, StreamChunk};
use crate::services::memory;
use crate::services::metadata::MetadataStore;

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
#[tauri::command]
pub async fn hybrid_search(
    state: State<'_, AppState>,
    query: String,
    project_id: Option<String>,
    top_k: Option<usize>,
) -> Result<Vec<HybridSearchResult>, String> {
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
pub async fn set_llm_config(
    state: State<'_, AppState>,
    config: LLMConfig,
) -> Result<(), String> {
    state.llm.set_config(config)
}

/// 获取当前 LLM 配置（API 密钥已脱敏）
#[tauri::command]
pub async fn get_llm_config(
    state: State<'_, AppState>,
) -> Result<LLMConfig, String> {
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
pub async fn is_llm_configured(
    state: State<'_, AppState>,
) -> Result<bool, String> {
    Ok(state.llm.is_configured())
}

/// 测试 LLM API 连通性
#[tauri::command]
pub async fn test_llm_connection(
    state: State<'_, AppState>,
) -> Result<String, String> {
    state.llm.test_connection().await
}

/// RAG 查询：混合搜索 → 上下文组装 → LLM 流式补全。
#[tauri::command]
pub async fn rag_query(
    state: State<'_, AppState>,
    query: String,
    project_id: Option<String>,
    conversation_history: Option<Vec<ChatMessage>>,
) -> Result<RAGResponse, String> {
    let history = conversation_history.unwrap_or_default();
    state
        .llm
        .rag_query_sync(
            &query,
            project_id.as_deref(),
            history,
            &state.embedding,
            &state.vector_index,
            &state.bm25,
            &state.metadata,
        )
        .await
}

/// RAG 流式查询：增量返回分块。
#[tauri::command]
pub async fn rag_query_stream(
    state: State<'_, AppState>,
    query: String,
    project_id: Option<String>,
    conversation_history: Option<Vec<ChatMessage>>,
) -> Result<Vec<StreamChunk>, String> {
    let history = conversation_history.unwrap_or_default();
    state
        .llm
        .rag_query(
            &query,
            project_id.as_deref(),
            history,
            &state.embedding,
            &state.vector_index,
            &state.bm25,
            &state.metadata,
        )
        .await
}

/// 通过 Tauri 事件启动实时流式聊天会话。
#[tauri::command]
pub async fn start_chat_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    query: String,
    project_id: Option<String>,
    conversation_history: Option<Vec<ChatMessage>>,
) -> Result<(), String> {
    let history = conversation_history.unwrap_or_default();

    let embedding = state.embedding.clone();
    let vector_index = state.vector_index.clone();
    let bm25 = state.bm25.clone();
    let metadata = state.metadata.clone();
    let llm = state.llm.clone();
    let pid = project_id.clone();
    let q = query.clone();

    // 步骤 1: 预先运行 hybrid_search
    let search_results = crate::services::hybrid_search::hybrid_search(
        &q,
        pid.as_deref(),
        5,
        &*embedding,
        &*vector_index,
        &*bm25,
        &*metadata,
    )?;

    let sources: Vec<RAGSource> = search_results
        .iter()
        .map(|r| RAGSource {
            title: r.title.clone(),
            section_path: r.section_path.clone(),
            content_snippet: crate::services::llm_service::truncate_to_tokens(&r.content, 100),
            score: r.score,
        })
        .collect();

    // 步骤 2: 检查 LLM 配置
    if !llm.is_configured() {
        let answer = llm.fallback_response(&search_results);
        let content: String = answer.iter().map(|c| c.content.as_str()).collect();
        let sources_clone = sources.clone();
        tokio::spawn(async move {
            use tauri::Emitter;
            if !content.is_empty() {
                let _ = app.emit(
                    "chat_chunk",
                    serde_json::json!({"type": "text_delta", "content": content}),
                );
            }
            if !sources_clone.is_empty() {
                let _ = app.emit(
                    "chat_chunk",
                    serde_json::json!({"type": "sources", "sources": sources_clone}),
                );
            }
            let _ = app.emit("chat_chunk", serde_json::json!({"type": "done"}));
        });
        return Ok(());
    }

    // 步骤 3: 流式传输通道
    let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamChunk>(64);

    let llm_clone = llm.clone();
    tokio::spawn(async move {
        let _ = llm_clone
            .rag_query_to_sender(
                &q,
                pid.as_deref(),
                history,
                &*embedding,
                &*vector_index,
                &*bm25,
                &*metadata,
                tx,
                Some(search_results),
            )
            .await;
    });

    tokio::spawn(async move {
        use tauri::Emitter;
        while let Some(chunk) = rx.recv().await {
            if chunk.done {
                break;
            }
            if let Some(thinking) = &chunk.thinking {
                if !thinking.is_empty() {
                    let _ = app.emit(
                        "chat_chunk",
                        serde_json::json!({"type": "thinking", "content": thinking}),
                    );
                }
            }
            if !chunk.content.is_empty() {
                let _ = app.emit(
                    "chat_chunk",
                    serde_json::json!({"type": "text_delta", "content": chunk.content}),
                );
            }
        }
        if !sources.is_empty() {
            let _ = app.emit(
                "chat_chunk",
                serde_json::json!({"type": "sources", "sources": sources}),
            );
        }
        let _ = app.emit("chat_chunk", serde_json::json!({"type": "done"}));
    });

    Ok(())
}

/// 保存聊天记忆：归档对话 + LLM 提取 → 摄入知识库。
#[tauri::command]
pub async fn save_chat_memory(
    state: State<'_, AppState>,
    conversation: Vec<ChatMessage>,
) -> Result<(), String> {
    let data_dir = dirs::home_dir()
        .ok_or("Cannot find home directory")?
        .join(".kingdee-kb");

    let llm = state.llm.clone();
    let embedding = state.embedding.clone();
    let vector_index = state.vector_index.clone();
    let metadata = state.metadata.clone();

    tokio::spawn(async move {
        memory::save_chat_memory(
            &conversation,
            &data_dir,
            &llm,
            &embedding,
            &vector_index,
            &metadata,
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
