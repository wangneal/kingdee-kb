mod app_state;
mod commands;
mod error;
pub mod services;

use anyhow::anyhow;
use std::sync::Mutex;
use tauri::Manager;
use tracing::warn;
use tracing_subscriber::EnvFilter;

use crate::app_state::AppState;
pub use commands::core::{ensure_data_dir, init_app_state, setup_backend_async, SetupState};
pub use error::{AppError, AppResult};
pub use services::template_docx;
pub use services::template_schema;
pub use services::template_xlsx;

/// 在线 ASR 配置存储（腾讯云）— 使用系统钥匙串保护密钥
pub struct AsrConfigStore {
    pub tencent_secret_id: Option<String>,
    pub tencent_secret_key: Option<String>,
}

const KEYRING_SERVICE: &str = "kingdee-kb";
const ASR_SECRET_ID_ACCOUNT: &str = "tencent_asr_secret_id";
const ASR_SECRET_KEY_ACCOUNT: &str = "tencent_asr_secret_key";

impl AsrConfigStore {
    pub fn new() -> Self {
        let mut store = Self {
            tencent_secret_id: None,
            tencent_secret_key: None,
        };
        store.load();
        store
    }

    fn load(&mut self) {
        self.tencent_secret_id = Self::read_credential(ASR_SECRET_ID_ACCOUNT);
        self.tencent_secret_key = Self::read_credential(ASR_SECRET_KEY_ACCOUNT);
    }

    fn read_credential(account: &str) -> Option<String> {
        let entry = match keyring::Entry::new(KEYRING_SERVICE, account) {
            Ok(e) => e,
            Err(e) => {
                warn!(account, "无法访问系统凭据存储: {e}");
                return None;
            }
        };
        match entry.get_password() {
            Ok(val) if !val.is_empty() => Some(val),
            Ok(_) => None,
            Err(keyring::Error::NoEntry) => None,
            Err(e) => {
                warn!(account, "读取凭据失败: {e}");
                None
            }
        }
    }

    fn write_credential(account: &str, value: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account)
            .map_err(|e| format!("无法访问系统凭据存储: {}", e))?;
        entry
            .set_password(value)
            .map_err(|e| format!("写入凭据失败: {}", e))
    }

    fn delete_credential(account: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account)
            .map_err(|e| format!("无法访问系统凭据存储: {}", e))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("删除凭据失败: {}", e)),
        }
    }

    /// 保存腾讯云 ASR 凭据到系统钥匙串，并更新内存状态。
    ///
    /// # Errors
    ///
    /// 当钥匙串写入或删除失败时返回错误。如果 `secret_key` 写入失败，
    /// 会尝试将 `secret_id` 回滚到旧值后再返回错误。
    pub fn save_tencent(
        &mut self,
        secret_id: Option<String>,
        secret_key: Option<String>,
    ) -> Result<(), String> {
        // 写入钥匙串时保证原子性：secret_key 失败则回滚 secret_id
        let old_id = self.tencent_secret_id.clone();
        match secret_id {
            Some(ref id) if !id.trim().is_empty() => {
                Self::write_credential(ASR_SECRET_ID_ACCOUNT, id.trim())?;
            }
            _ => {
                Self::delete_credential(ASR_SECRET_ID_ACCOUNT)?;
            }
        }
        if let Err(e) = (|| -> Result<(), String> {
            match secret_key {
                Some(ref key) if !key.trim().is_empty() => {
                    Self::write_credential(ASR_SECRET_KEY_ACCOUNT, key.trim())
                }
                _ => Self::delete_credential(ASR_SECRET_KEY_ACCOUNT),
            }
        })() {
            // secret_key 写入失败，回滚 secret_id 到旧值
            if let Some(ref prev_id) = old_id {
                let _ = Self::write_credential(ASR_SECRET_ID_ACCOUNT, prev_id);
            } else {
                let _ = Self::delete_credential(ASR_SECRET_ID_ACCOUNT);
            }
            return Err(e);
        }
        // 钥匙串写入全部成功，更新内存状态
        self.tencent_secret_id = secret_id;
        self.tencent_secret_key = secret_key;
        Ok(())
    }

    /// 返回 ASR 配置状态
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({
            "tencent_configured": self.tencent_secret_id.is_some(),
        })
    }
}

