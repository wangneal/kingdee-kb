#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

mod app_state;
mod services;

/// 递归复制目录及其所有内容
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("Failed to create dir {}: {}", dst.display(), e))?;
    for entry in fs::read_dir(src).map_err(|e| format!("Failed to read dir {}: {}", src.display(), e))? {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("Failed to copy {} to {}: {}", src_path.display(), dst_path.display(), e))?;
        }
    }
    Ok(())
}

use app_state::AppState;
use services::bm25_service::BM25SearchResult;
use services::deliverable_recipes::DeliverableRecipe;
use services::doc_generator::{GeneratedDoc, GenerateDocRequest, RecipeDocRequest, RecipeDocResult};
use services::embedding::{start_download_progress_polling, EmbeddingModelConfig};
use services::hybrid_search::HybridSearchResult;
use services::ingestion::{DirectoryIngestionResult, IngestionResult, ingest_text as ingest_text_fn, ingest_file as ingest_file_fn, ingest_directory as ingest_directory_fn};
use services::llm_service::{ChatMessage, LLMConfig, RAGResponse, RAGSource, StreamChunk};
use services::memory;
use services::metadata::{ChunkMeta, DocumentMeta, KnowledgeStats};
use services::product_store::ProductMeta;
use services::research_outline::Edition;
use services::smart_completion::{SmartFillRequest, SmartFillResult};
use services::question_recommend::{RecommendRequest, RecommendedQuestion, FollowUpRequest, FollowUpResult};
use services::research_session::{ResearchSession, QARecord, SessionDetail};
use services::risk_control::{ContractScopeItem, ScopeCreepResult, ProjectHealthScore, DefenseScriptRequest, DefenseScriptResult};
use services::template_docx::FieldInfo;
use services::template_scanner::TemplateInfo;
use services::template_schema::TemplateSchema;
use services::vector_index::SearchResult;
use services::whisper_service::{TranscriptionResult, WhisperStatus};
use services::video_transcriber::{VideoTranscriptionResult, VideoPipelineResult, MeetingMinutesResult};
use services::model_downloader;

const KEYRING_SERVICE: &str = "com.neal.kingdee-kb";

/// 跟踪启动任务完成状态，用于关闭启动画面
struct SetupState {
    frontend_task: bool,
    backend_task: bool,
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// 确保 ~/.kingdee-kb/ 数据目录结构存在
fn ensure_data_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    let data_dir = home.join(".kingdee-kb");

    let subdirs = ["knowledge", "index", "models", "bm25_index", "products"];
    for subdir in subdirs {
        fs::create_dir_all(data_dir.join(subdir))
            .map_err(|e| format!("Failed to create {}: {}", subdir, e))?;
    }

    Ok(data_dir)
}

/// 获取数据目录路径（供前端使用）
#[tauri::command]
fn get_data_dir() -> Result<String, String> {
    let data_dir = ensure_data_dir()?;
    Ok(data_dir.to_string_lossy().to_string())
}

/// 存储 API 密钥到系统凭据存储
#[tauri::command]
fn set_api_key(service: String, key: String) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &service)
        .map_err(|e| format!("Failed to create keyring entry: {}", e))?;
    entry
        .set_password(&key)
        .map_err(|e| format!("Failed to store API key: {}", e))?;
    Ok(())
}

/// 从系统凭据存储获取 API 密钥
#[tauri::command]
fn get_api_key(service: String) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &service)
        .map_err(|e| format!("Failed to access keyring: {}", e))?;
    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Failed to retrieve API key: {}", e)),
    }
}

/// 从系统凭据存储删除 API 密钥
#[tauri::command]
fn delete_api_key(service: String) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &service)
        .map_err(|e| format!("Failed to access keyring: {}", e))?;
    entry
        .delete_credential()
        .map_err(|e| format!("Failed to delete API key: {}", e))?;
    Ok(())
}

/// 前端 React 挂载完成后的回调
#[tauri::command]
async fn set_complete(
    app: AppHandle,
    state: State<'_, Mutex<SetupState>>,
    task: String,
) -> Result<(), String> {
    let mut state_lock = state.lock().map_err(|e| e.to_string())?;
    match task.as_str() {
        "frontend" => state_lock.frontend_task = true,
        "backend" => state_lock.backend_task = true,
        _ => return Err(format!("invalid task: {}", task)),
    }

    if state_lock.frontend_task && state_lock.backend_task {
        if let Some(splash_window) = app.get_webview_window("splashscreen") {
            let _ = splash_window.close();
        }
        if let Some(main_window) = app.get_webview_window("main") {
            let _ = main_window.show();
            let _ = main_window.set_focus();
        }
    }

    Ok(())
}

// ─── 阶段 2: 嵌入模型与向量存储命令 ───

/// 获取当前模型状态（就绪/未就绪）。
/// 检查持有实际模型实例的 EmbeddingService。
#[tauri::command]
async fn get_model_status(
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let emb = state.embedding.lock().map_err(|e| e.to_string())?;
    Ok(emb.is_ready())
}

/// 初始化嵌入模型（首次调用时下载）。
/// 初始化后，将模型转移到 EmbeddingService，
/// 以便 RAG 查询可以使用向量搜索。
#[tauri::command]
async fn init_model(
    state: State<'_, AppState>,
) -> Result<bool, String> {
    // 启动进度轮询，以便前端显示下载进度条
    let download_progress = state.download_progress.clone();
    download_progress.store(0, Ordering::Relaxed);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    start_download_progress_polling(
        &fastembed::EmbeddingModel::BGESmallZHV15,
        download_progress.clone(),
        stop,
    );

    // 步骤 1: 在 ModelManager 中初始化模型（可能从 HuggingFace 下载）
    let result = {
        let mut mm = state.model_manager.lock().map_err(|e| e.to_string())?;
        mm.init()
    };

    // 通知轮询线程停止
    stop_clone.store(true, Ordering::Relaxed);

    match result {
        Ok(()) => {
            download_progress.store(100, Ordering::Relaxed);
            let model = {
                let mut mm = state.model_manager.lock().map_err(|e| e.to_string())?;
                mm.take_model().ok_or("Model initialized but no model returned")?
            };
            // 注入到 EmbeddingService
            let mut emb = state.embedding.lock().map_err(|e| e.to_string())?;
            emb.set_model(model);
            Ok(true)
        }
        Err(e) => {
            download_progress.store(0, Ordering::Relaxed);
            Err(e)
        }
    }
}

/// 获取嵌入模型的下载进度（0–100）。
#[tauri::command]
async fn get_download_progress(
    state: State<'_, AppState>,
) -> Result<u32, String> {
    Ok(state.download_progress.load(Ordering::Relaxed))
}

/// 嵌入单个文本 — 返回 512 维向量
#[tauri::command]
async fn get_embedding_model_config(
    state: State<'_, AppState>,
) -> Result<EmbeddingModelConfig, String> {
    let mm = state.model_manager.lock().map_err(|e| e.to_string())?;
    Ok(mm.embedding_config())
}

#[tauri::command]
async fn set_embedding_model_config(
    state: State<'_, AppState>,
    custom_model_dir: Option<String>,
) -> Result<bool, String> {
    {
        let mut mm = state.model_manager.lock().map_err(|e| e.to_string())?;
        mm.set_custom_model_dir(custom_model_dir)?;
        mm.init()?;
    }

    let model = {
        let mut mm = state.model_manager.lock().map_err(|e| e.to_string())?;
        mm.take_model().ok_or("Model initialized but no model returned")?
    };
    let mut emb = state.embedding.lock().map_err(|e| e.to_string())?;
    emb.set_model(model);
    Ok(true)
}

