pub mod agent_router;
pub mod agent_timeout;
pub mod analysis_cache;
pub mod audio_capture;
pub mod bm25_service;
pub mod chinese_postprocess;
pub mod chunker;
pub mod desensitize;
pub mod document_analysis;
pub mod docx_filler;
pub mod embedding;
pub mod file_extractor;
pub mod harness;
pub mod hybrid_search;
pub mod image_processor;
pub mod ingest_cache;
pub mod ingestion;
pub mod ingestion_helpers;
pub mod ingestion_pipeline;
pub mod ingestion_queue;
pub mod knowledge_graph;
pub mod llm_providers;
pub mod llm_service;
pub mod meeting_minutes_service;
pub mod meeting_store;
pub mod meeting_sync;
pub mod memory;
pub mod metadata;
pub mod missing_detection;
pub mod model_downloader;
pub mod model_metadata;
pub mod outline;
pub mod planner;
pub mod product_store;
pub mod project_store;
pub mod prompt_assembler;
pub mod prompts;
pub mod question_tool;
pub mod raw_source;
pub mod agent_event;
pub mod rerank;
pub mod research_outline;
pub mod research_session;
pub mod rig_agent;
pub mod rig_provider;
pub mod rig_tool;
pub mod risk_control;
pub mod safety_filter;
pub mod signal_writer;
pub mod skill_executor;
pub mod skill_loader;
pub mod skill_manager;
pub mod skill_trigger;
pub mod skill_types;
pub mod spawn_safe;
pub mod template_docx;
pub mod template_schema;
pub mod template_xlsx;
pub mod tencent_asr;
pub mod tencent_meeting_mcp;
pub mod text_cleaner;
pub mod token;
pub mod tool_policy;
pub mod traits;
pub mod types;
pub mod vector_index;
pub mod verification;
pub mod video_transcriber;
pub mod whisper_service;
pub mod wiki_page;
pub mod wikilink_parser;

#[cfg(test)]
pub mod test_support;
pub mod xlsx_filler;

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