/// 启动时补偿：重试 deletion_outbox 中 status='pending' 的记录
///
/// 返回 `anyhow::Result<()>` 而非 `()`：之前的版本在锁失败 / DB 失败时
/// 静默 `return;`，导致线上无人察觉。现版本调用方应 `let _ = ... ;`
/// 忽略非致命错误，或用 `?` 透传给上层。
pub fn compensate_pending_deletions(state: &AppState) -> anyhow::Result<()> {
    let meta = state
        .metadata
        .lock()
        .map_err(|e| anyhow!("获取 metadata 锁失败: {}", e))?;

    let pending = meta
        .get_pending_deletions()
        .map_err(|e| anyhow!("读取 pending 删除记录失败: {}", e))?;

    if pending.is_empty() {
        return Ok(());
    }

    tracing::info!("发现 {} 条 pending 删除记录，开始补偿", pending.len());
    drop(meta);

    for (id, doc_id, _project, keys_json) in &pending {
        tracing::info!("补偿删除: outbox_id={}, document_id={}", id, doc_id);

        let vector_keys: Vec<i64> = match serde_json::from_str(keys_json) {
            Ok(k) => k,
            Err(e) => {
                tracing::warn!("解析 vector_keys 失败(outbox_id={}): {}", id, e);
                if let Ok(meta) = state.metadata.lock() {
                    let _ = meta.update_deletion_status(
                        *id,
                        "failed",
                        Some(&format!("解析 vector_keys 失败: {}", e)),
                    );
                }
                continue;
            }
        };

        let _ = state.get_or_init_bm25();
        if let Ok(bm25) = state.bm25.write() {
            if let Err(e) = bm25.remove_chunks(&vector_keys) {
                tracing::warn!("补偿 BM25 删除失败(outbox_id={}): {}", id, e);
            }
        }

        if let Ok(idx) = state.vector_index.write() {
            if let Err(e) = idx.remove_keys(&vector_keys) {
                tracing::warn!("补偿 usearch 删除失败(outbox_id={}): {}", id, e);
            }
        }

        if let Ok(meta) = state.metadata.lock() {
            let _ = meta.update_deletion_status(*id, "completed", None);
        }
    }

    tracing::info!("删除补偿完成，共处理 {} 条记录", pending.len());
    Ok(())
}

