pub mod agent;
pub mod knowledge;
pub mod project;
pub mod risk;
pub mod skill;
pub mod media;
pub mod security;
pub mod common;
pub mod harness;
pub mod verification;
pub mod docx_image_helpers;

// Re-export submodules to maintain exact path compatibility (e.g. crate::services::metadata::MetadataStore)
pub use agent::agent_event;
pub use agent::agent_router;
pub use agent::agent_timeout;
pub use agent::llm_providers;
pub use agent::llm_service;
pub use agent::memory;
pub use agent::model_downloader;
pub use agent::model_metadata;
pub use agent::planner;
pub use agent::prompt_assembler;
pub use agent::prompts;
pub use agent::question_tool;
pub use agent::rig_agent;
pub use agent::rig_provider;
pub use agent::rig_tool;
pub use agent::token;
pub use agent::tool_policy;

pub use knowledge::analysis_cache;
pub use knowledge::bm25_service;
pub use knowledge::chinese_postprocess;
pub use knowledge::chunker;
pub use knowledge::document_analysis;
pub use knowledge::embedding;
pub use knowledge::file_extractor;
pub use knowledge::hybrid_search;
pub use knowledge::ingest_cache;
pub use knowledge::ingestion;
pub use knowledge::ingestion_helpers;
pub use knowledge::ingestion_pipeline;
pub use knowledge::ingestion_queue;
pub use knowledge::knowledge_graph;
pub use knowledge::rerank;
pub use knowledge::text_cleaner;
pub use knowledge::vector_index;
pub use knowledge::wiki_page;
pub use knowledge::wikilink_parser;

pub use project::metadata;
pub use project::outline;
pub use project::product_store;
pub use project::project_store;
pub use project::raw_source;
pub use project::research_outline;
pub use project::research_session;

pub use risk::missing_detection;
pub use risk::risk_control;

pub use skill::docx_filler;
pub use skill::skill_executor;
pub use skill::skill_loader;
pub use skill::skill_manager;
pub use skill::skill_trigger;
pub use skill::skill_types;
pub use skill::template_docx;
pub use skill::template_schema;
pub use skill::template_xlsx;
pub use skill::xlsx_filler;

pub use media::audio_capture;
pub use media::image_processor;
pub use media::meeting_minutes_service;
pub use media::meeting_store;
pub use media::meeting_sync;
pub use media::tencent_asr;
pub use media::tencent_meeting_mcp;
pub use media::video_transcriber;
pub use media::whisper_service;

pub use security::desensitize;
pub use security::safety_filter;

pub use common::signal_writer;
pub use common::spawn_safe;
pub use common::traits;
pub use common::types;

#[cfg(test)]
pub use common::test_support;

/// 从 LLM 响应文本中提取 JSON：剥 markdown 代码块（```json ... ```），
/// 退化为首个 `{` 到末个 `}`（或 `[` 到 `]`）截取。
///
/// LLM 即使被要求"只输出 JSON"也常套一层代码块围栏，
/// 直接 serde_json::from_str 会因首字符是反引号而失败。
/// 所有解析 LLM JSON 响应的地方都应先调此函数清洗。
pub fn extract_json_text(text: &str) -> String {
    let text = text.trim();

    // 剥 ```json ... ``` 或 ``` ... ``` 代码块
    if text.starts_with("```") {
        let after_first_line = text.split_once('\n').map(|(_, rest)| rest).unwrap_or("");
        let without_fence = after_first_line
            .rsplit_once("```")
            .map(|(body, _)| body)
            .unwrap_or(after_first_line);
        let cleaned = without_fence.trim();
        if !cleaned.is_empty() {
            return cleaned.to_string();
        }
    }

    // 退化为首个 { 到末个 }（JSON 对象）截取
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return text[start..=end].to_string();
        }
    }

    // 退化为首个 [ 到末个 ]（JSON 数组）截取
    if let Some(start) = text.find('[') {
        if let Some(end) = text.rfind(']') {
            return text[start..=end].to_string();
        }
    }

    text.to_string()
}