#[tauri::command]
async fn embed_text(
    state: State<'_, AppState>,
    text: String,
) -> Result<Vec<f32>, String> {
    let mut emb = state.embedding.lock().map_err(|e| e.to_string())?;
    emb.embed_text(&text)
}

/// 批量嵌入多个文本
#[tauri::command]
async fn embed_batch(
    state: State<'_, AppState>,
    texts: Vec<String>,
) -> Result<Vec<Vec<f32>>, String> {
    let mut emb = state.embedding.lock().map_err(|e| e.to_string())?;
    let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
    emb.embed_batch(&refs)
}

/// 在 HNSW 索引中搜索相似向量
#[tauri::command]
async fn search_similar(
    state: State<'_, AppState>,
    query: Vec<f32>,
    top_k: u32,
) -> Result<Vec<SearchResult>, String> {
    let index = state.vector_index.lock().map_err(|e| e.to_string())?;
    index.search(&query, top_k as usize)
}

/// 从磁盘加载向量索引
#[tauri::command]
async fn load_index(
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let index = state.vector_index.lock().map_err(|e| e.to_string())?;
    Ok(index.len())
}

/// 获取向量索引统计信息
#[tauri::command]
async fn get_index_stats(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let index = state.vector_index.lock().map_err(|e| e.to_string())?;
    let stats = index.stats();
    serde_json::to_value(stats).map_err(|e| format!("Serialization error: {}", e))
}

/// 获取知识库统计信息（文档和分块数量）
#[tauri::command]
async fn get_knowledge_stats(
    state: State<'_, AppState>,
) -> Result<KnowledgeStats, String> {
    let meta = state.metadata.lock().map_err(|e| e.to_string())?;
    meta.get_stats()
}

// ─── 阶段 3: 知识摄入管道命令 ───

/// 摄入纯文本（来自粘贴或文本框）
#[tauri::command]
async fn ingest_text(
    state: State<'_, AppState>,
    app: AppHandle,
    text: String,
    title: String,
    project: String,
) -> Result<IngestionResult, String> {
    ingest_text_fn(
        &text,
        &title,
        &project,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        Some(&app),
    )
}

/// 摄入单个文件
#[tauri::command]
async fn ingest_file(
    state: State<'_, AppState>,
    app: AppHandle,
    file_path: String,
    project: String,
) -> Result<IngestionResult, String> {
    ingest_file_fn(
        PathBuf::from(&file_path).as_path(),
        &project,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        Some(&app),
    )
}

/// 摄入目录中的所有支持文件
#[tauri::command]
async fn ingest_directory(
    state: State<'_, AppState>,
    app: AppHandle,
    dir_path: String,
    project: String,
) -> Result<DirectoryIngestionResult, String> {
    ingest_directory_fn(
        PathBuf::from(&dir_path).as_path(),
        &project,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        Some(&app),
    )
}

// ─── 文档管理命令 ───

/// 列出所有文档，可按项目筛选
#[tauri::command]
async fn list_documents(
    state: State<'_, AppState>,
    project: Option<String>,
) -> Result<Vec<DocumentMeta>, String> {
    let meta = state.metadata.lock().map_err(|e| e.to_string())?;
    meta.list_documents(project.as_deref())
}

/// 获取指定文档的所有分块
#[tauri::command]
async fn get_document_chunks(
    state: State<'_, AppState>,
    document_id: i64,
) -> Result<Vec<ChunkMeta>, String> {
    let meta = state.metadata.lock().map_err(|e| e.to_string())?;
    meta.get_chunks_by_document(document_id)
}

/// 删除文档及其所有关联分块（同时从向量索引中移除向量）
#[tauri::command]
async fn delete_document(
    state: State<'_, AppState>,
    document_id: i64,
) -> Result<(), String> {
    // Step 1: Get vector keys for this document's chunks before deleting metadata
    let vector_keys: Vec<i64> = {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        meta.get_vector_keys_by_document_ids(&[document_id])?
    };

    // Step 2: Remove vectors from usearch index
    {
        let idx = state.vector_index.lock().map_err(|e| e.to_string())?;
        for key in &vector_keys {
            let _ = idx.remove(*key as u64); // ignore errors for orphaned keys
        }
    }

    // Step 3: Delete metadata (chunks + document) from SQLite
    {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        meta.delete_document(document_id)?
    }

    Ok(())
}

/// 批量删除多个文档（及其所有关联分块和向量），在单个事务中执行
#[tauri::command]
async fn delete_documents_batch(
    state: State<'_, AppState>,
    document_ids: Vec<i64>,
) -> Result<u64, String> {
    if document_ids.is_empty() {
        return Ok(0);
    }

    // Step 1: Get all vector keys for these documents before deleting metadata
    let vector_keys: Vec<i64> = {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        meta.get_vector_keys_by_document_ids(&document_ids)?
    };

    // Step 2: Remove vectors from usearch index
    {
        let idx = state.vector_index.lock().map_err(|e| e.to_string())?;
        for key in &vector_keys {
            let _ = idx.remove(*key as u64); // ignore errors for orphaned keys
        }
    }

    // Step 3: Batch-delete metadata from SQLite
    let count: u64 = {
        let meta = state.metadata.lock().map_err(|e| e.to_string())?;
        meta.delete_documents_batch(document_ids)?
    };

    Ok(count)
}

/// 获取知识库统计信息（get_knowledge_stats 的别名）
#[tauri::command]
async fn get_stats(
    state: State<'_, AppState>,
) -> Result<KnowledgeStats, String> {
    let meta = state.metadata.lock().map_err(|e| e.to_string())?;
    meta.get_stats()
}

// ─── 阶段 4: BM25 全文搜索命令 ───