pub fn run() {
    // 初始化 tracing 日志（可通过 RUST_LOG 环境变量控制级别）
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("kingdee_kb=info".parse().unwrap()),
        )
        .with_target(true)
        .with_thread_ids(true)
        .init();

    tauri::Builder::default()
        .setup(|app| {
            app.manage(Mutex::new(SetupState {
                frontend_task: false,
                backend_task: false,
            }));

            let app_handle = app.handle().clone();
            // 同步初始化 AppState 并托管，消除启动时的竞争条件
            match init_app_state(&app_handle) {
                Ok(app_state) => {
                    app.manage(app_state);
                }
                Err(e) => {
                    tracing::error!("AppState 初始化致命错误，降级到 minimal 模式: {}", e);
                    // 降级到 minimal AppState：磁盘初始化失败时仍能启动应用，
                    // 后续功能（whisper、LLM 等）由调用方按需提示用户重试
                    let fallback_dir = std::env::temp_dir().join("kingdee-kb-fallback");
                    let _ = std::fs::create_dir_all(&fallback_dir);
                    let app_state = AppState::minimal(&fallback_dir);
                    app.manage(app_state);
                }
            }

            tauri::async_runtime::spawn(async move {
                if let Err(e) = setup_backend_async(app_handle).await {
                    tracing::error!("后端异步初始化失败: {}", e);
                }
            });

            // 启动腾讯会议定时同步（每 30 分钟）
            crate::services::meeting_sync::start_sync_loop(app.handle().clone());

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
                                let _ = app.emit("spotlight-toggle", ());
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
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            // Core
            commands::core::set_complete,
            commands::core::export_report,
            // Phase 2: Embedding & Vector Store
            commands::embedding::get_model_status,
            commands::embedding::get_embedding_model_config,
            commands::embedding::set_embedding_model_config,
            // Phase 3: Ingestion Pipeline
            commands::ingestion::ingest_text,
            commands::ingestion::ingest_file,
            commands::ingestion::extract_file_text,
            commands::ingestion::ingest_directory,
            // KB Compilation Config
            commands::kb_compilation::get_kb_compilation_enabled,
            commands::kb_compilation::set_kb_compilation_enabled,
            commands::kb_compilation::recompile_failed_kb_sources,
            commands::kb_compilation::start_kb_recompile,
            commands::kb_compilation::get_kb_recompile_status,
            // Phase 2: 持久化摄入队列
            commands::ingestion_queue::enqueue_ingestion,
            commands::ingestion_queue::list_ingestion_queue,
            commands::ingestion_queue::retry_project_failed_ingestions,
            commands::ingestion_queue::process_project_ingestion_queue,
            // Document Management
            commands::document::list_documents,
            commands::document::get_document_chunks,
            commands::document::delete_document,
            commands::document::delete_documents_batch,
            commands::document::get_stats,
            // Phase 4: BM25 Search
            commands::search_llm::bm25_search,
            commands::search_llm::hybrid_search,
            // Phase 6: LLM Integration
            commands::search_llm::save_chat_memory,
            commands::search_llm::count_tokens,
            // Phase 12: Product Management
            commands::product::list_products,
            commands::product::delete_product,
            commands::product::export_product,
            // Unified Project Management
            commands::project::ensure_default_project,
            commands::project::create_project,
            commands::project::list_projects,
            commands::project::get_project,
            commands::project::get_project_phases,
            commands::project::update_project,
            commands::project::update_project_phase_plan,
            commands::project::archive_project,
            commands::project::restore_project,
            commands::project::delete_project,
            commands::project::set_current_project_phase,
            commands::project::list_project_products,
            commands::project::add_project_product,
            commands::project::delete_project_product,
            // Phase 12: Whisper Voice Recognition
            commands::media::load_whisper_model,
            commands::media::get_whisper_status,
            commands::media::list_audio_input_devices,
            commands::media::start_whisper_recording,
            commands::media::transcribe_whisper_recording_chunk,
            commands::media::review_transcription_text,
            commands::media::stop_whisper_recording,
            // ASR Provider management
            commands::media::list_asr_providers,
            commands::media::save_asr_config,
            commands::media::get_asr_config_status,
            // Phase 14: Video Transcription
            commands::media::transcribe_video_file,
            commands::media::transcribe_and_ingest_video,
            commands::media::generate_meeting_minutes_from_transcript,
            commands::media::check_ffmpeg_status,
            // Tencent Meeting MCP
            commands::tencent_meeting::save_tencent_meeting_token,
            commands::tencent_meeting::get_tencent_meeting_config_status,
            commands::tencent_meeting::save_kdclub_token,
            commands::tencent_meeting::get_kdclub_token,
            commands::tencent_meeting::list_tencent_meeting_tools,
            commands::tencent_meeting::call_tencent_meeting_tool,
            commands::tencent_meeting::fetch_tencent_meeting_transcript,
            commands::tencent_meeting::convert_tencent_meeting_timestamp,
            commands::tencent_meeting::schedule_tencent_meeting,
            commands::tencent_meeting::update_tencent_meeting,
            commands::tencent_meeting::cancel_tencent_meeting,
            commands::tencent_meeting::get_tencent_meeting,
            commands::tencent_meeting::get_tencent_meeting_by_code,
            commands::tencent_meeting::list_tencent_user_meetings,
            commands::tencent_meeting::list_tencent_user_ended_meetings,
            commands::tencent_meeting::list_tencent_meeting_records,
            commands::tencent_meeting::submit_tencent_meeting_feedback,
            // Meeting management (local store + sync)
            commands::meeting::sync_tencent_meetings,
            commands::meeting::list_meetings,
            commands::meeting::get_meeting_with_assets,
            commands::meeting::link_meeting_to_project,
            commands::meeting::unlink_meeting_from_project,
            commands::meeting::ignore_unlinked_meeting,
            commands::meeting::fetch_meeting_transcript,
            commands::meeting::generate_meeting_minutes,
            commands::meeting::regenerate_meeting_minutes,
            commands::meeting::list_recent_meeting_minutes,
            commands::meeting::read_project_activity_log,
            // Phase 13: Research Session Management
            commands::research::create_research_session,
            commands::research::list_research_sessions,
            commands::research::get_research_session,
            commands::research::update_research_session,
            commands::research::delete_research_session,
            commands::research::add_qa_record,
            commands::research::update_qa_record,
            commands::research::delete_qa_record,
            commands::research::get_session_records,
            commands::research::export_session_csv,
            commands::research::export_session_markdown,
            commands::research::reorder_qa_records,
            // P1: 双轨风险把控舱
            commands::risk_blueprint::add_scope_item,
            commands::risk_blueprint::list_scope_items,
            commands::risk_blueprint::delete_scope_item,
            commands::risk_blueprint::check_scope_creep,
            commands::risk_blueprint::record_health_metric,
            commands::risk_blueprint::get_project_health,
            commands::risk_blueprint::generate_risk_report,
            commands::risk_blueprint::generate_defense_script,
            // P1.5: 合同范围提取
            commands::risk_blueprint::extract_scope_from_document,
            commands::risk_blueprint::confirm_scope_items,
            // P1.6: 整库备份
            commands::risk_blueprint::export_database,
            commands::risk_blueprint::import_database,
            // P2: 蓝图提炼/Fit-Gap/脱敏
            commands::risk_blueprint::desensitize_text,
            commands::risk_blueprint::add_sensitive_keyword,
            commands::risk_blueprint::list_sensitive_keywords,
            commands::risk_blueprint::remove_sensitive_keyword,
            commands::risk_blueprint::analyze_fit_gap,
            commands::agent::list_agent_tool_profiles,
            commands::agent::list_agent_tool_audit,
            commands::agent::list_agent_tool_audit_summary,
            commands::agent::get_agent_tool_config,
            commands::agent::set_agent_tool_config,
            commands::agent::read_agent_tool_output,
            commands::agent::list_skill_permission_rules,
            commands::agent::revoke_skill_permission_rule,
            commands::agent::get_latest_agent_session,
            commands::agent::get_agent_session,
            commands::risk_blueprint::agent_chat,
            commands::risk_blueprint::answer_question,
            commands::risk_blueprint::reject_question,
            commands::risk_blueprint::cancel_agent_stream,
            // Skill system
            commands::skill::list_skills,
            commands::skill::get_skill,
            commands::skill::search_skills,
            commands::skill::get_skill_stats,
            commands::skill::rescan_skills,
            commands::skill::match_skill,
            commands::skill::import_skill,
            commands::skill::get_skill_full,
            commands::skill::list_shared_resources,
            commands::skill::read_skill_file,
            commands::skill::list_skill_files,
            // Skill Phase 2: Trigger Matching
            commands::skill::trigger_skill_match,
            commands::skill::match_skill_candidates,
            commands::skill::get_skill_list_prompt,
            commands::skill::get_skill_prompt_entries,
            // Skill Phase 3: Script Execution & Templates
            commands::skill::execute_skill_script,
            commands::skill::get_template_manifest,
            commands::skill::save_template_manifest,
            // LLM Provider Management
            commands::llm_provider::list_llm_providers,
            commands::llm_provider::list_runtime_llm_providers,
            commands::llm_provider::get_provider_policy,
            commands::llm_provider::set_provider_policy,
            commands::llm_provider::fetch_llm_endpoint_models,
            commands::llm_provider::add_llm_provider,
            commands::llm_provider::update_llm_provider,
            commands::llm_provider::delete_llm_provider,
            commands::llm_provider::set_default_llm_provider,
            commands::llm_provider::probe_all_providers,
            commands::llm_provider::get_ocr_config,
            commands::llm_provider::save_ocr_config,
            commands::llm_provider::clear_ocr_config,
            commands::llm_provider::get_excluded_image_types,
            commands::llm_provider::set_excluded_image_types,
            commands::llm_provider::set_default_api_key,
            commands::llm_provider::set_default_model,
            commands::llm_provider::probe_model_multimodal,
            commands::llm_provider::is_llm_configured,
            // Raw source management
            commands::raw_source::create_raw_source,
            commands::raw_source::list_raw_sources,
            commands::raw_source::soft_delete_raw_source,
            // Verification
            commands::verification::run_verification,
            // Wiki Page management
            commands::wiki_page::create_wiki_page,
            commands::wiki_page::get_wiki_page,
            commands::wiki_page::get_wiki_page_by_slug,
            commands::wiki_page::list_wiki_pages,
            commands::wiki_page::update_wiki_page,
            commands::wiki_page::delete_wiki_page,
            commands::wiki_page::batch_delete_wiki_pages,
            commands::wiki_page::approve_wiki_page,
            commands::wiki_page::approve_auto_wiki_pages,
            commands::wiki_page::reject_wiki_page,
            // Phase 5: Wikilink 编辑器
            commands::wiki_page::search_wikilink_candidates,
            commands::wiki_page::add_wikilink,
            commands::wiki_page::remove_wikilink,
            commands::wiki_page::get_wikilink_targets,
            commands::wiki_page::get_backlinks,
            // Phase 4: 大纲编辑器
            commands::outline::create_outline_node,
            commands::outline::update_outline_node,
            commands::outline::delete_outline_node,
            commands::outline::move_outline_node,
            commands::outline::get_outline_tree,
            commands::outline::export_outline,
            commands::outline::import_markdown_outline,
            commands::outline::get_outline_stats,
            // Phase 5: 知识图谱
            commands::knowledge_graph::build_knowledge_graph,
            commands::knowledge_graph::get_graph_neighbors,
            commands::knowledge_graph::get_full_graph,
            commands::knowledge_graph::graph_expand_search,
            // File operations
            commands::core::save_attachment_as,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

