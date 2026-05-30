#![allow(dead_code)]

mod app_state;
mod commands;
mod services;

use std::sync::Mutex;
use tauri::Manager;
use tracing_subscriber::EnvFilter;

pub use commands::core::{ensure_data_dir, setup_backend, SetupState};
pub use services::template_docx;
pub use services::template_schema;
pub use services::template_xlsx;

/// 在线 ASR 配置存储（腾讯/讯飞）
pub struct AsrConfigStore {
    pub tencent_secret_id: Option<String>,
    pub tencent_secret_key: Option<String>,
    pub tencent_app_id: Option<i64>,
    pub xfyun_app_id: Option<String>,
    pub xfyun_api_key: Option<String>,
    pub xfyun_api_secret: Option<String>,
}

impl AsrConfigStore {
    pub fn new(_db_path: &std::path::Path) -> Self {
        Self {
            tencent_secret_id: None,
            tencent_secret_key: None,
            tencent_app_id: None,
            xfyun_app_id: None,
            xfyun_api_key: None,
            xfyun_api_secret: None,
        }
    }
    pub fn save_tencent(
        &mut self,
        secret_id: Option<String>,
        secret_key: Option<String>,
        app_id: Option<i64>,
    ) {
        self.tencent_secret_id = secret_id;
        self.tencent_secret_key = secret_key;
        self.tencent_app_id = app_id;
    }
    pub fn save_xfyun(
        &mut self,
        app_id: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
    ) {
        self.xfyun_app_id = app_id;
        self.xfyun_api_key = api_key;
        self.xfyun_api_secret = api_secret;
    }
    pub fn get_status(&self) -> serde_json::Value {
        serde_json::json!({
            "tencent_configured": self.tencent_secret_id.is_some(),
            "xfyun_configured": self.xfyun_app_id.is_some()
        })
    }
}

pub fn run() {
    // 初始化 tracing 日志（可通过 RUST_LOG 环境变量控制级别）
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("kingdee_kb=info".parse().unwrap()),
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
        .invoke_handler(tauri::generate_handler![
            // Core
            commands::core::greet,
            commands::core::get_data_dir,
            commands::core::set_api_key,
            commands::core::get_api_key,
            commands::core::delete_api_key,
            commands::core::set_complete,
            commands::core::export_report,
            // Phase 2: Embedding & Vector Store
            commands::embedding::get_model_status,
            commands::embedding::init_model,
            commands::embedding::get_download_progress,
            commands::embedding::get_embedding_model_config,
            commands::embedding::set_embedding_model_config,
            commands::embedding::embed_text,
            commands::embedding::embed_batch,
            commands::embedding::search_similar,
            commands::embedding::load_index,
            commands::embedding::get_index_stats,
            commands::embedding::get_knowledge_stats,
            commands::embedding::get_available_providers,
            // Phase 3: Ingestion Pipeline
            commands::ingestion::ingest_text,
            commands::ingestion::ingest_file,
            commands::ingestion::extract_file_text,
            commands::ingestion::ingest_directory,
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
            commands::search_llm::set_llm_config,
            commands::search_llm::get_llm_config,
            commands::search_llm::is_llm_configured,
            commands::search_llm::test_llm_connection,
            commands::search_llm::save_chat_memory,
            commands::search_llm::count_tokens,
            // Phase 9: Template Engine
            commands::template_doc::scan_templates,
            commands::template_doc::extract_template_fields,
            commands::template_doc::get_template_schema,
            commands::template_doc::generate_templates_index,
            // Phase 10: Document Generation
            commands::template_doc::fill_template,
            commands::template_doc::generate_doc,
            commands::template_doc::generate_recipe_doc_cmd,
            commands::template_doc::generate_from_research,
            commands::template_doc::generate_from_meeting,
            // Phase 11: Smart Completion
            commands::template_doc::smart_fill,
            commands::template_doc::probe_missing_fields,
            commands::template_doc::get_deliverable_recipe,
            // Phase 12: Product Management
            commands::product::list_products,
            commands::product::get_product,
            commands::product::delete_product,
            commands::product::export_product,
            commands::product::regenerate_product,
            // Phase 12: Whisper Voice Recognition
            commands::media::load_whisper_model,
            commands::media::get_whisper_status,
            commands::media::start_whisper_recording,
            commands::media::stop_whisper_recording,
            // Phase 14: Video Transcription
            commands::media::transcribe_video_file,
            commands::media::transcribe_and_ingest_video,
            commands::media::generate_meeting_minutes_from_transcript,
            // Phase 9: Research Edition Commands
            commands::research::get_current_edition,
            commands::research::set_edition,
            commands::research::list_research_modules,
            commands::research::import_research_outlines,
            // Phase 11: Question Recommendation
            commands::research::recommend_questions,
            commands::research::generate_followup_questions,
            commands::research::smart_fill_for_question,
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
            // P1.4: 风险项目管理
            commands::risk_blueprint::create_risk_project,
            commands::risk_blueprint::list_risk_projects,
            commands::risk_blueprint::delete_risk_project,
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
            commands::risk_blueprint::extract_blueprint,
            commands::risk_blueprint::analyze_fit_gap,
            commands::risk_blueprint::agent_chat,
            commands::risk_blueprint::answer_question,
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
            // Skill Phase 4: Image Processing
            commands::skill::check_image_deps,
            commands::skill::save_image_config,
            commands::skill::process_image,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