/// 使用 BM25 按关键词搜索分块（jieba 分词 + tantivy 评分）
#[tauri::command]
async fn bm25_search(
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
async fn hybrid_search(
    state: State<'_, AppState>,
    query: String,
    project_id: Option<String>,
    top_k: Option<usize>,
) -> Result<Vec<HybridSearchResult>, String> {
    services::hybrid_search::hybrid_search(
        &query,
        project_id.as_deref(),
        top_k.unwrap_or(5),
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
}

// ─── 阶段 6: LLM 集成与 RAG 命令 ───

/// 配置 LLM 提供商（API 密钥、基础 URL、模型等）
#[tauri::command]
async fn set_llm_config(
    state: State<'_, AppState>,
    config: LLMConfig,
) -> Result<(), String> {
    state.llm.set_config(config)
}

/// 获取当前 LLM 配置（API 密钥已脱敏）
#[tauri::command]
async fn get_llm_config(
    state: State<'_, AppState>,
) -> Result<LLMConfig, String> {
    let mut config = state.llm.get_config()?;
    // 为安全起见脱敏 API 密钥 — 长密钥仅显示前 3 和后 3 个字符
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
async fn is_llm_configured(
    state: State<'_, AppState>,
) -> Result<bool, String> {
    Ok(state.llm.is_configured())
}

/// 测试 LLM API 连通性，无需嵌入模型。
///
/// 发送最小请求以验证 API 密钥和端点是否有效。
/// 返回成功消息或描述性错误。
#[tauri::command]
async fn test_llm_connection(
    state: State<'_, AppState>,
) -> Result<String, String> {
    state.llm.test_connection().await
}

/// RAG 查询：混合搜索 → 上下文组装 → LLM 流式补全。
///
/// 返回完整的流式分块列表响应。
/// 如果 LLM 不可用，以回退模式返回搜索结果。
#[tauri::command]
async fn rag_query(
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
///
/// 前端应监听 StreamChunk 事件。
#[tauri::command]
async fn rag_query_stream(
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
///
/// 生成一个后台任务，发出 `chat_chunk` Tauri 事件：
/// - `{"type": "text_delta", "content": "..."}` — 文本分块
/// - `{"type": "sources", "sources": [...]}` — RAG 来源引用
/// - `{"type": "done"}` — 流完成
/// - `{"type": "error", "message": "..."}` — 发生错误
///
/// 立即返回；前端应监听 `chat_chunk` 事件。
#[tauri::command]
async fn start_chat_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    query: String,
    project_id: Option<String>,
    conversation_history: Option<Vec<ChatMessage>>,
) -> Result<(), String> {
    let history = conversation_history.unwrap_or_default();

    // 为后台任务克隆状态
    let embedding = state.embedding.clone();
    let vector_index = state.vector_index.clone();
    let bm25 = state.bm25.clone();
    let metadata = state.metadata.clone();
    let llm = state.llm.clone();
    let pid = project_id.clone();
    let q = query.clone();

    // 步骤 1: 预先运行 hybrid_search 以捕获 UI 的来源
    let search_results = services::hybrid_search::hybrid_search(
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
            content_snippet: services::llm_service::truncate_to_tokens(&r.content, 100),
            score: r.score,
        })
        .collect();

    // 步骤 2: 检查 LLM 配置 — 如果未配置则立即回退
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

    // 步骤 3: 用于从后台任务流式传输分块的通道
    let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamChunk>(64);

    // 任务 A: 运行 RAG 管道（使用预计算的 search_results），流式传输到通道
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
                Some(search_results), // pass pre-computed results
            )
            .await;
    });

    // 任务 B: 将通道中的分块 + 来源转发到 Tauri 事件
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
        // 流式传输完成后，发出来源事件
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
///
/// 在后台运行 — 立即返回。在每次聊天流完成后调用。
#[tauri::command]
async fn save_chat_memory(
    state: State<'_, AppState>,
    conversation: Vec<ChatMessage>,
) -> Result<(), String> {
    let data_dir = dirs::home_dir()
        .ok_or("Cannot find home directory")?
        .join(".kingdee-kb");

    // 为后台任务克隆状态
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

/// 统计文本中的 token 数量（前端工具函数）
#[tauri::command]
async fn count_tokens(text: String) -> Result<u32, String> {
    Ok(services::llm_service::count_tokens(&text))
}

// ─── 阶段 9: 模板解析引擎命令 ───

/// 扫描模板目录并返回按阶段排序的所有模板。
///
/// 模板从数据目录下的 templates/ 加载。
#[tauri::command]
async fn scan_templates(template_dir: Option<String>) -> Result<Vec<TemplateInfo>, String> {
    let root = match template_dir {
        Some(dir) => PathBuf::from(dir),
        None => {
            let home = dirs::home_dir().ok_or("Cannot find home directory")?;
            home.join(".kingdee-kb").join("templates")
        }
    };
    services::template_scanner::scan_templates(&root)
}

/// 从 .docx 或 .xlsx 模板文件中提取字段占位符。
///
/// 返回 `{field_name}` 占位符及其元数据列表。
#[tauri::command]
async fn extract_template_fields(file_path: String) -> Result<Vec<FieldInfo>, String> {
    let path = PathBuf::from(&file_path);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "docx" => services::template_docx::extract_docx_fields(&path),
        "xlsx" => {
            let xlsx_fields = services::template_xlsx::extract_xlsx_fields(&path)?;
            // 将 XlsxFieldInfo 转换为 FieldInfo 以统一前端 API
            Ok(xlsx_fields
                .into_iter()
                .map(|f| FieldInfo {
                    name: f.name,
                    field_type: f.field_type,
                    context: f.cell_refs.join(", "),
                    count: f.count,
                })
                .collect())
        }
        _ => Err(format!("Unsupported template format: .{}", ext)),
    }
}

/// Generate a YAML schema for a template (docx or xlsx).
///
/// Parses the template, extracts fields, and returns the schema as a YAML string.
/// If `write_sidecar` is true, also writes a `.schema.yaml` file next to the template.
#[tauri::command]
async fn get_template_schema(
    template_id: String,
    template_name: String,
    file_path: String,
    phase: String,
    write_sidecar: Option<bool>,
) -> Result<TemplateSchema, String> {
    let path = PathBuf::from(&file_path);

    // Step 1: Check for pre-existing sidecar YAML
    if let Some(schema) = services::template_schema::load_schema_sidecar(&path)? {
        return Ok(schema);
    }

    // Step 2: No sidecar — extract fields from the template file
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let schema = match ext.as_str() {
        "docx" => {
            let fields = services::template_docx::extract_docx_fields(&path)?;
            services::template_schema::generate_schema_from_docx(
                &template_id,
                &template_name,
                &phase,
                &fields,
            )
        }
        "xlsx" => {
            let fields = services::template_xlsx::extract_xlsx_fields(&path)?;
            services::template_schema::generate_schema_from_xlsx(
                &template_id,
                &template_name,
                &phase,
                &fields,
            )
        }
        _ => return Err(format!("Unsupported template format: .{}", ext)),
    };

    // Optionally write sidecar YAML file for future fast loading
    if write_sidecar.unwrap_or(false) {
        services::template_schema::write_schema_sidecar(&path, &schema)?;
    }

    Ok(schema)
}

/// Generate templates.json index file listing all templates with categories.
///
/// Scans the template directory, builds a structured index grouped by phase,
/// writes it to `templates.json` in the template directory, and returns the JSON.
#[tauri::command]
async fn generate_templates_index(template_dir: Option<String>) -> Result<String, String> {
    let root = match template_dir {
        Some(dir) => PathBuf::from(dir),
        None => {
            let home = dirs::home_dir().ok_or("Cannot find home directory")?;
            home.join(".kingdee-kb").join("templates")
        }
    };
    let output_path = root.join("templates.json");
    services::template_scanner::write_templates_json(&root, &output_path)
}

// ─── 阶段 10: 文档生成命令 ───

/// 使用字段值填充模板（无 LLM，简单替换）。
///
/// 直接替换 .docx 或 .xlsx 模板中的 `{field_name}` 占位符。
/// 返回输出路径和字段数量。
#[tauri::command]
async fn fill_template(
    template_path: String,
    fields: std::collections::HashMap<String, String>,
    output_path: String,
) -> Result<GeneratedDoc, String> {
    services::doc_generator::fill_template(
        std::path::Path::new(&template_path),
        &fields,
        std::path::Path::new(&output_path),
    )
}

/// 通过填充模板生成文档，可选 LLM 字段生成。
///
/// 完整管道：路由到 docx/xlsx 填充器，为 `ai`/`llm` 策略字段调用 LLM（如果提供了 schema），
/// 验证必填字段，返回元数据。
#[tauri::command]
async fn generate_doc(
    state: State<'_, AppState>,
    request: GenerateDocRequest,
) -> Result<GeneratedDoc, String> {
    services::doc_generator::generate_document(request, &state.llm).await
}

/// 使用交付物配方生成文档（配方感知生成）。
///
/// 完整管道：配方查找 → 字段覆盖 → KB 搜索 kb-strategy 字段
/// → 使用配方特定 system_prompt 的 LLM 生成 → 模板填充 → 产品保存。
#[tauri::command]
async fn generate_recipe_doc_cmd(
    state: State<'_, AppState>,
    request: RecipeDocRequest,
) -> Result<RecipeDocResult, String> {
    // 在移动 request 之前捕获产品存储的请求数据
    let recipe_id = request.recipe_id.clone();
    let project = request.project_name.clone().unwrap_or_default();
    let user_field_count = request.fields.len() as i64;
    let schema_fields_json: String = serde_json::to_string(&request.schema_fields)
        .unwrap_or_else(|_| "[]".to_string());

    let result = services::doc_generator::generate_recipe_doc(
        request,
        &state.llm,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
    .await?;

    // 保存产品到产品存储以支持重新生成
    let input_json = serde_json::to_string(&serde_json::json!({
        "recipe_id": recipe_id,
        "schema_fields": schema_fields_json,
        "project_name": project,
    }))
    .unwrap_or_else(|_| "{}".to_string());

    {
        let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
        let _ = store.create(
            &recipe_id,
            &result.recipe_name,
            &project,
            &result.doc.output_path,
            user_field_count,
            result.doc.ai_fields.len() as i64,
            &input_json,
        );
    }

    Ok(result)
}

/// 便捷命令：从调研笔记生成文档。
///
/// 包装 `generate_recipe_doc`，`context = Some(research_notes)`。
/// 通常用于 recipe_id = "investigation_report"。
#[tauri::command]
async fn generate_from_research(
    state: State<'_, AppState>,
    recipe_id: String,
    template_path: String,
    output_path: String,
    fields: std::collections::HashMap<String, String>,
    schema_fields: Option<Vec<services::template_schema::SchemaField>>,
    project_name: Option<String>,
    research_notes: String,
    project_id: Option<String>,
) -> Result<RecipeDocResult, String> {
    let request = RecipeDocRequest {
        recipe_id,
        template_path,
        output_path,
        fields,
        schema_fields: schema_fields.unwrap_or_default(),
        project_name,
        context: Some(research_notes),
        project_id,
    };
    services::doc_generator::generate_recipe_doc(
        request,
        &state.llm,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
    .await
}

/// 便捷命令：从会议记录生成文档。
///
/// 包装 `generate_recipe_doc`，`context = Some(meeting_transcript)`。
/// 通常用于 recipe_id = "meeting_minutes"。
#[tauri::command]
async fn generate_from_meeting(
    state: State<'_, AppState>,
    recipe_id: String,
    template_path: String,
    output_path: String,
    fields: std::collections::HashMap<String, String>,
    schema_fields: Option<Vec<services::template_schema::SchemaField>>,
    project_name: Option<String>,
    meeting_transcript: String,
    project_id: Option<String>,
) -> Result<RecipeDocResult, String> {
    let request = RecipeDocRequest {
        recipe_id,
        template_path,
        output_path,
        fields,
        schema_fields: schema_fields.unwrap_or_default(),
        project_name,
        context: Some(meeting_transcript),
        project_id,
    };
    services::doc_generator::generate_recipe_doc(
        request,
        &state.llm,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
    .await
}

// ─── 阶段 12: 产品管理命令 ───

/// 列出产品，可按项目筛选。
#[tauri::command]
async fn list_products(
    state: State<'_, AppState>,
    project: Option<String>,
) -> Result<Vec<ProductMeta>, String> {
    let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.list(project.as_deref(), None, None)
}

/// 根据 ID 获取单个产品。
#[tauri::command]
async fn get_product(
    state: State<'_, AppState>,
    id: i64,
) -> Result<ProductMeta, String> {
    let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store
        .get(id)?
        .ok_or_else(|| format!("Product not found: {}", id))
}

/// 删除产品及其所有版本。
#[tauri::command]
async fn delete_product(
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.delete(id)
}

/// 将产品的输出文件导出到目标目录。
/// 返回导出的文件路径。
#[tauri::command]
async fn export_product(
    state: State<'_, AppState>,
    id: i64,
    target_dir: String,
) -> Result<String, String> {
    let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    store.export_product(id, &target_dir)
}

/// 使用更新的字段值重新生成产品。
///
/// 使用最新版本的模板信息重新运行文档生成，
/// 但使用提供的更新字段。创建新版本。
#[tauri::command]
async fn regenerate_product(
    state: State<'_, AppState>,
    id: i64,
    updated_fields: std::collections::HashMap<String, String>,
) -> Result<ProductMeta, String> {
    use services::doc_generator::GenerateDocRequest;
    use std::path::PathBuf;

    // 在块中从存储中提取所有需要的数据，然后释放锁
    let (product_output_path, original_input, _latest_input_data) = {
        let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;

        // 获取现有产品
        let product = store
            .get(id)?
            .ok_or_else(|| format!("Product not found: {}", id))?;

        // 获取最新版本以找到原始模板路径
        let latest = store
            .get_latest_version(id)?
            .ok_or_else(|| format!("No versions found for product: {}", id))?;

        let original_input: serde_json::Value =
            serde_json::from_str(&latest.input_data)
                .unwrap_or_else(|_| serde_json::json!({}));

        (product.output_path.clone(), original_input, latest.input_data.clone())
    }; // 存储在此释放

    let template_path = original_input
        .get("template_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Original input missing template_path".to_string())?
        .to_string();

    let schema_fields = original_input
        .get("schema_fields")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let project_name = original_input
        .get("project_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let context = original_input
        .get("context")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // 生成新输出路径（使用 std::time 而不是 chrono）
    let timestamp = {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("{}", now)
    };
    let output_dir = PathBuf::from(&product_output_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let stem = PathBuf::from(&product_output_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output")
        .to_string();
    let ext = PathBuf::from(&product_output_path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("docx")
        .to_string();
    let new_output_path = output_dir
        .join(format!("{}_v{}.{}", stem, timestamp, ext))
        .to_string_lossy()
        .to_string();

    // 构建生成请求
    let request = GenerateDocRequest {
        template_path,
        output_path: new_output_path.clone(),
        fields: updated_fields.clone(),
        schema_fields,
        project_name,
        context,
    };

    // 生成文档（此处不持有 mutex）
    let result = services::doc_generator::generate_document(request, &state.llm).await?;

    // 保存新版本并更新产品
    let store = state.products.lock().map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
    let input_json = serde_json::to_string(&serde_json::json!({
        "template_path": original_input.get("template_path"),
        "fields": updated_fields,
        "schema_fields": original_input.get("schema_fields"),
        "project_name": original_input.get("project_name"),
        "context": original_input.get("context"),
    }))
    .unwrap_or_else(|_| "{}".to_string());

    store.add_version(id, &input_json, &result.output_path)?;

    // 返回更新后的产品
    store
        .get(id)?
        .ok_or_else(|| format!("Product not found after regeneration: {}", id))
}

/// 执行后端初始化任务（异步，不阻塞 UI 启动）
async fn setup_backend(app: AppHandle) -> Result<(), String> {
    let data_dir = ensure_data_dir()?;
    println!("Data directory initialized at: {:?}", data_dir);

    // 初始化阶段 2 服务
    match AppState::new(&data_dir) {
        Ok(app_state) => {
            app.manage(app_state);
            println!("Phase 2 services initialized (embedding, vector index, metadata)");
        }
        Err(e) => {
            eprintln!("WARNING: Phase 2 services failed to initialize: {}", e);
            eprintln!("The app will start in limited mode (no embedding/search/LLM).");
            app.manage(AppState::minimal(&data_dir));
        }
    }

    // 异步自动加载缓存的嵌入模型（后台加载，不阻塞前端启动）
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app_clone.state::<AppState>();
        // clone Arcs to avoid lifetime issues with State
        let mm_arc = state.model_manager.clone();
        let emb_arc = state.embedding.clone();
        drop(state);

        // Step 1: init model + take_model (scope ensures MutexGuard is freed before Step 2)
        let model = {
            let mut mm = match mm_arc.lock() {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("Auto-load: model_manager lock error: {}", e);
                    return;
                }
            };
            if let Err(e) = mm.init() {
                eprintln!("Auto-load embedding model init failed: {}", e);
                return;
            }
            match mm.take_model() {
                Some(m) => m,
                None => {
                    eprintln!("Auto-load: model initialized but take_model returned None");
                    return;
                }
            }
        };

        // Step 2: set model into EmbeddingService (separate scope)
        {
            let mut emb = match emb_arc.lock() {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("Auto-load: embedding lock error: {}", e);
                    return;
                }
            };
            emb.set_model(model);
            println!("Embedding model auto-loaded from local cache!");
            let _ = app_clone.emit("model-ready", ());
        }
    });

    // 确保模板目录存在，如果为空则同步内置模板
    let template_dir = data_dir.join("templates");
    if !template_dir.exists() {
        std::fs::create_dir_all(&template_dir)
            .map_err(|e| format!("Failed to create templates directory: {}", e))?;
        println!("Created templates directory at: {:?}", template_dir);
    }

    // 如果模板目录为空，从应用包中复制内置模板
    if std::fs::read_dir(&template_dir)
        .map_err(|e| format!("Failed to read templates directory: {}", e))?
        .next()
        .is_none()
    {
        // 首先尝试 exe 目录（用于生产构建）
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let resource_dir = exe_dir.join("templates");
                if resource_dir.exists() {
                    match copy_dir_recursive(&resource_dir, &template_dir) {
                        Ok(_) => println!("Copied built-in templates to {:?}", template_dir),
                        Err(e) => eprintln!("Warning: Failed to copy built-in templates: {}", e),
                    }
                }
            }
        }
        // 开发期间也尝试项目根目录
        let dev_template_dir = std::path::PathBuf::from("../templates");
        if template_dir
            .read_dir()
            .map_err(|e| format!("Failed to read templates directory: {}", e))?
            .next()
            .is_none()
            && dev_template_dir.exists()
        {
            match copy_dir_recursive(&dev_template_dir, &template_dir) {
                Ok(_) => println!("Copied dev templates to {:?}", template_dir),
                Err(e) => eprintln!("Warning: Failed to copy dev templates: {}", e),
            }
        }
    }

    let _ = set_complete(
        app.clone(),
        app.state::<Mutex<SetupState>>(),
        "backend".to_string(),
    )
    .await;

    Ok(())
}

// ─── 阶段 11: 智能补全命令 ───

/// 智能填充：使用 hybrid_search + LLM 进行 KB 辅助字段填充
#[tauri::command]
async fn smart_fill(state: State<'_, AppState>, request: SmartFillRequest) -> Result<SmartFillResult, String> {
    services::smart_completion::smart_fill(
        request,
        &state.llm,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
    .await
}

/// 探测缺失字段：返回未填必填字段的详细诊断信息
#[tauri::command]
async fn probe_missing_fields(
    state: State<'_, AppState>,
    request: GenerateDocRequest,
) -> Result<GeneratedDoc, String> {
    // 生成文档以获取最新的缺失字段详情
    services::doc_generator::generate_document(request, &state.llm).await
}

/// 根据 template_id 获取交付物配方
#[tauri::command]
fn get_deliverable_recipe(template_id: String) -> Result<DeliverableRecipe, String> {
    services::deliverable_recipes::get_recipe_by_template_id(&template_id)
        .ok_or_else(|| format!("No recipe found for template_id: {}", template_id))
}

// ─── 阶段 9: Tauri 命令注册 ───

/// 获取当前研究版本（"enterprise" 或 "flagship"）
#[tauri::command]
fn get_current_edition(state: State<'_, AppState>) -> Result<String, String> {
    let edition = state.edition_config.current();
    Ok(edition.as_str().to_string())
}

/// 切换研究版本
#[tauri::command]
fn set_edition(state: State<'_, AppState>, edition: String) -> Result<(), String> {
    let edition = Edition::from_str(&edition)
        .ok_or_else(|| format!("Invalid edition: {}", edition))?;
    state.edition_config.set(&edition)
}

/// 列出当前版本的所有已导入研究模块
#[tauri::command]
fn list_research_modules(state: State<'_, AppState>) -> Result<Vec<(i64, String, String)>, String> {
    let edition = state.edition_config.current();
    state.research_indexer.list_outlines(&edition)
}

/// 从目录批量导入研究大纲
#[tauri::command]
fn import_research_outlines(state: State<'_, AppState>, dir: String) -> Result<String, String> {
    let edition = state.edition_config.current();
    let result = state.research_indexer.import_directory(std::path::Path::new(&dir), edition)?;
    let mut summary = format!(
        "导入成功: {} 个模块, {} 个问题\n跳过: {} 个文件",
        result.imported, result.total_questions, result.skipped
    );
    if !result.errors.is_empty() {
        let error_list: Vec<&str> = result.errors.iter().take(3).map(|s| s.as_str()).collect();
        summary.push_str(&format!("\n错误 (前{}个): {}", error_list.len(), error_list.join("; ")));
    }
    if result.imported == 0 && !result.errors.is_empty() {
        return Err(format!("导入失败: {}", result.errors.join("; ")));
    }
    Ok(summary)
}

// ─── 阶段 11: 问题推荐命令 ───

/// 根据当前对话主题推荐相关研究问题。
///
/// 使用 hybrid_search + 版本筛选 + 已回答问题排除。
/// KB 搜索失败时回退到仅 DB 推荐。
#[tauri::command]
fn recommend_questions(
    state: State<'_, AppState>,
    request: RecommendRequest,
) -> Result<Vec<RecommendedQuestion>, String> {
    let edition = state.edition_config.current();
    services::question_recommend::recommend_questions(
        &request,
        &state.research_indexer,
        &edition,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
}

/// 基于已回答的问答对生成后续问题。
///
/// 搜索 KB 获取相关上下文，然后调用 LLM 建议 3-5 个后续问题。
/// 优雅降级：LLM 或 KB 失败时返回空列表。
#[tauri::command]
async fn generate_followup_questions(
    state: State<'_, AppState>,
    request: FollowUpRequest,
) -> Result<FollowUpResult, String> {
    services::question_recommend::generate_followup_questions(
        &request,
        &state.llm,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
    .await
}

/// 使用 KB 上下文 + LLM 智能填充研究问题答案。
///
/// 查找匹配问题，搜索 KB 获取相关上下文，
/// 然后调用 LLM 生成答案草稿。
#[tauri::command]
async fn smart_fill_for_question(
    state: State<'_, AppState>,
    question_text: String,
    project_name: Option<String>,
) -> Result<SmartFillResult, String> {
    let edition = state.edition_config.current();
    services::question_recommend::smart_fill_for_question(
        &question_text,
        &edition,
        project_name.as_deref(),
        &state.research_indexer,
        &state.llm,
        &state.embedding,
        &state.vector_index,
        &state.bm25,
        &state.metadata,
    )
    .await
}

// ─── 阶段 12: Whisper 语音识别命令 ───

/// 加载 Whisper 模型用于语音转录。
///
/// 如果本地未缓存，从 HuggingFace 下载模型。
/// 支持的大小："tiny"（~75MB）、"base"（~142MB）、"small"（~466MB）。
#[tauri::command]
async fn load_whisper_model(
    state: State<'_, AppState>,
    model_size: String,
) -> Result<(), String> {
    // 使用 state.data_dir（在 AppState::new() 或 ensure_data_dir() 中初始化）
    let data_dir = &state.data_dir;
    let _model_path = model_downloader::ensure_model(data_dir, &model_size)?;

    // 将模型加载到 WhisperService
    let mut whisper = state.whisper_service.lock().map_err(|e| e.to_string())?;
    whisper.load_model(data_dir, &model_size)?;

    Ok(())
}

/// 获取 Whisper 服务状态（模型已加载、当前模型大小、语言）。
#[tauri::command]
fn get_whisper_status(state: State<'_, AppState>) -> Result<WhisperStatus, String> {
    let whisper = state.whisper_service.lock().map_err(|e| e.to_string())?;
    Ok(whisper.status())
}

/// 开始麦克风录音。
///
/// 捕获 16kHz 单声道 PCM 音频。调用 `stop_whisper_recording`
/// 停止并获取转录结果。
#[tauri::command]
fn start_whisper_recording(state: State<'_, AppState>) -> Result<(), String> {
    let capture = state.audio_capture.lock().map_err(|e| e.to_string())?;
    capture.start_recording()
}

/// 停止录音并转录音频。
///
/// 管道：停止麦克风 → PCM 数据 → Whisper 转录 →
/// 中文后处理 → 返回结果。
#[tauri::command]
async fn stop_whisper_recording(state: State<'_, AppState>) -> Result<TranscriptionResult, String> {
    // 步骤 1: 停止录音并获取 PCM 数据
    let pcm_data = {
        let capture = state.audio_capture.lock().map_err(|e| e.to_string())?;
        capture.stop_recording()?
    };

    if pcm_data.is_empty() {
        return Err("No audio data captured. Microphone may not be working.".to_string());
    }

    // 步骤 2: VAD — 检测语音段
    let speech_segments = services::audio_capture::AudioCapture::detect_speech_segments(
        &pcm_data, 16000, 500, 0.01,
    );

    // 如果未检测到语音，返回空结果
    if speech_segments.is_empty() {
        return Ok(TranscriptionResult {
            text: String::new(),
            segments: vec![],
            confidence: 0.0,
            processing_time_ms: 0,
        });
    }

    // 步骤 3: 将语音段连接到一个缓冲区供 Whisper 使用
    let speech_pcm: Vec<f32> = speech_segments.iter()
        .flat_map(|(start, end)| pcm_data[*start..*end].to_vec())
        .collect();

    // 步骤 4: 通过 Whisper 转录（卸载到阻塞线程，因为 WhisperContext 不是 async）
    let whisper_result = {
        let whisper = state.whisper_service.lock().map_err(|e| e.to_string())?;
        if !whisper.is_model_loaded() {
            return Err("Whisper model not loaded. Call load_whisper_model first.".to_string());
        }
        // Whisper 转录是同步且 CPU 密集型的
        whisper.transcribe(&speech_pcm)?
    };

    // 步骤 5: 中文后处理
    let processed_text = services::chinese_postprocess::postprocess_chinese(&whisper_result.text);

    Ok(TranscriptionResult {
        text: processed_text,
        segments: whisper_result.segments,
        confidence: whisper_result.confidence,
        processing_time_ms: whisper_result.processing_time_ms,
    })
}

// ─── 阶段 14: 视频文件转写 ───

/// 内部转写逻辑（分片流式处理，内存安全）
///
/// 步骤：ffmpeg → 临时 PCM 文件 → 分片读取 → Whisper 分段转写 → 中文后处理
fn do_transcribe_video(
    whisper_service: &std::sync::MutexGuard<'_, services::whisper_service::WhisperService>,
    video_path: &str,
    data_dir: &std::path::Path,
    app_handle: Option<&AppHandle>,
) -> Result<VideoTranscriptionResult, String> {
    let path = std::path::Path::new(video_path);
    if !path.exists() {
        return Err(format!("视频文件不存在: {}", video_path));
    }

    // 步骤 1: 流式提取音频到临时文件（不会 OOM）
    emit_video_progress(app_handle, "extracting", 0.0, "正在提取音频...");
    let extract_start = std::time::Instant::now();
    let (pcm_path, duration_secs) = services::video_transcriber::extract_audio_to_file(path, data_dir)?;
    let extraction_time_ms = extract_start.elapsed().as_millis() as u64;

    // 确保临时文件最终被清理
    let _cleanup = TempCleanup(pcm_path.clone());

    emit_video_progress(app_handle, "transcribing", 0.0, "正在转写语音...");

    // 步骤 2: 分片转写（内存安全：每次只加载 30s 分片）
    let app_handle_clone = app_handle.map(|h| h.clone());
    let result = services::video_transcriber::transcribe_chunks(
        &pcm_path,
        whisper_service,
        app_handle_clone.map(|h| move |chunk_idx: usize, total_chunks: usize| {
            let pct = chunk_idx as f32 / total_chunks as f32 * 100.0;
            let msg = format!("转写中 ({}/{})", chunk_idx, total_chunks);
            h.emit("video_progress", serde_json::json!({
                "step": "transcribing",
                "progress": pct,
                "message": msg
            })).ok();
        }),
    )?;

    // 步骤 3: 中文后处理
    let processed_text = services::chinese_postprocess::postprocess_chinese(&result.text);

    Ok(VideoTranscriptionResult {
        video_path: video_path.to_string(),
        text: processed_text,
        segments: result.segments,
        confidence: result.confidence,
        extraction_time_ms,
        transcription_time_ms: result.processing_time_ms,
        duration_secs,
    })
}

/// RAII 临时文件清理
struct TempCleanup(std::path::PathBuf);
impl Drop for TempCleanup {
    fn drop(&mut self) {
        services::video_transcriber::cleanup_temp_file(&self.0);
    }
}

/// 向前端发送视频处理进度事件
fn emit_video_progress(app_handle: Option<&AppHandle>, step: &str, progress: f32, message: &str) {
    if let Some(handle) = app_handle {
        let payload = serde_json::json!({
            "step": step,
            "progress": progress,
            "message": message
        });
        let _ = handle.emit("video_progress", payload);
    }
}

/// 从视频文件中提取音频并通过 Whisper 转写。
#[tauri::command]
async fn transcribe_video_file(
    state: State<'_, AppState>,
    video_path: String,
    app_handle: AppHandle,
) -> Result<VideoTranscriptionResult, String> {
    let whisper = state.whisper_service.lock().map_err(|e| e.to_string())?;
    if !whisper.is_model_loaded() {
        return Err("Whisper 模型未加载。请先在研究助手页面加载 Whisper 模型。".to_string());
    }
    do_transcribe_video(&whisper, &video_path, &state.data_dir, Some(&app_handle))
}

/// 视频转写一站式管道：提取音频 → 转写 → 入库 → 可选生成会议纪要。
#[tauri::command]
async fn transcribe_and_ingest_video(
    state: State<'_, AppState>,
    video_path: String,
    project: String,
    generate_minutes: bool,
    app_handle: AppHandle,
) -> Result<VideoPipelineResult, String> {
    // 步骤 1: 转写
    let transcription = {
        let whisper = state.whisper_service.lock().map_err(|e| e.to_string())?;
        if !whisper.is_model_loaded() {
            return Err("Whisper 模型未加载。请先在研究助手页面加载 Whisper 模型。".to_string());
        }
        do_transcribe_video(&whisper, &video_path, &state.data_dir, Some(&app_handle))?
    };

    if transcription.text.is_empty() {
        return Err("转写结果为空，无法入库".to_string());
    }

    // 步骤 2: 入库知识库
    emit_video_progress(Some(&app_handle), "ingesting", 0.0, "正在入库知识库...");
    let title = std::path::Path::new(&transcription.video_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("视频转写")
        .to_string();

    let ingestion_result = services::ingestion::ingest_text(
        &transcription.text,
        &format!("[视频转写] {}", title),
        &project,
        &state.embedding,
        &state.vector_index,
        &state.metadata,
        None,
    )?;

    // 步骤 3: 可选生成会议纪要
    let meeting_minutes = if generate_minutes {
        emit_video_progress(Some(&app_handle), "generating_minutes", 0.0, "正在生成会议纪要...");
        Some(services::video_transcriber::generate_meeting_minutes(
            &transcription.text,
            &state.llm,
        )?)
    } else {
        None
    };

    emit_video_progress(Some(&app_handle), "done", 100.0, "全部完成");

    Ok(VideoPipelineResult {
        transcription,
        ingestion_document_id: Some(ingestion_result.document_id),
        meeting_minutes,
    })
}

/// 从已有转写文本生成会议纪要。
#[tauri::command]
async fn generate_meeting_minutes_from_transcript(
    state: State<'_, AppState>,
    transcript: String,
) -> Result<MeetingMinutesResult, String> {
    if transcript.is_empty() {
        return Err("转写文本为空".to_string());
    }
    services::video_transcriber::generate_meeting_minutes(&transcript, &state.llm)
}

// ─── 阶段 13: 研究会话管理 ───

#[tauri::command]
fn create_research_session(
    state: State<'_, AppState>,
    title: String,
    edition: String,
    module_code: String,
    interviewee: String,
    session_date: String,
) -> Result<i64, String> {
    state.research_session_store
        .create_session(&title, &edition, &module_code, &interviewee, &session_date)
}

#[tauri::command]
fn list_research_sessions(state: State<'_, AppState>) -> Result<Vec<ResearchSession>, String> {
    state.research_session_store.list_sessions()
}

#[tauri::command]
fn get_research_session(state: State<'_, AppState>, session_id: i64) -> Result<Option<SessionDetail>, String> {
    state.research_session_store.get_session_detail(session_id)
}

#[tauri::command]
fn update_research_session(
    state: State<'_, AppState>,
    session_id: i64,
    title: String,
    interviewee: String,
    session_date: String,
    status: String,
) -> Result<(), String> {
    state.research_session_store
        .update_session(session_id, &title, &interviewee, &session_date, &status)
}

#[tauri::command]
fn delete_research_session(state: State<'_, AppState>, session_id: i64) -> Result<(), String> {
    state.research_session_store.delete_session(session_id)
}

#[tauri::command]
fn add_qa_record(
    state: State<'_, AppState>,
    session_id: i64,
    question_id: Option<i64>,
    question_text: String,
    answer_text: String,
    notes: String,
    sort_order: i32,
) -> Result<i64, String> {
    state.research_session_store
        .add_record(session_id, question_id, &question_text, &answer_text, &notes, sort_order)
}

#[tauri::command]
fn update_qa_record(
    state: State<'_, AppState>,
    record_id: i64,
    answer_text: String,
    notes: String,
) -> Result<(), String> {
    state.research_session_store.update_record(record_id, &answer_text, &notes)
}

#[tauri::command]
fn delete_qa_record(state: State<'_, AppState>, record_id: i64) -> Result<(), String> {
    state.research_session_store.delete_record(record_id)
}

#[tauri::command]
fn get_session_records(state: State<'_, AppState>, session_id: i64) -> Result<Vec<QARecord>, String> {
    state.research_session_store.get_records(session_id)
}

#[tauri::command]
fn export_session_csv(state: State<'_, AppState>, session_id: i64) -> Result<String, String> {
    state.research_session_store.export_csv(session_id)
}

#[tauri::command]
fn export_session_markdown(state: State<'_, AppState>, session_id: i64) -> Result<String, String> {
    state.research_session_store.export_markdown(session_id)
}

#[tauri::command]
fn reorder_qa_records(
    state: State<'_, AppState>,
    session_id: i64,
    record_ids: Vec<i64>,
) -> Result<(), String> {
    state.research_session_store.reorder_records(session_id, &record_ids)
}

// ─── P1: 双轨风险把控舱 ───

#[tauri::command]
fn add_scope_item(
    state: State<'_, AppState>,
    category: String,
    description: String,
    is_in_scope: bool,
    detail: String,
) -> Result<i64, String> {
    state.risk_control_store.add_scope_item(&category, &description, is_in_scope, &detail)
}

#[tauri::command]
fn list_scope_items(state: State<'_, AppState>) -> Result<Vec<ContractScopeItem>, String> {
    state.risk_control_store.list_scope_items(None, None)
}

#[tauri::command]
fn delete_scope_item(state: State<'_, AppState>, item_id: i64) -> Result<(), String> {
    state.risk_control_store.delete_scope_item(item_id)
}

#[tauri::command]
async fn check_scope_creep(
    state: State<'_, AppState>,
    requirement: String,
) -> Result<ScopeCreepResult, String> {
    state.risk_control_store.check_scope_creep(&state.llm, &requirement).await
}

#[tauri::command]
fn record_health_metric(
    state: State<'_, AppState>,
    indicator_type: String,
    value: f64,
    notes: String,
) -> Result<i64, String> {
    state.risk_control_store.record_health_metric(&indicator_type, value, &notes)
}

#[tauri::command]
fn get_project_health(state: State<'_, AppState>) -> Result<ProjectHealthScore, String> {
    state.risk_control_store.calculate_health_score()
}

#[tauri::command]
async fn generate_risk_report(
    state: State<'_, AppState>,
    context: String,
) -> Result<String, String> {
    state.risk_control_store.generate_risk_report(&state.llm, &context).await
}

#[tauri::command]
async fn generate_defense_script(
    state: State<'_, AppState>,
    request: DefenseScriptRequest,
) -> Result<DefenseScriptResult, String> {
    state.risk_control_store.generate_defense_script(&state.llm, &request).await
}

// --- P2.1: 本地脱敏 ---

#[tauri::command]
fn desensitize_text(
    state: State<'_, AppState>,
    text: String,
) -> Result<services::desensitize::DesensitizeResult, String> {
    let result = state.desensitizer.desensitize(&text);
    Ok(services::desensitize::DesensitizeResult {
        safe_text: result.safe_text,
        mapping: result.mapping,
    })
}

#[tauri::command]
fn add_sensitive_keyword(state: State<'_, AppState>, keyword: String) -> Result<(), String> {
    state.desensitizer.add_keyword(&keyword);
    Ok(())
}

#[tauri::command]
fn list_sensitive_keywords(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    Ok(state.desensitizer.get_keywords())
}

#[tauri::command]
fn remove_sensitive_keyword(state: State<'_, AppState>, keyword: String) -> Result<bool, String> {
    Ok(state.desensitizer.remove_keyword(&keyword))
}

// --- P2.2: 蓝图提炼 ---

const BLUEPRINT_SYSTEM_PROMPT: &str = "\
你是一个金蝶ERP业务架构师。根据调研记录提炼业务蓝图。\n\
按四段结构：\n\
1.【现有流程 As-Is】具体流程步骤和角色\n\
2.【系统标准流程 To-Be】含系统路径\n\
3.【差异配置点】配置路径: 配置值\n\
4.【对应单据类型】单据名称（编码规则）\n\
禁止空话，不确定写[待确认]";

#[tauri::command]
async fn extract_blueprint(
    state: State<'_, AppState>,
    research_context: String,
) -> Result<String, String> {
    use services::llm_service::ChatMessage;
    let messages = vec![
        ChatMessage { role: "system".to_string(), content: BLUEPRINT_SYSTEM_PROMPT.to_string() },
        ChatMessage { role: "user".to_string(), content: research_context },
    ];
    let config = state.llm.get_config()?;
    state.llm.chat_completion(&messages, &config).await
}

// --- P2.3: Fit-Gap 分析 ---

const FITGAP_SYSTEM_PROMPT: &str = "\
你是一个ERP差异分析专家。分析以下需求，每项判断Fit/Gap。\n\
严格Markdown表格：|序号|需求|分类|Fit/Gap|理由|建议方案|\n\
理由必须具体到模块功能，建议必须可执行。";

#[tauri::command]
async fn analyze_fit_gap(
    state: State<'_, AppState>,
    requirements: String,
) -> Result<String, String> {
    use services::llm_service::ChatMessage;
    let messages = vec![
        ChatMessage { role: "system".to_string(), content: FITGAP_SYSTEM_PROMPT.to_string() },
        ChatMessage { role: "user".to_string(), content: requirements },
    ];
    let config = state.llm.get_config()?;
    state.llm.chat_completion(&messages, &config).await
}

// --- ReAct 对话 ---

#[tauri::command]
async fn react_chat(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    message: String,
    system_extra: String,
    session_id: String,
) -> Result<(), String> {
    use services::react_agent::ReActEvent;
    use tauri::Emitter;
    use tokio::sync::mpsc;

    let (tx, mut rx) = mpsc::unbounded_channel::<ReActEvent>();

    let sid = session_id;

    // 在单独的任务中运行 agent — 从 AppHandle 获取状态以避免生命周期问题
    let pending = state.pending_questions.clone();
    let ah = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        let state = ah.state::<AppState>();
        services::rig_agent::RigAgent::run(
            &state.llm,
            &message,
            &system_extra,
            &[],
            tx,
            &sid,
            pending,
        )
        .await;
    });

    // 转发到达的事件（实时流式传输）
    while let Some(event) = rx.recv().await {
        let payload = serde_json::to_value(&event).unwrap_or_default();
        if app_handle.emit("react-event", payload).is_err() {
            break;
        }
        match &event {
            ReActEvent::Done { .. } | ReActEvent::Error { .. } => break,
            _ => {}
        }
    }

    Ok(())
}

/// 回答问题工具的待处理问题（前端在用户选择/输入答案后调用）
#[tauri::command]
async fn answer_question(
    state: State<'_, AppState>,
    question_id: String,
    answer: String,
) -> Result<(), String> {
    services::question_tool::answer_question(&state.pending_questions, &question_id, &answer).await
}

/// 使用 PowerShell 将内容写入文件（UTF-8 BOM 编码）
/// 通过绕过 std::fs 避免 Windows 上的中文编码问题
fn write_file_via_powershell(path: &Path, content: &str) -> Result<(), String> {
    // 首先通过 Rust 写入临时文件（正确处理 UTF-8）
    let temp_path = path.with_extension("tmp");
    std::fs::write(&temp_path, content)
        .map_err(|e| format!("Failed to write temp file: {}", e))?;

    // 使用 PowerShell 复制并显式指定 UTF-8 BOM 编码
    let ps_script = format!(
        "$c = Get-Content -Path '{}' -Raw -Encoding UTF8; [System.IO.File]::WriteAllText('{}', $c, [System.Text.UTF8Encoding]::new($true))",
        temp_path.to_string_lossy().replace("'", "''"),
        path.to_string_lossy().replace("'", "''")
    );

    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps_script])
        .output()
        .map_err(|e| format!("PowerShell failed: {}", e))?;

    // 清理临时文件
    let _ = std::fs::remove_file(&temp_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("PowerShell write error: {}", stderr));
    }

    Ok(())
}

/// 将任意内容导出到文件（UTF-8 BOM 编码）
/// 使用 PowerShell 确保中文文本不乱码
#[tauri::command]
async fn export_report(
    content: String,
    file_path: String,
) -> Result<String, String> {
    let path = PathBuf::from(&file_path);
    write_file_via_powershell(&path, &content)?;
    Ok(file_path)
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // 初始化启动状态跟踪
            app.manage(Mutex::new(SetupState {
                frontend_task: false,
                backend_task: false,
            }));

            // 生成后端初始化任务（异步，不阻塞 UI 启动）
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = setup_backend(app_handle).await {
                    eprintln!("Backend setup error: {}", e);
                }
            });

