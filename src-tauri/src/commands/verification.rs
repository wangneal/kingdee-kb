//! 验证命令 — 前端在 Chat 完成后调用，获取验证报告

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::app_state::AppState;
use crate::services::verification::pipeline::VerificationPipeline;
use crate::services::verification::types::{
    ScenarioType, VerificationInput, VerificationReport,
};

/// 前端提交的验证请求
#[derive(Debug, Deserialize)]
pub struct VerifyRequest {
    pub generated_text: String,
    pub scenario: String, // "chat" | "search" | "doc_gen" | "research"
    pub session_id: Option<String>, // 新增：会话ID，用于获取缓存的检索片段
}

/// 前端收到的验证结果
#[derive(Debug, Serialize)]
pub struct VerifyResponse {
    pub report: VerificationReport,
}

/// 对已生成的文本执行验证（前端在 done 事件后调用）
#[tauri::command]
pub async fn run_verification(
    _state: State<'_, AppState>,
    request: VerifyRequest,
) -> Result<VerifyResponse, String> {
    let scenario = match request.scenario.as_str() {
        "search" => ScenarioType::SearchQA,
        "doc_gen" => ScenarioType::DocGen,
        "research" => ScenarioType::Research,
        _ => ScenarioType::Chat,
    };

    // 如果提供了会话 ID，拉取缓存的检索片段并清除缓存
    let (retrieved_chunks, chunk_titles, available_chunk_ids) = if let Some(ref sid) = request.session_id {
        let results = crate::services::verification::get_session_rag_results(sid);
        let chunks: Vec<String> = results.iter().map(|r| r.content.clone()).collect();
        let titles: Vec<String> = results.iter().map(|r| r.title.clone()).collect();
        let ids: Vec<i64> = results.iter().map(|r| r.chunk_id).collect();

        // 验证完成后清空该会话缓存
        crate::services::verification::clear_session_rag_results(sid);

        (chunks, titles, ids)
    } else {
        (vec![], vec![], vec![])
    };

    let pipeline = VerificationPipeline::default_with_all();
    let input = VerificationInput {
        generated_text: request.generated_text,
        retrieved_chunks,
        chunk_titles,
        available_chunk_ids,
        query: String::new(),
        scenario,
    };

    let report = pipeline.verify(&input).await;
    Ok(VerifyResponse { report })
}