// 注册全局快捷键：Alt+Space → 切换 spotlight 覆盖层
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::{Code, Modifiers, ShortcutState};
                app.handle().plugin(
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_shortcuts(["alt+space"])?
                        .with_handler(|app, shortcut, event| {
                            if event.state == ShortcutState::Pressed
                                && shortcut.matches(Modifiers::ALT, Code::Space)
                            {
                                use tauri::Emitter;
                                // 始终向前端发出切换事件
                                let _ = app.emit("spotlight-toggle", ());
                                // 确保窗口可见并获得焦点
                                if let Some(window) = app.get_webview_window("main") {
                                    if window.is_minimized().unwrap_or(false) {
                                        let _ = window.unminimize();
                                    }
                                    if !window.is_visible().unwrap_or(false) {
                                        let _ = window.show();
                                    }
                                    let _ = window.set_focus();
                                }
                            }
                        })
                        .build(),
                )?;
            }

            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            get_data_dir,
            set_api_key,
            get_api_key,
            delete_api_key,
            set_complete,
            // Phase 2: Embedding & Vector Store
            get_model_status,
            init_model,
            get_download_progress,
            get_embedding_model_config,
            set_embedding_model_config,
            embed_text,
            embed_batch,
            search_similar,
            load_index,
            get_index_stats,
            get_knowledge_stats,
            // Phase 3: Ingestion Pipeline
            ingest_text,
            ingest_file,
            ingest_directory,
            // Document Management
            list_documents,
            get_document_chunks,
            delete_document,
            delete_documents_batch,
            get_stats,
            // Phase 4: BM25 Search
            bm25_search,
            hybrid_search,
            // Phase 6: LLM Integration
            set_llm_config,
            get_llm_config,
            is_llm_configured,
            test_llm_connection,
            rag_query,
            rag_query_stream,
            start_chat_stream,
            save_chat_memory,
            count_tokens,
            // Phase 9: Template Engine
            scan_templates,
            extract_template_fields,
            get_template_schema,
            generate_templates_index,
            // Phase 10: Document Generation
            fill_template,
            generate_doc,
            generate_recipe_doc_cmd,
            generate_from_research,
            generate_from_meeting,
            // Phase 12: Product Management
            list_products,
            get_product,
            delete_product,
            export_product,
            regenerate_product,
            // Phase 11: Smart Completion
            smart_fill,
            probe_missing_fields,
            get_deliverable_recipe,
            // Phase 11: Question Recommendation
            recommend_questions,
            generate_followup_questions,
            smart_fill_for_question,
            // Phase 12: Whisper Voice Recognition
            load_whisper_model,
            get_whisper_status,
            start_whisper_recording,
            stop_whisper_recording,
            // Phase 14: Video Transcription
            transcribe_video_file,
            transcribe_and_ingest_video,
            generate_meeting_minutes_from_transcript,
            // Phase 9: Research Edition Commands
            get_current_edition,
            set_edition,
            list_research_modules,
            import_research_outlines,
            // Phase 13: Research Session Management
            create_research_session,
            list_research_sessions,
            get_research_session,
            update_research_session,
            delete_research_session,
            add_qa_record,
            update_qa_record,
            delete_qa_record,
            get_session_records,
            export_session_csv,
            export_session_markdown,
            reorder_qa_records,
            // P1: 双轨风险把控舱
            add_scope_item,
            list_scope_items,
            delete_scope_item,
            check_scope_creep,
            record_health_metric,
            get_project_health,
            generate_risk_report,
            generate_defense_script,
            // P2: 蓝图提炼/Fit-Gap/脱敏
            desensitize_text,
            add_sensitive_keyword,
            list_sensitive_keywords,
            remove_sensitive_keyword,
            extract_blueprint,
            analyze_fit_gap,
            react_chat,
            answer_question,
            export_report,
        ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
